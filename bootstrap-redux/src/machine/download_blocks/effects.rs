use redux::{ActionMeta, Store};

use super::{
    Action,
    super::rpc::{Action as RpcAction, OutgoingAction as RpcOutgoingAction, Request as RpcRequest},
};
use crate::{
    Service,
    machine::{State as GlobalState, Action as GlobalAction},
};

impl Action {
    pub fn effects(self, _: &ActionMeta, store: &mut Store<GlobalState, Service, GlobalAction>) {
        match self {
            Action::Continue(block) => {
                let slot = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .curr_global_slot
                    .slot_number
                    .as_u32();
                let epoch_slot = store.state().sync_transitions.epoch_slot;
                log::info!("downloaded: {slot}, epoch: {epoch_slot}");
                if epoch_slot < slot {
                    let prev = block.header.protocol_state.previous_state_hash.0.clone();
                    // TODO: choose most suitable peer
                    let mut peers = store.state().rpc.outgoing.keys();
                    let (peer_id, connection_id) = peers.next().unwrap();
                    store.dispatch(RpcAction::Outgoing {
                        peer_id: *peer_id,
                        connection_id: *connection_id,
                        inner: RpcOutgoingAction::Init(RpcRequest::GetTransitionChain(vec![prev])),
                    });
                } else {
                    if let Some(last) = store.state().sync_transitions.blocks.last() {
                        store.dispatch::<GlobalAction>(Action::Apply(last.clone()).into());
                    }
                }
            }
            Action::Apply(block) => {
                let slot = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .curr_global_slot
                    .slot_number
                    .as_u32();
                log::info!("will apply: {slot}");
                store.service().ledger_storage.apply_block(&block);
                if let Some(last) = store.state().sync_transitions.blocks.last() {
                    store.dispatch::<GlobalAction>(Action::Apply(last.clone()).into());
                }
            }
        }
    }
}
