use std::{collections::BTreeMap, path::Path, fs::File};

use mina_p2p_messages::{
    v2,
    rpc::{GetBestTipV2, ProofCarryingDataStableV1},
    rpc_kernel::RpcMethod,
};
use binprot::BinProtRead;
use thiserror::Error;
use reqwest::blocking::Client;
use serde::Deserialize;

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

pub fn test(path_main: &Path, height: u32, url: String) {
    let client = Client::builder().timeout(None).build().unwrap();

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
        global_slot: Option<u32>,
        height: Option<u32>,
        hash: String,
        pred_hash: String,
        status: String,
        fetch_start: Option<u64>,
        fetch_end: Option<u64>,
        apply_start: Option<u64>,
        apply_end: Option<u64>,
    }

    let fetch_events = || {
        let text = client.get(&url).send().unwrap().text().unwrap();
        serde_json::from_str::<Vec<Event>>(&text)
            .map_err(|err| {
                log::error!("{err}");
                log::info!("{text}");
            })
            .unwrap()
    };
    // || serde_json::from_reader::<_, Vec<Event>>(File::open("target/sync.json").unwrap()).unwrap();

    let path = path_main.join(height.to_string());

    let mut best_tip_file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <<GetBestTipV2 as RpcMethod>::Response>::binprot_read(&mut best_tip_file)
        .unwrap()
        .unwrap();

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
        log::info!("snarked ledger hash {snarked_ledger_hash_str} is ok");

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
        if head_block.height != Some(head_height) {
            return Err(TestError::HeadBlockIsWrong {
                expected: head_height,
                actual: head_block.height.unwrap_or_default(),
            });
        }
        log::info!("block {head_height} is ok");

        //
        log::debug!("check head block is applied");
        if head_block.status != "Applied" {
            return Err(TestError::HeadBlockIsNotApplied);
        }
        log::info!("block {head_height} is applied");

        //
        log::debug!("check head block hash");

        let current_protocol_state_hash = current_protocol_state.hash().to_string();
        if head_block.hash != current_protocol_state_hash {
            return Err(TestError::HeadBlockHashMismatch {
                expected: current_protocol_state_hash,
                actual: head_block.hash.clone(),
            });
        }
        log::info!("block {head_height} hash {current_protocol_state_hash} is ok");

        Ok(())
    }

    loop {
        match test_inner(fetch_events(), &best_tip) {
            Ok(()) => break,
            Err(err) if !err.fatal() => {
                log::info!("{err}... wait");
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            Err(err) => {
                log::error!("{err}");
                std::process::exit(1);
            }
        }
    }
}

pub fn test_graphql(path_main: &Path, height: u32, url: String) {
    let client = Client::builder().timeout(None).build().unwrap();

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Response {
        sync_status: String,
        best_chain: Vec<Chain>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Chain {
        state_hash: String,
        protocol_state: ProtocolState,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProtocolState {
        consensus_state: ConsensusState,
        blockchain_state: BlockchainState,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ConsensusState {
        block_height: serde_json::Value,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BlockchainState {
        snarked_ledger_hash: String,
    }

    let query = r#"
    query MyQuery {
        syncStatus
        bestChain(maxLength: 1) {
            stateHash
            protocolState {
                consensusState {
                    blockHeight
                }
                blockchainState {
                    snarkedLedgerHash
                }
            }
        }
    }"#;
    let fetch = || -> Response {
        let s = client
            .get(&url)
            .query(&[("query", query)])
            .send()
            .unwrap()
            .text()
            .unwrap();
        let mut value = serde_json::from_str::<serde_json::Value>(dbg!(&s)).unwrap();
        let value = value.as_object_mut().unwrap().remove("data").unwrap();
        serde_json::from_value(value).unwrap()
    };

    let path = path_main.join(height.to_string());

    let mut best_tip_file = File::open(path.join("best_tip")).unwrap();
    let best_tip = <<GetBestTipV2 as RpcMethod>::Response>::binprot_read(&mut best_tip_file)
        .unwrap()
        .unwrap();

    type T = ProofCarryingDataStableV1<
        v2::MinaBlockBlockStableV2,
        (
            Vec<v2::MinaBaseStateBodyHashStableV1>,
            v2::MinaBlockBlockStableV2,
        ),
    >;

    fn test_inner(value: Response, best_tip: &T) -> Result<(), TestError> {
        //
        log::debug!("check if bootstrap is done");
        if value.sync_status != "SYNCED" {
            return Err(TestError::BootstrapNotDone);
        }
        let Some(head) = value.best_chain.first() else {
            return Err(TestError::BootstrapNotDone);
        };

        //
        log::debug!("check head block height");

        let current_protocol_state = &best_tip.data.header.protocol_state;

        let head_height = current_protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        let height_value = &head.protocol_state.consensus_state.block_height;
        let height = height_value
            .as_i64()
            .map(|x| x as u32)
            .or_else(|| height_value.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_default();
        if height < head_height {
            return Err(TestError::BootstrapNotDone);
        } else if height > head_height {
            return Err(TestError::HeadBlockIsWrong {
                expected: head_height,
                actual: height,
            });
        }
        log::info!("block {head_height} is ok");

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
        if snarked_ledger_hash_str != head.protocol_state.blockchain_state.snarked_ledger_hash {
            return Err(TestError::SnarkedLedgerHashMismatch {
                expected: snarked_ledger_hash_str,
                actual: head
                    .protocol_state
                    .blockchain_state
                    .snarked_ledger_hash
                    .clone(),
            });
        }
        log::info!("snarked ledger hash {snarked_ledger_hash_str} is ok");

        //
        log::debug!("check head block hash");

        let current_protocol_state_hash = current_protocol_state.hash().to_string();
        if head.state_hash != current_protocol_state_hash {
            return Err(TestError::HeadBlockHashMismatch {
                expected: current_protocol_state_hash,
                actual: head.state_hash.clone(),
            });
        }
        log::info!("block {head_height} hash {current_protocol_state_hash} is ok");

        Ok(())
    }

    loop {
        match test_inner(fetch(), &best_tip) {
            Ok(()) => break,
            Err(err) if !err.fatal() => {
                log::info!("{err}... wait");
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            Err(err) => {
                log::error!("{err}");
                std::process::exit(1);
            }
        }
    }
}
