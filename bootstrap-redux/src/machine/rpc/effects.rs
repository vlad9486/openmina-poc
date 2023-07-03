use redux::{ActionMeta, Store};
use mina_p2p_messages::{rpc_kernel::Message};
use binprot::BinProtWrite;

use super::{Action, OutgoingAction};
use crate::{
    Service,
    machine::{State as GlobalState, Action as GlobalAction},
};

fn make_heartbeat() -> Vec<u8> {
    let msg = Message::<()>::Heartbeat;
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    bytes
}

impl Action {
    pub fn effects(self, _: &ActionMeta, store: &mut Store<GlobalState, Service, GlobalAction>) {
        match self {
            Action::Heartbeat {
                peer_id,
                connection_id,
            } => {
                store
                    .service()
                    .send(peer_id, connection_id, make_heartbeat());
            }
            Action::Outgoing {
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(request),
            } => {
                let s = store
                    .state()
                    .rpc
                    .outgoing
                    .get(&peer_id)
                    .expect("reducer must register this request");
                log::info!("Outgoing request {}", request);
                let id = s.last_id - 1;
                let query = s.pending[&id].query.clone();
                store.service().send(peer_id, connection_id, query);
                store.dispatch(Action::Outgoing {
                    peer_id,
                    connection_id,
                    inner: OutgoingAction::Pending,
                });
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}
