use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

use super::state::State;
use crate::machine::State as GlobalState;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    Continue(v2::MinaBlockBlockStableV2),
    Apply(v2::MinaBlockBlockStableV2),
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, _state: &State) -> bool {
        match self {
            Action::Continue(_) => true,
            Action::Apply(_) => true,
        }
    }
}

impl redux::EnablingCondition<GlobalState> for Action {
    fn is_enabled(&self, state: &GlobalState) -> bool {
        self.is_enabled(&state.sync_transitions)
    }
}
