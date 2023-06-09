use libp2p::PeerId;
use serde::{Serialize, Deserialize};

use super::{
    state::State, rpc::Action as RpcAction, sync_ledger::Action as SyncLedgerAction,
    download_blocks::Action as SyncTransitionsAction,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    GossipMessage,
    RpcNegotiated {
        peer_id: PeerId,
        connection_id: usize,
    },
    RpcRawBytes {
        peer_id: PeerId,
        connection_id: usize,
        bytes: Vec<u8>,
    },
    RpcClosed {
        peer_id: PeerId,
        connection_id: usize,
    },
    Rpc(RpcAction),
    SyncLedger(SyncLedgerAction),
    SyncLedgerDone,
    SyncTransitions(SyncTransitionsAction),
    SyncTransitionsDone,
}

impl From<SyncTransitionsAction> for Action {
    fn from(value: SyncTransitionsAction) -> Self {
        Action::SyncTransitions(value)
    }
}

impl From<RpcAction> for Action {
    fn from(value: RpcAction) -> Self {
        Action::Rpc(value)
    }
}

impl From<SyncLedgerAction> for Action {
    fn from(value: SyncLedgerAction) -> Self {
        Action::SyncLedger(value)
    }
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, state: &State) -> bool {
        match self {
            Action::Rpc(inner) => inner.is_enabled(&state.rpc),
            Action::RpcRawBytes { bytes, .. } => !bytes.is_empty(),
            Action::SyncLedgerDone => state.best_tip_block.is_some(),
            _ => true,
        }
    }
}
