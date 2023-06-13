use mina_p2p_messages::rpc_kernel::NeedsLength;
use redux::ActionWithMeta;

use super::{
    state::State,
    action::Action,
    rpc::{Message, Response},
};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        let meta = action.meta().clone();
        match action.action() {
            Action::GossipMessage => {}
            Action::RpcNegotiated { .. } => {}
            Action::RpcClosed { .. } => {}
            Action::RpcRawBytes {
                peer_id,
                connection_id,
                bytes,
            } => {
                let s = self
                    .rpc
                    .outgoing
                    .entry((*peer_id, *connection_id))
                    .or_default();

                s.put_slice(&*bytes);

                // TODO: show errors
                self.last_responses = s.into_iter().filter_map(Result::ok).collect();
                for response in &self.last_responses {
                    if let Message::Response {
                        body: Response::BestTip(v),
                        ..
                    } = response
                    {
                        if let Ok(NeedsLength(Some(v))) = &v.0 {
                            self.best_tip_block = Some(v.data.clone());
                            self.sync_transitions.height = v
                                .proof
                                .1
                                .header
                                .protocol_state
                                .body
                                .consensus_state
                                .blockchain_length
                                .as_u32();
                        }
                    }
                }
            }
            Action::ApplyBlockDone => {}
            Action::Rpc(inner) => self.rpc.reducer(&meta.with_action(inner.clone())),
            Action::SyncLedger(inner) => self.sync_ledger.reducer(&meta.with_action(inner.clone())),
            Action::SyncLedgerDone => {}
            Action::SyncTransitions(inner) => self
                .sync_transitions
                .reducer(&meta.with_action(inner.clone())),
            Action::SyncTransitionsDone => {}
        }
    }
}
