use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        let meta = action.meta().clone();
        match action.action() {
            Action::RpcNegotiated { .. } => {}
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
            }
            Action::Rpc(inner) => self.rpc.reducer(&meta.with_action(inner.clone())),
            Action::SyncLedger(inner) => self.sync_ledger.reducer(&meta.with_action(inner.clone())),
            _ => {}
        }
    }
}
