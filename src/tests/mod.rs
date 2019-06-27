mod dao;
mod secp256k1_blake160_sighash_all;

use ckb_core::{
    cell::CellMeta,
    extras::BlockExt,
    transaction::{CellOutPoint, CellOutput, Transaction, TransactionBuilder, Witness},
    Bytes,
};
use ckb_crypto::secp::Privkey;
use ckb_script::DataLoader;
use lazy_static::lazy_static;
use numext_fixed_hash::H256;
use std::collections::HashMap;

pub const MAX_CYCLES: u64 = std::u64::MAX;

lazy_static! {
    pub static ref SIGHASH_ALL_BIN: Bytes =
        Bytes::from(&include_bytes!("../../specs/cells/secp256k1_blake160_sighash_all")[..]);
    pub static ref DAO_BIN: Bytes = Bytes::from(&include_bytes!("../../specs/cells/dao")[..]);
}

#[derive(Default)]
pub struct DummyDataLoader {
    pub cells: HashMap<CellOutPoint, CellOutput>,
}

impl DummyDataLoader {
    fn new() -> Self {
        Self::default()
    }
}

impl DataLoader for DummyDataLoader {
    // load CellOutput
    fn lazy_load_cell_output(&self, cell: &CellMeta) -> CellOutput {
        cell.cell_output.clone().unwrap_or_else(|| {
            self.cells
                .get(&cell.out_point)
                .cloned()
                .expect("must exists")
        })
    }
    // load BlockExt
    fn get_block_ext(&self, _hash: &H256) -> Option<BlockExt> {
        unreachable!()
    }
}

pub fn sign_tx(tx: Transaction, key: &Privkey) -> Transaction {
    let signed_witnesses: Vec<Witness> = tx
        .inputs()
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let witness = tx.witnesses().get(i).cloned().unwrap_or(vec![]);
            let mut blake2b = ckb_hash::new_blake2b();
            let mut message = [0u8; 32];
            blake2b.update(&tx.hash()[..]);
            for data in &witness {
                blake2b.update(&data);
            }
            blake2b.finalize(&mut message);
            let message = H256::from(message);
            let sig = key.sign_recoverable(&message).expect("sign");
            let mut signed_witness = vec![Bytes::from(sig.serialize())];
            for data in &witness {
                signed_witness.push(data.clone());
            }
            signed_witness
        })
        .collect();
    // calculate message
    TransactionBuilder::from_transaction(tx)
        .witnesses_clear()
        .witnesses(signed_witnesses)
        .build()
}