use redux::{ActionMeta, SubStore};
use mina_p2p_messages::{rpc_kernel as mina_rpc, rpc::GetBestTipV2};
use binprot::BinProtWrite;

use super::{Action, State, OutgoingAction, Request};
use crate::{
    Service,
    machine::{State as GlobalState},
};

fn make<T: mina_rpc::RpcMethod>(id: i64, query: T::Query) -> Vec<u8> {
    let msg = mina_rpc::Message::Query(mina_rpc::Query {
        tag: T::NAME.into(),
        version: T::VERSION,
        id,
        data: mina_rpc::NeedsLength(query),
    });
    let mut bytes = b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfd\x52\x50\x43\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 23) as u64;
    bytes[15..23].clone_from_slice(&len.to_le_bytes());
    bytes
}

#[allow(dead_code)]
fn heartbeat() -> Vec<u8> {
    use mina_p2p_messages::rpc_kernel::Message;

    let msg = Message::<()>::Heartbeat;
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    bytes
}

impl Action {
    pub fn effects<Store: SubStore<GlobalState, State, Service = Service, SubAction = Action>>(
        self,
        _: &ActionMeta,
        store: &mut Store,
    ) {
        match self {
            Action::Outgoing {
                peer_id,
                connection_id,
                inner: OutgoingAction::Init(request),
            } => {
                let Some(s) = store.state().outgoing.get(&(peer_id, connection_id)) else {
                    // TODO: error
                    return;
                };
                let data = match request {
                    Request::BestTip(query) => make::<GetBestTipV2>(s.last_id - 1, query),
                };
                store.service().send(peer_id, connection_id, data);
                store.dispatch(Action::Outgoing {
                    peer_id,
                    connection_id,
                    inner: OutgoingAction::Pending,
                });
            }
            Action::Outgoing {
                inner: OutgoingAction::Pending,
                ..
            } => {}
            _ => {}
        }
    }
}
