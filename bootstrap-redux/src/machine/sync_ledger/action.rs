use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

use super::state::State;
use crate::machine::State as GlobalState;

#[derive(derive_more::From, Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    Start,
    Continue(Option<v2::MinaLedgerSyncLedgerAnswerStableV2>),
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, state: &State) -> bool {
        match self {
            Action::Start => true,
            Action::Continue(_) => state.epoch_ledger_hash.is_some(),
        }
    }
}

impl redux::EnablingCondition<GlobalState> for Action {
    fn is_enabled(&self, state: &GlobalState) -> bool {
        self.is_enabled(&state.sync_ledger)
    }
}
