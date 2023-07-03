use mina_p2p_messages::v2;
use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    fn step(&mut self) {
        if self.syncing_depth == 0 {
            self.syncing_depth += 1;
        } else {
            if self.syncing_depth == 32 {
                self.syncing_pos = self.syncing_pos + 1;
                if self.syncing_pos > self.num_accounts {
                    self.syncing_depth += 1;
                    self.syncing_pos = 0;
                }
            } else {
                if let Some(new_pos) = self.syncing_pos.checked_add(1 << (32 - self.syncing_depth))
                {
                    self.syncing_pos = new_pos;
                    if self.syncing_pos >= self.num_accounts || self.syncing_pos < 0 {
                        self.syncing_depth += 1;
                        self.syncing_pos = 0;
                    }
                } else {
                    self.syncing_depth += 1;
                    self.syncing_pos = 0;
                }
            }
        }
    }

    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Start => {}
            Action::Continue(v) => match v {
                Some(v2::MinaLedgerSyncLedgerAnswerStableV2::NumAccounts(num, hash)) => {
                    log::info!(
                        "Ledger: {}, accounts: {}, hash: {hash}",
                        self.epoch_ledger_hash.as_ref().unwrap(),
                        num.0
                    );
                    self.num_accounts = (num.0 as i32) / 8;
                    // TODO:
                    // when we have a persistent ledger, we should start with
                    // `syncing_depth` equal to 0 and check hashes to avoid downloading ledgers
                    // that are already up to date.
                    self.syncing_depth = 32;
                    self.syncing_pos = 0;
                }
                Some(v2::MinaLedgerSyncLedgerAnswerStableV2::ChildHashesAre(left, right)) => {
                    log::info!(
                        "Ledger: {}, children: {left} {right}",
                        self.epoch_ledger_hash.as_ref().unwrap(),
                    );

                    self.step();
                }
                Some(v2::MinaLedgerSyncLedgerAnswerStableV2::ContentsAre(..)) => {
                    self.step();
                }
                None => self.step(),
            },
        }
    }
}
