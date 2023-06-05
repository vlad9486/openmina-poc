use mina_p2p_messages::rpc::{
    GetBestTipV2, AnswerSyncLedgerQueryV2, GetTransitionChainProofV1ForV2, GetTransitionChainV2,
};
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
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(Request::SyncLedger(_)),
            } => {
                self.outgoing
                    .entry((*peer_id, *connection_id))
                    .or_default()
                    .register::<AnswerSyncLedgerQueryV2>();
            }
            Action::Outgoing {
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(Request::GetTransitionChainProof(_)),
            } => {
                self.outgoing
                    .entry((*peer_id, *connection_id))
                    .or_default()
                    .register::<GetTransitionChainProofV1ForV2>();
            }
            Action::Outgoing {
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(Request::GetTransitionChain(_)),
            } => {
                self.outgoing
                    .entry((*peer_id, *connection_id))
                    .or_default()
                    .register::<GetTransitionChainV2>();
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}
