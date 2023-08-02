use std::{collections::BTreeMap, fs::File, path::Path};

use binprot::BinProtRead;
use mina_p2p_messages::{
    rpc::{GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response, GetBestTipV2},
    v2,
    rpc_kernel::RpcMethod,
};
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

pub async fn again(path_main: &Path, height: u32) {
    let path_blocks = path_main.join("blocks");
    let path = path_main.join(height.to_string());

    let mut best_tip_file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <<GetBestTipV2 as RpcMethod>::Response>::binprot_read(&mut best_tip_file)
        .unwrap()
        .unwrap();

    let head = best_tip.data;
    let last_protocol_state = best_tip.proof.1.header.protocol_state;
    let last_protocol_state_hash = last_protocol_state.hash();

    let snarked_ledger_hash = last_protocol_state
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
    let snarked_ledger = match File::open(path.join("ledgers").join(snarked_ledger_hash_str)) {
        Ok(file) => SnarkedLedger::load_bin(file).unwrap(),
        Err(_) => SnarkedLedger::empty(),
    };

    let mut file = File::open(path.join("staged_ledger_aux")).unwrap();
    let info =
        GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response::binprot_read(&mut file).unwrap();

    let expected_hash = last_protocol_state
        .body
        .blockchain_state
        .staged_ledger_hash
        .clone();
    let mut storage = Storage::new(snarked_ledger.inner, info, expected_hash);

    let file = File::open(path_main.join("blocks").join("table.json")).unwrap();
    let table = serde_json::from_reader::<_, BTreeMap<String, u32>>(file).unwrap();

    let mut last = head.header.protocol_state.previous_state_hash.clone();
    let mut blocks = vec![];
    blocks.push(head);
    while last.0 != last_protocol_state_hash.inner().0 {
        let height = table.get(&last.to_string()).unwrap();
        let path = path_blocks.join(height.to_string()).join(last.to_string());

        let mut file = File::open(path).unwrap();
        let new = v2::MinaBlockBlockStableV2::binprot_read(&mut file).unwrap();
        last = new.header.protocol_state.previous_state_hash.clone();
        blocks.push(new);
    }

    let mut last_protocol_state = last_protocol_state;
    while let Some(block) = blocks.pop() {
        storage.apply_block(&block, &last_protocol_state);
        last_protocol_state = block.header.protocol_state.clone();
    }
}

