use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        let meta = action.meta().clone();
        match action.action() {
            Action::RpcNegotiated { .. } => {}
            Action::Rpc(inner) => self.rpc.reducer(&meta.with_action(inner.clone())),
            _ => {}
        }
    }
}
