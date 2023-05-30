use mina_p2p_messages::v2;
use redux::ActionWithMeta;

use super::{state::State, action::Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Start(v) => {
                let ledger_hash = v
                    .data
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .next_epoch_data
                    .ledger
                    .hash
                    .clone();
                self.epoch_ledger_hash = Some(ledger_hash.clone());
            }
            Action::Continue(v) => match v {
                v2::MinaLedgerSyncLedgerAnswerStableV2::NumAccounts(num, hash) => {
                    log::info!(
                        "Ledger: {}, accounts: {}, hash: {hash}",
                        self.epoch_ledger_hash.as_ref().unwrap(),
                        num.0
                    );
                    self.num_accounts = num.0; // WARNING: do not divide by 16, it is for debug
                    self.syncing_depth = 0;
                    self.syncing_pos = 0;

                    // TODO:
                    // self.ledger = Some(Ledger {
                    //     hash: hash.clone(),
                    //     content: LedgerContent::Absent,
                    // });
                }
                v2::MinaLedgerSyncLedgerAnswerStableV2::ChildHashesAre(left, right) => {
                    log::info!(
                        "Ledger: {}, children: {left} {right}",
                        self.epoch_ledger_hash.as_ref().unwrap(),
                    );

                    // TODO:
                    // let ledger = self.ledger.as_mut().expect("must exist");
                    // go to depth
                    // let ledger = (0..self.syncing_depth).fold(ledger, |ledger, depth| {
                    //     let b = self.syncing_pos >> (32 - depth);
                    //     ledger.content.unwrap_node().0
                    // });

                    if self.syncing_depth == 0 {
                        self.syncing_depth += 1;
                    } else {
                        if let Some(new_pos) =
                            self.syncing_pos.checked_add(1 << (32 - self.syncing_depth))
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
                v2::MinaLedgerSyncLedgerAnswerStableV2::ContentsAre(contents) => {
                    // TODO:
                    let _ = contents;

                    if self.syncing_pos < self.num_accounts {
                        self.syncing_pos += 1;
                    } else {
                        self.syncing_depth += 1;
                    }
                }
            },
        }
    }
}
