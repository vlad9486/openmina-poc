use redux::{Store, ActionWithMeta};

use super::{
    state::State,
    action::Action,
    rpc::{Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest},
};
use crate::Service;

pub fn run(store: &mut Store<State, Service, Action>, action: ActionWithMeta<Action>) {
    match action.action() {
        Action::RpcMessage { .. } => {
            // TODO:
            // log::info!("recv {peer_id}, {connection_id:?}, {}", hex::encode(bytes));
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
