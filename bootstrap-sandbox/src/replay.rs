use std::{path::Path, fs::File};

use mina_p2p_messages::{
    rpc_kernel::QueryHeader,
    rpc_kernel::RpcMethod,
    rpc::{VersionedRpcMenuV1, GetBestTipV2},
};
use mina_rpc::{Engine, PeerContext};
use binprot::BinProtRead;

pub async fn run(swarm: libp2p::Swarm<mina_transport::Behaviour>, height: u32) {
    let path_main = AsRef::<Path>::as_ref("target/record");
    // let path_blocks = path_main.join("blocks");
    let path = path_main.join(height.to_string());

    let mut file = File::open(path_main.join("menu")).unwrap();
    let menu = <VersionedRpcMenuV1 as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <GetBestTipV2 as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut engine = Engine::new(swarm, Box::new(drop));

    let closure = |q: QueryHeader, ctx: &mut PeerContext| -> Vec<u8> {
        let tag = std::str::from_utf8(q.tag.as_ref()).unwrap();
        log::info!("handling {tag}, {}", q.version);
        match (tag, q.version) {
            (VersionedRpcMenuV1::NAME, VersionedRpcMenuV1::VERSION) => {
                ctx.make_response::<VersionedRpcMenuV1>(menu.clone(), q.id)
            }
            (GetBestTipV2::NAME, GetBestTipV2::VERSION) => {
                ctx.make_response::<GetBestTipV2>(best_tip.clone(), q.id)
            }
            (name, version) => {
                log::warn!("TODO: unhandled {name}, {version}");
                vec![]
            }
        }
    };

    engine.wait_for_request(closure).await.unwrap();
}
