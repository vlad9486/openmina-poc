use std::{path::Path, fs::File, io::Write, time::Duration};

use binprot::BinProtRead;
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
        let list = it
            .cloned()
            .map(|a| {
                let a = a.as_object().unwrap();
                let parse_balance = |value: &serde_json::Value| {
                    v2::CurrencyBalanceStableV1(v2::CurrencyAmountStableV1::from(
                        mina_tree::scan_state::currency::Balance::of_mina_string_exn(
                            value.as_str().unwrap(),
                        ),
                    ))
                };
                let public_key =
                    serde_json::from_value::<v2::NonZeroCurvePoint>(a.get("pk").unwrap().clone())
                        .unwrap();
                let fp_zero = mina_tree::VotingFor::dummy().0;
                v2::MinaBaseAccountBinableArgStableV2 {
                    public_key: public_key.clone(),
                    token_id: v2::TokenIdKeyHash::from(v2::MinaBaseAccountIdDigestStableV1(
                        mina_tree::TokenId::default().0.into(),
                    )),
                    token_symbol: v2::MinaBaseZkappAccountZkappUriStableV1::default(),
                    balance: parse_balance(a.get("balance").unwrap()),
                    nonce: v2::UnsignedExtendedUInt32StableV1::from(0),
                    receipt_chain_hash: v2::MinaBaseReceiptChainHashStableV1(fp_zero.into()),
                    delegate: {
                        let d = a
                            .get("delegate")
                            .cloned()
                            .and_then(|d| {
                                if let serde_json::Value::Null = d {
                                    None
                                } else {
                                    Some(d)
                                }
                            })
                            .map(|d| serde_json::from_value(d).unwrap())
                            .unwrap_or(public_key);
                        Some(d)
                    },
                    voting_for: v2::StateHash::from(v2::DataHashLibStateHashStableV1(
                        fp_zero.into(),
                    )),
                    timing: {
                        if let Some(timing) = a.get("timing") {
                            let obj = timing.as_object().unwrap();
                            let initial_minimum_balance =
                                obj.get("initial_minimum_balance").unwrap();
                            let cliff_time = obj
                                .get("cliff_time")
                                .unwrap()
                                .as_array()
                                .unwrap()
                                .get(1)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .parse::<u32>()
                                .unwrap();
                            let cliff_amount = obj.get("cliff_amount").unwrap();
                            let vesting_increment = obj.get("vesting_increment").unwrap();
                            let vesting_period = obj
                                .get("vesting_period")
                                .unwrap()
                                .as_array()
                                .unwrap()
                                .get(1)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .parse::<u32>()
                                .unwrap();
                            v2::MinaBaseAccountTimingStableV2::Timed {
                                initial_minimum_balance: parse_balance(initial_minimum_balance),
                                cliff_time:
                                    v2::MinaNumbersGlobalSlotSinceGenesisMStableV1::SinceGenesis(
                                        cliff_time.into(),
                                    ),
                                cliff_amount: parse_balance(cliff_amount).0,
                                vesting_period:
                                    v2::MinaNumbersGlobalSlotSpanStableV1::GlobalSlotSpan(
                                        vesting_period.into(),
                                    ),
                                vesting_increment: parse_balance(vesting_increment).0,
                            }
                        } else {
                            v2::MinaBaseAccountTimingStableV2::Untimed
                        }
                    },
                    permissions: v2::MinaBasePermissionsStableV2 {
                        edit_state: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        access: v2::MinaBasePermissionsAuthRequiredStableV2::None,
                        send: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        receive: v2::MinaBasePermissionsAuthRequiredStableV2::None,
                        set_delegate: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_permissions: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_verification_key:
                            v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_zkapp_uri: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        edit_action_state: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_token_symbol: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        increment_nonce: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_voting_for: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                        set_timing: v2::MinaBasePermissionsAuthRequiredStableV2::Signature,
                    },
                    zkapp: None,
                }
            })
            .map(|a| Account::from(&a))
            .collect();
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
    let root = v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(root.into()));
    println!("{root}");

    let block_file = File::open(path.as_ref().join("blocks/1.json")).unwrap();
    let block = serde_json::from_reader::<_, Block>(block_file).unwrap();

    // FIXME: Using `supercharge_coinbase` (from block) above does not work
    let supercharge_coinbase = false;

    let coinbase_receiver = block.protocol_state.consensus_state.coinbase_receiver;

    let mut cursor = std::io::Cursor::new(include_bytes!("protocol_state.bin"));
    let dummy_state = v2::MinaStateProtocolStateValueStableV2::binprot_read(&mut cursor).unwrap();
    println!("{}", serde_json::to_string(&dummy_state).unwrap());

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
