use std::collections::BTreeMap;

use libp2p::PeerId;
use mina_p2p_messages::rpc::{
    GetBestTipV2, AnswerSyncLedgerQueryV2, GetTransitionChainProofV1ForV2, GetTransitionChainV2,
    GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
};
use serde::{Serialize, Deserialize};

use super::{Message, Request, Response};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub outgoing_best_tip: bool,
    pub outgoing_staged_ledger: bool,
    pub outgoing: BTreeMap<(PeerId, usize), Outgoing>,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct Outgoing {
    pub pending: BTreeMap<i64, (i32, Vec<u8>)>,
    pub last_id: i64,
    pub accumulator: Accumulator,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Accumulator {
    Length { have: usize, buffer: [u8; 8] },
    Chunk { length: usize, buffer: Vec<u8> },
}

impl Default for Accumulator {
    fn default() -> Self {
        Self::Length {
            have: 0,
            buffer: [0; 8],
        }
    }
}

impl Accumulator {
    fn put_slice(&mut self, slice: &[u8]) {
        match self {
            Accumulator::Length { have, buffer } => {
                let end = slice.len() + *have;
                if end < 8 {
                    buffer[*have..end].clone_from_slice(slice);
                    *have = end;
                } else {
                    buffer[*have..].clone_from_slice(&slice[..(8 - *have)]);
                    let length = u64::from_le_bytes(*buffer) as usize;
                    let buffer = slice[(8 - *have)..].to_vec();
                    *self = Self::Chunk { length, buffer };
                }
            }
            Accumulator::Chunk { buffer, .. } => {
                buffer.extend_from_slice(slice);
            }
        }
    }
}

impl Outgoing {
    pub fn register<M: mina_p2p_messages::rpc_kernel::RpcMethod>(&mut self) {
        self.pending
            .insert(self.last_id, (M::VERSION, M::NAME.as_bytes().to_vec()));
        self.last_id += 1;
    }

    pub fn put_slice(&mut self, slice: &[u8]) {
        self.accumulator.put_slice(slice);
    }
}

impl Iterator for Outgoing {
    type Item = Result<Message, binprot::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        use binprot::BinProtRead;
        use mina_p2p_messages::rpc_kernel::{MessageHeader, RpcMethod, QueryHeader, ResponseHeader};

        fn read_message(
            mut s: &[u8],
            o: &mut BTreeMap<i64, (i32, Vec<u8>)>,
        ) -> Result<Message, binprot::Error> {
            match MessageHeader::binprot_read(&mut s)? {
                MessageHeader::Heartbeat => Ok(Message::Heartbeat),
                MessageHeader::Query(QueryHeader { id, version, tag }) => {
                    match (version, std::str::from_utf8(tag.as_ref()).unwrap()) {
                        (GetBestTipV2::VERSION, GetBestTipV2::NAME) => {
                            let body = Request::BestTip(BinProtRead::binprot_read(&mut s)?);
                            Ok(Message::Request { id, body })
                        }
                        (v, t) => {
                            let err = adhocerr::err!("unknown version: {}, tag: {}", v, t);
                            Err(binprot::Error::CustomError(Box::new(err)))
                        }
                    }
                }
                MessageHeader::Response(ResponseHeader { id }) => {
                    if let Some((version, tag)) = o.remove(&id) {
                        match (version, std::str::from_utf8(tag.as_ref()).unwrap()) {
                            (GetBestTipV2::VERSION, GetBestTipV2::NAME) => {
                                let body = Response::BestTip(BinProtRead::binprot_read(&mut s)?);
                                Ok(Message::Response { id, body })
                            }
                            (
                                GetStagedLedgerAuxAndPendingCoinbasesAtHashV2::VERSION,
                                GetStagedLedgerAuxAndPendingCoinbasesAtHashV2::NAME,
                            ) => {
                                let body = Response::StagedLedgerAuxAndPendingCoinbasesAtHash(
                                    BinProtRead::binprot_read(&mut s)?,
                                );
                                Ok(Message::Response { id, body })
                            }
                            (AnswerSyncLedgerQueryV2::VERSION, AnswerSyncLedgerQueryV2::NAME) => {
                                let body = Response::SyncLedger(BinProtRead::binprot_read(&mut s)?);
                                Ok(Message::Response { id, body })
                            }
                            (
                                GetTransitionChainProofV1ForV2::VERSION,
                                GetTransitionChainProofV1ForV2::NAME,
                            ) => {
                                let body = Response::GetTransitionChainProof(
                                    BinProtRead::binprot_read(&mut s)?,
                                );
                                Ok(Message::Response { id, body })
                            }
                            (GetTransitionChainV2::VERSION, GetTransitionChainV2::NAME) => {
                                let body = Response::GetTransitionChain(BinProtRead::binprot_read(
                                    &mut s,
                                )?);
                                Ok(Message::Response { id, body })
                            }
                            (v, t) => {
                                let err = adhocerr::err!("unknown version: {}, tag: {}", v, t);
                                Err(binprot::Error::CustomError(Box::new(err)))
                            }
                        }
                    } else if id == i64::from_le_bytes(*b"RPC\x00\x00\x00\x00\x00") {
                        Ok(Message::Magic)
                    } else {
                        let err = adhocerr::err!("unknown request: {}", id);
                        Err(binprot::Error::CustomError(Box::new(err)))
                    }
                }
            }
        }

        match &mut self.accumulator {
            Accumulator::Length { .. } => None,
            Accumulator::Chunk { length, buffer } => {
                if buffer.len() < *length {
                    None
                } else {
                    let x = match read_message(&*buffer, &mut self.pending) {
                        Err(err) => return Some(Err(err)),
                        Ok(v) => v,
                    };
                    let mut new = Accumulator::default();
                    new.put_slice(&buffer[*length..]);
                    self.accumulator = new;

                    log::info!("Incoming message {}", x);
                    Some(Ok(x))
                }
            }
        }
    }
}
