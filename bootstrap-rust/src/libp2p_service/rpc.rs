use std::task::{Poll, Context};

use libp2p::{
    swarm::{
        NetworkBehaviour, ConnectionId, ConnectionHandler, FromSwarm, THandlerOutEvent,
        PollParameters, THandlerInEvent, ToSwarm, SubstreamProtocol, ConnectionHandlerEvent,
        KeepAlive, handler::ConnectionEvent,
    },
    PeerId,
    core::upgrade::ReadyUpgrade,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Failure {
    #[error("f")]
    F,
}

pub struct Handler;

const NAME: [u8; 15] = *b"coda/rpcs/0.0.1";

impl ConnectionHandler for Handler {
    type InEvent = ();
    type OutEvent = ();
    type Error = Failure;
    type InboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundProtocol = ReadyUpgrade<[u8; 15]>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(NAME), ())
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
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, _event: Self::InEvent) {}

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
    }
}

pub struct Behaviour;

#[derive(Debug)]
pub enum Event {
    N,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        Handler
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {}

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        _event: THandlerOutEvent<Self>,
    ) {
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}
