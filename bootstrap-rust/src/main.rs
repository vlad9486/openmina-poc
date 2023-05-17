mod libp2p_service;
use libp2p_service::{OutputEvent, BehaviourEvent, futures::StreamExt};

#[tokio::main]
async fn main() {
    env_logger::init();

    let local_key = libp2p_service::generate_identity();
    let peers = [
        "/dns4/seed-1.berkeley.o1test.net/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs".parse().unwrap(),
        "/dns4/seed-2.berkeley.o1test.net/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag".parse().unwrap(),
        "/dns4/seed-3.berkeley.o1test.net/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv".parse().unwrap(),
    ];
    let listen_on = "/ip4/0.0.0.0/tcp/8302".parse().unwrap();
    let chain_id = b"8c4908f1f873bd4e8a52aeb4981285a148914a51e61de6ac39180e61d0144771";

    let mut events = libp2p_service::run(local_key, chain_id, listen_on, peers);
    loop {
        match events.select_next_some().await {
            OutputEvent::ConnectionEstablished { peer_id, .. } => {
                log::info!("{peer_id}");
            }
            OutputEvent::Behaviour(BehaviourEvent::Message {
                propagation_source,
                message_id,
                ..
            }) => {
                log::info!("gossipsub message from: {propagation_source}, id: {message_id}");
            }
            _ => {}
        }
    }
}
