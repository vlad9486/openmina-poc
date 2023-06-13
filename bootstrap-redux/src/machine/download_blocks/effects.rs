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
                let this_height = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .blockchain_length
                    .as_u32();
                let height = store.state().sync_transitions.height;
                log::info!("downloaded: {this_height}, epoch: {height}");
                if this_height > height + 1 {
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
                store.service().apply_block(block);
            }
        }
    }
}
