#![forbid(unsafe_code)]

mod state;

use std::io;

use libp2p::{futures::StreamExt, swarm::ConnectionId};
use mina_p2p_messages::{
    rpc_kernel::{RpcMethod, ResponsePayload, MessageHeader, Error, NeedsLength, QueryHeader},
    v2,
    gossip::GossipNetMessageV2,
};

pub use self::state::PeerContext;

pub struct Engine {
    swarm: libp2p::Swarm<mina_transport::Behaviour>,
    state: state::P2pState,
    block_sender: Box<dyn Fn(v2::MinaBlockBlockStableV2) + Send>,
}

impl Engine {
    pub fn new(
        swarm: libp2p::Swarm<mina_transport::Behaviour>,
        block_sender: Box<dyn Fn(v2::MinaBlockBlockStableV2) + Send>,
    ) -> Self {
        Engine {
            swarm,
            state: state::P2pState::default(),
            block_sender,
        }
    }

    async fn drive(&mut self) -> Option<state::Event> {
        if let Some(event) = self.swarm.next().await {
            self.state.on_event(event)
        } else {
            None
        }
    }

    fn handle_gossip(&self, message: Result<GossipNetMessageV2, binprot::Error>) {
        match message {
            Err(err) => {
                log::error!("received corrupted gossip message {err}");
            }
            Ok(GossipNetMessageV2::NewState(block)) => {
                let height = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .blockchain_length
                    .as_u32();
                log::info!("received gossip block {height}");
                (self.block_sender)(block);
            }
            _ => {}
        }
    }

