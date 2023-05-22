use redux::ActionWithMeta;

use super::{state::State, action::Action};

pub fn run(state: &mut State, action: &ActionWithMeta<Action>) {
    let _meta = action.meta().clone();
    let _ = state;
    match action.action() {
        Action::RpcNegotiated { .. } => {}
        _ => {}
    }

    // must be the last.
    // state.action_applied(action);
}
