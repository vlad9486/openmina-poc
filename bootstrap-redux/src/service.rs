use std::path::Path;

use mina_p2p_messages::v2;
use mina_transport::{Behaviour, OutputEvent};
use mina_tree::{Database, V2, BaseLedger, Account};
use libp2p::{
    PeerId,
    futures::StreamExt,
    swarm::{Swarm, ConnectionId},
};

pub struct LedgerStorageService {
    database: Database<V2>,
}

impl Default for LedgerStorageService {
    fn default() -> Self {
        LedgerStorageService {
            database: Database::create_with_dir(
                35,
                Some(AsRef::<Path>::as_ref("target/db").to_owned()),
            ),
        }
    }
}

impl LedgerStorageService {
    pub fn add_accounts(
        &mut self,
        accounts: Vec<v2::MinaBaseAccountBinableArgStableV2>,
    ) -> Result<(), mina_tree::DatabaseError> {
        for account in accounts {
            let account = Account::from(account);
            self.database.get_or_create_account(account.id(), account)?;
        }
        Ok(())
    }

    pub fn root_hash(&mut self) {
        // use mina_tree::{AccountIndex, MerklePath};
        // let path = self.database.merkle_path_at_index(AccountIndex(0));
        // let description = path
        //     .iter()
        //     .map(|s| match s {
        //         MerklePath::Left(hash) => format!("left:{hash:?}"),
        //         MerklePath::Right(hash) => format!("right:{hash:?}"),
        //     })
        //     .collect::<Vec<_>>()
        //     .join(",");
        // log::info!("path at 0: {description}");
        // log::info!("path at 0: {path:?}");
        let root = self.database.merkle_root();
        log::info!("hash {root:?}");
    }
}

pub struct Service {
    ctx: tokio::sync::mpsc::UnboundedSender<(PeerId, ConnectionId, Vec<u8>)>,
    pub ledger_storage: LedgerStorageService,
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

        let ledger_storage = LedgerStorageService::default();

        (
            Service {
                ctx,
                ledger_storage,
            },
            erx,
        )
    }

    pub fn send(&self, peer_id: PeerId, cn: usize, data: Vec<u8>) {
        let cn = ConnectionId::new_unchecked(cn);
        self.ctx.send((peer_id, cn, data)).unwrap_or_default();
    }
}

impl redux::TimeService for Service {}

impl redux::Service for Service {}
