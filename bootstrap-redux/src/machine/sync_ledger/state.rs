use mina_p2p_messages::v2;
use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub epoch_ledger_hash: Option<v2::LedgerHash>,
    pub ledger: Option<Ledger>,
    pub num_accounts: i32,
    pub syncing_depth: i32,
    pub syncing_pos: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Ledger {
    pub hash: v2::LedgerHash,
    pub content: LedgerContent,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LedgerContent {
    Absent,
    Node(Box<Ledger>, Box<Ledger>),
    Leaf(Vec<v2::MinaBaseAccountBinableArgStableV2>),
}
