mod service;
use self::service::Service;

mod machine;

use mina_transport::{OutputEvent, BehaviourEvent, gossipsub, rpc as rpc_transport};

fn transform_id(id: libp2p::swarm::ConnectionId) -> usize {
    format!("{id:?}")
        .split('(')
        .nth(1)
        .unwrap()
        .trim_end_matches(')')
        .parse()
        .unwrap()
}

fn main() {
    env_logger::init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(3)
        .build()
        .unwrap();
    let _rt_guard = rt.enter();
    let local_set = tokio::task::LocalSet::new();
    let _local_set_guard = local_set.enter();

    let swarm = {
        let local_key = mina_transport::generate_identity();
        let peers = [
            "/dns4/seed-1.berkeley.o1test.net/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs".parse().unwrap(),
            // "/dns4/seed-2.berkeley.o1test.net/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag".parse().unwrap(),
            // "/dns4/seed-3.berkeley.o1test.net/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv".parse().unwrap(),
        ];
        let listen_on = "/ip4/0.0.0.0/tcp/8302".parse().unwrap();
        let chain_id = b"8c4908f1f873bd4e8a52aeb4981285a148914a51e61de6ac39180e61d0144771";
        mina_transport::swarm(local_key, chain_id, listen_on, peers)
    };

    let (service, mut rx) = Service::spawn(swarm);
    let mut store = redux::Store::<_, _, machine::Action>::new(
        machine::State::reducer,
        machine::effects,
        service,
        redux::SystemTime::now(),
        machine::State::default(),
    );

    while let Some(event) = rx.blocking_recv() {
        match event {
            OutputEvent::ConnectionEstablished { peer_id, .. } => {
                store.dispatch(machine::Action::PeerConnectionEstablished { peer_id });
            }
            // TODO:
            OutputEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                ..
            })) => {
                store.dispatch(machine::Action::GossipMessage);
            }
            OutputEvent::Behaviour(BehaviourEvent::Rpc(
                rpc_transport::Event::ConnectionEstablished {
                    peer_id,
                    connection_id,
                },
            )) => {
                log::debug!("rpc stream {peer_id}, {connection_id:?}");
                store.dispatch(machine::Action::RpcNegotiated {
                    peer_id,
                    connection_id: transform_id(connection_id),
                });
            }
            OutputEvent::Behaviour(BehaviourEvent::Rpc(rpc_transport::Event::RecvMsg {
                peer_id,
                connection_id,
                bytes,
            })) => {
                store.dispatch(machine::Action::RpcMessage {
                    peer_id,
                    connection_id: transform_id(connection_id),
                    bytes,
                });
            }
            _ => {}
        }
    }
}
