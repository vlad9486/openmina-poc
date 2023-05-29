use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Start(v) => {
                let ledger_hash = v
                    .data
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .next_epoch_data
                    .ledger
                    .hash
                    .clone();
                self.epoch_ledger_hash = Some(ledger_hash);
            }
            Action::Continue(v) => {
                // TODO: store results
                let _ = v;
            }
        }
    }
}
