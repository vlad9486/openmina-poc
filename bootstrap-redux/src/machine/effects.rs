use mina_p2p_messages::v2;
use redux::{Store, ActionWithMeta};

use super::{
    state::State,
    action::Action,
    rpc::{
        Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest, Message,
        Response,
    },
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
                        let Ok(best_tip) = &b.0 else {
                            log::error!("get best tip failed");
                            return;
                        };
                        let Some(best_tip) = &best_tip.0 else {
                            log::warn!("best tip is none");
                            return;
                        };
                        let ledger_hash = best_tip
                            .data
                            .header
                            .protocol_state
                            .body
                            .consensus_state
                            .staking_epoch_data
                            .ledger
                            .hash
                            .clone();
                        log::info!("Synchronizing Ledger: {ledger_hash}");
                        let q = (
                            ledger_hash.0.clone(),
                            v2::MinaLedgerSyncLedgerQueryStableV1::NumAccounts,
                        );
                        store.dispatch(Action::Rpc(RpcAction::Outgoing {
                            peer_id: *peer_id,
                            connection_id: *connection_id,
                            inner: RpcOutgoingAction::Init(RpcRequest::SyncLedger(q)),
                        }));
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
        _ => {}
    }
}
