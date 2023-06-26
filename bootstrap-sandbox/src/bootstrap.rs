use std::{
    collections::{BTreeMap, VecDeque, BTreeSet},
    time::Duration,
    fs::File,
    path::Path,
};

use tokio::sync::mpsc;

use binprot::{BinProtWrite, BinProtRead};
use mina_p2p_messages::{
    rpc::{
        GetBestTipV2, AnswerSyncLedgerQueryV2, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
        GetTransitionChainV2, VersionedRpcMenuV1,
        GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response, ProofCarryingDataStableV1,
    },
    v2,
    hash::MinaHash,
};
use mina_rpc::Engine;
use mina_tree::{
    mask::Mask,
    staged_ledger::{staged_ledger::StagedLedger, diff::Diff},
    verifier::Verifier,
    scan_state::{
        scan_state::ConstraintConstants,
        currency::{Amount, Fee},
        transaction_logic::{local_state::LocalState, protocol_state},
        self,
    },
};
use mina_signer::CompressedPubKey;

use super::snarked_ledger::SnarkedLedger;

const CONSTRAINT_CONSTANTS: ConstraintConstants = ConstraintConstants {
    sub_windows_per_window: 11,
    ledger_depth: 35,
    work_delay: 2,
    block_window_duration_ms: 180000,
    transaction_capacity_log_2: 7,
    pending_coinbase_depth: 5,
    coinbase_amount: Amount::from_u64(720000000000),
    supercharged_coinbase_factor: 2,
    account_creation_fee: Fee::from_u64(1000000000),
    fork: None,
};

pub async fn again() {
    let mut best_tip_file = File::open("target/last_best_tip.bin").unwrap();
    let best_tip = <ProofCarryingDataStableV1<
        v2::MinaBlockBlockStableV2,
        (
            Vec<v2::MinaBaseStateBodyHashStableV1>,
            v2::MinaBlockBlockStableV2,
        ),
    > as BinProtRead>::binprot_read(&mut best_tip_file)
    .unwrap();

    let head = best_tip.data;
    let last_protocol_state = best_tip.proof.1.header.protocol_state;
    let last_protocol_state_hash = last_protocol_state.hash();

    let snarked_ledger = match File::open("target/ledger.bin") {
        Ok(file) => SnarkedLedger::load_bin(file).unwrap(),
        Err(_) => SnarkedLedger::empty(),
    };

    let mut last = head.header.protocol_state.previous_state_hash.clone();
    let mut blocks = vec![];
    blocks.push(head);
    let dir = AsRef::<Path>::as_ref("target/blocks");
    while last.0 != last_protocol_state_hash.into() {
        let mut file = File::open(dir.join(last.to_string())).unwrap();
        let new = v2::MinaBlockBlockStableV2::binprot_read(&mut file).unwrap();
        last = new.header.protocol_state.previous_state_hash.clone();
        blocks.push(new);
    }

    let mut file = File::open("target/last_staged_ledger_aux.bin").unwrap();
    let info =
        GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response::binprot_read(&mut file).unwrap();

    let expected_hash = last_protocol_state
        .body
        .blockchain_state
        .staged_ledger_hash
        .clone();
    let mut storage = Storage::new(snarked_ledger.inner, info, expected_hash);
    let mut last_protocol_state = last_protocol_state;
    while let Some(block) = blocks.pop() {
        storage.apply_block(&block, &last_protocol_state);
        last_protocol_state = block.header.protocol_state.clone();
    }
}

