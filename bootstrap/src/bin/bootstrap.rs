use std::{fs, env, path::PathBuf};

use structopt::StructOpt;
use reqwest::Url;

use libp2p_helper_ffi::{Process, Config};

#[derive(StructOpt)]
pub struct Args {
    #[structopt(long)]
    root: Option<PathBuf>,
    #[structopt(long)]
    port: u16,
    #[structopt(long)]
    peer_list_url: Url,
    #[structopt(long)]
    external_multiaddr: Option<String>,
}

fn main() {
    env_logger::init();

    let Args {
        root,
        port,
        peer_list_url,
        external_multiaddr,
    } = Args::from_args();

    let root = root.unwrap_or_else(|| {
        let home = env::var("HOME").unwrap_or("/root".to_owned());
        PathBuf::from(home).join(".mina")
    });

    fs::create_dir_all(&root).unwrap();

    let (process, event_stream, mut rpc_client) = Process::spawn("coda-libp2p_helper");
    let mut process = Some(process);

    if let Err(err) = ctrlc::set_handler(move || {
        if let Some(process) = process.take() {
            log::info!("Received ctrlc, terinating...");
            process.stop_receiving();
            process.stop().unwrap();
        }
    }) {
        log::error!("failed to set ctrlc handler {err}");
        return;
    }

    let (peer_id, _public, secret_key) = rpc_client.generate_keypair().unwrap();
    log::info!("Generated identity: {peer_id}");

    let peers = reqwest::blocking::get(peer_list_url)
        .unwrap()
        .text()
        .unwrap();
    let peers = peers.split_ascii_whitespace().collect::<Vec<_>>();

    let listen_on = format!("/ip4/0.0.0.0/tcp/{port}");
    let external_multiaddr = external_multiaddr.unwrap_or(listen_on.clone());
    let topic = "coda/consensus-messages/0.0.1";

    let config = Config::new(
        &root.display().to_string(),
        &secret_key,
        "8c4908f1f873bd4e8a52aeb4981285a148914a51e61de6ac39180e61d0144771",
        &[&listen_on],
        &external_multiaddr,
        &peers,
        &[&[topic]],
    );

    rpc_client.configure(config).unwrap();
    log::info!("Configured libp2p");

    rpc_client.subscribe(0, topic.to_owned()).unwrap();
    log::info!("Subscribed for \"{topic}\"");

    bootstrap::run(rpc_client, event_stream);
}
