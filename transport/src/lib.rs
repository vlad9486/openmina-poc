#![forbid(unsafe_code)]

use libp2p::Swarm;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{tcp, noise, pnet, yamux, core::upgrade, Transport};
use libp2p::{
    swarm::SwarmBuilder,
    futures::{AsyncRead, AsyncWrite},
    identity, PeerId, Multiaddr,
};
pub use libp2p::identity::{ed25519, Keypair};

pub use libp2p::futures;

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
    let noise =
        noise::NoiseAuthenticated::xx(&local_key).expect("signing libp2p-noise static keypair");
    let yamux = {
        use std::{
            pin::Pin,
            task::{self, Context, Poll},
            io,
        };
        use libp2p::core::{UpgradeInfo, InboundUpgrade, OutboundUpgrade};

        #[derive(Clone)]
        struct CodaYamux(yamux::YamuxConfig);

        pin_project_lite::pin_project! {
            struct SocketWrapper<C> {
                #[pin]
                inner: C,
            }
        }

        impl<C> AsyncWrite for SocketWrapper<C>
        where
            C: AsyncWrite,
        {
            fn poll_write(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<io::Result<usize>> {
                let this = self.project();
                let len = task::ready!(this.inner.poll_write(cx, buf))?;
                if len != 0 {
                    log::debug!("<- {}", hex::encode(&buf[..len]));
                }

                Poll::Ready(Ok(len))
            }

            fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                let this = self.project();
                this.inner.poll_flush(cx)
            }

            fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                let this = self.project();
                this.inner.poll_close(cx)
            }
        }

        impl<C> AsyncRead for SocketWrapper<C>
        where
            C: AsyncRead,
        {
            fn poll_read(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &mut [u8],
            ) -> Poll<io::Result<usize>> {
                let this = self.project();
                let len = task::ready!(this.inner.poll_read(cx, buf))?;
                if len != 0 {
                    log::debug!("-> {}", hex::encode(&buf[..len]));
                }

                Poll::Ready(Ok(len))
            }
        }

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
            type Output = <yamux::YamuxConfig as InboundUpgrade<SocketWrapper<C>>>::Output;
            type Error = <yamux::YamuxConfig as InboundUpgrade<C>>::Error;
            type Future = <yamux::YamuxConfig as InboundUpgrade<SocketWrapper<C>>>::Future;

            fn upgrade_inbound(self, socket: C, info: Self::Info) -> Self::Future {
                self.0
                    .upgrade_inbound(SocketWrapper { inner: socket }, info)
            }
        }

        impl<C> OutboundUpgrade<C> for CodaYamux
        where
            C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        {
            type Output = <yamux::YamuxConfig as OutboundUpgrade<SocketWrapper<C>>>::Output;
            type Error = <yamux::YamuxConfig as OutboundUpgrade<C>>::Error;
            type Future = <yamux::YamuxConfig as OutboundUpgrade<SocketWrapper<C>>>::Future;

            fn upgrade_outbound(self, socket: C, info: Self::Info) -> Self::Future {
                self.0
                    .upgrade_outbound(SocketWrapper { inner: socket }, info)
            }
        }

        CodaYamux(yamux::YamuxConfig::default())
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
