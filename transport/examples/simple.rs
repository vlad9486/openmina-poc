use libp2p::swarm::SwarmEvent;
use mina_transport::futures::StreamExt;
use mina_rpc_behaviour::{Behaviour, Event, StreamId};

#[tokio::main]
async fn main() {
    env_logger::init();

    let local_key = mina_transport::generate_identity();
    let peers = [
        // "/ip4/135.181.217.23/tcp/30737/p2p/12D3KooWAVvZjW5m5LmhJrCUq2VtvG3drAsWxewMobgoUpewtqcp",
        // "/ip4/35.192.28.217/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs",
        "/ip4/34.170.114.52/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag",
        // "/ip4/34.123.4.144/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv",
    ]
    .into_iter()
    .map(|x| x.parse())
    .flatten();
    let chain_id = b"667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db";
    let listen_on = [
        "/ip6/::/tcp/8302".parse().unwrap(),
        "/ip4/0.0.0.0/tcp/8302".parse().unwrap(),
    ];
    let behaviour = Behaviour::default();

    let mut swarm = mina_transport::swarm(local_key, chain_id, listen_on, peers, behaviour);
    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::Behaviour((peer_id, Event::ConnectionEstablished)) => {
                log::info!("new connection {peer_id}");
                // send heartbeat for each new peer
                swarm.behaviour_mut().send(
                    peer_id,
                    StreamId::Outgoing(0),
                    b"\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec(),
                );

                use mina_p2p_messages::{
                    rpc_kernel::{Message, RpcMethod, Query, NeedsLength},
                    rpc::VersionedRpcMenuV1,
                };
                let msg = Message::<<VersionedRpcMenuV1 as RpcMethod>::Query>::Query(Query {
                    tag: VersionedRpcMenuV1::NAME.into(),
                    version: VersionedRpcMenuV1::VERSION,
                    id: 0,
                    data: NeedsLength(()),
                });
                let mut bytes = vec![0; 8];
                binprot::BinProtWrite::binprot_write(&msg, &mut bytes).unwrap();
                let len = (bytes.len() - 8) as u64;
                bytes[..8].clone_from_slice(&len.to_le_bytes());
                swarm
                    .behaviour_mut()
                    .send(peer_id, StreamId::Outgoing(0), bytes);
            }
            SwarmEvent::Behaviour((peer_id, Event::ConnectionClosed)) => {
                log::info!("connection closed {peer_id}");
            }
            SwarmEvent::Behaviour((peer_id, Event::OutboundNegotiated { stream_id })) => {
                log::info!("new stream {peer_id} {stream_id:?}");
            }
            SwarmEvent::Behaviour((peer_id, Event::Stream { stream_id, event })) => {
                log::info!(
                    "new msg from {peer_id}, stream {stream_id:?}, msg {}",
                    hex::encode(event)
                );
            }
            _ => {}
        }
    }
}
