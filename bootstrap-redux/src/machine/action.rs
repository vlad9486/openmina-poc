use libp2p::PeerId;
use serde::{Serialize, Deserialize};

use super::{state::State, rpc::Action as RpcAction};

#[derive(derive_more::From, Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    PeerConnectionEstablished {
        peer_id: PeerId,
    },
    GossipMessage,
    RpcNegotiated {
        peer_id: PeerId,
        connection_id: usize,
    },
    RpcMessage {
        peer_id: PeerId,
        connection_id: usize,
        bytes: Vec<u8>,
    },
    Rpc(RpcAction),
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, state: &State) -> bool {
        match self {
            Action::Rpc(inner) => inner.is_enabled(&state.rpc),
            Action::RpcMessage { bytes, .. } => !bytes.is_empty(),
            _ => true,
        }
    }
}
