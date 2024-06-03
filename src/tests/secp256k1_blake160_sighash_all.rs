use super::{
    blake160, sign_tx, sign_tx_by_input_group, DummyDataLoader, SigHashAllBinType, MAX_CYCLES,
    SECP256K1_DATA_BIN,
};
use ckb_crypto::secp::{Generator, Privkey};
use ckb_error::assert_error_eq;
use ckb_script::{ScriptError, TransactionScriptsVerifier};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::{CellMetaBuilder, ResolvedTransaction},
        Capacity, DepType, ScriptHashType, TransactionBuilder, TransactionView,
    },
    packed::{CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs, WitnessArgsBuilder},
    prelude::*,
    H256,
};
use rand::{thread_rng, Rng, RngCore, SeedableRng};
use std::sync::Arc;

const ERROR_ENCODING: i8 = -2;
const ERROR_WITNESS_SIZE: i8 = -22;
const ERROR_PUBKEY_BLAKE160_HASH: i8 = -31;

struct FixRng {
    count: u64,
}

impl Default for FixRng {
    fn default() -> Self {
        Self { count: 0 }
    }
}

impl RngCore for FixRng {
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut buf = Vec::with_capacity(dest.len());
        buf.resize(dest.len(), self.count as u8);
        self.count += 1;
        dest.copy_from_slice(&buf);
    }

    fn next_u32(&mut self) -> u32 {
        let r = self.count;
        self.count += 1;
        r as u32
    }

    fn next_u64(&mut self) -> u64 {
        let r = self.count;
        self.count += 1;
        r
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        let mut buf = Vec::with_capacity(dest.len());
        buf.resize(dest.len(), self.count as u8);
        self.count += 1;
        dest.copy_from_slice(&buf);
        Ok(())
    }
}

fn gen_lock_script(lock_args: Bytes, t: &SigHashAllBinType) -> Script {
    let sighash_all_cell_data_hash = CellOutput::calc_data_hash(t.get_bin());
    Script::new_builder()
        .args(lock_args.pack())
        .code_hash(sighash_all_cell_data_hash)
        .hash_type(ScriptHashType::Data1.into())
        .build()
}

fn gen_tx(dummy: &mut DummyDataLoader, lock_args: Bytes, t: &SigHashAllBinType) -> TransactionView {
    let mut rng = thread_rng();
    gen_tx_with_grouped_args(dummy, vec![(lock_args, 1)], &mut rng, t)
}

