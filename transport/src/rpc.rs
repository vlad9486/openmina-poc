use std::{
    task::{Poll, Context, Waker, self},
    time::Duration,
    collections::VecDeque,
    pin::Pin,
    io, mem,
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
    substream: SubstreamState,
    inner_state: InnerState,
}

#[derive(Default)]
enum SubstreamState {
    #[default]
    None,
    Opening,
    FirstSeen(Negotiated<SubstreamBox>, bool),
    Negotiated(Negotiated<SubstreamBox>),
}

#[derive(Default)]
struct InnerState {
    outbound: VecDeque<(usize, Vec<u8>)>,
    buffer: Option<Vec<u8>>,
    direction: bool,
    waker: Option<Waker>,
    inbound: bool,
}

#[derive(Debug)]
pub enum InEvent {
    SendBytes { bytes: Vec<u8> },
}

#[derive(Debug)]
pub enum OutEvent {
    RecvBytes(Vec<u8>),
    Negotiated { inbound: bool },
}

type HandlerEvent<H> = ConnectionHandlerEvent<
    <H as ConnectionHandler>::OutboundProtocol,
    <H as ConnectionHandler>::OutboundOpenInfo,
    <H as ConnectionHandler>::OutEvent,
    <H as ConnectionHandler>::Error,
>;

impl InnerState {
    fn poll_write<T>(&mut self, mut io: &mut T, cx: &mut Context<'_>) -> Poll<HandlerEvent<Handler>>
    where
        T: Unpin + AsyncWrite,
    {
        if let Some((offset, bytes)) = self.outbound.front_mut() {
            match task::ready!(Pin::new(&mut io).poll_write(cx, &bytes[*offset..])) {
                Ok(written) => {
                    *offset += written;
                    if *offset >= bytes.len() {
                        self.outbound.pop_front();
                    }
                    self.poll_write(io, cx)
                }
                Err(err) => Poll::Ready(ConnectionHandlerEvent::Close(err)),
            }
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_read<T>(&mut self, mut io: &mut T, cx: &mut Context<'_>) -> Poll<HandlerEvent<Handler>>
    where
        T: Unpin + AsyncRead,
    {
        let mut buffer = self.buffer.get_or_insert_with(|| vec![0; 0x10000]);
        match task::ready!(Pin::new(&mut io).poll_read(cx, &mut buffer)) {
            Ok(read) => {
                let event = OutEvent::RecvBytes(buffer[..read].to_vec());
                Poll::Ready(ConnectionHandlerEvent::Custom(event))
            }
            Err(err) => Poll::Ready(ConnectionHandlerEvent::Close(err)),
        }
    }
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
        match &self.substream {
            SubstreamState::Opening => KeepAlive::No,
            _ => {
                if self.inner_state.inbound {
                    KeepAlive::No
                } else {
                    KeepAlive::Yes
                }
            }
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<HandlerEvent<Self>> {
        match &mut self.substream {
            SubstreamState::None => {
                self.inner_state.waker = Some(cx.waker().clone());
                self.substream = SubstreamState::Opening;
                Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(ReadyUpgrade::new(NAME), ())
                        .with_timeout(Duration::from_secs(15)),
                })
            }
            SubstreamState::Opening => {
                self.inner_state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            SubstreamState::FirstSeen(..) => {
                let (io, inbound) = match mem::replace(&mut self.substream, SubstreamState::Opening)
                {
                    SubstreamState::FirstSeen(io, inbound) => (io, inbound),
                    _ => panic!(),
                };
                self.substream = SubstreamState::Negotiated(io);
                Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::Negotiated {
                    inbound,
                }))
            }
            SubstreamState::Negotiated(io) => {
                self.inner_state.direction = !self.inner_state.direction;
                let r = if self.inner_state.direction {
                    match self.inner_state.poll_write(io, cx) {
                        Poll::Pending => self.inner_state.poll_read(io, cx),
                        x => x,
                    }
                } else {
                    match self.inner_state.poll_read(io, cx) {
                        Poll::Pending => self.inner_state.poll_write(io, cx),
                        x => x,
                    }
                };
                if let Poll::Ready(event) = &r {
                    match event {
                        ConnectionHandlerEvent::Close(err) => {
                            log::warn!("explicit close: {err}, {}", err.kind());
                            self.substream = SubstreamState::None;
                        }
                        ConnectionHandlerEvent::Custom(OutEvent::RecvBytes(b)) if b.is_empty() => {
                            log::warn!("implicit close");
                            self.substream = SubstreamState::None;
                            return self.poll(cx);
                        }
                        _ => (),
                    }
                }
                r
            }
        }
    }

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        match event {
            InEvent::SendBytes { bytes } => self.inner_state.outbound.push_back((0, bytes)),
        }
        self.inner_state.waker.as_ref().map(Waker::wake_by_ref);
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
                self.substream = SubstreamState::FirstSeen(x.protocol, true);
                self.inner_state.inbound = true;
                self.inner_state.waker.as_ref().map(Waker::wake_by_ref);
            }
            ConnectionEvent::FullyNegotiatedOutbound(x) => {
                log::debug!("FullyNegotiatedOutbound {:?}", x.protocol);
                self.substream = SubstreamState::FirstSeen(x.protocol, false);
                self.inner_state.inbound = false;
                self.inner_state.waker.as_ref().map(Waker::wake_by_ref);
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
    Negotiated {
        peer_id: PeerId,
        connection_id: ConnectionId,
        inbound: bool,
    },
}

impl Behaviour {
    pub fn send(&mut self, peer_id: PeerId, connection_id: ConnectionId, bytes: Vec<u8>) {
        self.queue.push_back(ToSwarm::NotifyHandler {
            peer_id,
            handler: NotifyHandler::One(connection_id),
            event: InEvent::SendBytes { bytes },
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
            }
            OutEvent::Negotiated { inbound } => {
                self.queue
                    .push_back(ToSwarm::GenerateEvent(Event::Negotiated {
                        peer_id,
                        connection_id,
                        inbound,
                    }));
            }
        }
        self.waker.as_ref().map(Waker::wake_by_ref);
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
