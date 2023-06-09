use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

use super::rpc::{State as RpcState, Message as RpcMessage};
use super::{sync_ledger::State as SyncLedgerState, download_blocks::State as SyncTransitionsState};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub rpc: RpcState,
    pub sync_ledger: SyncLedgerState,
    pub sync_transitions: SyncTransitionsState,
    pub best_tip_block: Option<v2::MinaBlockBlockStableV2>,
    pub last_responses: Vec<RpcMessage>,
}
