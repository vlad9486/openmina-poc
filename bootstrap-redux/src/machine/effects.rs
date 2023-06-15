use mina_p2p_messages::core::Info;
use redux::{Store, ActionWithMeta};

use super::{
    state::State,
    action::Action,
    rpc::{
        Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest, Message,
        Response,
    },
    sync_ledger::Action as SyncLedgerAction,
    download_blocks::Action as SyncTransitionsAction,
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
            for msg in msgs {
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
                        let Ok(v) = b.0 else {
                            log::error!("get best tip failed");
                            return;
                        };
                        let Some(_) = v.0 else {
                            log::warn!("best tip is none");
                            return;
                        };

                        store.dispatch(SyncLedgerAction::Start);
                    }
                    Message::Response {
                        body: Response::StagedLedgerAuxAndPendingCoinbasesAtHash(b),
                        ..
                    } => {
                        let Ok(v) = b.0 else {
                            log::error!("get staged ledger failed");
                            return;
                        };

                        // TODO: separate action
                        store.service().init_staged_ledger(v.0);

                        let best_tip_block = store
                            .state()
                            .best_tip_block
                            .clone()
                            .expect("enabling conditions");
                        store.dispatch(Action::SyncTransitions(SyncTransitionsAction::Continue(
                            best_tip_block,
                        )));
                    }
                    Message::Response {
                        body: Response::GetTransitionChainProof(v),
                        ..
                    } => {
                        let v = serde_json::to_string(&v.0.unwrap().0.unwrap()).unwrap();
                        log::info!("{v}");
                    }
                    Message::Response {
                        body: Response::GetTransitionChain(v),
                        ..
                    } => {
                        let v = &v.0.unwrap().0.unwrap()[0];
                        store.dispatch(Action::SyncTransitions(SyncTransitionsAction::Continue(
                            v.clone(),
                        )));
                    }
                    Message::Response {
                        body: Response::SyncLedger(b),
                        ..
                    } => {
                        let Ok(v) = b.0 else {
                            log::error!("sync ledger failed");
                            return;
                        };
                        match v.0 .0 {
                            Err(err) => {
                                if let Info::CouldNotConstruct(s) = err {
                                    log::warn!("sync ledger failed {}", s.to_string_lossy());
                                } else {
                                    log::warn!("sync ledger failed {err:?}")
                                }
                                store.dispatch(SyncLedgerAction::Continue(None));
                            }
                            Ok(v) => {
                                store.dispatch(SyncLedgerAction::Continue(Some(v)));
                            }
                        }
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
        Action::ApplyBlockDone => {
            if let Some(last) = store.state().sync_transitions.blocks.last() {
                store.dispatch(Action::SyncTransitions(SyncTransitionsAction::Apply(
                    last.clone(),
                )));
            }
        }
        Action::Rpc(inner) => inner.clone().effects(action.meta(), store),
        Action::SyncLedger(inner) => inner.clone().effects(action.meta(), store),
        Action::SyncLedgerDone => {
            let hash = store
                .state()
                .best_tip_ground_block_hash
                .as_ref()
                .unwrap()
                .clone();

            // TODO: choose most suitable peer
            let mut peers = store.state().rpc.outgoing.keys();
            let (peer_id, connection_id) = peers.next().unwrap();

            store.dispatch(Action::Rpc(RpcAction::Outgoing {
                peer_id: *peer_id,
                connection_id: *connection_id,
                inner: RpcOutgoingAction::Init(
                    RpcRequest::StagedLedgerAuxAndPendingCoinbasesAtHash(hash),
                ),
            }));
        }
        Action::SyncTransitions(inner) => inner.clone().effects(action.meta(), store),
        _ => {}
    }
}
