use std::{
    io,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    os::fd::{AsRawFd, RawFd},
    ffi::OsStr,
    collections::BTreeMap,
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
pub enum Error {
    #[error("{_0}")]
    Io(#[from] io::Error),
    #[error("{_0}")]
    Capnp(#[from] capnp::Error),
    #[error("libp2p pipe closed")]
    Closed,
    #[error("custom {_0}")]
    Custom(String),
}

#[derive(Debug, Error)]
pub enum InternalError {
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
    stdout_handler: thread::JoinHandle<Result<ChildStdout, InternalError>>,
    push_tx: mpsc::Sender<PushEvent>,
}

impl Process {
    pub fn spawn<S: AsRef<OsStr>>(program: S) -> (Self, Stream, Client) {
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
        let stdout_handler = thread::spawn({
            let push_tx = push_tx.clone();
            move || {
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
                                return Err(InternalError::Capnp(err));
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
                                .map_err(InternalError::Capnp)?;
                            match root.which() {
                                Err(x) => return Err(InternalError::NotInSchema(x.0)),
                                Ok(message::RpcResponse(Ok(_))) => {
                                    rpc_tx.send(reader).unwrap_or_default()
                                }
                                Ok(message::RpcResponse(Err(err))) => {
                                    return Err(InternalError::BadRpcReader(err))
                                }
                                Ok(message::PushMessage(Ok(_))) => push_tx
                                    .send(PushEvent::RawReader(reader))
                                    .unwrap_or_default(),
                                Ok(message::PushMessage(Err(err))) => {
                                    return Err(InternalError::BadPushReader(err))
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
            }
        });

        (
            Process {
                this,
                stdin_fd,
                ctx,
                stdout_handler,
                push_tx: push_tx.clone(),
            },
            Stream::new(push_rx),
            Client {
                stdin,
                rpc_rx,
                push_tx,
            },
        )
    }

    pub fn stop_receiving(&self) {
        self.push_tx.send(PushEvent::Terminate).unwrap_or_default();
        self.ctx.send(None).unwrap_or_default()
    }

    pub fn stop(mut self) -> Result<Option<i32>, InternalError> {
        nix::unistd::close(self.stdin_fd).unwrap();

        let status = self.this.wait()?;

        // read remaining data in pipe
        let _stdout = self.stdout_handler.join().unwrap()?;

        Ok(status.code())
    }
}

pub struct Client {
    stdin: ChildStdin,
    rpc_rx: mpsc::Receiver<Reader<OwnedSegments>>,
    push_tx: mpsc::Sender<PushEvent>,
}

impl Client {
    fn inner(&mut self, msg: Msg) -> Result<Reader<OwnedSegments>, Error> {
        let mut message = Builder::new_default();
        msg.build(message.init_root())?;
        serialize::write_message(&mut self.stdin, &message)?;
        self.rpc_rx.recv().map_err(|_| Error::Closed)
    }

    pub fn generate_keypair(&mut self) -> Result<(String, Vec<u8>, Vec<u8>), Error> {
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

    pub fn configure(&mut self, config: Config) -> Result<(), Error> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Configure(config)))?;

        Ok(())
    }

    pub fn list_peers(&mut self) -> Result<(), Error> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::ListPeers))?;

        Ok(())
    }

    pub fn subscribe(&mut self, id: u64, topic: String) -> Result<(), Error> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Subscribe { id, topic }))?;

        Ok(())
    }

    pub fn publish(&mut self, topic: String, data: Vec<u8>) -> Result<(), Error> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::Publish { topic, data }))?;

        Ok(())
    }

    pub fn open_stream(
        &mut self,
        peer_id: &str,
        protocol: &str,
    ) -> Result<(u64, StreamReader), Error> {
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
                        let stream_id = reader.get_stream_id()?.get_id();
                        let (tx, rx) = mpsc::channel();
                        self.push_tx
                            .send(PushEvent::StreamSender(stream_id, tx))
                            .unwrap_or_default();

                        Ok((
                            stream_id,
                            StreamReader {
                                stream_rx: rx,
                                remaining: None,
                            },
                        ))
                    }
                    _ => panic!(),
                },
                Ok(rpc_response::Error(Ok(err))) => Err(Error::Custom(err.to_string())),
                _ => panic!(),
            }
        } else {
            unreachable!("checked above");
        }
    }

    pub fn send_stream(&mut self, stream_id: u64, data: Vec<u8>) -> Result<(), Error> {
        let _response = self.inner(Msg::RpcRequest(RpcRequest::SendStream { data, stream_id }))?;

        Ok(())
    }
}

pub enum Message {
    Terminate,
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
        reader: StreamReader,
    },
    StreamComplete {
        stream_id: u64,
    },
    StreamLost {
        stream_id: u64,
    },
}

pub struct Stream {
    push_rx: mpsc::Receiver<PushEvent>,
    stream_writers: BTreeMap<u64, mpsc::Sender<Reader<OwnedSegments>>>,
}

enum PushEvent {
    RawReader(Reader<OwnedSegments>),
    StreamSender(u64, mpsc::Sender<Reader<OwnedSegments>>),
    Terminate,
}

pub struct StreamReader {
    stream_rx: mpsc::Receiver<Reader<OwnedSegments>>,
    remaining: Option<(usize, Reader<OwnedSegments>)>,
}

