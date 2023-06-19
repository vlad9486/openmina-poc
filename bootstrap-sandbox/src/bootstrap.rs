use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
    fs::File,
    path::Path,
};

use binprot::BinProtWrite;
use mina_p2p_messages::{
    rpc::{
        GetBestTipV2, AnswerSyncLedgerQueryV2, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
        GetTransitionChainV2, VersionedRpcMenuV1,
        GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response,
    },
    v2,
    hash::MinaHash,
};
use mina_rpc::Engine;
use mina_tree::{
    mask::Mask,
    staged_ledger::staged_ledger::StagedLedger,
    verifier::Verifier,
    scan_state::{
        scan_state::{
            ConstraintConstants,
            transaction_snark::{work::Work, OneOrTwo},
        },
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

pub async fn run(mut engine: Engine, block: Option<String>) {
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

    let mut blocks = VecDeque::new();
    blocks.push_back(head);
    download_blocks(&mut engine, &mut blocks, head_height, snarked_height).await;

    let expected_hash = snarked_protocol_state
        .body
        .blockchain_state
        .staged_ledger_hash
        .clone();

    log::info!("downloading staged_ledger_aux and pending_coinbases at {snarked_block_hash}");
    let info = engine
        .rpc::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>(snarked_block_hash.into())
        .await
        .unwrap()
        .unwrap();

    let mut storage = Storage::new(snarked_ledger.inner, info, expected_hash);
    while let Some(block) = blocks.pop_back() {
        storage.apply_block(&block);
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

    pub fn apply_block(&mut self, block: &v2::MinaBlockBlockStableV2) {
        let length = block
            .header
            .protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        let previous_state_hash = block.header.protocol_state.previous_state_hash.clone();
        log::info!("will apply: {length} prev: {previous_state_hash}");

        let staged_ledger = &mut self.staged_ledger;
        let global_slot = block
            .header
            .protocol_state
            .body
            .consensus_state
            .curr_global_slot
            .slot_number
            .clone();
        let coinbase_receiver = match &block.body.staged_ledger_diff.diff.0.coinbase {
            v2::StagedLedgerDiffDiffPreDiffWithAtMostTwoCoinbaseStableV2Coinbase::One(Some(pk)) => {
                pk.receiver_pk.clone()
            }
            _ => {
                let addr = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .block_creator
                    .to_string();

                let pk = CompressedPubKey::from_address(&addr).unwrap();
                (&pk).into()
            }
        };
        let current_state_view = protocol_state::protocol_state_view(&block.header.protocol_state);
        let works_two = block
            .body
            .staged_ledger_diff
            .diff
            .0
            .completed_works
            .iter()
            .map(|work| Work {
                fee: (&work.fee).into(),
                proofs: match &work.proofs {
                    v2::TransactionSnarkWorkTStableV2Proofs::One(x) => OneOrTwo::One(x.into()),
                    v2::TransactionSnarkWorkTStableV2Proofs::Two((x, y)) => {
                        OneOrTwo::Two((x.into(), y.into()))
                    }
                },
                prover: (&work.prover).into(),
            });
        let works_one = block
            .body
            .staged_ledger_diff
            .diff
            .1
            .as_ref()
            .map(|x| {
                x.completed_works.iter().map(|work| Work {
                    fee: (&work.fee).into(),
                    proofs: match &work.proofs {
                        v2::TransactionSnarkWorkTStableV2Proofs::One(x) => OneOrTwo::One(x.into()),
                        v2::TransactionSnarkWorkTStableV2Proofs::Two((x, y)) => {
                            OneOrTwo::Two((x.into(), y.into()))
                        }
                    },
                    prover: (&work.prover).into(),
                })
            })
            .into_iter()
            .flatten();
        let works = works_two.chain(works_one).collect::<Vec<_>>();
        let transactions_by_fee_two = block.body.staged_ledger_diff.diff.0.commands.iter();
        let transactions_by_fee_one = block
            .body
            .staged_ledger_diff
            .diff
            .1
            .as_ref()
            .map(|x| x.commands.iter())
            .into_iter()
            .flatten();
        let transactions_by_fee = transactions_by_fee_two
            .chain(transactions_by_fee_one)
            .map(|x| (&x.data).into())
            .collect();
        let (diff, _) = staged_ledger
            .create_diff(
                &CONSTRAINT_CONSTANTS,
                (&global_slot).into(),
                None,
                (&coinbase_receiver).into(),
                (),
                &current_state_view,
                transactions_by_fee,
                |key| works.iter().find(|x| x.statement() == *key).cloned(),
                false,
            )
            .unwrap();

        let result = staged_ledger
            .apply(
                None,
                &CONSTRAINT_CONSTANTS,
                (&global_slot).into(),
                diff.forget(),
                (),
                &Verifier,
                &current_state_view,
                scan_state::protocol_state::hashes(&block.header.protocol_state),
                (&coinbase_receiver).into(),
                false,
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
