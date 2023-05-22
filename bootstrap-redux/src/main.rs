use libp2p_service::{OutputEvent, BehaviourEvent, gossipsub, rpc, futures::StreamExt};
use mina_p2p_messages::rpc_kernel as mina_rpc;
use binprot::BinProtWrite;

fn heartbeat() -> Vec<u8> {
    let msg = mina_rpc::Message::<()>::Heartbeat;
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    bytes
}

fn query<T: mina_rpc::RpcMethod>(id: i64, query: T::Query) -> Vec<u8> {
    let msg = mina_rpc::Message::Query(mina_rpc::Query {
        tag: T::NAME.into(),
        version: T::VERSION,
        id,
        data: mina_rpc::NeedsLength(query),
    });
    let mut bytes = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfd\x52\x50\x43\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 23) as u64;
    bytes[15..23].clone_from_slice(&len.to_le_bytes());
    bytes
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let local_key = libp2p_service::generate_identity();
    let peers = [
        // "/dns4/seed-1.berkeley.o1test.net/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs".parse().unwrap(),
        "/dns4/seed-2.berkeley.o1test.net/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag".parse().unwrap(),
        // "/dns4/seed-3.berkeley.o1test.net/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv".parse().unwrap(),
    ];
    let listen_on = "/ip4/0.0.0.0/tcp/8302".parse().unwrap();
    let chain_id = b"8c4908f1f873bd4e8a52aeb4981285a148914a51e61de6ac39180e61d0144771";

    let mut events = libp2p_service::run(local_key, chain_id, listen_on, peers);
    let mut request_sent = false;
    loop {
        match events.select_next_some().await {
            OutputEvent::ConnectionEstablished { peer_id, .. } => {
                log::debug!("established {peer_id}");
            }
            OutputEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id,
                ..
            })) => {
                log::debug!("gossipsub message from: {propagation_source}, id: {message_id}");
            }
            OutputEvent::Behaviour(BehaviourEvent::Rpc(rpc::Event::ConnectionEstablished {
                peer_id,
                connection_id,
            })) => {
                log::debug!("rpc stream {peer_id}, {connection_id:?}");

                if request_sent {
                    continue;
                }
                request_sent = true;
                events.behaviour_mut().rpc.send(
                    peer_id,
                    connection_id,
                    query::<mina_p2p_messages::rpc::GetBestTipV2>(0, ()),
                );
            }
            OutputEvent::Behaviour(BehaviourEvent::Rpc(rpc::Event::RecvMsg {
                peer_id,
                connection_id,
                bytes,
            })) => {
                events
                    .behaviour_mut()
                    .rpc
                    .send(peer_id, connection_id, heartbeat());
                log::info!("recv {peer_id}, {connection_id:?}, {}", hex::encode(bytes));
            }
            _ => {}
        }
    }
}
