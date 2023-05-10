#![forbid(unsafe_code)]

mod rpc;

use libp2p_helper_ffi::{RpcClient, PushReceiver, PushMessage};

pub fn run(mut rpc_client: RpcClient, event_stream: PushReceiver) {
    rpc_client.list_peers().unwrap();

    let rpc_stream = rpc_client
        .open_stream(
            "12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag",
            "coda/rpcs/0.0.1",
        )
        .unwrap();
    log::info!(
        "Opened stream \"coda/rpcs/0.0.1\" with 12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag"
    );

    while let Ok(msg) = event_stream.recv() {
        use std::io::Cursor;
        use mina_p2p_messages::{utils, JSONifyPayloadRegistry, rpc_kernel::MessageHeader};
        use binprot::BinProtRead;

        log::info!("Push {msg:?}");
        match msg {
            PushMessage::StreamMessage { data, stream_id } => {
                if stream_id == rpc_stream {
                    let mut s = Cursor::new(&data);
                    let _ = utils::stream_decode_size(&mut s).unwrap();
                    match MessageHeader::binprot_read(&mut s) {
                        Ok(MessageHeader::Heartbeat) => {
                            log::info!("rpc headtbeat");
                        }
                        Ok(MessageHeader::Query(q)) => {
                            log::info!("rpc query {}", q.tag.to_string_lossy());
                            let _ = JSONifyPayloadRegistry::v2();
                        }
                        Ok(MessageHeader::Response(v)) => {
                            if v.id == 4411474 {
                                // some magic rpc, send it back
                                rpc_client.send_stream(stream_id, data).unwrap();
                            } else {
                                log::info!("rpc response {}", v.id);
                                let _ = v;
                            }
                        }
                        Err(err) => {
                            log::error!("rpc message error: {err}");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
