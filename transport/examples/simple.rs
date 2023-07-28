use std::{
    fs::File,
    io::{Read, Write},
};

use libp2p::swarm::SwarmEvent;
use mina_transport::futures::StreamExt;
use mina_rpc_behaviour::{Event, BehaviourBuilder};

#[tokio::main]
async fn main() {
    env_logger::init();

    let local_key = match File::open("target/identity") {
        Ok(mut file) => {
            let mut bytes = [0; 64];
            file.read_exact(&mut bytes).unwrap();
            mina_transport::ed25519::Keypair::try_from_bytes(&mut bytes)
                .unwrap()
                .into()
        }
        Err(_) => {
            let k = mina_transport::generate_identity();
            let bytes = k.clone().try_into_ed25519().unwrap().to_bytes();
            File::create("target/identity")
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            k
        }
    };

    let peers = [
        // "/ip4/135.181.217.23/tcp/30737/p2p/12D3KooWAVvZjW5m5LmhJrCUq2VtvG3drAsWxewMobgoUpewtqcp",
        // "/ip4/35.192.28.217/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs",
        "/ip4/34.170.114.52/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag",
        // "/ip4/34.123.4.144/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv",
    ]
    .into_iter()
    .map(|x| x.parse())
    // .filter(|_| false)
    .flatten();
    let chain_id = b"667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db";
    let listen_on = [
        "/ip6/::/tcp/8302".parse().unwrap(),
        "/ip4/0.0.0.0/tcp/8302".parse().unwrap(),
    ];
    let behaviour = BehaviourBuilder::default().build();

    let mut swarm = mina_transport::swarm(local_key, chain_id, listen_on, peers, behaviour);
    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::Behaviour((peer_id, Event::ConnectionEstablished)) => {
                log::info!("new connection {peer_id}");

                swarm.behaviour_mut().open(peer_id, 0);
            }
            SwarmEvent::Behaviour((peer_id, Event::ConnectionClosed)) => {
                log::info!("connection closed {peer_id}");
            }
            SwarmEvent::Behaviour((peer_id, Event::StreamNegotiated { stream_id, menu })) => {
                log::info!("new stream {peer_id} {stream_id:?} {menu:?}");
            }
            SwarmEvent::Behaviour((
                peer_id,
                Event::Stream {
                    stream_id,
                    header,
                    bytes,
                },
            )) => {
                log::info!(
                    "new msg from {peer_id}, stream {stream_id:?}, msg {header:?} {}",
                    hex::encode(bytes)
                );
            }
            _ => {}
        }
    }
}
