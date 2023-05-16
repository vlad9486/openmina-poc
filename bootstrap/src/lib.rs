#![forbid(unsafe_code)]

mod rpc;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use libp2p_helper_ffi as p2p;
use mina_p2p_messages::{
    v2,
    rpc::{GetBestTipV2, AnswerSyncLedgerQueryV2},
};

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
            let (rpc_client, rpc_stream) = rpc::create(client, reader, stream_id);
            rpc_client.send_query::<GetBestTipV2>(());
            for event in rpc_stream {
                match event {
                    rpc::Event::BestTip(msg) => {
                        let msg = msg.0.unwrap().0.unwrap();
                        log::info!("{}", serde_json::to_string(&msg).unwrap());
                        let ledger_hash = msg
                            .data
                            .header
                            .protocol_state
                            .body
                            .consensus_state
                            .next_epoch_data
                            .ledger
                            .hash
                            .0
                            .clone();
                        // let ledger_hash = msg
                        //     .data
                        //     .header
                        //     .protocol_state
                        //     .body
                        //     .blockchain_state
                        //     .ledger_proof_statement
                        //     .0
                        //     .clone();
                        rpc_client.send_query::<AnswerSyncLedgerQueryV2>((
                            ledger_hash,
                            v2::MinaLedgerSyncLedgerQueryStableV1::NumAccounts,
                        ));
                    }
                    rpc::Event::SyncLedgerResponse(response) => {
                        let response = response.0.unwrap().0 .0.unwrap();
                        log::info!("RPC response {response:?}");
                    }
                    _ => (),
                }
            }
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
    let (_, rpc_stream) = rpc::create(client, reader, id);
    for msg in rpc_stream {
        log::info!("RPC message: {msg:?}");
    }
}
