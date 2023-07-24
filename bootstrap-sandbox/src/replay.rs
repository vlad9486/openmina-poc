use std::{path::Path, fs::File, collections::BTreeMap};

use mina_p2p_messages::{
    rpc_kernel::{QueryHeader, QueryPayload},
    rpc_kernel::{RpcMethod, RpcResult},
    rpc::{
        VersionedRpcMenuV1, GetBestTipV2, GetAncestryV2, AnswerSyncLedgerQueryV2,
        GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
    },
    v2::{LedgerHash, self},
};
use mina_rpc::{Engine, PeerContext};
use binprot::BinProtRead;

use crate::snarked_ledger::SnarkedLedger;

pub async fn run(swarm: libp2p::Swarm<mina_transport::Behaviour>, height: u32) {
    let path_main = AsRef::<Path>::as_ref("target/record");
    // let path_blocks = path_main.join("blocks");
    let path = path_main.join(height.to_string());

    let mut file = File::open(path_main.join("menu")).unwrap();
    let menu = <VersionedRpcMenuV1 as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <GetBestTipV2 as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut file = File::open(path.join("ancestry")).unwrap();
    let ancestry = <GetAncestryV2 as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut file = File::open(path.join("staged_ledger_aux")).unwrap();
    type T = GetStagedLedgerAuxAndPendingCoinbasesAtHashV2;
    let staged_ledger_aux = <T as RpcMethod>::Response::binprot_read(&mut file).unwrap();

    let mut ledgers = BTreeMap::new();

    let mut engine = Engine::new(swarm, Box::new(drop));

    let closure = |q: QueryHeader, ctx: &mut PeerContext| -> Vec<u8> {
        let tag = std::str::from_utf8(q.tag.as_ref()).unwrap();
        log::info!("handling {tag}, {}", q.version);
        match (tag, q.version) {
            (VersionedRpcMenuV1::NAME, VersionedRpcMenuV1::VERSION) => {
                let _ = ctx
                    .read_remaining::<QueryPayload<<VersionedRpcMenuV1 as RpcMethod>::Query>>()
                    .unwrap();

                ctx.make_response::<VersionedRpcMenuV1>(menu.clone(), q.id)
            }
            (GetBestTipV2::NAME, GetBestTipV2::VERSION) => {
                let _ = ctx
                    .read_remaining::<QueryPayload<<GetBestTipV2 as RpcMethod>::Query>>()
                    .unwrap();

                ctx.make_response::<GetBestTipV2>(best_tip.clone(), q.id)
            }
            (GetAncestryV2::NAME, GetAncestryV2::VERSION) => {
                let _ = ctx
                    .read_remaining::<QueryPayload<<GetAncestryV2 as RpcMethod>::Query>>()
                    .unwrap();

                ctx.make_response::<GetAncestryV2>(ancestry.clone(), q.id)
            }
            (AnswerSyncLedgerQueryV2::NAME, AnswerSyncLedgerQueryV2::VERSION) => {
                let (hash, query) = ctx
                    .read_remaining::<QueryPayload<<AnswerSyncLedgerQueryV2 as RpcMethod>::Query>>()
                    .unwrap()
                    .0;

                let hash = LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(hash));
                let hash_str = match serde_json::to_value(&hash).unwrap() {
                    serde_json::Value::String(s) => s,
                    _ => panic!(),
                };
                let file = File::open(path.join(hash_str)).unwrap();

                let ledger = ledgers
                    .entry(hash)
                    .or_insert_with(|| SnarkedLedger::load_bin(file).unwrap());
                let response = ledger.serve_query(query);

                ctx.make_response::<AnswerSyncLedgerQueryV2>(RpcResult(Ok(response)), q.id)
            }
            (
                GetStagedLedgerAuxAndPendingCoinbasesAtHashV2::NAME,
                GetStagedLedgerAuxAndPendingCoinbasesAtHashV2::VERSION,
            ) => {
                let _ = ctx
                    .read_remaining::<QueryPayload<
                        <GetStagedLedgerAuxAndPendingCoinbasesAtHashV2 as RpcMethod>::Query,
                    >>()
                    .unwrap();

                ctx.make_response::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>(
                    staged_ledger_aux.clone(),
                    q.id,
                )
            }
            (name, version) => {
                log::warn!("TODO: unhandled {name}, {version}");
                vec![]
            }
        }
    };

    engine.wait_for_request(closure).await.unwrap();
}
