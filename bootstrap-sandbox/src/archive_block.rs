use std::{path::Path, fs::File, io::Write, time::Duration};

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
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStateProof {
    base64: String,
}

#[cfg(test)]
#[test]
fn graphql_block() {
    let s = include_str!("7260.graphql.json");
    let block = serde_json::from_str::<Block>(s).unwrap();

    use base64::{
        engine::{Engine, GeneralPurpose, GeneralPurposeConfig},
        alphabet,
    };
    use binprot::BinProtRead;

    let engine = GeneralPurpose::new(&alphabet::URL_SAFE, GeneralPurposeConfig::new());
    let s = engine.decode(&block.protocol_state_proof.base64).unwrap();
    let mut s = s.as_slice();
    let p = v2::MinaBaseProofStableV2::binprot_read(&mut s).unwrap();
    println!("{}", serde_json::to_string(&p).unwrap());
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
