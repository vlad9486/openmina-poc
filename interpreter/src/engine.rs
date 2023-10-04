use std::collections::BTreeMap;

use binprot::BinProtRead;
use libp2p::{
    Swarm,
    futures::StreamExt,
    swarm::{SwarmEvent, THandlerErr},
    core::ConnectedPoint,
    PeerId, Multiaddr,
};
use libp2p_rpc_behaviour::{Behaviour, StreamId, Event, Received};
use mina_p2p_messages::{
    rpc,
    rpc_kernel::{RpcMethod, ResponsePayload},
};

use super::command::{Command, CommandInner, CommandStream};

#[derive(Default)]
struct Context {
    peers: BTreeMap<PeerId, PeerContext>,
}

#[derive(Default)]
struct PeerContext {
    req_id: i64,
    pending: BTreeMap<i64, (&'static str, i32)>,
}

pub struct SeedPeer(pub Multiaddr);

impl BinProtRead for SeedPeer {
    fn binprot_read<R: std::io::Read + ?Sized>(r: &mut R) -> Result<Self, binprot::Error>
    where
        Self: Sized,
    {
        use std::net::IpAddr;
        use libp2p::multiaddr::Protocol;

        let (ip, port, peer_id) = <(String, u16, String)>::binprot_read(r)?;
        let ip = ip
            .parse::<IpAddr>()
            .map_err(|err| err.to_string().into())
            .map_err(binprot::Error::CustomError)?;
        let peer_id = peer_id
            .parse::<PeerId>()
            .map_err(|err| err.to_string().into())
            .map_err(binprot::Error::CustomError)?;

        let mut addr = Multiaddr::empty();
        match ip {
            IpAddr::V4(v4) => addr.push(Protocol::Ip4(v4)),
            IpAddr::V6(v6) => addr.push(Protocol::Ip6(v6)),
        }
        addr.push(Protocol::Tcp(port));
        addr.push(Protocol::P2p(peer_id.into()));

        Ok(Self(addr))
    }
}

pub async fn run(mut swarm: Swarm<Behaviour>) {
    let mut rx = CommandStream::new();
    let mut ctx = Context::default();
    loop {
        tokio::select! {
            event = swarm.next() => {
                let Some(event) = event else {
                    break;
                };
                ctx.handle_event(event);
            }
            command = rx.next() => {
                match command {
                    Err(err) => println!("{err}"),
                    Ok(Command::Continue) => {},
                    Ok(Command::Terminate) => break,
                    Ok(Command::Inner(cmd)) => ctx.execute(&mut swarm, cmd),
                }
            }
        }
    }
}

impl Context {
    pub fn execute(&mut self, swarm: &mut Swarm<Behaviour>, cmd: CommandInner) {
        match cmd {
            CommandInner::Connect { addr } => swarm.dial(addr).unwrap(),
            CommandInner::GetSeedPeers { peer_id } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    swarm
                        .behaviour_mut()
                        .query::<rpc::GetSomeInitialPeersV1ForV2>(
                            peer_id,
                            StreamId::Outgoing(0),
                            peer.req_id,
                            (),
                        )
                        .unwrap();
                    peer.pending.insert(
                        peer.req_id,
                        (
                            rpc::GetSomeInitialPeersV1ForV2::NAME,
                            rpc::GetSomeInitialPeersV1ForV2::VERSION,
                        ),
                    );
                    peer.req_id += 1;
                }
            }
        }
    }
    pub fn handle_event(&mut self, event: SwarmEvent<(PeerId, Event), THandlerErr<Behaviour>>) {
        match event {
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                let addr = match endpoint {
                    ConnectedPoint::Dialer { address, .. } => address,
                    ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
                };
                log::info!("new {peer_id} {addr}");
                self.peers.entry(peer_id).or_default();
            }
            SwarmEvent::Behaviour((peer_id, Event::Stream { received, .. })) => {
                let Some(peer_ctx) = self.peers.get_mut(&peer_id) else {
                    log::warn!("recv response, peer not exist {peer_id}");
                    return;
                };
                match received {
                    Received::Response { header, bytes } => {
                        match peer_ctx.pending.remove(&header.id) {
                            None => {
                                log::warn!(
                                    "recv response, from {peer_id}, unexpected id {}",
                                    header.id
                                );
                            }
                            Some((
                                rpc::GetSomeInitialPeersV1ForV2::NAME,
                                rpc::GetSomeInitialPeersV1ForV2::VERSION,
                            )) => {
                                let mut s = bytes.as_slice();
                                type R = Vec<SeedPeer>;
                                let response = ResponsePayload::<R>::binprot_read(&mut s)
                                    .unwrap()
                                    .0
                                    .unwrap()
                                    .0;
                                for SeedPeer(addr) in response {
                                    println!("{addr}");
                                }
                            }
                            Some((name, version)) => {
                                log::warn!("unexpected response {name} {version}");
                            }
                        }
                        //
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
