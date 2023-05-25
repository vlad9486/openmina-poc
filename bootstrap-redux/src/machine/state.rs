use serde::{Serialize, Deserialize};

use super::rpc::{State as RpcState, Message as RpcMessage};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub rpc: RpcState,
    pub last_msg: Vec<RpcMessage>,
}
