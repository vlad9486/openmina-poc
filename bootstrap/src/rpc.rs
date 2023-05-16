use std::{
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{Ordering, AtomicI64},
    },
};

use binprot::{BinProtRead, BinProtWrite};
use mina_p2p_messages::{
    rpc, utils,
    rpc_kernel::{MessageHeader, Message, NeedsLength, Query, ResponsePayload},
    string::CharString,
    JSONifyPayloadRegistry,
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
    };
    (client.clone(), Stream { client, p2p_reader })
}

#[derive(Clone)]
pub struct Client {
    p2p_client: Arc<Mutex<p2p::Client>>,
    p2p_stream_id: u64,
    counter: Arc<AtomicI64>,
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

    pub fn get_best_tip(&self) {
        self.send_magic();
        let q = Query {
            tag: CharString::from("get_best_tip"),
            version: 2,
            id: self.counter.fetch_add(1, Ordering::SeqCst),
            data: NeedsLength(()),
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
    Query,
}

impl Iterator for Stream {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let _ = JSONifyPayloadRegistry::v2();
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
                    // TODO:
                    reader_limited.read_to_end(&mut vec![]).unwrap();
                    return Some(Event::Query);
                }
                Ok(MessageHeader::Response(v)) => {
                    if v.id == 4411474 {
                        // some magic rpc
                        reader_limited.read_to_end(&mut vec![]).unwrap();
                    } else {
                        log::info!("rpc id: {id}, response id: {}", v.id);
                        let msg = BinProtRead::binprot_read(&mut reader_limited).unwrap();
                        return Some(Event::BestTip(msg));
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
