use redux::{ActionMeta, Store};
use mina_p2p_messages::{
    rpc_kernel::{Message, Query, RpcMethod, NeedsLength},
    rpc::{
        GetBestTipV2, AnswerSyncLedgerQueryV2, GetTransitionChainProofV1ForV2,
        GetTransitionChainV2, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
    },
};
use binprot::BinProtWrite;

use super::{Action, OutgoingAction, Request};
use crate::{
    Service,
    machine::{State as GlobalState, Action as GlobalAction},
};

fn make<T: RpcMethod>(id: i64, query: T::Query) -> Vec<u8> {
    let msg = Message::Query(Query {
        tag: T::NAME.into(),
        version: T::VERSION,
        id,
        data: NeedsLength(query),
    });
    let magic = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfdRPC\x00\x01".to_vec();
    let bytes = {
        let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        msg.binprot_write(&mut bytes).unwrap();
        let len = (bytes.len() - 8) as u64;
        bytes[..8].clone_from_slice(&len.to_le_bytes());
        bytes
    };
    let mut output = vec![];
    if id == 0 {
        output.extend_from_slice(&magic);
    }
    output.extend_from_slice(&bytes);
    output
}

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
                    .get(&(peer_id, connection_id))
                    .expect("reducer must register this request");
                log::info!("Outgoing request {}", request);
                let data = match request {
                    Request::BestTip(query) => make::<GetBestTipV2>(s.last_id - 1, query),
                    Request::StagedLedgerAuxAndPendingCoinbasesAtHash(query) => {
                        make::<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2>(s.last_id - 1, query)
                    }
                    Request::SyncLedger(query) => {
                        make::<AnswerSyncLedgerQueryV2>(s.last_id - 1, query)
                    }
                    Request::GetTransitionChainProof(query) => {
                        make::<GetTransitionChainProofV1ForV2>(s.last_id - 1, query)
                    }
                    Request::GetTransitionChain(query) => {
                        make::<GetTransitionChainV2>(s.last_id - 1, query)
                    }
                };
                store.service().send(peer_id, connection_id, data);
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
