use libp2p::{swarm::NetworkBehaviour, identity::Keypair};

pub use libp2p::gossipsub;

use super::rpc;

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    gossipsub:
        gossipsub::Behaviour<gossipsub::IdentityTransform, gossipsub::AllowAllSubscriptionFilter>,
    rpc: rpc::Behaviour,
}

impl Behaviour {
    pub fn new(local_key: Keypair) -> Self {
        let message_authenticity = gossipsub::MessageAuthenticity::Signed(local_key);
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .max_transmit_size(1024 * 1024 * 32)
            .build()
            .unwrap();
        let mut gossipsub =
            gossipsub::Behaviour::new(message_authenticity, gossipsub_config).unwrap();
        gossipsub
            .subscribe(&gossipsub::IdentTopic::new("coda/consensus-messages/0.0.1"))
            .unwrap();

        let rpc = rpc::Behaviour;

        Behaviour { gossipsub, rpc }
    }
}
