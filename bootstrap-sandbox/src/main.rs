#![forbid(unsafe_code)]

mod snarked_ledger;
mod bootstrap;

mod record;
mod replay;

use std::{
    fs::File,
    io::{Write, Read},
};

use mina_transport::Behaviour;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(long)]
    block: Option<String>,
    #[structopt(long)]
    again: bool,
    #[structopt(long)]
    record: bool,
    #[structopt(long)]
    replay: Option<u32>,
}

#[tokio::main]
async fn main() {
    let Args {
        block,
        again,
        record,
        replay,
    } = Args::from_args();

    env_logger::init();

    if again {
        return bootstrap::again().await;
    }

    let swarm = {
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
        .filter(|_| replay.is_none())
        .map(|x| x.parse())
        .flatten();
        // /dns4/seed-1.berkeley.o1test.net
        log::info!("{}", local_key.public().to_peer_id());
        let chain_id = b"667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db";
        let listen_on = [
            "/ip6/::/tcp/8302".parse().unwrap(),
            "/ip4/0.0.0.0/tcp/8302".parse().unwrap(),
        ];
        let behaviour = Behaviour::new(local_key.clone()).unwrap();
        mina_transport::swarm(local_key, chain_id, listen_on, peers, behaviour)
    };

    if record {
        record::run(swarm).await;
    } else if let Some(height) = replay {
        replay::run(swarm, height).await;
    } else {
        bootstrap::run(swarm, block).await;
    }
}
