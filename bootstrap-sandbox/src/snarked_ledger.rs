use std::{io, future::Future, pin::Pin};
use binprot::{BinProtWrite, BinProtRead};
use mina_rpc::Engine;
use thiserror::Error;
use serde::Deserialize;

use mina_p2p_messages::v2;
use mina_tree::{Mask, Database, Account, BaseLedger, Address, AccountIndex};

pub struct SnarkedLedger {
    pub inner: Mask,
    pub num: u32,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl SnarkedLedger {
    pub fn empty() -> Self {
        SnarkedLedger {
            inner: Mask::new_root(Database::create(35)),
            num: 0,
        }
    }

    #[allow(dead_code)]
    pub fn load_json<R>(source: R) -> io::Result<Self>
    where
        R: io::Read,
    {
        #[derive(Deserialize)]
        struct Genesis {
            #[allow(dead_code)]
            genesis_state_timestamp: String,
        }

        #[derive(Deserialize)]
        struct LedgerInner {
            #[allow(dead_code)]
            name: String,
            accounts: Vec<v2::MinaBaseAccountBinableArgStableV2>,
        }

        #[derive(Deserialize)]
        struct LedgerFile {
            #[allow(dead_code)]
            genesis: Genesis,
            ledger: LedgerInner,
        }

        let LedgerFile {
            ledger: LedgerInner { accounts, .. },
            ..
        } = serde_json::from_reader(source)?;

        let mut inner = Mask::new_root(Database::create(35));
        let num = accounts.len() as _;
        for account in accounts {
            let account = Account::from(account);
            let account_id = account.id();
            inner.get_or_create_account(account_id, account).unwrap();
        }

        Ok(SnarkedLedger { inner, num })
    }

    // for debugging
    pub fn store_bin<W>(&self, mut writer: W) -> io::Result<()>
    where
        W: io::Write,
    {
        let accounts = self.inner.fold(vec![], |mut accounts, account| {
            accounts.push(account.clone());
            accounts
        });
        accounts.binprot_write(&mut writer)
    }

    pub fn load_bin<R>(mut reader: R) -> Result<Self, binprot::Error>
    where
        R: io::Read,
    {
        let accounts = Vec::<Account>::binprot_read(&mut reader)?;

        let num = accounts.len() as _;
        let mut inner = Mask::new_root(Database::create(35));
        for account in accounts {
            let account_id = account.id();
            inner.get_or_create_account(account_id, account).unwrap();
        }

        Ok(SnarkedLedger { inner, num })
    }

    pub async fn sync(&mut self, engine: &mut Engine, root: &v2::LedgerHash, hash: v2::LedgerHash) {
        self.sync_at_depth(engine, root.clone(), hash.clone(), 0, 0)
            .await;
        let actual_hash = self.inner.merkle_root();
        let actual_hash = v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(actual_hash.into()));
        assert_eq!(actual_hash, root.clone());
    }

    fn sync_at_depth_boxed<'a, 'b: 'a>(
        &'b mut self,
        engine: &'a mut Engine,
        root: v2::LedgerHash,
        hash: v2::LedgerHash,
        depth: i32,
        pos: u32,
    ) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
        Box::pin(self.sync_at_depth(engine, root, hash, depth, pos))
    }

    async fn sync_at_depth(
        &mut self,
        engine: &mut Engine,
        root: v2::LedgerHash,
        hash: v2::LedgerHash,
        depth: i32,
        pos: u32,
    ) {
        use mina_p2p_messages::rpc::AnswerSyncLedgerQueryV2;

        let addr = Address::from_index(AccountIndex(pos as _), depth as _);
        let actual_hash = self.inner.get_inner_hash_at_addr(addr.clone()).unwrap();
        if depth == 0 && root.0 == actual_hash.into() || depth > 0 && hash.0 == actual_hash.into() {
            return;
        }

        if depth == 32 {
            let p = pos.to_be_bytes().to_vec();
            let q = v2::MinaLedgerSyncLedgerQueryStableV1::WhatContents(
                v2::MerkleAddressBinableArgStableV1((depth as i64).into(), p.into()),
            );
            log::info!("{}", serde_json::to_string(&q).unwrap());
            let r = engine
                .rpc::<AnswerSyncLedgerQueryV2>((root.0.clone(), q))
                .await
                .unwrap()
                .unwrap()
                .0
                .unwrap();
            match r {
                v2::MinaLedgerSyncLedgerAnswerStableV2::ContentsAre(accounts) => {
                    for (o, account) in accounts.into_iter().enumerate() {
                        let account = Account::from(account);
                        self.inner
                            .set_at_index(AccountIndex((pos * 8) as u64 + o as u64), account)
                            .unwrap();
                    }
                }
                _ => panic!(),
            }
        } else {
            let b = ((depth as usize + 7) / 8).min(4);
            let p = pos * (1 << (32 - depth));
            let p = p.to_be_bytes()[..b].to_vec();
            let q = v2::MinaLedgerSyncLedgerQueryStableV1::WhatChildHashes(
                v2::MerkleAddressBinableArgStableV1((depth as i64).into(), p.into()),
            );
            log::info!("{}", serde_json::to_string(&q).unwrap());
            let r = engine
                .rpc::<AnswerSyncLedgerQueryV2>((root.0.clone(), q))
                .await
                .unwrap()
                .unwrap()
                .0
                .unwrap();
            match r {
                v2::MinaLedgerSyncLedgerAnswerStableV2::ChildHashesAre(l, r) => {
                    self.sync_at_depth_boxed(engine, root.clone(), l, depth + 1, pos * 2)
                        .await;
                    self.sync_at_depth_boxed(engine, root.clone(), r, depth + 1, pos * 2 + 1)
                        .await;
                }
                _ => panic!(),
            };
        }

        let addr = Address::from_index(AccountIndex(pos as _), depth as _);
        let actual_hash = self.inner.get_inner_hash_at_addr(addr).unwrap();
        let actual_hash = v2::LedgerHash::from(v2::MinaBaseLedgerHash0StableV1(actual_hash.into()));
        if depth == 0 {
            assert_eq!(root, actual_hash);
        } else {
            assert_eq!(hash, actual_hash);
        }
    }
}