pub fn test(path_main: &Path, height: u32, url: String) {
    use reqwest::blocking::Client;
    use serde::Deserialize;
    use thiserror::Error;
    use mina_p2p_messages::rpc::ProofCarryingDataStableV1;

    let client = Client::builder().build().unwrap();

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct Event {
        kind: String,
        best_tip_received: Option<u64>,
        synced: Option<u64>,
        ledgers: BTreeMap<String, Ledgers>,
        blocks: Vec<Block>,
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct Ledgers {
        snarked: Option<Ledger>,
        staged: Option<Ledger>,
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct Ledger {
        hash: String,
        fetch_parts_start: Option<u64>,
        fetch_parts_end: Option<u64>,
        reconstruct_start: Option<u64>,
        reconstruct_end: Option<u64>,
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct Block {
        global_slot: u32,
        height: u32,
        hash: String,
        pred_hash: String,
        status: String,
        fetch_start: Option<u64>,
        fetch_end: Option<u64>,
        apply_start: Option<u64>,
        apply_end: Option<u64>,
    }

    let fetch_events =
        || serde_json::from_reader::<_, Vec<Event>>(client.get(&url).send().unwrap()).unwrap();
    // || serde_json::from_reader::<_, Vec<Event>>(File::open("target/sync.json").unwrap()).unwrap();

    let path = path_main.join(height.to_string());

    let mut best_tip_file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <<GetBestTipV2 as RpcMethod>::Response>::binprot_read(&mut best_tip_file)
        .unwrap()
        .unwrap();

    #[derive(Debug, Error)]
    enum TestError {
        #[error("there is no bootstrap event, yet")]
        BootstrapNotStarted,
        #[error("the bootstrap is in progress, yet")]
        BootstrapNotDone,
        #[error("the root ledger is absent, yet")]
        RootLedgerIsAbsent,
        #[error("the snarked ledger hash mismatch, expected: {expected}, actual: {actual}")]
        SnarkedLedgerHashMismatch { expected: String, actual: String },
        #[error("head block height is wrong, expected: {expected}, actual: {actual}")]
        HeadBlockIsWrong { expected: u32, actual: u32 },
        #[error("head block is not applied")]
        HeadBlockIsNotApplied,
        #[error("head block hash mismatch, expected: {expected}, actual: {actual}")]
        HeadBlockHashMismatch { expected: String, actual: String },
    }

    impl TestError {
        fn fatal(&self) -> bool {
            use TestError::*;
            !matches!(
                self,
                BootstrapNotStarted | BootstrapNotDone | RootLedgerIsAbsent
            )
        }
    }

    type T = ProofCarryingDataStableV1<
        v2::MinaBlockBlockStableV2,
        (
            Vec<v2::MinaBaseStateBodyHashStableV1>,
            v2::MinaBlockBlockStableV2,
        ),
    >;

    fn test_inner(events: Vec<Event>, best_tip: &T) -> Result<(), TestError> {
        let current_protocol_state = &best_tip.data.header.protocol_state;

        //
        log::debug!("check if bootstrap event exist...");
        let bootstrap = events
            .iter()
            .find(|event| event.kind == "Bootstrap")
            .ok_or(TestError::BootstrapNotStarted)?;

        //
        log::debug!("check if bootstrap is done");
        if bootstrap.synced.is_none() {
            return Err(TestError::BootstrapNotDone);
        }

        //
        log::debug!("check root ledger is present");
        let root = bootstrap
            .ledgers
            .get("root")
            .ok_or(TestError::RootLedgerIsAbsent)?;
        let root_snarked = root.snarked.as_ref().ok_or(TestError::RootLedgerIsAbsent)?;
        let _root_staged = root.staged.as_ref().ok_or(TestError::RootLedgerIsAbsent)?;

        //
        log::debug!("check snarked ledger hash");

        let snarked_ledger_hash = best_tip
            .proof
            .1
            .header
            .protocol_state
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
        if snarked_ledger_hash_str != root_snarked.hash {
            return Err(TestError::SnarkedLedgerHashMismatch {
                expected: snarked_ledger_hash_str,
                actual: root_snarked.hash.clone(),
            });
        }

        //
        log::debug!("check head block height");

        let head_height = current_protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        let head_block = bootstrap
            .blocks
            .first()
            .ok_or(TestError::HeadBlockIsWrong {
                expected: head_height,
                actual: 0,
            })?;
        if head_block.height != head_height {
            return Err(TestError::HeadBlockIsWrong {
                expected: head_height,
                actual: head_block.height,
            });
        }

        //
        log::debug!("check head block is applied");
        if head_block.status != "Applied" {
            return Err(TestError::HeadBlockIsNotApplied);
        }

        //
        log::debug!("check head block hash");

        let current_protocol_state_hash = current_protocol_state.hash().to_string();
        if head_block.hash != current_protocol_state_hash {
            return Err(TestError::HeadBlockHashMismatch {
                expected: current_protocol_state_hash,
                actual: head_block.hash.clone(),
            });
        }

        Ok(())
    }

    loop {
        match test_inner(fetch_events(), &best_tip) {
            Ok(()) => break,
            Err(err) if !err.fatal() => {
                log::info!("{err}");
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            Err(err) => {
                log::error!("{err}");
                std::process::exit(1);
            }
        }
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
            .map(|state| (state.hash().to_fp().unwrap(), state))
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
            prev_protocol_state.hash().inner().0.clone(),
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
