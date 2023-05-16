#![forbid(unsafe_code)]

mod rpc;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use libp2p_helper_ffi as p2p;

pub fn run(rpc_client: Arc<Mutex<p2p::Client>>, mut event_stream: p2p::Stream) {
    let mut client_lock = rpc_client.lock().expect("poisoned");

    client_lock.list_peers().unwrap();

    thread::sleep(Duration::from_secs(1));
    let (stream_id, reader) = client_lock
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
            run_rpc_handler(client, reader, stream_id);
        }
    });

    while let Ok(msg) = event_stream.recv() {
        match msg {
            p2p::Message::Terminate => break,
            p2p::Message::IncomingStream {
                stream_id,
                reader,
                protocol,
                ..
            } => {
                if protocol == "coda/rpcs/0.0.1" {
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

pub fn run_rpc_handler(client: Arc<Mutex<p2p::Client>>, reader: p2p::StreamReader, id: u64) {
    let (rpc_client, rpc_stream) = rpc::create(client, reader, id);
    rpc_client.get_best_tip();
    for msg in rpc_stream {
        log::info!("RPC message: {msg:?}");
    }
}