    pub async fn wait_infinite(mut self) {
        'drive: loop {
            log::info!("waiting for events...");

            match self.drive().await {
                Some(state::Event::Gossip { message, .. }) => self.handle_gossip(message),
                Some(state::Event::ReadyToRead(peer_id, mut ctx)) => loop {
                    match ctx.read_header() {
                        Err(binprot::Error::IoError(err))
                            if err.kind() == io::ErrorKind::WouldBlock =>
                        {
                            self.state.cns().insert(peer_id, ctx);
                            continue 'drive;
                        }
                        Err(binprot::Error::IoError(err))
                            if err.kind() == io::ErrorKind::UnexpectedEof =>
                        {
                            self.state.cns().insert(peer_id, ctx);
                            continue 'drive;
                        }
                        Err(err) => panic!("{err}"),
                        Ok(MessageHeader::Heartbeat) => {
                            log::info!("send heartbeat");
                            let connection_id = ConnectionId::new_unchecked(ctx.id());
                            self.swarm.behaviour_mut().rpc.send(
                                peer_id,
                                connection_id,
                                b"\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec(),
                            );
                        }
                        _ => (),
                    }
                },
                _ => {}
            }
        }
    }

    pub async fn wait_for_request<F>(&mut self, mut closure: F) -> Result<(), binprot::Error>
    where
        F: FnMut(QueryHeader, &mut PeerContext) -> Vec<u8>,
    {
        'drive: loop {
            match self.drive().await {
                Some(state::Event::NewConnection(peer_id, ctx)) => {
                    self.state.cns().insert(peer_id, ctx);
                }
                Some(state::Event::ReadyToRead(peer_id, mut ctx)) => {
                    loop {
                        match ctx.read_header() {
                            Err(binprot::Error::IoError(err))
                                if err.kind() == io::ErrorKind::WouldBlock =>
                            {
                                self.state.cns().insert(peer_id, ctx);
                                continue 'drive;
                            }
                            Err(err) => return Err(err),
                            Ok(MessageHeader::Heartbeat) => {
                                let connection_id = ConnectionId::new_unchecked(ctx.id());
                                self.swarm.behaviour_mut().rpc.send(
                                    peer_id,
                                    connection_id,
                                    b"\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec(),
                                );
                            }
                            Ok(MessageHeader::Query(q)) => {
                                let connection_id = ConnectionId::new_unchecked(ctx.id());

                                let bytes = closure(q, &mut ctx);
                                self.state.cns().insert(peer_id, ctx);
                                self.swarm
                                    .behaviour_mut()
                                    .rpc
                                    .send(peer_id, connection_id, bytes);

                                continue 'drive;
                            }
                            Ok(MessageHeader::Response(h)) => {
                                if h.id == i64::from_le_bytes(*b"RPC\x00\x00\x00\x00\x00") {
                                    ctx.read_remaining::<u8>()?;
                                    // TODO: process this message
                                } else {
                                    unimplemented!()
                                }
                                continue 'drive;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn rpc<M: RpcMethod>(
        &mut self,
        query: M::Query,
    ) -> Result<Result<M::Response, Error>, binprot::Error>
    where
        M::Query: Clone,
    {
        let (peer_id, mut ctx) = if let Some(peer_id) = self.state.cns().keys().next().cloned() {
            (
                peer_id,
                self.state.cns().remove(&peer_id).expect("checked above"),
            )
        } else {
            loop {
                match self.drive().await {
                    Some(state::Event::Gossip { message, .. }) => self.handle_gossip(message),
                    Some(state::Event::NewConnection(peer_id, ctx)) => break (peer_id, ctx),
                    _ => {}
                }
            }
        };

        let bytes = ctx.make::<M>(query.clone());
        let connection_id = ConnectionId::new_unchecked(ctx.id());
        self.state.cns().insert(peer_id, ctx);
        self.swarm
            .behaviour_mut()
            .rpc
            .send(peer_id, connection_id, bytes);

        'drive: loop {
            match self.drive().await {
                Some(state::Event::Gossip { message, .. }) => self.handle_gossip(message),
                Some(state::Event::NewConnection(new_peer_id, mut ctx)) => {
                    if peer_id == new_peer_id {
                        let bytes = ctx.make::<M>(query.clone());
                        let connection_id = ConnectionId::new_unchecked(ctx.id());
                        self.state.cns().insert(peer_id, ctx);
                        self.swarm
                            .behaviour_mut()
                            .rpc
                            .send(peer_id, connection_id, bytes);
                    }
                }
                Some(state::Event::Closed(this_peer_id)) => {
                    if peer_id == this_peer_id {
                        continue 'drive;
                    }
                }
                Some(state::Event::ReadyToRead(this_peer_id, mut ctx)) => {
                    if peer_id == this_peer_id {
                        loop {
                            match ctx.read_header() {
                                Err(binprot::Error::IoError(err))
                                    if err.kind() == io::ErrorKind::WouldBlock =>
                                {
                                    self.state.cns().insert(peer_id, ctx);
                                    continue 'drive;
                                }
                                Err(err) => return Err(err),
                                Ok(MessageHeader::Heartbeat) => {
                                    let connection_id = ConnectionId::new_unchecked(ctx.id());

                                    self.swarm.behaviour_mut().rpc.send(
                                        peer_id,
                                        connection_id,
                                        b"\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec(),
                                    );
                                }
                                Ok(MessageHeader::Query(q)) => {
                                    let connection_id = ConnectionId::new_unchecked(ctx.id());

                                    // TODO: process query
                                    use mina_p2p_messages::rpc::VersionedRpcMenuV1;

                                    let tag = std::str::from_utf8(q.tag.as_ref()).unwrap();
                                    match (tag, q.version) {
                                        (VersionedRpcMenuV1::NAME, VersionedRpcMenuV1::VERSION) => {
                                            let bytes = ctx
                                                .make_response::<VersionedRpcMenuV1>(vec![], q.id);
                                            self.swarm.behaviour_mut().rpc.send(
                                                peer_id,
                                                connection_id,
                                                bytes,
                                            );
                                        }
                                        _ => unimplemented!(),
                                    }
                                }
                                Ok(MessageHeader::Response(h)) => {
                                    if h.id == i64::from_le_bytes(*b"RPC\x00\x00\x00\x00\x00") {
                                        ctx.read_remaining::<u8>()?;
                                        // TODO: process this message
                                    } else {
                                        let r = ctx.read_remaining::<ResponsePayload<_>>()?;
                                        self.state.cns().insert(peer_id, ctx);
                                        return Ok(r.0.map(|NeedsLength(x)| x));
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