pub async fn run(swarm: libp2p::Swarm<mina_transport::Behaviour>, block: Option<String>) {
    let (block_sender, mut block_receiver) = mpsc::unbounded_channel();
    let mut engine = Engine::new(
        swarm,
        Box::new(move |block| block_sender.send(block).unwrap()),
    );

    let _menu = engine.rpc::<VersionedRpcMenuV1>(()).await.unwrap().unwrap();

    let best_tip = loop {
        match engine.rpc::<GetBestTipV2>(()).await.unwrap().unwrap() {
            Some(v) => break v,
            None => {
                let retry = Duration::from_secs(30);
                log::warn!("best tip is nil, retry in {retry:?}");
                tokio::time::sleep(retry).await;
            }
        }
    };
    let mut file = File::create("target/last_best_tip.bin").unwrap();
    best_tip.binprot_write(&mut file).unwrap();

    let head = best_tip.data;
    let head_height = head
        .header
        .protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();

    let snarked_protocol_state = match block {
        None => best_tip.proof.1.header.protocol_state,
        Some(block) => {
            let value = serde_json::Value::String(block);
            let block = serde_json::from_value::<v2::StateHash>(value).unwrap();
            let blocks = engine
                .rpc::<GetTransitionChainV2>(vec![block.0.clone()])
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            blocks.first().unwrap().header.protocol_state.clone()
        }
    };
    let snarked_height = snarked_protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();
    log::info!("will bootstrap: {}..={head_height}", snarked_height);

    let snarked_ledger_hash = snarked_protocol_state
        .body
        .blockchain_state
        .ledger_proof_statement
        .target
        .first_pass_ledger
        .clone();
    let snarked_block_hash = snarked_protocol_state.hash();

    log::info!(
        "snarked_ledger_hash: {}",
        serde_json::to_string(&snarked_ledger_hash).unwrap()
    );

    let q = v2::MinaLedgerSyncLedgerQueryStableV1::NumAccounts;
    let r = engine
        .rpc::<AnswerSyncLedgerQueryV2>((snarked_ledger_hash.0.clone(), q))
        .await
        .unwrap()
        .unwrap()
        .0
        .unwrap();
    let (_num, hash) = match r {
        v2::MinaLedgerSyncLedgerAnswerStableV2::NumAccounts(num, hash) => (num.0, hash),
        _ => panic!(),
    };

    let mut snarked_ledger = match File::open("target/ledger.bin") {
        Ok(file) => SnarkedLedger::load_bin(file).unwrap(),
        Err(_) => SnarkedLedger::empty(),
    };
    snarked_ledger
        .sync(&mut engine, &snarked_ledger_hash, hash)
        .await;
    snarked_ledger
        .store_bin(File::create("target/ledger.bin").unwrap())
        .unwrap();

    let expected_hash = snarked_protocol_state
        .body
        .blockchain_state
        .staged_ledger_hash
        .clone();

    let snarked_block_hash = v2::StateHash::from(v2::DataHashLibStateHashStableV1(
        snarked_block_hash.clone().into(),
    ));
    log::info!("downloading staged_ledger_aux and pending_coinbases at {snarked_block_hash}");
    let info = engine
        .rpc::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>(snarked_block_hash.0.clone())
        .await
        .unwrap()
        .unwrap();
    let mut file = File::create("target/last_staged_ledger_aux.bin").unwrap();
    info.binprot_write(&mut file).unwrap();

    let mut storage = Storage::new(snarked_ledger.inner, info, expected_hash);

    let mut blocks = VecDeque::new();
    blocks.push_back(head);
    download_blocks(&mut engine, &mut blocks, head_height, snarked_height).await;

    let mut prev_protocol_state = snarked_protocol_state;
    while let Some(block) = blocks.pop_back() {
        storage.apply_block(&block, &prev_protocol_state);
        prev_protocol_state = block.header.protocol_state.clone();
    }
    let last_height = prev_protocol_state
        .body
        .consensus_state
        .blockchain_length
        .as_u32();

    tokio::spawn(engine.wait_infinite());

    let mut new_blocks = BTreeSet::new();
    while let Some(block) = block_receiver.recv().await {
        let height = block
            .header
            .protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        if !new_blocks.insert(height) {
            continue;
        }
        if last_height + 1 > height {
            log::warn!("skip already applied {height}");
            continue;
        }
        storage.apply_block(&block, &prev_protocol_state);
        prev_protocol_state = block.header.protocol_state.clone();
    }
}

pub struct Storage {
    staged_ledger: StagedLedger,
}

impl Storage {
    pub fn new(
        snarked_ledger: Mask,
        info: GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response,
        expected_hash: v2::MinaBaseStagedLedgerHashStableV1,
    ) -> Self {
        let (scan_state, expected_ledger_hash, pending_coinbase, states) = info.unwrap();

        let states = states
            .into_iter()
            .map(|state| (state.hash(), state))
            .collect::<BTreeMap<_, _>>();

        let mut staged_ledger = StagedLedger::of_scan_state_pending_coinbases_and_snarked_ledger(
            (),
            &CONSTRAINT_CONSTANTS,
            Verifier,
            (&scan_state).into(),
            snarked_ledger.clone(),
            LocalState::empty(),
            expected_ledger_hash.clone().into(),
            (&pending_coinbase).into(),
            |key| states.get(&key).cloned().unwrap(),
        )
        .unwrap();

        let expected_hash_str = serde_json::to_string(&expected_hash).unwrap();
        log::info!("expected staged ledger hash: {expected_hash_str}");

        let actual_hash = v2::MinaBaseStagedLedgerHashStableV1::from(&staged_ledger.hash());
        let actual_hash_str = serde_json::to_string(&actual_hash).unwrap();
        log::info!("actual staged ledger hash {actual_hash_str}");

        assert_eq!(expected_hash, actual_hash);

        Storage { staged_ledger }
    }

