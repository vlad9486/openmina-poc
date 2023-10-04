use std::{str::FromStr, io};

use libp2p::{Multiaddr, PeerId};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};

pub enum Command {
    Terminate,
    Continue,
    Inner(CommandInner),
}

pub enum CommandInner {
    Connect { addr: Multiaddr },
    GetSeedPeers { peer_id: PeerId },
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("io {0}")]
    Io(#[from] io::Error),
    #[error("unrecognized")]
    Unrecognized,
    #[error("argument needed")]
    ArgumentNeeded,
    #[error("multiaddr {0}")]
    Multiaddr(#[from] libp2p::multiaddr::Error),
    #[error("peer_id {0}")]
    PeerId(#[from] libp2p::identity::ParseError),
}

impl FromStr for Command {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.split_whitespace();
        let Some(first) = words.next() else {
            return Ok(Self::Continue);
        };
        match first {
            "exit" => Ok(Self::Terminate),
            "connect" => {
                let addr = words.next().ok_or(CommandError::ArgumentNeeded)?.parse()?;
                Ok(Self::Inner(CommandInner::Connect { addr }))
            }
            "seed" => {
                let peer_id = words.next().ok_or(CommandError::ArgumentNeeded)?.parse()?;
                Ok(Self::Inner(CommandInner::GetSeedPeers { peer_id }))
            }
            _ => Err(CommandError::Unrecognized),
        }
    }
}

pub struct CommandStream(Lines<BufReader<tokio::io::Stdin>>);

impl CommandStream {
    pub fn new() -> Self {
        Self(BufReader::new(tokio::io::stdin()).lines())
    }
}

impl CommandStream {
    pub async fn next(&mut self) -> Result<Command, CommandError> {
        self.0.next_line().await?.unwrap_or_default().parse()
    }
}
