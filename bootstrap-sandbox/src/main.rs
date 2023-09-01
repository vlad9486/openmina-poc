#![forbid(unsafe_code)]

mod client;
mod snarked_ledger;
mod bootstrap;
mod check;
mod archive_block;

mod record;
mod replay;

use std::{env, path::PathBuf};

use libp2p::Multiaddr;
use libp2p_rpc_behaviour::BehaviourBuilder;
use structopt::StructOpt;
use mina_transport::ed25519::SecretKey;

#[derive(StructOpt)]
struct Args {
    #[structopt(long, default_value = "target/default")]
    path: PathBuf,
    #[structopt(
        long,
        default_value = "667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db"
    )]
    chain_id: String,
    #[structopt(long)]
    listen: Vec<Multiaddr>,
    #[structopt(long)]
    peer: Vec<Multiaddr>,
    #[structopt(subcommand)]
    cmd: Command,
}

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
    Empty,
    Test {
        height: u32,
        url: String,
    },
    TestGraphql {
        height: u32,
        url: String,
        #[structopt(long)]
        verbose: bool,
    },
    Archive {
        state: String,
    },
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let Args {
        path,
        chain_id,
        listen,
        peer,
        cmd,
    } = Args::from_args();

    let sk = env::var("OPENMINA_P2P_SEC_KEY")
        .map(|key| {
            let mut bytes = bs58::decode(key).with_check(Some(0x80)).into_vec().unwrap();
            SecretKey::from_bytes(&mut bytes[1..]).unwrap()
        })
        .unwrap_or_else(|_| {
            let mut bytes = rand::random::<[u8; 32]>();
            log::info!(
                "{}",
                bs58::encode(&bytes).with_check_version(0x80).into_string()
            );
            let sk = SecretKey::from_bytes(&mut bytes).unwrap();
            sk
        });

    let local_key: libp2p::identity::Keypair = mina_transport::ed25519::Keypair::from(sk).into();
    log::info!("{}", local_key.public().to_peer_id());

    // let listen_on = [
    //     "/ip6/::/tcp/8302".parse().unwrap(),
    //     "/ip4/0.0.0.0/tcp/8302".parse().unwrap(),
    // ];
    // let peers = [
    //     "/ip4/35.192.28.217/tcp/10000/p2p/12D3KooWAdgYL6hv18M3iDBdaK1dRygPivSfAfBNDzie6YqydVbs",
    //     "/ip4/34.170.114.52/tcp/10001/p2p/12D3KooWLjs54xHzVmMmGYb7W5RVibqbwD1co7M2ZMfPgPm7iAag",
    //     "/ip4/34.123.4.144/tcp/10002/p2p/12D3KooWEiGVAFC7curXWXiGZyMWnZK9h8BKr88U8D5PKV3dXciv",
    //     "/ip4/127.0.0.1/tcp/8303/p2p/12D3KooWP9mwZjBdyrr2rDMKWDMo2vdLpajDobXLeyYaQtJUg8NT",
    // ]
    // .into_iter()
    // .map(|x| x.parse())
    // .flatten();

    match cmd {
        Command::Again { height } => {
            bootstrap::again(&path, height).await;
        }
        Command::Record { bootstrap } => {
            let behaviour = BehaviourBuilder::default().build();
            let swarm =
                mina_transport::swarm(local_key, chain_id.as_bytes(), listen, peer, behaviour);

            record::run(swarm, &path, bootstrap).await
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
            let swarm =
                mina_transport::swarm(local_key, chain_id.as_bytes(), listen, [], behaviour);

            replay::run(swarm, &path, height).await
        }
        Command::Empty => {
            use libp2p::{futures::StreamExt, swarm::SwarmEvent};
            use libp2p_rpc_behaviour::{Event, Received};
            use mina_p2p_messages::{rpc::GetBestTipV2, rpc_kernel::RpcMethod};

            let behaviour = BehaviourBuilder::default()
                .register_method::<GetBestTipV2>()
                .build();
            let mut swarm =
                mina_transport::swarm(local_key, chain_id.as_bytes(), listen, peer, behaviour);
            while let Some(event) = swarm.next().await {
                match event {
                    SwarmEvent::Behaviour((peer_id, event)) => match event {
                        Event::Stream {
                            stream_id,
                            received: Received::Query { header, .. },
                        } => match (header.tag.to_string_lossy().as_str(), header.version) {
                            (GetBestTipV2::NAME, GetBestTipV2::VERSION) => {
                                swarm
                                    .behaviour_mut()
                                    .respond::<GetBestTipV2>(
                                        peer_id,
                                        stream_id,
                                        header.id,
                                        Ok(None),
                                    )
                                    .unwrap();
                            }
                            _ => {}
                        },
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        Command::Test { height, url } => {
            check::test(&path, height, url);
        }
        Command::TestGraphql {
            height,
            url,
            verbose,
        } => {
            check::test_graphql(&path, height, url, verbose);
        }
        Command::Archive { state } => archive_block::store(&path, state.parse().unwrap()),
    }
}
