use mina_p2p_messages::rpc::GetBestTipV2;
use redux::ActionWithMeta;

use super::{State, Action, action::OutgoingAction, Request};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Outgoing {
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(Request::BestTip(_)),
            } => {
                self.outgoing_best_tip = true;
                self.outgoing
                    .entry((*peer_id, *connection_id))
                    .or_default()
                    .register::<GetBestTipV2>();
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}
