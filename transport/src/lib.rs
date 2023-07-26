#![forbid(unsafe_code)]

mod behaviour;
pub use self::behaviour::{Behaviour, BehaviourEvent, gossipsub};

pub mod rpc;

use libp2p::Swarm;
use libp2p::swarm::{THandlerErr, NetworkBehaviour};
use libp2p::{tcp, noise, pnet, yamux, core::upgrade, Transport};
use libp2p::{
    swarm::{SwarmBuilder, SwarmEvent},
    futures::{AsyncRead, AsyncWrite},
    identity::{self, Keypair},
    PeerId, Multiaddr,
};
pub use libp2p::identity::ed25519;

pub use libp2p::futures;

pub type OutputEvent = SwarmEvent<BehaviourEvent, THandlerErr<Behaviour>>;

/// Create a new random identity.
/// Use the same identity type as `Mina` uses.
pub fn generate_identity() -> Keypair {
    identity::Keypair::generate_ed25519()
}

/// Create and configure a libp2p swarm. This will be able to talk to the Mina node.
pub fn swarm<B, I, J>(
    local_key: Keypair,
    chain_id: &[u8],
    listen_on: J,
    peers: I,
    behaviour: B,
) -> Swarm<B>
where
    B: NetworkBehaviour,
    I: IntoIterator<Item = Multiaddr>,
    J: IntoIterator<Item = Multiaddr>,
{
    let local_peer_id = PeerId::from(local_key.public());

    let pnet = {
        use blake2::{
            digest::{Update, VariableOutput, generic_array::GenericArray},
            Blake2bVar,
        };

        let mut key = GenericArray::default();
        Blake2bVar::new(32)
            .expect("valid constant")
            .chain(b"/coda/0.0.1/")
            .chain(chain_id)
            .finalize_variable(&mut key)
            .expect("good buffer size");

        pnet::PnetConfig::new(pnet::PreSharedKey::new(key.into()))
    };
    let noise = noise::Config::new(&local_key).expect("signing libp2p-noise static keypair");
    let yamux = {
        use libp2p::core::{UpgradeInfo, InboundUpgrade, OutboundUpgrade};

        #[derive(Clone)]
        struct CodaYamux(yamux::Config);

        impl UpgradeInfo for CodaYamux {
            type Info = &'static [u8];
            type InfoIter = std::iter::Once<Self::Info>;

            fn protocol_info(&self) -> Self::InfoIter {
                std::iter::once(b"/coda/yamux/1.0.0")
            }
        }

        impl<C> InboundUpgrade<C> for CodaYamux
        where
            C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        {
            type Output = <yamux::Config as InboundUpgrade<C>>::Output;
            type Error = <yamux::Config as InboundUpgrade<C>>::Error;
            type Future = <yamux::Config as InboundUpgrade<C>>::Future;

            fn upgrade_inbound(self, socket: C, info: Self::Info) -> Self::Future {
                self.0.upgrade_inbound(socket, info)
            }
        }

        impl<C> OutboundUpgrade<C> for CodaYamux
        where
            C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        {
            type Output = <yamux::Config as OutboundUpgrade<C>>::Output;
            type Error = <yamux::Config as OutboundUpgrade<C>>::Error;
            type Future = <yamux::Config as OutboundUpgrade<C>>::Future;

            fn upgrade_outbound(self, socket: C, info: Self::Info) -> Self::Future {
                self.0.upgrade_outbound(socket, info)
            }
        }

        CodaYamux(yamux::Config::default())
    };
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .and_then(move |socket, _| pnet.handshake(socket))
        .upgrade(upgrade::Version::V1)
        .authenticate(noise)
        .multiplex(yamux)
        .timeout(std::time::Duration::from_secs(20))
        .boxed();
    // let transport = dns::TokioDnsConfig::system(transport).unwrap().boxed();
    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();
    for addr in listen_on {
        swarm.listen_on(addr).unwrap();
    }
    for peer in peers {
        swarm.dial(peer).unwrap();
    }

    swarm
}
