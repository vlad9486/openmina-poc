use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    // pub config: SnarkerConfig,

    // pub p2p: P2pState,
    // pub rpc: RpcState,

    // pub last_action: ActionMeta,
    // pub applied_actions_count: u64,
}
