use std::{path::Path, fs::File, io::Write, time::Duration, borrow::Cow};

use binprot::BinProtRead;
use mina_signer::CompressedPubKey;
use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

use mina_tree::{
    Account, Mask, Database, BaseLedger,
    staged_ledger::{staged_ledger::StagedLedger, diff::Diff},
    verifier::Verifier,
    scan_state::{
        currency::{Length, Amount, Slot},
        transaction_logic::protocol_state::{
            ProtocolStateView, EpochData as UntypedEpochData, EpochLedger as UntypedEpochLedger,
        },
        protocol_state::hashes_abstract,
        self,
    },
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    block_height: u32,
    canonical: bool,
    command_transaction_count: u32,
    creator: v2::NonZeroCurvePoint,
    protocol_state: ProtocolState,
    protocol_state_proof: ProtocolStateProof,
    state_hash: v2::StateHash,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStateProof {
    base64: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolState {
    previous_state_hash: v2::StateHash,
    consensus_state: ConsensusState,
    blockchain_state: BlockchainState,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockchainState {
    snarked_ledger_hash: v2::LedgerHash,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsensusState {
    #[serde(rename = "coinbaseReceiever")]
    coinbase_receiver: v2::NonZeroCurvePoint,
    blockchain_length: u32,
    min_window_density: u32,
    total_currency: u64,
    slot_since_genesis: u32,
    staking_epoch_data: EpochData,
    next_epoch_data: EpochData,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpochData {
    ledger: EpochLedger,
    seed: v2::EpochSeed,
    start_checkpoint: v2::StateHash,
    lock_checkpoint: v2::StateHash,
    epoch_length: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpochLedger {
    hash: v2::LedgerHash,
    total_currency: u64,
}

impl From<&EpochLedger> for UntypedEpochLedger<mina_curves::pasta::Fp> {
    fn from(value: &EpochLedger) -> Self {
        UntypedEpochLedger {
            hash: value.hash.to_field(),
            total_currency: Amount::from_u64(value.total_currency),
        }
    }
}

impl From<&EpochData> for UntypedEpochData<mina_curves::pasta::Fp> {
    fn from(value: &EpochData) -> Self {
        UntypedEpochData {
            ledger: (&value.ledger).into(),
            seed: value.seed.to_field(),
            start_checkpoint: value.start_checkpoint.to_field(),
            lock_checkpoint: value.start_checkpoint.to_field(),
            epoch_length: Length::from_u32(value.epoch_length),
        }
    }
}

impl ProtocolState {
    pub fn protocol_state_view(&self) -> ProtocolStateView {
        ProtocolStateView {
            snarked_ledger_hash: self.blockchain_state.snarked_ledger_hash.to_field(),
            blockchain_length: Length::from_u32(self.consensus_state.blockchain_length),
            min_window_density: Length::from_u32(self.consensus_state.min_window_density),
            total_currency: Amount::from_u64(self.consensus_state.total_currency),
            global_slot_since_genesis: Slot::from_u32(self.consensus_state.slot_since_genesis),
            staking_epoch_data: (&self.consensus_state.staking_epoch_data).into(),
            next_epoch_data: (&self.consensus_state.next_epoch_data).into(),
        }
    }
}

#[cfg(test)]
#[test]
fn ar() {
    run("docker/archive");
}

pub fn run<P>(path: P)
where
    P: AsRef<Path>,
{
    use super::bootstrap::CONSTRAINT_CONSTANTS;

    let accounts = |ledger: serde_json::Value| -> Option<Vec<Account>> {
        let it = ledger
            .as_object()?
            .get("ledger")?
            .as_object()?
            .get("accounts")?
            .as_array()?
            .iter();
        let mut hashes = vec![];
        let mut list: Vec<_> = it
            .cloned()
            .map(|mut a| {
                use mina_tree::{
                    scan_state::currency::{SlotSpan, Balance},
                    Timing,
                };

                let account_value = a.as_object_mut().unwrap();

                let mut account = Account::empty();
                account.public_key = CompressedPubKey::from_address(
                    account_value
                        .remove("pk")
                        .unwrap()
                        .clone()
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
                if let Some(balance) = account_value.remove("balance") {
                    let balance = balance.as_str().unwrap();
                    let balance = if !balance.contains('.') {
                        Cow::Owned(format!("{balance}.000000000"))
                    } else {
                        Cow::Borrowed(balance)
                    };
                    account.balance = Balance::of_mina_string_exn(&balance);
                }
                if let Some(delegate) = account_value
                    .remove("delegate")
                    .and_then(|a| a.as_str().map(ToOwned::to_owned))
                {
                    account.delegate = Some(CompressedPubKey::from_address(&delegate).unwrap());
                } else {
                    account.delegate = Some(account.public_key.clone());
                }
                if let Some(timing) = account_value.remove("timing") {
                    #[derive(Deserialize, Debug)]
                    struct Timed {
                        initial_minimum_balance: String,
                        cliff_time: [String; 2],
                        cliff_amount: String,
                        vesting_period: [String; 2],
                        vesting_increment: String,
                    }

                    let Timed {
                        mut initial_minimum_balance,
                        cliff_time,
                        mut cliff_amount,
                        vesting_period,
                        mut vesting_increment,
                    } = serde_json::from_value(timing.clone()).unwrap();

                    if !initial_minimum_balance.contains('.') {
                        initial_minimum_balance.extend(".000000000".chars());
                    }
                    if !cliff_amount.contains('.') {
                        cliff_amount.extend(".000000000".chars());
                    }
                    if !vesting_increment.contains('.') {
                        vesting_increment.extend(".000000000".chars());
                    }

                    account.timing = Timing::Timed {
                        initial_minimum_balance: Balance::of_mina_string_exn(
                            &initial_minimum_balance,
                        ),
                        cliff_time: Slot::from_u32(cliff_time[1].parse().unwrap()),
                        cliff_amount: Balance::of_mina_string_exn(&cliff_amount).to_amount(),
                        vesting_period: SlotSpan::from_u32(vesting_period[1].parse().unwrap()),
                        vesting_increment: Balance::of_mina_string_exn(&vesting_increment)
                            .to_amount(),
                    };
                }
                account_value.remove("sk");

                hashes.push(
                    v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(account.hash().into()))
                        .to_string(),
                );

                assert!(account_value.is_empty());

                account
            })
            .collect();

        // println!("{}", serde_json::to_string(&hashes).unwrap());

        list.insert(0, {
            let mut account = Account::empty();
            account.public_key = CompressedPubKey::from_address(
                "B62qiy32p8kAKnny8ZFwoMhYpBppM1DWVCqAPBYNcXnsAHhnfAAuXgg",
            )
            .unwrap();

            account.balance = mina_tree::scan_state::currency::Balance::of_nanomina_int_exn(1000);
            account.delegate = Some(account.public_key.clone());
            // println!(
            //     "{}",
            //     v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(account.hash().into()))
            // );
            account
        });

        Some(list)
    };

    let ledger_file = File::open(path.as_ref().join("ledger.json")).unwrap();
    let value = serde_json::from_reader::<_, serde_json::Value>(ledger_file).unwrap();
    let accounts = accounts(value).unwrap();

    let mut inner = Mask::new_root(Database::create(35));
    for account in accounts {
        let account_id = account.id();
        inner.get_or_create_account(account_id, account).unwrap();
    }

    let root = inner.merkle_root();
    println!(
        "root: {}",
        v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(root.into()))
    );
    println!(
        "root: {}",
        v2::StateHash::from(v2::DataHashLibStateHashStableV1(root.into()))
    );

    let mut staged_ledger = StagedLedger::create_exn(CONSTRAINT_CONSTANTS, inner).unwrap();

    let block_file = File::open(path.as_ref().join("blocks/1.json")).unwrap();
    let block = serde_json::from_reader::<_, Block>(block_file).unwrap();
    let mut block_file_p2p =
        File::open(path.as_ref().join(format!("blocks/1/{}", block.state_hash))).unwrap();
    let block_p2p = v2::MinaBlockBlockStableV2::binprot_read(&mut block_file_p2p).unwrap();

    let block_file = File::open(path.as_ref().join("blocks/2.json")).unwrap();
    let block_2 = serde_json::from_reader::<_, Block>(block_file).unwrap();
    let mut block_file_p2p = File::open(
        path.as_ref()
            .join(format!("blocks/2/{}", block_2.state_hash)),
    )
    .unwrap();
    let block_2_p2p = v2::MinaBlockBlockStableV2::binprot_read(&mut block_file_p2p).unwrap();

    // let coinbase_receiver = block
    //     .protocol_state
    //     .consensus_state
    //     .coinbase_receiver
    //     .clone();

    // assert_eq!(Diff::empty(), (&block_p2p.body.staged_ledger_diff).into());

    // let result = staged_ledger
    //     .apply(
    //         None,
    //         &CONSTRAINT_CONSTANTS,
    //         (&v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(0.into())).into(),
    //         Diff::empty(),
    //         (),
    //         &Verifier,
    //         &block.protocol_state.protocol_state_view(),
    //         (
    //             hashes_abstract(
    //                 v2::StateHash::from(v2::DataHashLibStateHashStableV1(root.into())).to_field(),
    //                 block.state_hash.to_field(),
    //             ),
    //             block.state_hash.to_field(),
    //         ),
    //         (&coinbase_receiver).into(),
    //         false,
    //     )
    //     .unwrap();
    // let hash = v2::MinaBaseStagedLedgerHashStableV1::from(&result.hash_after_applying);
    // let hash_str = serde_json::to_string(&hash).unwrap();
    // println!("new staged ledger hash {hash_str}");

    let coinbase_receiver = block_2
        .protocol_state
        .consensus_state
        .coinbase_receiver
        .clone();

    let current_state_view = block.protocol_state.protocol_state_view();
    log::info!(
        "protocol_state_view: {} {} {} {}",
        current_state_view.blockchain_length.as_u32(),
        current_state_view.min_window_density.as_u32(),
        current_state_view.total_currency.as_u64(),
        current_state_view.global_slot_since_genesis.as_u32(),
    );

    let (diff, _) = staged_ledger
        .create_diff(
            &CONSTRAINT_CONSTANTS,
            (&v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(1u32.into())).into(),
            None,
            (&coinbase_receiver).into(),
            (),
            &current_state_view,
            vec![],
            |_| None,
            false,
        )
        .unwrap();

    assert_eq!(
        diff.clone().forget(),
        (&block_2_p2p.body.staged_ledger_diff).into()
    );

    let result = staged_ledger
        .apply(
            None,
            &CONSTRAINT_CONSTANTS,
            (&v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(1.into())).into(),
            (&block_2_p2p.body.staged_ledger_diff).into(),
            (),
            &Verifier,
            &current_state_view,
            scan_state::protocol_state::hashes(&block_p2p.header.protocol_state),
            (&coinbase_receiver).into(),
            false,
        )
        .unwrap();

    let hash = v2::MinaBaseStagedLedgerHashStableV1::from(&result.hash_after_applying);
    let hash_str = serde_json::to_string(&hash).unwrap();
    println!("new staged ledger hash {hash_str}");
}

pub fn store<P>(path: P, initial: v2::StateHash)
where
    P: AsRef<Path>,
{
    use reqwest::blocking::Client;

    let mut client = None;
    let url = "https://berkeley.api.minaexplorer.com";
    let url = url.parse::<reqwest::Url>().unwrap();

    let mut current = initial;

    loop {
        let get_block = |client: &mut Option<Client>| {
            client
                .get_or_insert_with(|| {
                    Client::builder()
                        .timeout(Some(Duration::from_secs(15)))
                        .build()
                        .unwrap()
                })
                .get(
                    url.join(&format!("blocks/{}", current.to_string()))
                        .unwrap(),
                )
                .send()?
                .text()
        };
        let mut tries = 8;
        let block = loop {
            tries -= 1;
            match get_block(&mut client) {
                Ok(b) => break b,
                Err(err) => {
                    client = None;
                    if tries == 0 {
                        Err::<(), _>(err).unwrap();
                    }
                }
            }
        };
        let block_parsed = serde_json::from_str::<Block>(&block).unwrap();
        let path = path
            .as_ref()
            .join(format!("{}.json", block_parsed.block_height));
        if path.exists() {
            break;
        }
        let mut file = File::create(path).unwrap();
        file.write_all(block.as_bytes()).unwrap();
        if block_parsed.block_height == 1 {
            break;
        }
        log::info!("{}, {current}", block_parsed.block_height);
        current = block_parsed.protocol_state.previous_state_hash;
    }
}
