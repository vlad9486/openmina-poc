use binprot::BinProtRead;
use libp2p::{Swarm, futures::StreamExt, swarm::SwarmEvent, PeerId};
use mina_p2p_messages::rpc_kernel::{self, RpcMethod, ResponseHeader, ResponsePayload};
use libp2p_rpc_behaviour::{Behaviour, Event, StreamId, Received};

use thiserror::Error;

pub struct Client {
    swarm: Swarm<Behaviour>,
    peer: Option<PeerId>,
    stream: Option<StreamId>,
    id: i64,
}

#[derive(Debug, Error)]
pub enum ClientError {
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
                SwarmEvent::Behaviour((
                    peer_id,
                    Event::Stream {
                        stream_id,
                        received,
                    },
                )) => match received {
                    Received::HandshakeDone => {
                        log::info!("new stream {peer_id} {stream_id:?}");
                        self.stream = Some(stream_id);

                        if let (Some(peer_id), Some(stream_id)) = (self.peer, self.stream) {
                            if let Some(query) = query.take() {
                                self.swarm
                                    .behaviour_mut()
                                    .query::<M>(peer_id, stream_id, self.id, query)?;
                                self.id += 1;
                            }
                        }
                    }
                    Received::Menu(menu) => {
                        log::info!("menu: {menu:?}");
                    }
                    Received::Query { .. } => {
                        unimplemented!()
                    }
                    Received::Response {
                        header: ResponseHeader { id },
                        bytes,
                    } => {
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
                },
                _ => {}
            }
        }
    }
}
