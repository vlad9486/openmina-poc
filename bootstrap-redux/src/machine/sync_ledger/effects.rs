use mina_p2p_messages::v2;
use redux::{Store, ActionMeta};

use super::{
    Action,
    super::rpc::{Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest},
};
use crate::{
    service::Service,
    machine::{State as GlobalState, Action as GlobalAction},
};

impl Action {
    pub fn effects(self, _: &ActionMeta, store: &mut Store<GlobalState, Service, GlobalAction>) {
        match self {
            Action::Start(_) => {
                let ledger_hash = store
                    .state()
                    .sync_ledger
                    .epoch_ledger_hash
                    .as_ref()
                    .expect("enabling conditions");
                log::info!("Synchronizing Ledger: {ledger_hash}");

                // TODO: choose most suitable peer
                let mut peers = store.state().rpc.outgoing.keys();
                let (peer_id, connection_id) = peers.next().unwrap();
                let q = (
                    ledger_hash.0.clone(),
                    v2::MinaLedgerSyncLedgerQueryStableV1::NumAccounts,
                );
                store.dispatch(RpcAction::Outgoing {
                    peer_id: *peer_id,
                    connection_id: *connection_id,
                    inner: RpcOutgoingAction::Init(RpcRequest::SyncLedger(q)),
                });
            }
            Action::Continue(v) => {
                let ledger_hash = store
                    .state()
                    .sync_ledger
                    .epoch_ledger_hash
                    .as_ref()
                    .expect("enabling conditions");
                match v {
                    v2::MinaLedgerSyncLedgerAnswerStableV2::NumAccounts(num, hash) => {
                        log::info!("Ledger: {ledger_hash}, accounts: {}, hash: {hash}", num.0);

                        // TODO: choose most suitable peer
                        let mut peers = store.state().rpc.outgoing.keys();
                        let (peer_id, connection_id) = peers.next().unwrap();
                        let q = (
                            ledger_hash.0.clone(),
                            v2::MinaLedgerSyncLedgerQueryStableV1::WhatChildHashes(
                                v2::MerkleAddressBinableArgStableV1(0.into(), vec![].into()),
                            ),
                        );
                        store.dispatch(RpcAction::Outgoing {
                            peer_id: *peer_id,
                            connection_id: *connection_id,
                            inner: RpcOutgoingAction::Init(RpcRequest::SyncLedger(q)),
                        });
                    }
                    v2::MinaLedgerSyncLedgerAnswerStableV2::ChildHashesAre(fst, scd) => {
                        log::info!("Ledger: {ledger_hash}, children: {fst} {scd}");
                    }
                    _ => {}
                }
            }
        }
    }
}