impl io::Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some((pos, v)) = self.remaining.take() {
            let root = v.get_root::<message::Reader>().expect("checked above");
            if let Ok(message::PushMessage(Ok(reader))) = root.which() {
                match reader.which() {
                    Ok(push_message::StreamMessageReceived(reader)) => {
                        let data = reader
                            .expect("checked above")
                            .get_msg()
                            .expect("checked above")
                            .get_data()
                            .expect("checked above");
                        if data.len() > pos {
                            let len = (data.len() - pos).min(buf.len());
                            buf[..len].clone_from_slice(&data[pos..(pos + len)]);
                            self.remaining = Some((pos + len, v));
                            return Ok(len);
                        } else if data.len() < pos {
                            panic!();
                        }
                    }
                    _ => unreachable!("checked above"),
                }
            } else {
                unreachable!("checked above");
            }
        }
        match self.stream_rx.recv() {
            Err(_) => return Ok(0),
            Ok(v) => {
                let root = v.get_root::<message::Reader>().expect("checked above");
                if let Ok(message::PushMessage(Ok(reader))) = root.which() {
                    // TODO: handle errors
                    match reader.which() {
                        Ok(push_message::StreamMessageReceived(reader)) => {
                            let data = reader
                                .expect("checked above")
                                .get_msg()
                                .expect("checked above")
                                .get_data()
                                .expect("checked above");
                            let len = data.len().min(buf.len());
                            buf[..len].clone_from_slice(&data[..len]);
                            if len < data.len() {
                                self.remaining = Some((len, v));
                            }
                            Ok(len)
                        }
                        _ => unreachable!("checked above"),
                    }
                } else {
                    unreachable!("checked above");
                }
            }
        }
    }
}

impl Stream {
    fn new(push_rx: mpsc::Receiver<PushEvent>) -> Self {
        Stream {
            push_rx,
            stream_writers: BTreeMap::default(),
        }
    }

    pub fn recv(&mut self) -> Result<Message, Error> {
        loop {
            if let Some(x) = self.recv_inner()? {
                break Ok(x);
            }
        }
    }

    fn recv_inner(&mut self) -> Result<Option<Message>, Error> {
        match self.push_rx.recv() {
            Err(_) => return Err(Error::Closed),
            Ok(PushEvent::Terminate) => Ok(Some(Message::Terminate)),
            Ok(PushEvent::StreamSender(stream_id, tx)) => {
                self.stream_writers.insert(stream_id, tx);
                Ok(None)
            }
            Ok(PushEvent::RawReader(v)) => {
                let root = v.get_root::<message::Reader>().expect("checked above");
                if let Ok(message::PushMessage(Ok(reader))) = root.which() {
                    // TODO: handle errors
                    match reader.which() {
                        Ok(push_message::PeerConnected(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer_id()?.get_id()?.to_owned();
                            Ok(Some(Message::Connected { peer }))
                        }
                        Ok(push_message::PeerDisconnected(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer_id()?.get_id()?.to_owned();
                            Ok(Some(Message::Disconnected { peer }))
                        }
                        Ok(push_message::GossipReceived(reader)) => {
                            let reader = reader?;
                            let sender = reader.get_sender()?.get_peer_id()?.get_id()?.to_owned();
                            Ok(Some(Message::Gossip { sender }))
                        }
                        Ok(push_message::IncomingStream(reader)) => {
                            let reader = reader?;
                            let peer = reader.get_peer()?;
                            let peer_id = peer.get_peer_id()?.get_id()?.to_owned();
                            let peer_host = peer.get_host()?.to_owned();
                            let peer_port = peer.get_libp2p_port();
                            let protocol = reader.get_protocol()?.to_owned();
                            let stream_id = reader.get_stream_id()?.get_id();
                            log::info!("Push incoming stream {stream_id}");
                            Ok(Some(Message::IncomingStream {
                                peer_id,
                                peer_host,
                                peer_port,
                                protocol,
                                stream_id,
                                reader: {
                                    let (tx, rx) = mpsc::channel();
                                    self.stream_writers.insert(stream_id, tx);
                                    StreamReader {
                                        stream_rx: rx,
                                        remaining: None,
                                    }
                                },
                            }))
                        }
                        Ok(push_message::StreamMessageReceived(reader)) => {
                            let reader = reader?.get_msg()?;
                            let stream_id = reader.get_stream_id()?.get_id();
                            if let Some(writer) = self.stream_writers.get(&stream_id) {
                                if let Err(_) = writer.send(v) {
                                    log::warn!("received message, stream has no rx");
                                }
                            } else {
                                log::error!("received message, no such stream: {stream_id}");
                            }
                            Ok(None)
                        }
                        Ok(push_message::StreamComplete(reader)) => {
                            let reader = reader?;
                            let stream_id = reader.get_stream_id()?.get_id();
                            self.stream_writers.remove(&stream_id);
                            log::info!("Push stream complete {stream_id}");
                            Ok(Some(Message::StreamComplete { stream_id }))
                        }
                        Ok(push_message::StreamLost(reader)) => {
                            let reader = reader?;
                            let stream_id = reader.get_stream_id()?.get_id();
                            self.stream_writers.remove(&stream_id);
                            log::info!("Push stream lost {stream_id}");
                            Ok(Some(Message::StreamLost { stream_id }))
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
