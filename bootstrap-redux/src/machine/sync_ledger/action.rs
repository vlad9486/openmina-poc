use mina_p2p_messages::{v2, rpc::ProofCarryingDataStableV1};
use serde::{Serialize, Deserialize};

use super::state::State;
use crate::machine::State as GlobalState;

#[derive(derive_more::From, Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    Start(
        ProofCarryingDataStableV1<
            v2::MinaBlockBlockStableV2,
            (
                Vec<v2::MinaBaseStateBodyHashStableV1>,
                v2::MinaBlockBlockStableV2,
            ),
        >,
    ),
    Continue(v2::MinaLedgerSyncLedgerAnswerStableV2),
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, state: &State) -> bool {
        state.epoch_ledger_hash.is_some()
    }
}

impl redux::EnablingCondition<GlobalState> for Action {
    fn is_enabled(&self, state: &GlobalState) -> bool {
        self.is_enabled(&state.sync_ledger)
    }
}
