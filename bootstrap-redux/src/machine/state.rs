use serde::{Serialize, Deserialize};

use super::rpc::State as RpcState;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct State {
    // pub config: SnarkerConfig,

    // pub p2p: P2pState,
    pub rpc: RpcState,
    pub last_action: redux::ActionMeta,
    pub applied_actions_count: u64,
}

impl Default for State {
    fn default() -> Self {
        State {
            rpc: RpcState::default(),
            last_action: redux::ActionMeta::ZERO,
            applied_actions_count: 0,
        }
    }
}
