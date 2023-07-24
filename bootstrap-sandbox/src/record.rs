use std::{
    fs::{self, File},
    path::Path,
    collections::VecDeque,
    io,
};

use binprot::BinProtWrite;
use mina_p2p_messages::{
    rpc::{
        GetBestTipV2, VersionedRpcMenuV1, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
        GetTransitionChainV2, GetAncestryV2, WithHashV1,
    },
    v2,
};
use mina_rpc::Engine;

use crate::{snarked_ledger::SnarkedLedger, bootstrap::Storage};

pub async fn run(swarm: libp2p::Swarm<mina_transport::Behaviour>) {
    let path_main = AsRef::<Path>::as_ref("target/record");
    fs::create_dir_all(path_main).unwrap();

    let mut engine = Engine::new(swarm, Box::new(drop));

    let menu = engine.rpc::<VersionedRpcMenuV1>(()).await.unwrap().unwrap();
    let mut file = File::create(path_main.join("menu")).unwrap();
    menu.binprot_write(&mut file).unwrap();

    let best_tip = engine
        .rpc::<GetBestTipV2>(())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let head_height = best_tip
        .data
        .header
        .protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();

    log::info!("will record {head_height}");
    let path = path_main.join(head_height.to_string());
    fs::create_dir_all(&path).unwrap();

    let mut file = File::create(path.join("best_tip")).unwrap();
    Some(best_tip.clone()).binprot_write(&mut file).unwrap();

    let q = best_tip
        .data
        .header
        .protocol_state
        .body
        .consensus_state
        .clone();
    let hash = best_tip.data.header.protocol_state.hash().0.clone();
    let q = WithHashV1 { data: q, hash };
    let ancestry = engine
        .rpc::<GetAncestryV2>(q)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let mut file = File::create(path.join("ancestry")).unwrap();
    Some(ancestry.clone()).binprot_write(&mut file).unwrap();

    let snarked_protocol_state = best_tip.proof.1.header.protocol_state;

    let mut any_ledger = match File::open("target/ledger.bin") {
        Ok(file) => SnarkedLedger::load_bin(file).unwrap(),
        Err(_) => SnarkedLedger::empty(),
    };

    let snarked_ledger_hash = snarked_protocol_state
        .body
        .blockchain_state
        .ledger_proof_statement
        .target
        .first_pass_ledger
        .clone();
    let snarked_ledger_hash_str = match serde_json::to_value(&snarked_ledger_hash).unwrap() {
        serde_json::Value::String(s) => s,
        _ => panic!(),
    };
    log::info!("snarked_ledger_hash: {snarked_ledger_hash_str}");

    any_ledger.sync(&mut engine, &snarked_ledger_hash).await;
    any_ledger
        .store_bin(File::create(path.join(snarked_ledger_hash_str)).unwrap())
        .unwrap();
    any_ledger
        .store_bin(File::create("target/ledger.bin").unwrap())
        .unwrap();

    let expected_hash = snarked_protocol_state
        .body
        .blockchain_state
        .staged_ledger_hash
        .clone();

    let snarked_block_hash = snarked_protocol_state.hash();
    let snarked_block_hash = v2::StateHash::from(v2::DataHashLibStateHashStableV1(
        snarked_block_hash.inner().0.clone(),
    ));
    log::info!("downloading staged_ledger_aux and pending_coinbases at {snarked_block_hash}");
    let info = engine
        .rpc::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>(snarked_block_hash.0.clone())
        .await
        .unwrap()
        .unwrap();
    let mut file = File::create(path.join("staged_ledger_aux")).unwrap();
    info.clone().binprot_write(&mut file).unwrap();

    let mut storage = Storage::new(any_ledger.inner, info, expected_hash);

    let snarked_height = snarked_protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();
    log::info!("will bootstrap: {}..={head_height}", snarked_height);

    let mut blocks = VecDeque::new();
    blocks.push_back(best_tip.data);
    download_blocks(
        &mut engine,
        &mut blocks,
        &path_main.join("blocks"),
        head_height,
        snarked_height,
    )
    .await;

    let mut prev_protocol_state = snarked_protocol_state;
    while let Some(block) = blocks.pop_back() {
        storage.apply_block(&block, &prev_protocol_state);
        prev_protocol_state = block.header.protocol_state.clone();
    }
}

async fn download_blocks(
    engine: &mut Engine,
    blocks: &mut VecDeque<v2::MinaBlockBlockStableV2>,
    dir: &Path,
    head_height: u32,
    snarked_height: u32,
) {
    let create_dir = |dir: &Path| {
        fs::create_dir_all(dir)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .unwrap()
    };
    create_dir(dir);

    log::info!("need blocks {}..{head_height}", snarked_height + 1);
    for i in ((snarked_height + 1)..head_height).rev() {
        let last_protocol_state = &blocks.back().unwrap().header.protocol_state;
        let this_hash = &last_protocol_state.previous_state_hash;
        let this_height = last_protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32()
            - 1;
        let dir = dir.join(this_height.to_string());
        create_dir(&dir);
        let new = if let Ok(mut file) = File::open(dir.join(this_hash.to_string())) {
            <Option<_> as binprot::BinProtRead>::binprot_read(&mut file)
                .unwrap()
                .unwrap()
        } else {
            log::info!("downloading block {i}");
            let new = engine
                .rpc::<GetTransitionChainV2>(vec![this_hash.0.clone()])
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let mut file = File::create(dir.join(this_hash.to_string())).unwrap();
            Some(new[0].clone()).binprot_write(&mut file).unwrap();
            new[0].clone()
        };
        blocks.push_back(new);
    }
    log::info!("have blocks {}..{head_height}", snarked_height + 1);
}
