use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        let meta = action.meta().clone();
        match action.action() {
            Action::RpcNegotiated { .. } => {}
            Action::RpcMessage {
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

                // TODO:
                self.last_msg.clear();
                for item in s {
                    let x = item.unwrap();
                    log::info!("Incoming {x}");
                    self.last_msg.push(x);
                }
            }
            Action::Rpc(inner) => self.rpc.reducer(&meta.with_action(inner.clone())),
            _ => {}
        }
    }
}
