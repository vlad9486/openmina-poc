use redux::{Store, ActionWithMeta};

use super::{state::State, action::Action, service::Service};

use mina_p2p_messages::{rpc_kernel as mina_rpc, rpc::GetBestTipV2};
use binprot::BinProtWrite;

fn heartbeat() -> Vec<u8> {
    let msg = mina_rpc::Message::<()>::Heartbeat;
    let mut bytes = b"\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    msg.binprot_write(&mut bytes).unwrap();
    let len = (bytes.len() - 8) as u64;
    bytes[0..8].clone_from_slice(&len.to_le_bytes());
    bytes
}

fn query<T: mina_rpc::RpcMethod>(id: i64, query: T::Query) -> Vec<u8> {
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

pub fn run(store: &mut Store<State, Service, Action>, action: ActionWithMeta<Action>) {
    match action.action() {
        Action::RpcMessage {
            peer_id,
            connection_id,
            bytes,
        } => {
            log::info!("recv {peer_id}, {connection_id:?}, {}", hex::encode(bytes));
            store.service().send(*peer_id, *connection_id, heartbeat());
        }
        Action::RpcNegotiated {
            peer_id,
            connection_id,
        } => {
            store
                .service()
                .send(*peer_id, *connection_id, query::<GetBestTipV2>(0, ()));
        }
        _ => {}
    }
}