    pub fn apply_block(
        &mut self,
        block: &v2::MinaBlockBlockStableV2,
        prev_protocol_state: &v2::MinaStateProtocolStateValueStableV2,
    ) {
        let length = block
            .header
            .protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        let previous_state_hash = block.header.protocol_state.previous_state_hash.clone();
        let _previous_state_hash = v2::StateHash::from(v2::DataHashLibStateHashStableV1(
            prev_protocol_state.hash().into(),
        ));
        assert_eq!(previous_state_hash, _previous_state_hash);
        log::info!("will apply: {length} prev: {previous_state_hash}");

        let staged_ledger = &mut self.staged_ledger;
        let global_slot = block
            .header
            .protocol_state
            .body
            .consensus_state
            .global_slot_since_genesis
            .clone();

        dbg!(block
            .header
            .protocol_state
            .body
            .consensus_state
            .global_slot_since_genesis
            .as_u32());
        dbg!(block
            .header
            .protocol_state
            .body
            .consensus_state
            .curr_global_slot
            .slot_number
            .as_u32());

        let prev_state_view = protocol_state::protocol_state_view(prev_protocol_state);

        let protocol_state = &block.header.protocol_state;
        let consensus_state = &protocol_state.body.consensus_state;
        let coinbase_receiver: CompressedPubKey = (&consensus_state.coinbase_receiver).into();
        let _supercharge_coinbase = consensus_state.supercharge_coinbase;

        dbg!(&coinbase_receiver, _supercharge_coinbase);

        // FIXME: Using `supercharge_coinbase` (from block) above does not work
        let supercharge_coinbase = false;

        let diff: Diff = (&block.body.staged_ledger_diff).into();

        let result = staged_ledger
            .apply(
                None,
                &CONSTRAINT_CONSTANTS,
                (&global_slot).into(),
                diff,
                (),
                &Verifier,
                &prev_state_view,
                scan_state::protocol_state::hashes(prev_protocol_state),
                coinbase_receiver,
                supercharge_coinbase,
            )
            .unwrap();
        let hash = v2::MinaBaseStagedLedgerHashStableV1::from(&result.hash_after_applying);
        let hash_str = serde_json::to_string(&hash).unwrap();
        log::info!("new staged ledger hash {hash_str}");
        let expected_hash_str = serde_json::to_string(
            &block
                .header
                .protocol_state
                .body
                .blockchain_state
                .staged_ledger_hash,
        )
        .unwrap();
        log::info!("expected staged ledger hash {expected_hash_str}");
        assert_eq!(hash_str, expected_hash_str);
    }
}

async fn download_blocks(
    engine: &mut Engine,
    blocks: &mut VecDeque<v2::MinaBlockBlockStableV2>,
    head_height: u32,
    snarked_height: u32,
) {
    log::info!("need blocks {}..{head_height}", snarked_height + 1);
    let dir = AsRef::<Path>::as_ref("target/blocks");
    for i in ((snarked_height + 1)..head_height).rev() {
        let last = &blocks
            .back()
            .unwrap()
            .header
            .protocol_state
            .previous_state_hash;
        let new = if let Ok(mut file) = File::open(dir.join(last.to_string())) {
            // log::info!("loading cached block {i}");
            binprot::BinProtRead::binprot_read(&mut file).unwrap()
        } else {
            log::info!("downloading block {i}");
            let new = engine
                .rpc::<GetTransitionChainV2>(vec![last.0.clone()])
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            new[0].clone()
        };
        let mut file = File::create(dir.join(last.to_string())).unwrap();
        new.binprot_write(&mut file).unwrap();
        blocks.push_back(new);
    }
    log::info!("have blocks {}..{head_height}", snarked_height + 1);
}
