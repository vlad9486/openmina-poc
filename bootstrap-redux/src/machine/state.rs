use serde::{Serialize, Deserialize};

use super::rpc::{State as RpcState, Message as RpcMessage};
use super::sync_ledger::{State as SyncLedgerState};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub rpc: RpcState,
    pub sync_ledger: SyncLedgerState,
    pub last_responses: Vec<RpcMessage>,
}
