use std::{path::Path, thread, sync::mpsc, collections::BTreeMap};

use mina_p2p_messages::{v2, rpc::GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response};
use mina_transport::{Behaviour, OutputEvent as P2pEvent};
use mina_tree::{
    Database, Account, BaseLedger,
    mask::Mask,
    staged_ledger::staged_ledger::StagedLedger,
    scan_state::{
        scan_state::{
            ConstraintConstants,
            transaction_snark::{work::Work, OneOrTwo},
        },
        currency::{Amount, Fee},
        transaction_logic::protocol_state,
        self,
        protocol_state::MinaHash,
    },
    verifier::Verifier,
};
use mina_signer::CompressedPubKey;

use libp2p::{
    PeerId,
    futures::StreamExt,
    swarm::{Swarm, ConnectionId},
};

pub struct LedgerStorageService {
    epoch_ledger: Mask,
    staged_ledger: Option<StagedLedger>,
}

impl Default for LedgerStorageService {
    fn default() -> Self {
        LedgerStorageService {
            epoch_ledger: Mask::new_root(Database::create_with_dir(
                35,
                Some(AsRef::<Path>::as_ref("target/db").to_owned()),
            )),
            staged_ledger: None,
        }
    }
}

impl LedgerStorageService {
    const CONSTRAINT_CONSTANTS: ConstraintConstants = ConstraintConstants {
        sub_windows_per_window: 11,
        ledger_depth: 35,
        work_delay: 2,
        block_window_duration_ms: 180000,
        transaction_capacity_log_2: 7,
        pending_coinbase_depth: 5,
        coinbase_amount: Amount::from_u64(720000000000),
        supercharged_coinbase_factor: 2,
        account_creation_fee: Fee::from_u64(1000000000),
        fork: None,
    };

    pub fn add_accounts(
        &mut self,
        accounts: Vec<v2::MinaBaseAccountBinableArgStableV2>,
    ) -> Result<(), mina_tree::DatabaseError> {
        for account in accounts {
            let account = Account::from(account);
            self.epoch_ledger
                .get_or_create_account(account.id(), account)?;
        }
        Ok(())
    }

    pub fn root_hash(&mut self) {
        let root = self.epoch_ledger.merkle_root();
        log::info!("hash {root:?}");
    }

    pub fn init(&mut self, info: GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response) {
        let Some((scan_state, expected_ledger_hash, pending_coinbase, states)) = info else {
            return;
        };

        // TODO: https://github.com/name-placeholder/ledger/blob/25d9ee54dfc664e8fcb2d6fe72b1c63bceec6d19/src/staged_ledger/staged_ledger.rs#L351
        // log::info!("obtain staged ledger: {expected_ledger_hash:?}");

        let states = states
            .into_iter()
            .map(|state| (state.hash(), state))
            .collect::<BTreeMap<_, _>>();

        let _ = (
            &mut self.staged_ledger,
            scan_state,
            expected_ledger_hash,
            pending_coinbase,
            states,
        );
        // self.staged_ledger = StagedLedger::of_scan_state_pending_coinbases_and_snarked_ledger(
        //     (),
        //     &Self::CONSTRAINT_CONSTANTS,
        //     Verifier,
        //     scan_state.scan_state,
        //     self.epoch_ledger.clone(),
        //     (),
        //     expected_ledger_hash.into(),
        //     pending_coinbase.into(),
        //     |key| states.get(&key).cloned().unwrap(),
        // )
        // .ok();
        unimplemented!()
    }

    pub fn apply_block(&mut self, block: &v2::MinaBlockBlockStableV2) {
        let length = block
            .header
            .protocol_state
            .body
            .consensus_state
            .blockchain_length
            .as_u32();
        let previous_state_hash = block.header.protocol_state.previous_state_hash.clone();
        log::info!("will apply: {length} prev: {previous_state_hash}");

        let staged_ledger = self.staged_ledger.as_mut().unwrap();
        let global_slot = block
            .header
            .protocol_state
            .body
            .consensus_state
            .curr_global_slot
            .slot_number
            .clone();
        let coinbase_receiver = match &block.body.staged_ledger_diff.diff.0.coinbase {
            v2::StagedLedgerDiffDiffPreDiffWithAtMostTwoCoinbaseStableV2Coinbase::One(Some(pk)) => {
                pk.receiver_pk.clone()
            }
            _ => {
                let addr = block
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .block_creator
                    .to_string();

                let pk = CompressedPubKey::from_address(&addr).unwrap();
                (&pk).into()
            }
        };
        let current_state_view = protocol_state::protocol_state_view(&block.header.protocol_state);
        let works_two = block
            .body
            .staged_ledger_diff
            .diff
            .0
            .completed_works
            .iter()
            .map(|work| Work {
                fee: (&work.fee).into(),
                proofs: match &work.proofs {
                    v2::TransactionSnarkWorkTStableV2Proofs::One(x) => OneOrTwo::One(x.into()),
                    v2::TransactionSnarkWorkTStableV2Proofs::Two((x, y)) => {
                        OneOrTwo::Two((x.into(), y.into()))
                    }
                },
                prover: (&work.prover).into(),
            });
        let works_one = block
            .body
            .staged_ledger_diff
            .diff
            .1
            .as_ref()
            .map(|x| {
                x.completed_works.iter().map(|work| Work {
                    fee: (&work.fee).into(),
                    proofs: match &work.proofs {
                        v2::TransactionSnarkWorkTStableV2Proofs::One(x) => OneOrTwo::One(x.into()),
                        v2::TransactionSnarkWorkTStableV2Proofs::Two((x, y)) => {
                            OneOrTwo::Two((x.into(), y.into()))
                        }
                    },
                    prover: (&work.prover).into(),
                })
            })
            .into_iter()
            .flatten();
        let works = works_two.chain(works_one).collect::<Vec<_>>();
        let transactions_by_fee_two = block.body.staged_ledger_diff.diff.0.commands.iter();
        let transactions_by_fee_one = block
            .body
            .staged_ledger_diff
            .diff
            .1
            .as_ref()
            .map(|x| x.commands.iter())
            .into_iter()
            .flatten();
        let transactions_by_fee = transactions_by_fee_two
            .chain(transactions_by_fee_one)
            .map(|x| (&x.data).into())
            .collect();
        let (diff, _) = staged_ledger
            .create_diff(
                &Self::CONSTRAINT_CONSTANTS,
                (&global_slot).into(),
                None,
                (&coinbase_receiver).into(),
                (),
                &current_state_view,
                transactions_by_fee,
                |key| works.iter().find(|x| x.statement() == *key).cloned(),
                false,
            )
            .unwrap();

        let result = staged_ledger
            .apply(
                None,
                &Self::CONSTRAINT_CONSTANTS,
                (&global_slot).into(),
                diff.forget(),
                (),
                &Verifier,
                &current_state_view,
                scan_state::protocol_state::hashes(&block.header.protocol_state),
                (&coinbase_receiver).into(),
                false,
            )
            .unwrap();
        log::info!("applied: {:?}", result.hash_after_applying)
    }
}

