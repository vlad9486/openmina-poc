use std::{
    io,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    os::fd::{AsRawFd, RawFd},
    ffi::OsStr,
};

use capnp::{
    message::{Builder, Reader},
    serialize::{self, OwnedSegments},
};
use thiserror::Error;

use crate::libp2p_ipc_capnp::{
    daemon_interface::{message, push_message},
    libp2p_helper_interface::{rpc_response, rpc_response_success},
};

use super::{
    config::Config,
    message::{CapnpEncode, Msg, RpcRequest},
};

#[derive(Debug, Error)]
pub enum Libp2pError {
    #[error("{_0}")]
    Io(#[from] io::Error),
    #[error("{_0}")]
    Capnp(#[from] capnp::Error),
    #[error("libp2p pipe closed")]
    Closed,
}

#[derive(Debug, Error)]
pub enum Libp2pInternalError {
    #[error("{_0}")]
    Io(#[from] io::Error),
    #[error("{_0}")]
    Capnp(#[from] capnp::Error),
    #[error("not in schema {_0}")]
    NotInSchema(u16),
    #[error("RPC: {_0}")]
    BadRpcReader(capnp::Error),
    #[error("Push: {_0}")]
    BadPushReader(capnp::Error),
}

pub struct Process {
    this: Child,
    stdin_fd: RawFd,
    ctx: mpsc::Sender<Option<Reader<OwnedSegments>>>,
    stdout_handler: thread::JoinHandle<Result<ChildStdout, Libp2pInternalError>>,
}

impl Process {
    pub fn spawn<S: AsRef<OsStr>>(program: S) -> (Self, PushReceiver, RpcClient) {
        let mut this = Command::new(program)
            .envs(std::env::vars())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("launcher executable");
        let stdin = this.stdin.take().expect("must be present");
        let stdin_fd = stdin.as_raw_fd();
        let stdout = this.stdout.take().expect("must be present");

        let (push_tx, push_rx) = mpsc::channel();
        let (rpc_tx, rpc_rx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel::<Option<capnp::message::Reader<OwnedSegments>>>();
        let c_ctx = ctx.clone();
        let stdout_handler = thread::spawn(move || {
            let mut stdout = stdout;
            let inner = thread::spawn(move || {
                loop {
                    match serialize::read_message(&mut stdout, Default::default()) {
                        Ok(x) => c_ctx.send(Some(x)).unwrap_or_default(),
                        Err(err) if err.description == "Premature end of file" => {
                            c_ctx.send(None).unwrap_or_default();
                            break;
                        }
                        Err(err) => {
                            c_ctx.send(None).unwrap_or_default();
                            return Err(Libp2pInternalError::Capnp(err));
                        }
                    }
                }
                Ok(stdout)
            });
            loop {
                match crx.recv() {
                    Ok(Some(reader)) => {
                        let root = reader
                            .get_root::<message::Reader>()
                            .map_err(Libp2pInternalError::Capnp)?;
                        match root.which() {
                            Err(x) => return Err(Libp2pInternalError::NotInSchema(x.0)),
                            Ok(message::RpcResponse(Ok(_))) => {
                                rpc_tx.send(reader).unwrap_or_default()
                            }
                            Ok(message::RpcResponse(Err(err))) => {
                                return Err(Libp2pInternalError::BadRpcReader(err))
                            }
                            Ok(message::PushMessage(Ok(_))) => {
                                push_tx.send(reader).unwrap_or_default()
                            }
                            Ok(message::PushMessage(Err(err))) => {
                                return Err(Libp2pInternalError::BadPushReader(err))
                            }
                        }
                    }
                    Ok(None) => {
                        log::info!("complete receiving");
                        break;
                    }
                    Err(_) => break,
                }
            }
            drop((push_tx, rpc_tx));
            inner.join().unwrap()
        });

        (
            Process {
                this,
                stdin_fd,
                ctx,
                stdout_handler,
            },
            PushReceiver(push_rx),
            RpcClient { stdin, rpc_rx },
        )
    }

    pub fn stop_receiving(&self) {
        self.ctx.send(None).unwrap_or_default()
    }

    pub fn stop(mut self) -> Result<Option<i32>, Libp2pInternalError> {
        nix::unistd::close(self.stdin_fd).unwrap();

        let status = self.this.wait()?;

        // read remaining data in pipe
        let _stdout = self.stdout_handler.join().unwrap()?;

        Ok(status.code())
    }
}

pub struct RpcClient {
    stdin: ChildStdin,
    rpc_rx: mpsc::Receiver<Reader<OwnedSegments>>,
}

impl RpcClient {
    fn inner(&mut self, msg: Msg) -> Result<Reader<OwnedSegments>, Libp2pError> {
        let mut message = Builder::new_default();
        msg.build(message.init_root())?;
        serialize::write_message(&mut self.stdin, &message)?;
        self.rpc_rx.recv().map_err(|_| Libp2pError::Closed)
    }

    pub fn generate_keypair(&mut self) -> Result<(String, Vec<u8>, Vec<u8>), Libp2pError> {
        let response = self.inner(Msg::RpcRequest(RpcRequest::GenerateKeypair))?;

        let root = response
            .get_root::<message::Reader>()
            .expect("checked above");
        if let Ok(message::RpcResponse(Ok(rpc_response))) = root.which() {
            match rpc_response.which() {
                Ok(rpc_response::Success(Ok(x))) => {
                    // TODO: handle errors
                    match x.which() {
                        Ok(rpc_response_success::GenerateKeypair(x)) => {
                            let x = x?.get_result()?;
                            let peer_id = x.get_peer_id()?.get_id()?.to_owned();
                            let public_key = x.get_public_key()?.to_owned();
                            let secret_key = x.get_private_key()?.to_owned();
                            Ok((peer_id, public_key, secret_key))
                        }
                        _ => panic!(),
                    }
                }
                _ => panic!(),
            }
        } else {
            unreachable!("checked above");
        }
    }

    pub fn configure(&mut self, config: Config) -> Result<(), Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Configure(config)))?;

        Ok(())
    }

    pub fn list_peers(&mut self) -> Result<(), Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::ListPeers))?;

