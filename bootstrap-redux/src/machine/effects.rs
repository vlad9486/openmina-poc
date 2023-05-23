use redux::{Store, ActionWithMeta};

use super::{
    state::State,
    action::Action,
    rpc::{Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest},
};
use crate::Service;

fn heartbeat() -> Vec<u8> {
    use mina_p2p_messages::rpc_kernel::Message;
    use binprot::BinProtWrite;

    let msg = Message::<()>::Heartbeat;
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    bytes
}

pub fn run(store: &mut Store<State, Service, Action>, action: ActionWithMeta<Action>) {
    match action.action() {
        Action::RpcMessage {
            peer_id,
            connection_id,
            bytes,
        } => {
            log::info!("recv {peer_id}, {connection_id:?}, {}", hex::encode(bytes));
            store.service().send(*peer_id, *connection_id, heartbeat());
        }
        Action::RpcNegotiated {
            peer_id,
            connection_id,
        } => {
            store.dispatch(Action::Rpc(RpcAction::Outgoing {
                peer_id: *peer_id,
                connection_id: *connection_id,
                inner: RpcOutgoingAction::Init(RpcRequest::BestTip(())),
            }));
        }
        Action::Rpc(inner) => inner.clone().effects(action.meta(), store),
        _ => {}
    }
}