pub struct Service {
    ctx: tokio::sync::mpsc::UnboundedSender<(PeerId, ConnectionId, Vec<u8>)>,
    ledger_ctx: mpsc::Sender<LedgerCommand>,
}

enum LedgerCommand {
    AddAccounts(Vec<v2::MinaBaseAccountBinableArgStableV2>),
    PrintRootHash,
    ApplyBlock(v2::MinaBlockBlockStableV2),
    Init(GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response),
}

type EventStream = tokio::sync::mpsc::UnboundedReceiver<ServiceEvent>;

pub enum ServiceEvent {
    P2p(P2pEvent),
    SyncLedgerDone,
    ApplyBlockDone,
}

impl Service {
    pub fn spawn(mut swarm: Swarm<Behaviour>) -> (Self, EventStream) {
        use tokio::sync::mpsc as tokio_mpsc;

        let (ctx, mut crx) = tokio_mpsc::unbounded_channel();
        let (etx, erx) = tokio_mpsc::unbounded_channel();
        // TODO: timeout
        tokio::spawn({
            let etx = etx.clone();
            async move {
                loop {
                    tokio::select! {
                        event = swarm.select_next_some() => etx.send(ServiceEvent::P2p(event)).unwrap_or_default(),
                        cmd = crx.recv() => if let Some((peer_id, cn, data)) = cmd {
                            swarm.behaviour_mut().rpc.send(peer_id, cn, data);
                        }
                    }
                }
            }
        });

        let (ledger_ctx, crx) = mpsc::channel();
        thread::spawn(move || {
            let mut ledger_storage = LedgerStorageService::default();
            while let Ok(cmd) = crx.recv() {
                match cmd {
                    LedgerCommand::AddAccounts(accounts) => {
                        ledger_storage.add_accounts(accounts).unwrap()
                    }
                    LedgerCommand::Init(init) => {
                        ledger_storage.init(init);
                    }
                    LedgerCommand::PrintRootHash => {
                        ledger_storage.root_hash();
                        etx.send(ServiceEvent::SyncLedgerDone).unwrap_or_default();
                    }
                    LedgerCommand::ApplyBlock(block) => {
                        ledger_storage.apply_block(&block);
                        etx.send(ServiceEvent::ApplyBlockDone).unwrap_or_default();
                    }
                }
            }
        });

        (Service { ctx, ledger_ctx }, erx)
    }

    pub fn send(&self, peer_id: PeerId, cn: usize, data: Vec<u8>) {
        let cn = ConnectionId::new_unchecked(cn);
        self.ctx.send((peer_id, cn, data)).unwrap_or_default();
    }

    pub fn add_accounts(&self, accounts: Vec<v2::MinaBaseAccountBinableArgStableV2>) {
        self.ledger_ctx
            .send(LedgerCommand::AddAccounts(accounts))
            .unwrap_or_default();
    }

    pub fn init_staged_ledger(&self, info: GetStagedLedgerAuxAndPendingCoinbasesAtHashV2Response) {
        self.ledger_ctx
            .send(LedgerCommand::Init(info))
            .unwrap_or_default()
    }

    pub fn root_hash(&self) {
        self.ledger_ctx
            .send(LedgerCommand::PrintRootHash)
            .unwrap_or_default();
    }

    pub fn apply_block(&self, block: v2::MinaBlockBlockStableV2) {
        self.ledger_ctx
            .send(LedgerCommand::ApplyBlock(block))
            .unwrap_or_default()
    }
}

impl redux::TimeService for Service {}

impl redux::Service for Service {}