        Ok(())
    }

    pub fn subscribe(&mut self, id: u64, topic: String) -> Result<(), Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Subscribe { id, topic }))?;

        Ok(())
    }

    pub fn publish(&mut self, topic: String, data: Vec<u8>) -> Result<(), Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Publish { topic, data }))?;

        Ok(())
    }

    pub fn open_stream(&mut self, peer_id: &str, protocol: &str) -> Result<u64, Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::AddStreamHandler {
            protocol: protocol.to_owned(),
        }))?;

        let response = self.inner(Msg::RpcRequest(RpcRequest::OpenStream {
            peer_id: peer_id.to_owned(),
            protocol: protocol.to_owned(),
        }))?;
        let root = response
            .get_root::<message::Reader>()
            .expect("checked above");
        if let Ok(message::RpcResponse(Ok(reader))) = root.which() {
            // TODO: handle errors
            match reader.which() {
                Ok(rpc_response::Success(Ok(reader))) => match reader.which() {
                    Ok(rpc_response_success::OpenStream(Ok(reader))) => {
                        Ok(reader.get_stream_id()?.get_id())
                    }
                    _ => panic!(),
                },
                _ => panic!(),
            }
        } else {
            unreachable!("checked above");
        }
    }

    pub fn send_stream(&mut self, stream_id: u64, data: Vec<u8>) -> Result<(), Libp2pError> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::SendStream { data, stream_id }))?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum PushMessage {
    Connected {
        peer: String,
    },
    Disconnected {
        peer: String,
    },
    Gossip {
        sender: String,
    },
    IncomingStream {
        peer_id: String,
        peer_host: String,
        peer_port: u16,
        protocol: String,
        stream_id: u64,
    },
    StreamMessage {
        data: Vec<u8>,
        stream_id: u64,
    },
    StreamComplete {
        stream_id: u64,
    },
    StreamLost {
        stream_id: u64,
    },
}

pub struct PushReceiver(mpsc::Receiver<Reader<OwnedSegments>>);

impl PushReceiver {
    pub fn recv(&self) -> Result<PushMessage, Libp2pError> {
        match self.0.recv() {
            Err(_) => return Err(Libp2pError::Closed),
            Ok(v) => {
                let root = v.get_root::<message::Reader>().expect("checked above");
                if let Ok(message::PushMessage(Ok(reader))) = root.which() {
                    // TODO: handle errors
                    match reader.which() {
                        Ok(push_message::PeerConnected(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer_id()?.get_id()?.to_owned();
                            Ok(PushMessage::Connected { peer })
                        }
                        Ok(push_message::PeerDisconnected(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer_id()?.get_id()?.to_owned();
                            Ok(PushMessage::Disconnected { peer })
                        }
                        Ok(push_message::GossipReceived(reader)) => {
                            let reader = reader?;
                            let sender = reader.get_sender()?.get_peer_id()?.get_id()?.to_owned();
                            Ok(PushMessage::Gossip { sender })
                        }
                        Ok(push_message::IncomingStream(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer()?;
                            Ok(PushMessage::IncomingStream {
                                peer_id: peer.get_peer_id()?.get_id()?.to_owned(),
                                peer_host: peer.get_host()?.to_owned(),
                                peer_port: peer.get_libp2p_port(),
                                protocol: reader.get_protocol()?.to_owned(),
                                stream_id: reader.get_stream_id()?.get_id(),
                            })
                        }
                        Ok(push_message::StreamMessageReceived(reader)) => {
                            let reader = reader?.get_msg()?;
                            let data = reader.get_data()?.to_owned();
                            let stream_id = reader.get_stream_id()?.get_id();
                            Ok(PushMessage::StreamMessage { data, stream_id })
                        }
                        Ok(push_message::StreamComplete(reader)) => {
                            let reader = reader?;
                            let stream_id = reader.get_stream_id()?.get_id();
                            Ok(PushMessage::StreamComplete { stream_id })
                        }
                        Ok(push_message::StreamLost(reader)) => {
                            let reader = reader?;
                            let stream_id = reader.get_stream_id()?.get_id();
                            Ok(PushMessage::StreamLost { stream_id })
                        }
                        _ => panic!(),
                    }
                } else {
                    unreachable!("checked above");
                }
            }
        }
    }
}
