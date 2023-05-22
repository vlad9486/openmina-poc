use libp2p_service::{futures::StreamExt, Behaviour, OutputEvent};
use libp2p::{
    PeerId,
    swarm::{Swarm, ConnectionId},
};

pub struct Service {
    ctx: tokio::sync::mpsc::UnboundedSender<(PeerId, ConnectionId, Vec<u8>)>,
}

type EventStream = tokio::sync::mpsc::UnboundedReceiver<OutputEvent>;

impl Service {
    pub fn spawn(mut swarm: Swarm<Behaviour>) -> (Self, EventStream) {
        use tokio::sync::mpsc;

        let (ctx, mut crx) = mpsc::unbounded_channel();
        let (etx, erx) = mpsc::unbounded_channel();
        // TODO: timeout
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = swarm.select_next_some() => etx.send(event).unwrap_or_default(),
                    cmd = crx.recv() => if let Some((peer_id, cn, data)) = cmd {
                        swarm.behaviour_mut().rpc.send(peer_id, cn, data);
                    }
                }
            }
        });

        (Service { ctx }, erx)
    }

    pub fn send(&self, peer_id: PeerId, cn: usize, data: Vec<u8>) {
        let cn = ConnectionId::new_unchecked(cn);
        self.ctx.send((peer_id, cn, data)).unwrap_or_default();
    }
}

impl redux::TimeService for Service {}

impl redux::Service for Service {}
