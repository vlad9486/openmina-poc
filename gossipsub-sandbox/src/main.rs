use std::{env, path::PathBuf, fs::{File, self}, io::{Write, Read}};

use libp2p::{Multiaddr, gossipsub, futures::StreamExt, swarm::SwarmEvent};
use mina_transport::ed25519::SecretKey;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(long, default_value = "target/gossipsub")]
    path: PathBuf,
    #[structopt(long, default_value = "667b328bfc09ced12191d099f234575b006b6b193f5441a6fa744feacd9744db")]
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
    Record,
    Replay,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let Args { path, chain_id, listen, peer, cmd } = Args::from_args();

    let sk = env::var("OPENMINA_P2P_SEC_KEY")
        .map(|key| {
            let mut bytes = bs58::decode(key).with_check(Some(0x80)).into_vec().unwrap();
            SecretKey::from_bytes(&mut bytes[1..]).unwrap()
        })
        .unwrap_or_else(|_| {
            let mut bytes = rand::random::<[u8; 32]>();
            log::info!("{}", bs58::encode(&bytes).with_check_version(0x80).into_string());
            let sk = SecretKey::from_bytes(&mut bytes).unwrap();
            sk
        });

    let local_key: libp2p::identity::Keypair = mina_transport::ed25519::Keypair::from(sk).into();
    log::info!("{}", local_key.public().to_peer_id());

    let message_authenticity = gossipsub::MessageAuthenticity::Signed(local_key.clone());
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .max_transmit_size(1024 * 1024 * 32)
        .build()
        .expect("the config must be a valid constant");
    let mut behaviour = gossipsub::Behaviour::<gossipsub::IdentityTransform, gossipsub::subscription_filter::AllowAllSubscriptionFilter>::new(message_authenticity, gossipsub_config)
        .expect("strict validation mode must be compatible with this `message_authenticity`");
    let topic = gossipsub::IdentTopic::new("coda/consensus-messages/0.0.1");
    behaviour.subscribe(&topic).unwrap();

    let mut swarm = mina_transport::swarm(local_key, chain_id.as_bytes(), listen, peer, behaviour);

    match cmd {
        Command::Record => {
            fs::create_dir_all(&path).unwrap();
            let mut counter = 0;
            while let Some(event) = swarm.next().await {
                match event {
                    SwarmEvent::Behaviour(gossipsub::Event::Message { message, .. }) => {
                        // GossipNetMessageV2::SnarkPoolDiff
                        if message.data[8] == 1 {
                            let path = path.join(format!("{counter:08x}"));
                            File::create(path).unwrap().write_all(&message.data).unwrap();
                            log::info!("{counter}");
                            counter += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        Command::Replay => {
            let mut counter = 0;
            while let Ok(mut file) = File::open(path.join(format!("{counter:08x}"))) {
                let mut data = vec![];
                file.read_to_end(&mut data).unwrap();
                swarm.behaviour_mut().publish(topic.clone(), data).unwrap();
                counter += 1;
            }
        }
    }
}
