use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct State {
    pub outgoing_best_tip: bool,
}
