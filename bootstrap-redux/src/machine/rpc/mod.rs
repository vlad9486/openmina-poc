mod state;
pub use self::state::State;

mod action;
pub use self::action::{Action, OutgoingAction};

mod reducer;

mod effects;

use mina_p2p_messages::{
    rpc_kernel::{RpcMethod, ResponsePayload},
    rpc::GetBestTipV2,
};
use serde::{Serialize, Deserialize};
use crate::{
    machine::{State as GlobalState, Action as GlobalAction},
    Service,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    BestTip(<GetBestTipV2 as RpcMethod>::Query),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    BestTip(ResponsePayload<<GetBestTipV2 as RpcMethod>::Response>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    Heartbeat,
    Magic,
    Request { id: i64, body: Request },
    Response { id: i64, body: Response },
}

impl redux::SubStore<GlobalState, State> for redux::Store<GlobalState, Service, GlobalAction> {
    type SubAction = Action;
    type Service = Service;

    fn state(&self) -> &State {
        &self.state().rpc
    }

    fn service(&mut self) -> &mut Self::Service {
        self.service()
    }

    fn state_and_service(&mut self) -> (&State, &mut Self::Service) {
        (&self.state.get().rpc, &mut self.service)
    }

    fn dispatch<A>(&mut self, action: A) -> bool
    where
        A: Into<Self::SubAction> + redux::EnablingCondition<GlobalState>,
    {
        self.sub_dispatch(action)
    }
}
