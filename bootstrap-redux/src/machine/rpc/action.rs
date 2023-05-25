use libp2p::PeerId;
use serde::{Serialize, Deserialize};

use super::{Request, State, Response};
use crate::machine::State as GlobalState;

#[derive(derive_more::From, Serialize, Deserialize, Debug, Clone)]
pub enum Action {
    Outgoing {
        peer_id: PeerId,
        connection_id: usize,
        inner: OutgoingAction,
    },
    Incoming {
        peer_id: PeerId,
        connection_id: usize,
        response: Response,
    },
    Heartbeat {
        peer_id: PeerId,
        connection_id: usize,
    },
}

#[derive(derive_more::From, Serialize, Deserialize, Debug, Clone)]
pub enum OutgoingAction {
    Init(Request),
    Pending,
}

impl redux::EnablingCondition<State> for OutgoingAction {
    fn is_enabled(&self, state: &State) -> bool {
        match self {
            Self::Init(Request::BestTip(_)) => !state.outgoing_best_tip,
            Self::Init(Request::SyncLedger(_)) => true,
            _ => true,
        }
    }
}

impl redux::EnablingCondition<State> for Action {
    fn is_enabled(&self, state: &State) -> bool {
        match self {
            Self::Outgoing { inner, .. } => inner.is_enabled(state),
            Self::Incoming { .. } => true,
            Self::Heartbeat { .. } => true,
        }
    }
}

impl redux::EnablingCondition<GlobalState> for Action {
    fn is_enabled(&self, state: &GlobalState) -> bool {
        self.is_enabled(&state.rpc)
    }
}