fn gen_tx_with_grouped_args<R: Rng>(
    dummy: &mut DummyDataLoader,
    grouped_args: Vec<(Bytes, usize)>,
    rng: &mut R,
    t: &SigHashAllBinType,
) -> TransactionView {
    // setup sighash_all dep
    let sighash_all_out_point = {
        let contract_tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(contract_tx_hash, 0)
    };
    // dep contract code
    let sighash_all_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(t.get_bin().len())
                .expect("script capacity")
                .pack(),
        )
        .build();
    dummy.cells.insert(
        sighash_all_out_point.clone(),
        (sighash_all_cell, t.get_bin().clone()),
    );
    // setup secp256k1_data dep
    let secp256k1_data_out_point = {
        let tx_hash = {
            let mut buf = [0u8; 32];
            rng.fill(&mut buf);
            buf.pack()
        };
        OutPoint::new(tx_hash, 0)
    };
    let secp256k1_data_cell = CellOutput::new_builder()
        .capacity(
            Capacity::bytes(SECP256K1_DATA_BIN.len())
                .expect("data capacity")
                .pack(),
        )
        .build();
    dummy.cells.insert(
        secp256k1_data_out_point.clone(),
        (secp256k1_data_cell, SECP256K1_DATA_BIN.clone()),
    );
    // setup default tx builder
    let dummy_capacity = Capacity::shannons(42);
    let mut tx_builder = TransactionBuilder::default()
        .cell_dep(
            CellDep::new_builder()
                .out_point(sighash_all_out_point)
                .dep_type(DepType::Code.into())
                .build(),
        )
        .cell_dep(
            CellDep::new_builder()
                .out_point(secp256k1_data_out_point)
                .dep_type(DepType::Code.into())
                .build(),
        )
        .output(
            CellOutput::new_builder()
                .capacity(dummy_capacity.pack())
                .build(),
        )
        .output_data(Bytes::new().pack());

    for (args, inputs_size) in grouped_args {
        // setup dummy input unlock script
        for _ in 0..inputs_size {
            let previous_tx_hash = {
                let mut buf = [0u8; 32];
                rng.fill(&mut buf);
                buf.pack()
            };
            let previous_out_point = OutPoint::new(previous_tx_hash, 0);
            let script = gen_lock_script(args.clone(), t);
            let previous_output_cell = CellOutput::new_builder()
                .capacity(dummy_capacity.pack())
                .lock(script)
                .build();
            dummy.cells.insert(
                previous_out_point.clone(),
                (previous_output_cell.clone(), Bytes::new()),
            );
            let mut random_extra_witness = [0u8; 32];
            rng.fill(&mut random_extra_witness);
            let witness_args = WitnessArgsBuilder::default()
                .input_type(Some(Bytes::from(random_extra_witness.to_vec())).pack())
                .build();
            tx_builder = tx_builder
                .input(CellInput::new(previous_out_point, 0))
                .witness(witness_args.as_bytes().pack());
        }
    }

    tx_builder.build()
}

fn sign_tx_hash(tx: TransactionView, key: &Privkey, tx_hash: &[u8]) -> TransactionView {
    // calculate message
    let mut blake2b = ckb_hash::new_blake2b();
    let mut message = [0u8; 32];
    blake2b.update(tx_hash);
    blake2b.finalize(&mut message);
    let message = H256::from(message);
    let sig = key.sign_recoverable(&message).expect("sign");
    let witness_args = WitnessArgsBuilder::default()
        .lock(Some(Bytes::from(sig.serialize())).pack())
        .build();
    tx.as_advanced_builder()
        .set_witnesses(vec![witness_args.as_bytes().pack()])
        .build()
}

fn build_resolved_tx(data_loader: &DummyDataLoader, tx: &TransactionView) -> ResolvedTransaction {
    let resolved_cell_deps = tx
        .cell_deps()
        .into_iter()
        .map(|deps_out_point| {
            let (dep_output, dep_data) =
                data_loader.cells.get(&deps_out_point.out_point()).unwrap();
            CellMetaBuilder::from_cell_output(dep_output.to_owned(), dep_data.to_owned())
                .out_point(deps_out_point.out_point())
                .build()
        })
        .collect();

    let mut resolved_inputs = Vec::new();
    for i in 0..tx.inputs().len() {
        let previous_out_point = tx.inputs().get(i).unwrap().previous_output();
        let (input_output, input_data) = data_loader.cells.get(&previous_out_point).unwrap();
        resolved_inputs.push(
            CellMetaBuilder::from_cell_output(input_output.to_owned(), input_data.to_owned())
                .out_point(previous_out_point)
                .build(),
        );
    }

    ResolvedTransaction {
        transaction: tx.clone(),
        resolved_cell_deps,
        resolved_inputs,
        resolved_dep_groups: vec![],
    }
}

#[test]
fn test_sighash_all_unlock() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let tx = gen_tx(&mut data_loader, pubkey_hash, &SigHashAllBinType::default());
    let tx = sign_tx(tx, &privkey);
    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    let cycles = verify_result.expect("pass verification");
    println!("cycles: {}", cycles);
}

