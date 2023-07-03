use binprot::BinProtWrite;
use mina_p2p_messages::{
    rpc::{
        GetBestTipV2, AnswerSyncLedgerQueryV2, GetTransitionChainProofV1ForV2,
        GetTransitionChainV2, GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
    },
    rpc_kernel::{RpcMethod, Message, Query, NeedsLength},
};
use redux::ActionWithMeta;

use super::{State, Action, action::OutgoingAction, Request};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Outgoing {
                peer_id,
                inner: OutgoingAction::Init(Request::BestTip(query)),
                ..
            } => {
                type T = GetBestTipV2;

                self.outgoing_best_tip = true;
                let outgoing = self.outgoing.entry(*peer_id).or_default();

                outgoing.register::<T>(make::<T>(outgoing.last_id, query.clone()));
            }
            Action::Outgoing {
                peer_id,
                inner:
                    OutgoingAction::Init(Request::StagedLedgerAuxAndPendingCoinbasesAtHash(query)),
                ..
            } => {
                type T = GetStagedLedgerAuxAndPendingCoinbasesAtHashV2;

                self.outgoing_staged_ledger = true;
                let outgoing = self.outgoing.entry(*peer_id).or_default();

                outgoing.register::<T>(make::<T>(outgoing.last_id, query.clone()));
            }
            Action::Outgoing {
                peer_id,
                inner: OutgoingAction::Init(Request::SyncLedger(query)),
                ..
            } => {
                type T = AnswerSyncLedgerQueryV2;

                let outgoing = self.outgoing.entry(*peer_id).or_default();

                outgoing.register::<T>(make::<T>(outgoing.last_id, query.clone()));
            }
            Action::Outgoing {
                peer_id,
                inner: OutgoingAction::Init(Request::GetTransitionChainProof(query)),
                ..
            } => {
                type T = GetTransitionChainProofV1ForV2;

                let outgoing = self.outgoing.entry(*peer_id).or_default();

                outgoing.register::<T>(make::<T>(outgoing.last_id, query.clone()));
            }
            Action::Outgoing {
                peer_id,
                inner: OutgoingAction::Init(Request::GetTransitionChain(query)),
                ..
            } => {
                type T = GetTransitionChainV2;

                let outgoing = self.outgoing.entry(*peer_id).or_default();

                outgoing.register::<T>(make::<T>(outgoing.last_id, query.clone()));
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}

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
