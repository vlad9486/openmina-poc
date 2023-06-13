use serde::{Serialize, Deserialize};

use mina_p2p_messages::v2;

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub blocks: Vec<v2::MinaBlockBlockStableV2>,
    pub height: u32,
}
