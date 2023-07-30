use std::{
    fs::{self, File},
    path::Path,
};

use binprot::{BinProtRead, BinProtWrite};
use libp2p::{Swarm, futures::StreamExt, swarm::SwarmEvent, PeerId};
use mina_p2p_messages::{
    rpc_kernel::{self, RpcMethod, MessageHeader, ResponseHeader, ResponsePayload},
    rpc::{GetBestTipV2, WithHashV1, GetAncestryV2},
};
use mina_rpc_behaviour::{Behaviour, Event, StreamId};

use thiserror::Error;

pub async fn run(swarm: Swarm<Behaviour>) {
    let mut client = Client::new(swarm);

    let path_main = AsRef::<Path>::as_ref("target/record");
    fs::create_dir_all(path_main).unwrap();

    let best_tip = client.rpc::<GetBestTipV2>(()).await.unwrap().unwrap();

    let head_height = best_tip
        .data
        .header
        .protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();

    log::info!("will record {head_height}");
    let path = path_main.join(head_height.to_string());
    fs::create_dir_all(path.join("ledgers")).unwrap();

    let mut file = File::create(path.join("best_tip")).unwrap();
    Some(best_tip.clone()).binprot_write(&mut file).unwrap();

    let q = best_tip
        .data
        .header
        .protocol_state
        .body
        .consensus_state
        .clone();
    let hash = best_tip.data.header.protocol_state.hash().0.clone();
    let q = WithHashV1 { data: q, hash };
    let ancestry = client.rpc::<GetAncestryV2>(q).await.unwrap().unwrap();

    let mut file = File::create(path.join("ancestry")).unwrap();
    Some(ancestry.clone()).binprot_write(&mut file).unwrap();
}

struct Client {
    swarm: Swarm<Behaviour>,
    peer: Option<PeerId>,
    stream: Option<StreamId>,
    id: i64,
}

#[derive(Debug, Error)]
enum ClientError {
    #[error("{0}")]
    Binprot(#[from] binprot::Error),
    #[error("{0:?}")]
    InternalError(rpc_kernel::Error),
    #[error("libp2p stop working")]
    Libp2p,
}

impl Client {
    pub fn new(swarm: Swarm<Behaviour>) -> Self {
        Client {
            swarm,
            peer: None,
            stream: None,
            id: 1,
        }
    }

    pub async fn rpc<M>(&mut self, query: M::Query) -> Result<M::Response, ClientError>
    where
        M: RpcMethod,
    {
        let mut query = Some(query);
        if let (Some(peer_id), Some(stream_id)) = (self.peer, self.stream) {
            if let Some(query) = query.take() {
                log::info!("sending request: id: {}, tag: {}", self.id, M::NAME);
                self.swarm
                    .behaviour_mut()
                    .query::<M>(peer_id, stream_id, self.id, query)?;
                self.id += 1;
            }
        }

        loop {
            match self.swarm.next().await.ok_or(ClientError::Libp2p)? {
                SwarmEvent::Behaviour((peer_id, Event::ConnectionEstablished)) => {
                    log::info!("new connection {peer_id}");

                    self.peer = Some(peer_id);
                    self.swarm.behaviour_mut().open(peer_id, 0);
                }
                SwarmEvent::Behaviour((peer_id, Event::ConnectionClosed)) => {
                    log::info!("connection closed {peer_id}");
                    if self.peer == Some(peer_id) {
                        self.peer = None;
                        // TODO: resend
                    }
                }
                SwarmEvent::Behaviour((peer_id, Event::StreamNegotiated { stream_id, menu })) => {
                    log::info!("new stream {peer_id} {stream_id:?} {menu:?}");
                    self.stream = Some(stream_id);

                    if let (Some(peer_id), Some(stream_id)) = (self.peer, self.stream) {
                        if let Some(query) = query.take() {
                            log::info!("sending request: id: {}, tag: {}", self.id, M::NAME);
                            self.swarm
                                .behaviour_mut()
                                .query::<M>(peer_id, stream_id, self.id, query)?;
                            self.id += 1;
                        }
                    }
                }
                SwarmEvent::Behaviour((_, Event::Stream { header, bytes, .. })) => match header {
                    MessageHeader::Query(_) => {
                        unimplemented!()
                    }
                    MessageHeader::Response(ResponseHeader { id }) => {
                        log::info!("received response: {id}");
                        if id + 1 == self.id {
                            let mut bytes = bytes.as_slice();
                            let response =
                                ResponsePayload::<M::Response>::binprot_read(&mut bytes)?
                                    .0
                                    .map_err(ClientError::InternalError)?
                                    .0;
                            return Ok(response);
                        }
                    }
                    MessageHeader::Heartbeat => {}
                },
                _ => {}
            }
        }
    }
}
