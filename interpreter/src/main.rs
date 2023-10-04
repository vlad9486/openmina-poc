mod command;
mod engine;

use std::env;

use structopt::StructOpt;
use mina_transport::ed25519::{SecretKey, Keypair};

#[derive(StructOpt)]
struct Args {
    #[structopt(long)]
    chain_id: String,
}

fn main() {
    env_logger::init();

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
    let local_key: libp2p::identity::Keypair = Keypair::from(sk).into();
    log::info!("{}", local_key.public().to_peer_id());

    let Args { chain_id } = Args::from_args();

    let swarm = {
        use libp2p_rpc_behaviour::BehaviourBuilder;

        let behaviour = BehaviourBuilder::default().build();
        mina_transport::swarm(local_key, chain_id.as_bytes(), [], [], behaviour)
    };

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .unwrap()
        .block_on(engine::run(swarm));
}
// 3c41383994b87449625df91769dff7b507825c064287d30fada9286f3f1cb15e