#[cfg(feature = "test_llvm_version")]
fn run_benchmark(t: SigHashAllBinType) -> u64 {
    let mut data_loader = DummyDataLoader::new();
    // let privkey = Generator::random_privkey();
    let privkey = Generator::non_crypto_safe_prng(123).gen_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    println!("--- pubkeyhash: {:02x?}", pubkey_hash.to_vec());

    let mut rng = FixRng::default();
    let tx = gen_tx_with_grouped_args(&mut data_loader, vec![(pubkey_hash, 1)], &mut rng, &t);

    let tx = sign_tx(tx, &privkey);
    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    let cycles = verify_result.expect("pass verification");
    cycles
}


#[cfg(feature = "test_llvm_version")]
#[test]
fn test_sighash_benchmark() {
    run_benchmark(SigHashAllBinType::Def);
    let c_16 = run_benchmark(SigHashAllBinType::LLVM16);
    let c_17 = run_benchmark(SigHashAllBinType::LLVM17);
    let c_18 = run_benchmark(SigHashAllBinType::LLVM18);

    println!(
        "-- LLVM 16 cycles: {}({:.2}k), size: {}({:.2}k)",
        c_16,
        c_16 as f64 / 1024.0,
        SigHashAllBinType::LLVM16.get_bin().len(),
        SigHashAllBinType::LLVM16.get_bin().len() as f64 / 1024.0
    );

    println!(
        "-- LLVM 17 cycles: {}({:.2}k), size: {}({:.2}k)",
        c_17,
        c_17 as f64 / 1024.0,
        SigHashAllBinType::LLVM17.get_bin().len(),
        SigHashAllBinType::LLVM17.get_bin().len() as f64 / 1024.0
    );

    println!(
        "-- LLVM 18 cycles: {}({:.2}k), size: {}({:.2}k)",
        c_18,
        c_18 as f64 / 1024.0,
        SigHashAllBinType::LLVM18.get_bin().len(),
        SigHashAllBinType::LLVM18.get_bin().len() as f64 / 1024.0,
    );
}

#[test]
fn test_sighash_all_with_extra_witness_unlock() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());
    let tx = gen_tx(&mut data_loader, pubkey_hash, &SigHashAllBinType::default());
    let extract_witness = vec![1, 2, 3, 4];
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![WitnessArgs::new_builder()
            .input_type(Some(Bytes::from(extract_witness)).pack())
            .build()
            .as_bytes()
            .pack()])
        .build();
    {
        let tx = sign_tx(tx.clone(), &privkey);
        let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
        let verify_result =
            TransactionScriptsVerifier::new(resolved_tx, data_loader.clone()).verify(MAX_CYCLES);
        verify_result.expect("pass verification");
    }
    {
        let tx = sign_tx(tx, &privkey);
        let wrong_witness = tx
            .witnesses()
            .get(0)
            .map(|w| {
                WitnessArgs::new_unchecked(w.unpack())
                    .as_builder()
                    .input_type(Some(Bytes::from(vec![0])).pack())
                    .build()
            })
            .unwrap();
        let tx = tx
            .as_advanced_builder()
            .set_witnesses(vec![wrong_witness.as_bytes().pack()])
            .build();
        let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
        let verify_result =
            TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
        assert_error_eq!(
            verify_result.unwrap_err(),
            ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
                .input_lock_script(0),
        );
    }
}

#[test]
fn test_sighash_all_with_grouped_inputs_unlock() {
    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());
    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 2)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    {
        let tx = sign_tx(tx.clone(), &privkey);
        let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
        let verify_result =
            TransactionScriptsVerifier::new(resolved_tx, data_loader.clone()).verify(MAX_CYCLES);
        verify_result.expect("pass verification");
    }
    {
        let tx = sign_tx(tx, &privkey);
        let wrong_witness = tx
            .witnesses()
            .get(1)
            .map(|w| {
                WitnessArgs::new_unchecked(w.unpack())
                    .as_builder()
                    .input_type(Some(Bytes::from(vec![0])).pack())
                    .build()
            })
            .unwrap();
        let tx = tx
            .as_advanced_builder()
            .set_witnesses(vec![
                tx.witnesses().get(0).unwrap(),
                wrong_witness.as_bytes().pack(),
            ])
            .build();
        let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
        let verify_result =
            TransactionScriptsVerifier::new(resolved_tx, data_loader.clone()).verify(MAX_CYCLES);
        assert_error_eq!(
            verify_result.unwrap_err(),
            ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
                .input_lock_script(0),
        );
    }
}

