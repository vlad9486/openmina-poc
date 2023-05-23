use redux::ActionWithMeta;

use super::{State, Action, action::OutgoingAction, Request};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Outgoing {
                inner: OutgoingAction::Init(Request::BestTip(_)),
                ..
            } => {
                self.outgoing_best_tip = true;
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}
