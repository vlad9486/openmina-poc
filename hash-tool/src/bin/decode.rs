use std::{fs::File, io::Read};

use mina_p2p_messages::{
    binprot::BinProtRead,
    rpc::GetStagedLedgerAuxAndPendingCoinbasesAtHashV2,
    rpc_kernel::{RpcMethod, DebuggerMessage},
};

fn main() {
    type T = <GetStagedLedgerAuxAndPendingCoinbasesAtHashV2 as RpcMethod>::Response;

    let mut f = File::open("target/11364").unwrap();
    f.read_exact(&mut [0; 8]).unwrap();
    let t = <DebuggerMessage<T> as BinProtRead>::binprot_read(&mut f).unwrap();
    println!("{}", serde_json::to_string_pretty(&t).unwrap());
}
