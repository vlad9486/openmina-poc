#![forbid(unsafe_code)]

mod client;
mod snarked_ledger;
mod bootstrap;

mod record;
mod replay;

use std::{
    fs::File,
    io::{Write, Read},
};

use mina_rpc_behaviour::BehaviourBuilder;
use structopt::StructOpt;

#[derive(StructOpt)]
enum Command {
    Again {
        height: u32,
    },
    Record {
        #[structopt(long)]
        bootstrap: bool,
    },
    Replay {
        height: u32,
    },
}

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
    log::info!("{}", local_key.public().to_peer_id());

    let chain_id = b"667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db";
    let listen_on = [
        "/ip6/::/tcp/8302".parse().unwrap(),
        "/ip4/0.0.0.0/tcp/8302".parse().unwrap(),
    ];
    let peers = [
        // "/ip4/135.181.217.23/tcp/30737/p2p/12D3KooWAVvZjW5m5LmhJrCUq2VtvG3drAsWxewMobgoUpewtqcp",
        // "/ip4/35.192.28.217/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs",
        "/ip4/34.170.114.52/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag",
        // "/ip4/34.123.4.144/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv",
    ]
    .into_iter()
    .map(|x| x.parse())
    .flatten();
    // /dns4/seed-1.berkeley.o1test.net

    match Command::from_args() {
        Command::Again { height } => {
            bootstrap::again(height).await;
        }
        Command::Record { bootstrap } => {
            let behaviour = BehaviourBuilder::default().build();
            let swarm = mina_transport::swarm(local_key, chain_id, listen_on, peers, behaviour);

            record::run(swarm, bootstrap).await
        }
        Command::Replay { height } => {
            use mina_p2p_messages::rpc::{
                GetBestTipV2, GetAncestryV2, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
                AnswerSyncLedgerQueryV2, GetTransitionChainV2, GetTransitionChainProofV1ForV2,
            };

            let behaviour = BehaviourBuilder::default()
                .register_method::<GetBestTipV2>()
                .register_method::<GetAncestryV2>()
                .register_method::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>()
                .register_method::<AnswerSyncLedgerQueryV2>()
                .register_method::<GetTransitionChainV2>()
                .register_method::<GetTransitionChainProofV1ForV2>()
                .build();
            let swarm = mina_transport::swarm(local_key, chain_id, listen_on, [], behaviour);

            replay::run(swarm, height).await
        }
    }
}
