use std::{
    task::{self, Context, Poll},
    io,
    sync::Arc,
    collections::BTreeSet,
};

use libp2p::core::{Negotiated, muxing::SubstreamBox};

use mina_p2p_messages::{
    rpc_kernel::{NeedsLength, Error, RpcMethod, MessageHeader, ResponseHeader, ResponsePayload},
    rpc::VersionedRpcMenuV1,
};
use binprot::BinProtRead;

use super::{
    state,
    behaviour::{StreamId, Event},
};

pub struct Stream {
    opening_state: Option<OpeningState>,
    inner_state: state::Inner,
}

enum OpeningState {
    Requested,
    Negotiated {
        io: Negotiated<SubstreamBox>,
        need_report: bool,
    },
}

pub enum StreamEvent {
    Request(u32),
    Event(Event),
}

impl Stream {
    pub fn new_outgoing() -> Self {
        Stream {
            opening_state: None,
            // empty menu for outgoing stream
            inner_state: state::Inner::new(Arc::new(BTreeSet::default())),
        }
    }

    pub fn new_incoming(menu: Arc<BTreeSet<(&'static str, i32)>>) -> Self {
        Stream {
            opening_state: None,
            inner_state: state::Inner::new(menu),
        }
    }

    pub fn negotiated(&mut self, io: Negotiated<SubstreamBox>, need_report: bool) {
        self.opening_state = Some(OpeningState::Negotiated { io, need_report });
    }

    pub fn add(&mut self, bytes: Vec<u8>) {
        self.inner_state.add(bytes);
    }

    pub fn poll_stream(
        &mut self,
        stream_id: StreamId,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamEvent>> {
        match &mut self.opening_state {
            None => {
                if let StreamId::Outgoing(id) = stream_id {
                    self.opening_state = Some(OpeningState::Requested);
                    Poll::Ready(Ok(StreamEvent::Request(id)))
                } else {
                    Poll::Pending
                }
            }
            Some(OpeningState::Requested) => Poll::Pending,
            Some(OpeningState::Negotiated { io, need_report }) => {
                if *need_report {
                    *need_report = false;
                    return Poll::Ready(Ok(StreamEvent::Event(Event::StreamNegotiated {
                        stream_id,
                        menu: vec![],
                    })));
                }
                let (header, bytes) = match task::ready!(self.inner_state.poll(cx, io)) {
                    Err(err) => {
                        if err.kind() == io::ErrorKind::UnexpectedEof {
                            if let StreamId::Outgoing(id) = stream_id {
                                log::warn!("requesting again");
                                self.opening_state = Some(OpeningState::Requested);
                                return Poll::Ready(Ok(StreamEvent::Request(id)));
                            } else {
                                return Poll::Ready(Err(err));
                            }
                        } else {
                            return Poll::Ready(Err(err));
                        }
                    }
                    Ok(v) => v,
                };
                if let MessageHeader::Response(ResponseHeader { id: 0 }) = &header {
                    let mut bytes_slice = bytes.as_slice();
                    type P = ResponsePayload<<VersionedRpcMenuV1 as RpcMethod>::Response>;
                    let menu = P::binprot_read(&mut bytes_slice)
                        .ok()
                        .map(|r| r.0)
                        .transpose()
                        .map_err(|err| match &err {
                            Error::Unimplemented_rpc(tag, version) => {
                                log::error!("unimplemented RPC {tag}, {version}");
                                err
                            }
                            _ => err,
                        })
                        .ok()
                        .flatten()
                        .map(|NeedsLength(x)| x)
                        .into_iter()
                        .flatten()
                        .map(|(tag, version)| (tag.to_string_lossy(), version))
                        .collect();
                    return Poll::Ready(Ok(StreamEvent::Event(Event::StreamNegotiated {
                        stream_id,
                        menu,
                    })));
                }
                Poll::Ready(Ok(StreamEvent::Event(Event::Stream {
                    stream_id,
                    header,
                    bytes,
                })))
            }
        }
    }
}
