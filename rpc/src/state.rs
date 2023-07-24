use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Read},
};

use binprot::{BinProtRead, BinProtWrite};
use libp2p::PeerId;
use mina_p2p_messages::{
    gossip::GossipNetMessageV2,
    rpc_kernel::{MessageHeader, RpcMethod, Message, Query, NeedsLength, Response, RpcResult},
};
use mina_transport::{
    OutputEvent as RawP2pEvent, BehaviourEvent, rpc::Event as RawRpcEvent,
    gossipsub::Event as RawGossipEvent,
};

pub enum Event {
    Gossip {
        propagation_source: PeerId,
        source: Option<PeerId>,
        message: Result<GossipNetMessageV2, binprot::Error>,
    },
    NewConnection(PeerId, PeerContext),
    ReadyToRead(PeerId, PeerContext),
    Closed(PeerId),
}

#[derive(Default)]
pub struct P2pState {
    connections: BTreeMap<PeerId, PeerContext>,
}

fn transform_id(id: libp2p::swarm::ConnectionId) -> usize {
    format!("{id:?}")
        .split('(')
        .nth(1)
        .unwrap()
        .trim_end_matches(')')
        .parse()
        .unwrap()
}

impl P2pState {
    pub fn cns(&mut self) -> &mut BTreeMap<PeerId, PeerContext> {
        &mut self.connections
    }

    pub fn on_event(&mut self, event: RawP2pEvent) -> Option<Event> {
        match event {
            RawP2pEvent::NewListenAddr {
                listener_id,
                address,
            } => {
                log::info!("listen {listener_id:?} on {address}");
                None
            }
            RawP2pEvent::Behaviour(BehaviourEvent::Gossipsub(RawGossipEvent::Message {
                propagation_source,
                message,
                ..
            })) => {
                let mut data = &message.data.as_slice()[8..];
                let source = message.source;
                let message = GossipNetMessageV2::binprot_read(&mut data);
                Some(Event::Gossip {
                    propagation_source,
                    source,
                    message,
                })
            }
            RawP2pEvent::Behaviour(BehaviourEvent::Rpc(RawRpcEvent::ConnectionEstablished {
                peer_id,
                connection_id,
            })) => {
                log::info!("connected {peer_id} {}", transform_id(connection_id));

                None
            }
            RawP2pEvent::Behaviour(BehaviourEvent::Rpc(RawRpcEvent::ConnectionClosed {
                peer_id,
                connection_id,
            })) => {
                log::info!("closed {}", transform_id(connection_id));

                if let Some(cn) = self.connections.get(&peer_id) {
                    if cn.connection_id == transform_id(connection_id) {
                        self.connections.remove(&peer_id);
                    }
                }

                None
            }
            RawP2pEvent::Behaviour(BehaviourEvent::Rpc(RawRpcEvent::Negotiated {
                peer_id,
                connection_id,
                inbound,
            })) => {
                log::info!(
                    "negotiated {peer_id} {} {inbound}",
                    transform_id(connection_id)
                );

                Some(Event::NewConnection(
                    peer_id,
                    PeerContext {
                        connection_id: transform_id(connection_id),
                        inner: PeerReaderWrapped::Unlimited(PeerReader::default()),
                        current: None,
                        req_id: 0,
                    },
                ))
            }
            RawP2pEvent::Behaviour(BehaviourEvent::Rpc(RawRpcEvent::RecvMsg {
                peer_id,
                connection_id,
                bytes,
            })) => {
                if let Some(mut ctx) = self.connections.remove(&peer_id) {
                    if ctx.connection_id == transform_id(connection_id) {
                        if bytes.is_empty() {
                            return Some(Event::Closed(peer_id));
                        }
                        ctx.inner.inner_mut().queue.push_back(bytes);
                        return Some(Event::ReadyToRead(peer_id, ctx));
                    }
                }

                None
            }
            _ => None,
        }
    }
}

pub struct PeerContext {
    connection_id: usize,
    inner: PeerReaderWrapped,
    current: Option<(&'static str, i32)>,
    req_id: i64,
}

enum PeerReaderWrapped {
    Unlimited(PeerReader),
    Limited(io::Take<PeerReader>),
}

impl PeerReaderWrapped {
    fn inner_mut(&mut self) -> &mut PeerReader {
        match self {
            Self::Unlimited(reader) => reader,
            Self::Limited(limited) => limited.get_mut(),
        }
    }
}

impl PeerContext {
    pub fn id(&self) -> usize {
        self.connection_id
    }

