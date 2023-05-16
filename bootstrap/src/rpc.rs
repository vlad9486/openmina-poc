use std::{
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{Ordering, AtomicI64},
    },
    collections::BTreeMap,
};

use binprot::{BinProtRead, BinProtWrite};
use mina_p2p_messages::{
    rpc::{self, AnswerSyncLedgerQueryV2},
    utils,
    rpc_kernel::{MessageHeader, Message, NeedsLength, Query, ResponsePayload, RpcMethod},
};

use crate::p2p;

pub fn create(
    p2p_client: Arc<Mutex<p2p::Client>>,
    p2p_reader: p2p::StreamReader,
    p2p_stream_id: u64,
) -> (Client, Stream) {
    let client = Client {
        p2p_client: p2p_client.clone(),
        p2p_stream_id,
        counter: Arc::new(AtomicI64::new(0)),
        pending: Arc::new(Mutex::new(BTreeMap::default())),
    };
    (client.clone(), Stream { client, p2p_reader })
}

#[derive(Clone)]
pub struct Client {
    p2p_client: Arc<Mutex<p2p::Client>>,
    p2p_stream_id: u64,
    counter: Arc<AtomicI64>,
    pending: Arc<Mutex<BTreeMap<i64, (i32, Vec<u8>)>>>,
}

impl Client {
    fn send_magic(&self) {
        let bytes = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfd\x52\x50\x43\x00\x01".to_vec();
        let mut lock = self.p2p_client.lock().expect("poisoned");
        lock.send_stream(self.p2p_stream_id, bytes).unwrap();
    }

    fn send_msg<T: BinProtWrite>(&self, msg: Message<T>) {
        let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        msg.binprot_write(&mut bytes).unwrap();
        let len = (bytes.len() - 8) as u64;
        bytes[0..8].clone_from_slice(&len.to_le_bytes());
        let mut lock = self.p2p_client.lock().expect("poisoned");
        lock.send_stream(self.p2p_stream_id, bytes).unwrap();
    }

    pub fn send_query<M: RpcMethod>(&self, request: M::Query) {
        let id: i64 = self.counter.fetch_add(1, Ordering::SeqCst);
        let version = M::VERSION;
        let tag = M::NAME.as_bytes().to_vec();
        self.pending
            .lock()
            .expect("poisoned")
            .insert(id, (version, tag.clone()));
        self.send_magic();
        let q = Query {
            tag: tag.into(),
            version,
            id,
            data: NeedsLength(request),
        };
        self.send_msg(Message::Query(q));
    }
}

pub struct Stream {
    client: Client,
    p2p_reader: p2p::StreamReader,
}

#[derive(Debug)]
pub enum Event {
    BestTip(ResponsePayload<rpc::GetBestTipV2Response>),
    SyncLedgerResponse(ResponsePayload<<AnswerSyncLedgerQueryV2 as RpcMethod>::Response>),
    GetBestTip,
    UnrecognizedQuery,
    UnrecognizedResponse,
}

impl Iterator for Stream {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.client.p2p_stream_id;

        while let Ok(length) = utils::stream_decode_size(&mut self.p2p_reader) {
            let mut reader_limited = (&mut self.p2p_reader).take(length as u64);
            match MessageHeader::binprot_read(&mut reader_limited) {
                Ok(MessageHeader::Heartbeat) => {
                    log::debug!("rpc id: {id}, heartbeat");
                    self.client.send_msg(Message::<()>::Heartbeat);
                }
                Ok(MessageHeader::Query(q)) => {
                    log::info!("rpc id: {id}, query {}", q.tag.to_string_lossy());
                    match q.tag.as_ref() {
                        // TODO: more cases
                        b"get_best_tip" => return Some(Event::GetBestTip),
                        _ => {
                            reader_limited.read_to_end(&mut vec![]).unwrap();
                            return Some(Event::UnrecognizedQuery);
                        }
                    }
                }
                Ok(MessageHeader::Response(v)) => {
                    if v.id == 4411474 {
                        // some magic rpc
                        reader_limited.read_to_end(&mut vec![]).unwrap();
                    } else if let Some((version, tag)) =
                        self.client.pending.lock().expect("poisoned").remove(&v.id)
                    {
                        log::debug!("rpc id: {id}, response id: {}, version: {}", v.id, version);
                        match (version, tag.as_slice()) {
                            // TODO: more cases
                            (2, b"get_best_tip") => {
                                let msg = BinProtRead::binprot_read(&mut reader_limited).unwrap();
                                return Some(Event::BestTip(msg));
                            }
                            (2, b"answer_sync_ledger_query") => {
                                let msg = BinProtRead::binprot_read(&mut reader_limited).unwrap();
                                return Some(Event::SyncLedgerResponse(msg));
                            }
                            _ => {
                                reader_limited.read_to_end(&mut vec![]).unwrap();
                                return Some(Event::UnrecognizedResponse);
                            }
                        }
                    }
                }
                Err(err) => {
                    log::error!("rpc id: {id}, err: {err}");
                    return None;
                }
            }
        }

        None
    }
}
