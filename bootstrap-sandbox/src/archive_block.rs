use std::{path::Path, fs::File, io::Write, time::Duration, borrow::Cow};

use binprot::BinProtRead;
use mina_signer::CompressedPubKey;
use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    block_height: u32,
    canonical: bool,
    command_transaction_count: u32,
    creator: v2::NonZeroCurvePoint,
    protocol_state: ProtocolState,
    protocol_state_proof: ProtocolStateProof,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolState {
    previous_state_hash: v2::StateHash,
    consensus_state: ConsensusState,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsensusState {
    #[serde(rename = "coinbaseReceiever")]
    coinbase_receiver: v2::NonZeroCurvePoint,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStateProof {
    base64: String,
}

pub fn run<P>(path: P)
where
    P: AsRef<Path>,
{
    use mina_tree::{
        Account,
        Mask,
        Database,
        BaseLedger,
        // staged_ledger::staged_ledger::StagedLedger,
        // verifier::Verifier,
        // scan_state::{protocol_state, transaction_logic::protocol_state::protocol_state_view},
    };
    // use super::bootstrap::CONSTRAINT_CONSTANTS;

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
                    scan_state::currency::{SlotSpan, Slot, Balance},
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

        println!("{}", serde_json::to_string(&hashes).unwrap());

        list.insert(0, {
            let mut account = Account::empty();
            account.public_key = CompressedPubKey::from_address(
                "B62qiy32p8kAKnny8ZFwoMhYpBppM1DWVCqAPBYNcXnsAHhnfAAuXgg",
            )
            .unwrap();

            account.balance = mina_tree::scan_state::currency::Balance::of_nanomina_int_exn(1000);
            account.delegate = Some(account.public_key.clone());
            println!(
                "{}",
                v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(account.hash().into()))
            );
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

    let block_file = File::open(path.as_ref().join("blocks/1.json")).unwrap();
    let block = serde_json::from_reader::<_, Block>(block_file).unwrap();

    // FIXME: Using `supercharge_coinbase` (from block) above does not work
    let _supercharge_coinbase = false;

    let _coinbase_receiver = block.protocol_state.consensus_state.coinbase_receiver;

    let mut cursor = std::io::Cursor::new(include_bytes!("protocol_state.bin"));
    let _dummy_state = v2::MinaStateProtocolStateValueStableV2::binprot_read(&mut cursor).unwrap();
    // println!("{}", serde_json::to_string(&dummy_state).unwrap());

    // let staged_ledger = StagedLedger::create_exn(CONSTRAINT_CONSTANTS, inner).unwrap();
    // let (diff, _) = staged_ledger
    //     .create_diff(
    //         &CONSTRAINT_CONSTANTS,
    //         (&v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(0u32.into())).into(),
    //         None,
    //         (&coinbase_receiver).into(),
    //         (),
    //         current_state_view,
    //         transactions_by_fee,
    //         get_completed_work,
    //         supercharge_coinbase,
    //     )
    //     .unwrap();
    // staged_ledger.apply(
    //     None,
    //     &CONSTRAINT_CONSTANTS,
    //     (&v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(0u32.into())).into(),
    //     diff.forget(),
    //     (),
    //     &Verifier,
    //     current_state_view,
    //     state_and_body_hash,
    //     (&coinbase_receiver).into(),
    //     supercharge_coinbase,
    // );
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