#[test]
fn test_sighash_all_with_2_different_inputs_unlock() {
    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    // key1
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    // key2
    let privkey2 = Generator::random_privkey();
    let pubkey2 = privkey2.pubkey().expect("pubkey");
    let pubkey_hash2 = blake160(&pubkey2.serialize());

    // sign with 2 keys
    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 2), (pubkey_hash2, 2)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 2);
    let tx = sign_tx_by_input_group(tx, &privkey2, 2, 2);

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    verify_result.expect("pass verification");
}

#[test]
fn test_signing_with_wrong_key() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let wrong_privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());
    let tx = gen_tx(&mut data_loader, pubkey_hash, &SigHashAllBinType::default());
    let tx = sign_tx(tx, &wrong_privkey);
    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(0),
    );
}

#[test]
fn test_signing_wrong_tx_hash() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());
    let tx = gen_tx(&mut data_loader, pubkey_hash, &SigHashAllBinType::default());
    let tx = {
        let mut rand_tx_hash = [0u8; 32];
        let mut rng = thread_rng();
        rng.fill(&mut rand_tx_hash);
        sign_tx_hash(tx, &privkey, &rand_tx_hash[..])
    };
    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(0),
    );
}

#[test]
fn test_super_long_witness() {
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());
    let tx = gen_tx(&mut data_loader, pubkey_hash, &SigHashAllBinType::default());
    let tx_hash = tx.hash();

    let mut buffer: Vec<u8> = vec![];
    buffer.resize(40000, 1);
    let super_long_message = Bytes::from(buffer);

    let mut blake2b = ckb_hash::new_blake2b();
    let mut message = [0u8; 32];
    blake2b.update(&tx_hash.raw_data());
    blake2b.update(&super_long_message[..]);
    blake2b.finalize(&mut message);
    let message = H256::from(message);
    let sig = privkey.sign_recoverable(&message).expect("sign");
    let witness = WitnessArgs::new_builder()
        .lock(Some(Bytes::from(sig.serialize())).pack())
        .input_type(Some(super_long_message).pack())
        .build();
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![witness.as_bytes().pack()])
        .build();

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_WITNESS_SIZE).input_lock_script(0),
    );
}

#[test]
fn test_sighash_all_2_in_2_out_cycles() {
    // Notice this is changed due to the fact that the old tests uses
    // a different definition of WitnessArgs, hence triggering the differences.
    const CONSUME_CYCLES: u64 = 3033463;

    let mut data_loader = DummyDataLoader::new();
    let mut generator = Generator::non_crypto_safe_prng(42);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);

    // key1
    let privkey = generator.gen_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    // key2
    let privkey2 = generator.gen_privkey();
    let pubkey2 = privkey2.pubkey().expect("pubkey");
    let pubkey_hash2 = blake160(&pubkey2.serialize());

    // sign with 2 keys
    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 1), (pubkey_hash2, 1)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 1);
    let tx = sign_tx_by_input_group(tx, &privkey2, 1, 1);

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    let cycles = verify_result.expect("pass verification");
    assert_eq!(CONSUME_CYCLES, cycles)
}

#[test]
fn test_sighash_all_witness_append_junk_data() {
    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());

    // sign with 2 keys
    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 2)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 2);
    let mut witnesses: Vec<_> = Unpack::<Vec<_>>::unpack(&tx.witnesses());
    // append junk data to first witness
    let mut witness = Vec::new();
    witness.resize(witnesses[0].len(), 0);
    witness.copy_from_slice(&witnesses[0]);
    witness.push(0);
    witnesses[0] = witness.into();

    let tx = tx
        .as_advanced_builder()
        .set_witnesses(witnesses.into_iter().map(|w| w.pack()).collect())
        .build();

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_ENCODING).input_lock_script(0),
    );
}

