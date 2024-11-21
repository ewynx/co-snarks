use ark_bn254::Bn254;
use ark_ff::PrimeField;
use co_acvm::{mpc::NoirWitnessExtensionProtocol, PlainAcvmSolver};
use co_ultrahonk::prelude::{CoUltraHonk, PlainCoBuilder, PlainUltraHonkDriver, ProvingKey};
use sha3::Keccak256;
use ultrahonk::{
    prelude::{HonkProof, Poseidon2Sponge, TranscriptFieldType, TranscriptHasher, UltraHonk},
    Utils,
};

fn promote_public_witness_vector<F: PrimeField, T: NoirWitnessExtensionProtocol<F>>(
    witness: Vec<F>,
) -> Vec<T::AcvmType> {
    witness.into_iter().map(|w| T::AcvmType::from(w)).collect()
}

fn plaindriver_test<H: TranscriptHasher<TranscriptFieldType>>(
    proof_file: &str,
    circuit_file: &str,
    witness_file: &str,
) {
    const CRS_PATH_G1: &str = "../co-builder/src/crs/bn254_g1.dat";
    const CRS_PATH_G2: &str = "../co-builder/src/crs/bn254_g2.dat";

    let constraint_system = Utils::get_constraint_system_from_file(circuit_file, true).unwrap();
    let witness = Utils::get_witness_from_file(witness_file).unwrap();

    let witness = promote_public_witness_vector::<_, PlainAcvmSolver<ark_bn254::Fr>>(witness);
    let mut driver = PlainAcvmSolver::new();
    let builder = PlainCoBuilder::<Bn254>::create_circuit(
        constraint_system,
        false, // We don't support recursive atm
        0,
        witness,
        true,
        false,
        &mut driver,
    )
    .unwrap();

    let crs =
        ProvingKey::<PlainUltraHonkDriver, _>::get_crs(&builder, CRS_PATH_G1, CRS_PATH_G2).unwrap();
    let (proving_key, verifying_key) =
        ProvingKey::create_keys(0, builder, crs, &mut driver).unwrap();

    let driver = PlainUltraHonkDriver;
    let prover = CoUltraHonk::<_, _, H>::new(driver);
    let proof = prover.prove(proving_key).unwrap();

    let proof_u8 = proof.to_buffer();
    let read_proof_u8 = std::fs::read(proof_file).unwrap();
    assert_eq!(proof_u8, read_proof_u8);

    let read_proof = HonkProof::from_buffer(&read_proof_u8).unwrap();
    assert_eq!(proof, read_proof);

    let is_valid = UltraHonk::<_, H>::verify(proof, verifying_key).unwrap();
    assert!(is_valid);
}

#[test]
fn poseidon_plaindriver_test_poseidon2sponge() {
    const PROOF_FILE: &str = "../../test_vectors/noir/poseidon/kat/pos_proof_with_pos";
    const CIRCUIT_FILE: &str = "../../test_vectors/noir/poseidon/kat/poseidon.json";
    const WITNESS_FILE: &str = "../../test_vectors/noir/poseidon/kat/poseidon.gz";
    plaindriver_test::<Poseidon2Sponge>(PROOF_FILE, CIRCUIT_FILE, WITNESS_FILE);
}

#[test]
fn poseidon_plaindriver_test_keccak256() {
    const PROOF_FILE: &str = "../../test_vectors/noir/poseidon/kat/pos_proof_with_kec";
    const CIRCUIT_FILE: &str = "../../test_vectors/noir/poseidon/kat/poseidon.json";
    const WITNESS_FILE: &str = "../../test_vectors/noir/poseidon/kat/poseidon.gz";
    plaindriver_test::<Keccak256>(PROOF_FILE, CIRCUIT_FILE, WITNESS_FILE);
}

#[test]
fn add3_plaindriver_test_keccak256() {
    const PROOF_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64_proof_with_kec";
    const CIRCUIT_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64.json";
    const WITNESS_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64.gz";
    plaindriver_test::<Keccak256>(PROOF_FILE, CIRCUIT_FILE, WITNESS_FILE);
}

#[test]
fn add3_plaindriver_test_poseidon2sponge() {
    const PROOF_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64_proof_with_pos";
    const CIRCUIT_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64.json";
    const WITNESS_FILE: &str = "../../test_vectors/noir/add3u64/kat/add3u64.gz";
    plaindriver_test::<Poseidon2Sponge>(PROOF_FILE, CIRCUIT_FILE, WITNESS_FILE);
}