#![forbid(unsafe_code)]

mod snarked_ledger;
mod bootstrap;

use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(long)]
    block: Option<String>,
    #[structopt(long)]
    again: bool,
}

#[tokio::main]
async fn main() {
    let Args { block, again } = Args::from_args();

    env_logger::init();

    if again {
        return bootstrap::again().await;
    }

    let swarm = {
        let local_key = mina_transport::generate_identity();
        let peers = [
            // "/ip4/135.181.217.23/tcp/30737/p2p/12D3KooWAVvZjW5m5LmhJrCUq2VtvG3drAsWxewMobgoUpewtqcp"
            //     .parse()
            //     .unwrap(),
            // "/ip4/35.192.28.217/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs"
            //     .parse()
            //     .unwrap(),
            "/ip4/34.170.114.52/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag"
                .parse()
                .unwrap(),
            // "/ip4/34.123.4.144/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv"
            //     .parse()
            //     .unwrap(),
            // "/dns4/seed-1.berkeley.o1test.net/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs".parse().unwrap(),
            // "/dns4/seed-2.berkeley.o1test.net/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag".parse().unwrap(),
            // "/dns4/seed-3.berkeley.o1test.net/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv".parse().unwrap(),
        ];
        // /dns4/seed-1.berkeley.o1test.net
        let listen_on = "/ip4/0.0.0.0/tcp/8302".parse().unwrap();
        let chain_id = b"667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db";
        mina_transport::swarm(local_key, chain_id, listen_on, peers)
    };

    bootstrap::run(swarm, block).await;
}