#[test]
fn test_sighash_all_witness_args_ambiguity() {
    // This test case build tx with WitnessArgs(lock, data, "")
    // and try unlock with WitnessArgs(lock, "", data)
    //
    // this case will fail if contract use a naive function to digest witness.

    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());

    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 2)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 2);
    let witnesses: Vec<_> = Unpack::<Vec<_>>::unpack(&tx.witnesses());
    // move input_type data to output_type
    let witnesses: Vec<_> = witnesses
        .into_iter()
        .map(|witness| {
            let witness = WitnessArgs::new_unchecked(witness);
            let data = witness.input_type();
            let empty: Option<Bytes> = None;
            witness
                .as_builder()
                .output_type(data)
                .input_type(empty.pack())
                .build()
        })
        .collect();

    let tx = tx
        .as_advanced_builder()
        .set_witnesses(witnesses.into_iter().map(|w| w.as_bytes().pack()).collect())
        .build();

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(0),
    );
}

#[test]
fn test_sighash_all_witnesses_ambiguity() {
    // This test case sign tx with [witness1, "", witness2]
    // and try unlock with [witness1, witness2, ""]
    //
    // this case will fail if contract use a naive function to digest witness.

    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());

    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 3)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let witness = Unpack::<Vec<_>>::unpack(&tx.witnesses()).remove(0);
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![
            witness.pack(),
            Bytes::new().pack(),
            Bytes::from(vec![42]).pack(),
        ])
        .build();
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 3);

    // exchange witness position
    let witness = Unpack::<Vec<_>>::unpack(&tx.witnesses()).remove(0);
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![
            witness.pack(),
            Bytes::from(vec![42]).pack(),
            Bytes::new().pack(),
        ])
        .build();

    assert_eq!(tx.witnesses().len(), tx.inputs().len());
    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result =
        TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(MAX_CYCLES);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(0),
    );
}

#[test]
fn test_sighash_all_cover_extra_witnesses() {
    let mut rng = thread_rng();
    let mut data_loader = DummyDataLoader::new();
    let privkey = Generator::random_privkey();
    let pubkey = privkey.pubkey().expect("pubkey");
    let pubkey_hash = blake160(&pubkey.serialize());
    let lock_script = gen_lock_script(pubkey_hash.clone(), &SigHashAllBinType::default());

    let tx = gen_tx_with_grouped_args(
        &mut data_loader,
        vec![(pubkey_hash, 2)],
        &mut rng,
        &SigHashAllBinType::default(),
    );
    let witness = Unpack::<Vec<_>>::unpack(&tx.witnesses()).remove(0);
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![
            witness.pack(),
            Bytes::from(vec![42]).pack(),
            Bytes::new().pack(),
        ])
        .build();
    let tx = sign_tx_by_input_group(tx, &privkey, 0, 3);
    assert!(tx.witnesses().len() > tx.inputs().len());

    // change last witness
    let mut witnesses = Unpack::<Vec<_>>::unpack(&tx.witnesses());
    let tx = tx
        .as_advanced_builder()
        .set_witnesses(vec![
            witnesses.remove(0).pack(),
            witnesses.remove(1).pack(),
            Bytes::from(vec![0]).pack(),
        ])
        .build();

    let resolved_tx = Arc::new(build_resolved_tx(&data_loader, &tx));
    let verify_result = TransactionScriptsVerifier::new(resolved_tx, data_loader).verify(60000000);
    assert_error_eq!(
        verify_result.unwrap_err(),
        ScriptError::validation_failure(&lock_script, ERROR_PUBKEY_BLAKE160_HASH)
            .input_lock_script(0),
    );
}