    pub fn make_response<T: RpcMethod>(&self, response: T::Response, id: i64) -> Vec<u8> {
        let data = RpcResult(Ok(NeedsLength(response)));
        let msg = Message::Response(Response { id, data });
        {
            let mut bytes = 0_u64.to_le_bytes().to_vec();
            msg.binprot_write(&mut bytes).unwrap();
            let len = (bytes.len() - 8) as u64;
            bytes[..8].clone_from_slice(&len.to_le_bytes());
            bytes
        }
    }

    pub fn make<T: RpcMethod>(&mut self, query: T::Query) -> Vec<u8> {
        self.req_id += 1;
        self.current = Some((T::NAME, T::VERSION));
        let msg = Message::Query(Query {
            tag: T::NAME.into(),
            version: T::VERSION,
            id: self.req_id,
            data: NeedsLength(query),
        });
        let magic = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfdRPC\x00\x01".to_vec();
        let bytes = {
            let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
            msg.binprot_write(&mut bytes).unwrap();
            let len = (bytes.len() - 8) as u64;
            bytes[..8].clone_from_slice(&len.to_le_bytes());
            bytes
        };
        let mut output = vec![];
        if self.req_id == 1 {
            output.extend_from_slice(&magic);
        }
        output.extend_from_slice(&bytes);
        output
    }

    fn read_length(&mut self) -> io::Result<()> {
        match &mut self.inner {
            PeerReaderWrapped::Unlimited(ref mut reader) => {
                if reader.buffered() < 8 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let mut length_bytes = [0; 8];
                reader.read_exact(&mut length_bytes)?;
                let limit = u64::from_le_bytes(length_bytes);
                let limited = reader.clone().take(limit);
                self.inner = PeerReaderWrapped::Limited(limited);
            }
            PeerReaderWrapped::Limited(_) => {}
        }

        Ok(())
    }

    pub fn read_header(&mut self) -> Result<MessageHeader, binprot::Error> {
        match &mut self.inner {
            PeerReaderWrapped::Limited(ref mut limited) => {
                if limited.get_ref().buffered() < limited.limit() as usize {
                    return Err(binprot::Error::IoError(io::ErrorKind::WouldBlock.into()));
                }
                let header = MessageHeader::binprot_read(limited)?;
                if limited.limit() == 0 {
                    self.inner = PeerReaderWrapped::Unlimited(limited.get_ref().clone());
                }
                Ok(header)
            }
            PeerReaderWrapped::Unlimited(_) => {
                self.read_length()?;
                self.read_header()
            }
        }
    }

    pub fn read_remaining<M: binprot::BinProtRead>(&mut self) -> Result<M, binprot::Error> {
        match &mut self.inner {
            PeerReaderWrapped::Limited(ref mut limited) => {
                let msg = M::binprot_read(limited)?;
                if limited.limit() != 0 {
                    limited.read_to_end(&mut vec![])?;
                }
                self.inner = PeerReaderWrapped::Unlimited(limited.get_ref().clone());
                Ok(msg)
            }
            PeerReaderWrapped::Unlimited(_) => {
                panic!("must read header first");
            }
        }
    }
}

#[derive(Default, Clone)]
struct PeerReader {
    queue: VecDeque<Vec<u8>>,
    offset: usize,
    remaining_bytes: Vec<u8>,
}

impl PeerReader {
    fn buffered(&self) -> usize {
        let rem = self.remaining_bytes.len() - self.offset;
        self.queue.iter().map(|x| x.len()).sum::<usize>() + rem
    }
}

impl io::Read for PeerReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rem = self.remaining_bytes.len() - self.offset;
        if rem == 0 {
            if let Some(bytes) = self.queue.pop_front() {
                self.offset = 0;
                self.remaining_bytes = bytes;
                self.read(buf)
            } else {
                Err(io::ErrorKind::UnexpectedEof.into())
            }
        } else {
            if rem < buf.len() {
                buf[..rem].clone_from_slice(&self.remaining_bytes[self.offset..]);
                self.offset = self.remaining_bytes.len();
                Ok(rem)
            } else {
                let new_offset = self.offset + buf.len();
                buf.clone_from_slice(&self.remaining_bytes[self.offset..new_offset]);
                self.offset = new_offset;

                Ok(buf.len())
            }
        }
    }
}
