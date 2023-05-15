#![forbid(unsafe_code)]

pub mod libp2p_ipc_capnp {
    include!(concat!(env!("OUT_DIR"), "/libp2p_ipc_capnp.rs"));
}

mod config;
pub use self::config::{Config, GatingConfig};

mod message;

mod process;
pub use self::process::{
    Process, PushReceiver, PushMessage, RpcClient, Libp2pError, Libp2pInternalError, StreamReader,
};
