#![forbid(unsafe_code)]

mod rpc;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    io::Read,
};

use binprot::{BinProtWrite, BinProtRead};
use libp2p_helper_ffi::{RpcClient, PushReceiver, PushMessage, StreamReader};
use mina_p2p_messages::{
    utils, JSONifyPayloadRegistry,
    string::CharString,
    rpc_kernel::{Message, MessageHeader, Query, NeedsLength},
};

fn send_magic(rpc_client: &mut RpcClient, rpc_stream: u64) {
    let bytes = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfd\x52\x50\x43\x00\x01".to_vec();
    rpc_client.send_stream(rpc_stream, bytes).unwrap();
}

fn send<T: BinProtWrite>(rpc_client: &mut RpcClient, rpc_stream: u64, msg: Message<T>) {
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    log::info!("sending: {}", hex::encode(&bytes));
    rpc_client.send_stream(rpc_stream, bytes).unwrap();
}

pub fn run(rpc_client: Arc<Mutex<RpcClient>>, mut event_stream: PushReceiver) {
    let mut client_lock = rpc_client.lock().expect("poisoned");

    client_lock.list_peers().unwrap();

    thread::sleep(Duration::from_secs(1));
    let (rpc_stream, reader) = client_lock
        .open_stream(
            "12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv",
            "coda/rpcs/0.0.1",
        )
        .unwrap();
    log::info!(
        "Opened stream \"coda/rpcs/0.0.1\" with \"12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv\""
    );

    drop(client_lock);

    thread::spawn({
        let client = rpc_client.clone();
        move || {
            run_rpc_handler(client, reader, rpc_stream);
        }
    });

    while let Ok(msg) = event_stream.recv() {
        match msg {
            PushMessage::IncomingStream {
                stream_id, reader, ..
            } => {
                if stream_id == rpc_stream {
                    thread::spawn({
                        let client = rpc_client.clone();
                        move || {
                            run_rpc_handler(client, reader, stream_id);
                        }
                    });
                }
            }
            _ => {}
        }
    }
}

pub fn run_rpc_handler(client: Arc<Mutex<RpcClient>>, mut reader: StreamReader, id: u64) {
    let _ = JSONifyPayloadRegistry::v2();

    let mut init = false;

    while let Ok(length) = utils::stream_decode_size(&mut reader) {
        let mut reader_limited = (&mut reader).take(length as u64);
        match MessageHeader::binprot_read(&mut reader_limited) {
            Ok(MessageHeader::Heartbeat) => {
                log::info!("rpc headtbeat");
                let mut client = client.lock().unwrap();
                send(&mut *client, id, Message::<()>::Heartbeat);
            }
            Ok(MessageHeader::Query(q)) => {
                log::info!("rpc query {}", q.tag.to_string_lossy());
                reader_limited.read_to_end(&mut vec![]).unwrap();
            }
            Ok(MessageHeader::Response(v)) => {
                if v.id == 4411474 {
                    // some magic rpc
                    reader_limited.read_to_end(&mut vec![]).unwrap();
                    {
                        let mut client = client.lock().unwrap();
                        send_magic(&mut *client, id);
                    }

                    if !init {
                        init = true;
                        let mut client = client.lock().unwrap();
                        let q = Query {
                            tag: CharString::from("get_best_tip"),
                            version: 2,
                            id: 0,
                            data: NeedsLength(()),
                        };
                        send(&mut *client, id, Message::Query(q));
                    }
                } else {
                    log::info!("rpc response {}", v.id);
                    let _ = v;
                    reader_limited.read_to_end(&mut vec![]).unwrap();
                }
            }
            Err(err) => {
                log::error!("rpc handler {id}, err: {err}");
                break;
            }
        }
    }
}
