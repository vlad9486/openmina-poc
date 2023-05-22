use std::{
    task::{Poll, Context, Waker, self},
    time::Duration,
    collections::VecDeque,
    pin::Pin,
    io,
};

use libp2p::{
    swarm::{
        NetworkBehaviour, ConnectionId, ConnectionHandler, FromSwarm, THandlerOutEvent,
        PollParameters, THandlerInEvent, ToSwarm, SubstreamProtocol, ConnectionHandlerEvent,
        KeepAlive, handler::ConnectionEvent, derive_prelude::ConnectionEstablished,
        ConnectionClosed, NotifyHandler,
    },
    PeerId,
    core::{upgrade::ReadyUpgrade, Negotiated, muxing::SubstreamBox},
    futures::{AsyncWrite, AsyncRead},
};

#[derive(Default)]
pub struct Handler {
    substream: Option<SubstreamState>,
    outbound: VecDeque<(usize, Vec<u8>)>,
    buffer: Option<Vec<u8>>,
    direction: bool,
    waker: Option<Waker>,
}

enum SubstreamState {
    Opening,
    Negotiated(Negotiated<SubstreamBox>),
}

#[derive(Debug)]
pub enum InEvent {
    SendQuery(Vec<u8>),
}

#[derive(Debug)]
pub enum OutEvent {
    RecvBytes(Vec<u8>),
}

const NAME: [u8; 15] = *b"coda/rpcs/0.0.1";

impl ConnectionHandler for Handler {
    type InEvent = InEvent;
    type OutEvent = OutEvent;
    type Error = io::Error;
    type InboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(NAME), ()).with_timeout(Duration::from_secs(15))
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::No
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(substream) = &mut self.substream {
            match substream {
                SubstreamState::Opening => {
                    self.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
                SubstreamState::Negotiated(io) => {
                    self.direction = !self.direction;
                    if self.direction {
                        if let Some((offset, bytes)) = self.outbound.front_mut() {
                            match task::ready!(Pin::new(io).poll_write(cx, &bytes[*offset..])) {
                                Ok(written) => {
                                    log::debug!(
                                        "written: {written}, {}",
                                        hex::encode(&bytes[*offset..(*offset + written)]),
                                    );
                                    *offset += written;
                                    if *offset >= bytes.len() {
                                        self.outbound.pop_front();
                                    }
                                    self.poll(cx)
                                }
                                Err(err) => Poll::Ready(ConnectionHandlerEvent::Close(err)),
                            }
                        } else {
                            self.waker = Some(cx.waker().clone());
                            Poll::Pending
                        }
                    } else {
                        let mut buffer = self.buffer.get_or_insert_with(|| vec![0; 0x10000]);
                        match task::ready!(Pin::new(io).poll_read(cx, &mut buffer)) {
                            Ok(read) => {
                                log::debug!("read: {read}, {}", hex::encode(&buffer[..read]),);
                                let event = OutEvent::RecvBytes(buffer[..read].to_vec());
                                Poll::Ready(ConnectionHandlerEvent::Custom(event))
                            }
                            Err(err) => Poll::Ready(ConnectionHandlerEvent::Close(err)),
                        }
                    }
                }
            }
        } else {
            self.waker = Some(cx.waker().clone());
            self.substream = Some(SubstreamState::Opening);
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(NAME), ())
                    .with_timeout(Duration::from_secs(15)),
            })
        }
    }

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        match event {
            InEvent::SendQuery(bytes) => self.outbound.push_back((0, bytes)),
        }
        self.waker.as_ref().map(Waker::wake_by_ref);
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(x) => {
                log::debug!("FullyNegotiatedInbound {:?}", x.protocol);
                self.substream = Some(SubstreamState::Negotiated(x.protocol));
            }
            ConnectionEvent::FullyNegotiatedOutbound(x) => {
                log::debug!("FullyNegotiatedOutbound {:?}", x.protocol);
                self.substream = Some(SubstreamState::Negotiated(x.protocol));
            }
            ConnectionEvent::AddressChange(x) => {
                log::debug!("AddressChange {}", x.new_address);
            }
            ConnectionEvent::DialUpgradeError(error) => {
                log::debug!("DialUpgradeError {}", error.error);
            }
            ConnectionEvent::ListenUpgradeError(error) => {
                log::debug!("ListenUpgradeError {}", error.error);
            }
        }
    }
}

#[derive(Default)]
pub struct Behaviour {
    queue: VecDeque<ToSwarm<Event, InEvent>>,
    waker: Option<Waker>,
}

#[derive(Debug)]
pub enum Event {
    ConnectionEstablished {
        peer_id: PeerId,
        connection_id: ConnectionId,
    },
    ConnectionClosed {
        peer_id: PeerId,
        connection_id: ConnectionId,
    },
    RecvMsg {
        peer_id: PeerId,
        connection_id: ConnectionId,
        bytes: Vec<u8>,
    },
}

impl Behaviour {
    pub fn send(&mut self, peer_id: PeerId, connection_id: ConnectionId, bytes: Vec<u8>) {
        self.queue.push_back(ToSwarm::NotifyHandler {
            peer_id,
            handler: NotifyHandler::One(connection_id),
            event: InEvent::SendQuery(bytes),
        });
        self.waker.as_ref().map(Waker::wake_by_ref);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        Handler::default()
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => {
                self.queue
                    .push_back(ToSwarm::GenerateEvent(Event::ConnectionEstablished {
                        peer_id,
                        connection_id,
                    }));
                self.waker.as_ref().map(Waker::wake_by_ref);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.queue
                    .push_back(ToSwarm::GenerateEvent(Event::ConnectionClosed {
                        peer_id,
                        connection_id,
                    }));
                self.waker.as_ref().map(Waker::wake_by_ref);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            OutEvent::RecvBytes(bytes) => {
                self.queue.push_back(ToSwarm::GenerateEvent(Event::RecvMsg {
                    peer_id,
                    connection_id,
                    bytes,
                }));
                self.waker.as_ref().map(Waker::wake_by_ref);
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.queue.pop_front() {
            Poll::Ready(event)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}