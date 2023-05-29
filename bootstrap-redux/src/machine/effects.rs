use redux::{Store, ActionWithMeta};

use super::{
    state::State,
    action::Action,
    rpc::{
        Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest, Message,
        Response,
    },
    sync_ledger::Action as SyncLedgerAction,
};
use crate::Service;

pub fn run(store: &mut Store<State, Service, Action>, action: ActionWithMeta<Action>) {
    match action.action() {
        Action::RpcRawBytes {
            peer_id,
            connection_id,
            ..
        } => {
            let msgs = store.state().last_responses.clone();
            for msg in &msgs {
                match msg {
                    Message::Heartbeat => {
                        store.dispatch(Action::Rpc(RpcAction::Heartbeat {
                            peer_id: *peer_id,
                            connection_id: *connection_id,
                        }));
                    }
                    Message::Response {
                        body: Response::BestTip(b),
                        ..
                    } => {
                        let Ok(v) = &b.0 else {
                            log::error!("get best tip failed");
                            return;
                        };
                        let Some(v) = &v.0 else {
                            log::warn!("best tip is none");
                            return;
                        };
                        store.dispatch(SyncLedgerAction::Start(v.clone()));
                    }
                    Message::Response {
                        body: Response::SyncLedger(b),
                        ..
                    } => {
                        let Ok(v) = &b.0 else {
                            log::error!("sync ledger failed");
                            return;
                        };
                        let Ok(v) = &v.0 .0 else {
                            log::warn!("sync ledger failed");
                            return;
                        };
                        store.dispatch(SyncLedgerAction::Continue(v.clone()));
                    }
                    _ => {}
                }
            }
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
        Action::SyncLedger(inner) => inner.clone().effects(action.meta(), store),
        _ => {}
    }
}
