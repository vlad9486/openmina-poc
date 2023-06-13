mod state;
pub use self::state::State;

mod action;
pub use self::action::{Action, OutgoingAction};

mod reducer;

mod effects;

use std::fmt;

use mina_p2p_messages::rpc::{
    GetBestTipV2, AnswerSyncLedgerQueryV2, GetTransitionChainProofV1ForV2, GetTransitionChainV2,
    GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
};
use mina_p2p_messages::{
    rpc_kernel::{RpcMethod, ResponsePayload},
};
use serde::{Serialize, Deserialize};
use crate::{
    machine::{State as GlobalState, Action as GlobalAction},
    Service,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    BestTip(<GetBestTipV2 as RpcMethod>::Query),
    StagedLedgerAuxAndPendingCoinbasesAtHash(
        <GetStagedLedgerAuxAndPendingCoinbasesAtHashV2 as RpcMethod>::Query,
    ),
    SyncLedger(<AnswerSyncLedgerQueryV2 as RpcMethod>::Query),
    GetTransitionChainProof(<GetTransitionChainProofV1ForV2 as RpcMethod>::Query),
    GetTransitionChain(<GetTransitionChainV2 as RpcMethod>::Query),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    BestTip(Box<ResponsePayload<<GetBestTipV2 as RpcMethod>::Response>>),
    StagedLedgerAuxAndPendingCoinbasesAtHash(
        Box<
            ResponsePayload<<GetStagedLedgerAuxAndPendingCoinbasesAtHashV2 as RpcMethod>::Response>,
        >,
    ),
    SyncLedger(Box<ResponsePayload<<AnswerSyncLedgerQueryV2 as RpcMethod>::Response>>),
    GetTransitionChainProof(
        Box<ResponsePayload<<GetTransitionChainProofV1ForV2 as RpcMethod>::Response>>,
    ),
    GetTransitionChain(Box<ResponsePayload<<GetTransitionChainV2 as RpcMethod>::Response>>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    Heartbeat,
    Magic,
    Request { id: i64, body: Request },
    Response { id: i64, body: Response },
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BestTip(_) => write!(f, "BestTip"),
            Self::StagedLedgerAuxAndPendingCoinbasesAtHash(_) => {
                write!(f, "StagedLedgerAuxAndPendingCoinbasesAtHash")
            }
            Self::SyncLedger(_) => write!(f, "SyncLedger"),
            Self::GetTransitionChainProof(_) => write!(f, "GetTransitionChainProof"),
            Self::GetTransitionChain(_) => write!(f, "GetTransitionChain"),
        }
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BestTip(x) => match &x.0 {
                Ok(_) => write!(f, "BestTip ok"),
                Err(err) => write!(f, "BestTip err {err:?}"),
            },
            Self::StagedLedgerAuxAndPendingCoinbasesAtHash(x) => match &x.0 {
                Ok(_) => write!(f, "StagedLedgerAuxAndPendingCoinbasesAtHash ok"),
                Err(err) => write!(f, "StagedLedgerAuxAndPendingCoinbasesAtHash err {err:?}"),
            },
            Self::SyncLedger(x) => match &x.0 {
                Ok(_) => write!(f, "SyncLedger ok"),
                Err(err) => write!(f, "SyncLedger err {err:?}"),
            },
            Self::GetTransitionChainProof(x) => match &x.0 {
                Ok(_) => write!(f, "GetTransitionChainProof ok"),
                Err(err) => write!(f, "GetTransitionChainProof err {err:?}"),
            },
            Self::GetTransitionChain(x) => match &x.0 {
                Ok(_) => write!(f, "GetTransitionChain ok"),
                Err(err) => write!(f, "GetTransitionChain err {err:?}"),
            },
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Heartbeat => write!(f, "Heartbeat"),
            Self::Magic => write!(f, "Magic"),
            Self::Request { id, body } => write!(f, "Request id: {id}, body: {body}"),
            Self::Response { id, body } => write!(f, "Response id: {id}, body: {body}"),
        }
    }
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
