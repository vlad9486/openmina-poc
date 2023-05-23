use libp2p::{swarm::NetworkBehaviour, identity::Keypair};

pub use libp2p::gossipsub;

use super::rpc;

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub gossipsub:
        gossipsub::Behaviour<gossipsub::IdentityTransform, gossipsub::AllowAllSubscriptionFilter>,
    pub rpc: rpc::Behaviour,
}

impl Behaviour {
    pub fn topic() -> gossipsub::IdentTopic {
        gossipsub::IdentTopic::new("coda/consensus-messages/0.0.1")
    }

    pub fn new(local_key: Keypair) -> Result<Self, gossipsub::SubscriptionError> {
        let message_authenticity = gossipsub::MessageAuthenticity::Signed(local_key);
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .max_transmit_size(1024 * 1024 * 32)
            .build()
            .expect("the config must be a valid constant");
        let mut gossipsub = gossipsub::Behaviour::new(message_authenticity, gossipsub_config)
            .expect("strict validation mode must be compatible with this `message_authenticity`");
        gossipsub.subscribe(&Self::topic())?;

        let rpc = rpc::Behaviour::default();

        Ok(Behaviour { gossipsub, rpc })
    }

    pub fn publish(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        self.gossipsub.publish(Self::topic(), bytes)
    }
}
