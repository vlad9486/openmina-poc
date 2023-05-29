use mina_p2p_messages::v2;
use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub epoch_ledger_hash: Option<v2::LedgerHash>,
    pub synchronization_in_progress: bool,
}
