mod state;

use std::{
    collections::{VecDeque, BTreeMap},
    task::{Waker, Context, Poll},
    io,
    time::Duration,
};

use libp2p::{
    swarm::{
        ToSwarm, NetworkBehaviour, NotifyHandler, ConnectionId, ConnectionHandler,
        SubstreamProtocol, KeepAlive, FromSwarm, THandlerOutEvent, PollParameters, THandlerInEvent,
        derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionHandlerEvent,
        handler::ConnectionEvent, THandler, ConnectionDenied,
    },
    PeerId,
    core::{upgrade::ReadyUpgrade, Endpoint, Negotiated, muxing::SubstreamBox},
    Multiaddr,
};

#[derive(Default)]
pub struct Behaviour {
    peers: BTreeMap<PeerId, ConnectionId>,
    queue: VecDeque<ToSwarm<(PeerId, Event), Command>>,
    pending: BTreeMap<PeerId, VecDeque<Command>>,
    waker: Option<Waker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamId {
    Incoming(u32),
    Outgoing(u32),
}

#[derive(Debug)]
pub struct Command {
    stream_id: StreamId,
    command: Vec<u8>,
}

#[derive(Debug)]
pub enum Event {
    ConnectionEstablished,
    ConnectionClosed,
    InboundNegotiated { stream_id: StreamId },
    OutboundNegotiated { stream_id: StreamId },
    Stream { stream_id: StreamId, event: Vec<u8> },
}

impl Behaviour {
    fn dispatch_command(&mut self, peer_id: PeerId, command: Command) {
        if let Some(connection_id) = self.peers.get(&peer_id) {
            self.queue.push_back(ToSwarm::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(*connection_id),
                event: command,
            });
            self.waker.as_ref().map(Waker::wake_by_ref);
        } else {
            self.pending.entry(peer_id).or_default().push_back(command);
        }
    }

    pub fn send(&mut self, peer_id: PeerId, stream_id: StreamId, data: Vec<u8>) {
        self.dispatch_command(
            peer_id,
            Command {
                stream_id,
                command: data,
            },
        )
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = (PeerId, Event);

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.peers.insert(peer, connection_id);
        Ok(Handler::default())
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.peers.insert(peer, connection_id);
        Ok(Handler::default())
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => {
                self.peers.insert(peer_id, connection_id);
                self.queue.push_back(ToSwarm::GenerateEvent((
                    peer_id,
                    Event::ConnectionEstablished,
                )));
                if let Some(queue) = self.pending.remove(&peer_id) {
                    for command in queue {
                        self.queue.push_back(ToSwarm::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::One(connection_id),
                            event: command,
                        });
                    }
                }
                self.waker.as_ref().map(Waker::wake_by_ref);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                if self.peers.get(&peer_id) == Some(&connection_id) {
                    self.peers.remove(&peer_id);
                }
                self.queue
                    .push_back(ToSwarm::GenerateEvent((peer_id, Event::ConnectionClosed)));
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
        self.peers.insert(peer_id, connection_id);
        self.queue
            .push_back(ToSwarm::GenerateEvent((peer_id, event)));
        self.waker.as_ref().map(Waker::wake_by_ref);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.queue.pop_front() {
            // if !self.queue.is_empty() {
            //     cx.waker().wake_by_ref();
            // }
            Poll::Ready(event)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Default)]
pub struct Handler {
    streams: BTreeMap<StreamId, Stream>,
    last_outgoing_id: VecDeque<u32>,
    last_incoming_id: u32,

    waker: Option<Waker>,
}

#[derive(Default)]
struct Stream {
    incoming: bool,
    opening_state: Option<OpeningState>,
    inner_state: state::Inner,
}

enum OpeningState {
    Requested,
    Negotiated {
        io: Negotiated<SubstreamBox>,
        reported: bool,
    },
}

impl Handler {
    const PROTOCOL_NAME: [u8; 15] = *b"coda/rpcs/0.0.1";

    fn add_stream(&mut self, incoming: bool, io: Negotiated<SubstreamBox>) {
        let opening_state = Some(OpeningState::Negotiated {
            io,
            reported: false,
        });
        if incoming {
            let id = self.last_incoming_id;
            self.last_incoming_id += 1;
            self.streams.insert(
                StreamId::Incoming(id),
                Stream {
                    incoming: true,
                    opening_state,
                    ..Default::default()
                },
            );
            self.waker.as_ref().map(Waker::wake_by_ref);
        } else if let Some(id) = self.last_outgoing_id.pop_front() {
            if let Some(stream) = self.streams.get_mut(&StreamId::Outgoing(id)) {
                stream.opening_state = opening_state;
                self.waker.as_ref().map(Waker::wake_by_ref);
            }
        }
    }
}

impl ConnectionHandler for Handler {
    type InEvent = Command;
    type OutEvent = Event;
    type Error = io::Error;
    type InboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(Self::PROTOCOL_NAME), ())
            .with_timeout(Duration::from_secs(15))
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
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
        for (&stream_id, stream) in &mut self.streams {
            match &mut stream.opening_state {
                None => {
                    stream.opening_state = Some(OpeningState::Requested);
                    return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            ReadyUpgrade::new(Self::PROTOCOL_NAME),
                            (),
                        ),
                    });
                }
                Some(OpeningState::Requested) => {}
                Some(OpeningState::Negotiated { io, reported }) => {
                    if !*reported {
                        *reported = true;
                        let r = if stream.incoming {
                            Event::InboundNegotiated { stream_id }
                        } else {
                            Event::OutboundNegotiated { stream_id }
                        };
                        return Poll::Ready(ConnectionHandlerEvent::Custom(r));
                    } else {
                        match stream.inner_state.poll(cx, io) {
                            Poll::Pending => (),
                            Poll::Ready(Err(err)) => {
                                return Poll::Ready(ConnectionHandlerEvent::Close(err));
                            }
                            Poll::Ready(Ok(event)) => {
                                // use mina_p2p_messages;
                                return Poll::Ready(ConnectionHandlerEvent::Custom(
                                    Event::Stream { stream_id, event },
                                ));
                            }
                        }
                    }
                }
            }
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        self.streams
            .entry(event.stream_id)
            .or_insert_with(|| {
                if let StreamId::Outgoing(id) = event.stream_id {
                    self.last_outgoing_id.push_back(id);
                }
                Stream::default()
            })
            .inner_state
            .add(event.command);
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
            ConnectionEvent::FullyNegotiatedInbound(io) => self.add_stream(true, io.protocol),
            ConnectionEvent::FullyNegotiatedOutbound(io) => self.add_stream(false, io.protocol),
            _ => {}
        }
    }
}
