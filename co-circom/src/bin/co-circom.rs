use ark_bls12_381::Bls12_381;
use ark_bn254::Bn254;
use ark_ec::pairing::Pairing;
use ark_ff::PrimeField;
use circom_mpc_compiler::CompilerBuilder;
use circom_types::R1CS;
use num_traits::Zero;

use circom_types::{
    groth16::{
        Groth16Proof, JsonVerificationKey as Groth16JsonVerificationKey, ZKey as Groth16ZKey,
    },
    plonk::{JsonVerificationKey as PlonkJsonVerificationKey, PlonkProof, ZKey as PlonkZKey},
    traits::{CircomArkworksPairingBridge, CircomArkworksPrimeFieldBridge},
    Witness,
};
use clap::{Parser, Subcommand};
use co_circom::GenerateProofCli;
use co_circom::GenerateProofConfig;
use co_circom::GenerateWitnessCli;
use co_circom::GenerateWitnessConfig;
use co_circom::MergeInputSharesCli;
use co_circom::MergeInputSharesConfig;
use co_circom::SplitInputCli;
use co_circom::SplitInputConfig;
use co_circom::SplitWitnessCli;
use co_circom::SplitWitnessConfig;
use co_circom::TranslateWitnessCli;
use co_circom::TranslateWitnessConfig;
use co_circom::VerifyCli;
use co_circom::VerifyConfig;
use co_circom::{file_utils, MPCCurve, MPCProtocol, ProofSystem};
use co_circom_snarks::{SharedInput, SharedWitness};
use co_groth16::CoGroth16;
use co_groth16::Groth16;
use co_plonk::CoPlonk;
use co_plonk::Plonk;
use color_eyre::eyre::{eyre, Context, ContextCompat};
use mpc_core::protocols::rep3::network::Rep3Network;
use mpc_core::protocols::shamir::network::ShamirNetwork;
use mpc_core::{
    protocols::{
        rep3::{self, network::Rep3MpcNet, Rep3Protocol},
        shamir::{network::ShamirMpcNet, ShamirProtocol},
    },
    traits::PrimeFieldMpcProtocol,
};
use num_bigint::BigUint;
use num_traits::Num;
use std::time::Instant;
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
    process::ExitCode,
};

fn install_tracing() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let fmt_layer = fmt::layer().with_target(true).with_line_number(true);
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Splits an existing witness file generated by Circom into secret shares for use in MPC
    SplitWitness(SplitWitnessCli),
    /// Splits a JSON input file into secret shares for use in MPC
    SplitInput(SplitInputCli),
    /// Merge multiple shared inputs received from multiple parties into a single one
    MergeInputShares(MergeInputSharesCli),
    /// Evaluates the extended witness generation for the specified circuit and input share in MPC
    GenerateWitness(GenerateWitnessCli),
    /// Translates the witness generated with one MPC protocol to a witness for a different one
    TranslateWitness(TranslateWitnessCli),
    /// Evaluates the prover algorithm for the specified circuit and witness share in MPC
    GenerateProof(GenerateProofCli),
    /// Verification of a Circom proof.
    Verify(VerifyCli),
}

fn main() -> color_eyre::Result<ExitCode> {
    install_tracing();
    let args = Cli::parse();

    match args.command {
        Commands::SplitWitness(cli) => {
            let config = SplitWitnessConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_split_witness::<Bn254>(config),
                MPCCurve::BLS12_381 => run_split_witness::<Bls12_381>(config),
            }
        }
        Commands::SplitInput(cli) => {
            let config = SplitInputConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_split_input::<Bn254>(config),
                MPCCurve::BLS12_381 => run_split_input::<Bls12_381>(config),
            }
        }
        Commands::MergeInputShares(cli) => {
            let config = MergeInputSharesConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_merge_input_shares::<Bn254>(config),
                MPCCurve::BLS12_381 => run_merge_input_shares::<Bls12_381>(config),
            }
        }
        Commands::GenerateWitness(cli) => {
            let config = GenerateWitnessConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_generate_witness::<Bn254>(config),
                MPCCurve::BLS12_381 => run_generate_witness::<Bls12_381>(config),
            }
        }
        Commands::TranslateWitness(cli) => {
            let config = TranslateWitnessConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_translate_witness::<Bn254>(config),
                MPCCurve::BLS12_381 => run_translate_witness::<Bls12_381>(config),
            }
        }
        Commands::GenerateProof(cli) => {
            let config = GenerateProofConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_generate_proof::<Bn254>(config),
                MPCCurve::BLS12_381 => run_generate_proof::<Bls12_381>(config),
            }
        }
        Commands::Verify(cli) => {
            let config = VerifyConfig::parse(cli).context("while parsing config")?;
            match config.curve {
                MPCCurve::BN254 => run_verify::<Bn254>(config),
                MPCCurve::BLS12_381 => run_verify::<Bls12_381>(config),
            }
        }
    }
}

fn run_split_witness<P: Pairing + CircomArkworksPairingBridge>(
    config: SplitWitnessConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let witness_path = config.witness;
    let r1cs = config.r1cs;
    let protocol = config.protocol;
    let out_dir = config.out_dir;
    let t = config.threshold;
    let n = config.num_parties;

    file_utils::check_file_exists(&witness_path)?;
    file_utils::check_file_exists(&r1cs)?;
    file_utils::check_dir_exists(&out_dir)?;

    // read the Circom witness file
    let witness_file =
        BufReader::new(File::open(&witness_path).context("while opening witness file")?);
    let witness = Witness::<P::ScalarField>::from_reader(witness_file)
        .context("while parsing witness file")?;

    // read the Circom r1cs file
    let r1cs_file = BufReader::new(File::open(&r1cs).context("while opening r1cs file")?);
    let r1cs = R1CS::<P>::from_reader(r1cs_file).context("while parsing r1cs file")?;

    let mut rng = rand::thread_rng();

    match protocol {
        MPCProtocol::REP3 => {
            if t != 1 {
                return Err(eyre!("REP3 only allows the threshold to be 1"));
            }
            if n != 3 {
                return Err(eyre!("REP3 only allows the number of parties to be 3"));
            }
            // create witness shares
            let start = Instant::now();
            let shares = SharedWitness::<Rep3Protocol<P::ScalarField, Rep3MpcNet>, P>::share_rep3(
                witness,
                r1cs.num_inputs,
                &mut rng,
            );
            let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
            tracing::info!("Sharing took {} ms", duration_ms);

            // write out the shares to the output directory
            let base_name = witness_path
                .file_name()
                .context("we have a file name")?
                .to_str()
                .context("witness file name is not valid UTF-8")?;
            for (i, share) in shares.iter().enumerate() {
                let path = out_dir.join(format!("{}.{}.shared", base_name, i));
                let out_file =
                    BufWriter::new(File::create(&path).context("while creating output file")?);
                bincode::serialize_into(out_file, share)
                    .context("while serializing witness share")?;
                tracing::info!("Wrote witness share {} to file {}", i, path.display());
            }
        }
        MPCProtocol::SHAMIR => {
            // create witness shares
            let start = Instant::now();
            let shares =
                SharedWitness::<ShamirProtocol<P::ScalarField, ShamirMpcNet>, P>::share_shamir(
                    witness,
                    r1cs.num_inputs,
                    t,
                    n,
                    &mut rng,
                );
            let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
            tracing::info!("Sharing took {} ms", duration_ms);

            // write out the shares to the output directory
            let base_name = witness_path
                .file_name()
                .context("we have a file name")?
                .to_str()
                .context("witness file name is not valid UTF-8")?;
            for (i, share) in shares.iter().enumerate() {
                let path = out_dir.join(format!("{}.{}.shared", base_name, i));
                let out_file =
                    BufWriter::new(File::create(&path).context("while creating output file")?);
                bincode::serialize_into(out_file, share)
                    .context("while serializing witness share")?;
                tracing::info!("Wrote witness share {} to file {}", i, path.display());
            }
        }
    }
    tracing::info!("Split witness into shares successfully");
    Ok(ExitCode::SUCCESS)
}

fn run_split_input<P: Pairing + CircomArkworksPairingBridge>(
    config: SplitInputConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let input = config.input;
    let circuit = config.circuit;
    let link_library = config.link_library;
    let protocol = config.protocol;
    let out_dir = config.out_dir;

    if protocol != MPCProtocol::REP3 {
        return Err(eyre!(
            "Only REP3 protocol is supported for splitting inputs"
        ));
    }
    file_utils::check_file_exists(&input)?;
    let circuit_path = PathBuf::from(&circuit);
    file_utils::check_file_exists(&circuit_path)?;
    file_utils::check_dir_exists(&out_dir)?;

    //get the public inputs if any from parser
    let mut builder = CompilerBuilder::<P>::new(config.compiler, circuit);
    for lib in link_library {
        builder = builder.link_library(lib);
    }
    let public_inputs = builder.build().get_public_inputs()?;

    // read the input file
    let input_file = BufReader::new(File::open(&input).context("while opening input file")?);

    let input_json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_reader(input_file).context("while parsing input file")?;

    // create input shares
    let mut shares = [
        SharedInput::<Rep3Protocol<P::ScalarField, Rep3MpcNet>, P>::default(),
        SharedInput::<Rep3Protocol<P::ScalarField, Rep3MpcNet>, P>::default(),
        SharedInput::<Rep3Protocol<P::ScalarField, Rep3MpcNet>, P>::default(),
    ];

    let mut rng = rand::thread_rng();
    let start = Instant::now();
    for (name, val) in input_json {
        let parsed_vals = if val.is_array() {
            parse_array(&val)?
        } else {
            vec![parse_field(&val)?]
        };
        if public_inputs.contains(&name) {
            shares[0]
                .public_inputs
                .insert(name.clone(), parsed_vals.clone());
            shares[1]
                .public_inputs
                .insert(name.clone(), parsed_vals.clone());
            shares[2].public_inputs.insert(name.clone(), parsed_vals);
        } else {
            let [share0, share1, share2] =
                rep3::utils::share_field_elements(&parsed_vals, &mut rng);
            shares[0].shared_inputs.insert(name.clone(), share0);
            shares[1].shared_inputs.insert(name.clone(), share1);
            shares[2].shared_inputs.insert(name.clone(), share2);
        }
    }
    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
    tracing::info!("Sharing took {} ms", duration_ms);

    // write out the shares to the output directory
    let base_name = input
        .file_name()
        .context("we have a file name")?
        .to_str()
        .context("input file name is not valid UTF-8")?;
    for (i, share) in shares.iter().enumerate() {
        let path = out_dir.join(format!("{}.{}.shared", base_name, i));
        let out_file = BufWriter::new(File::create(&path).context("while creating output file")?);
        bincode::serialize_into(out_file, share).context("while serializing witness share")?;
        tracing::info!("Wrote input share {} to file {}", i, path.display());
    }
    tracing::info!("Split input into shares successfully");
    Ok(ExitCode::SUCCESS)
}

fn run_merge_input_shares<P: Pairing + CircomArkworksPairingBridge>(
    config: MergeInputSharesConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let inputs = config.inputs;
    let protocol = config.protocol;
    let out = config.out;

    if inputs.len() < 2 {
        return Err(eyre!("Need at least two input shares to merge"));
    }
    for input in &inputs {
        file_utils::check_file_exists(input)?;
    }

    match protocol {
        MPCProtocol::REP3 => {
            merge_input_shares::<P, Rep3Protocol<P::ScalarField, Rep3MpcNet>>(inputs, out)?;
        }
        MPCProtocol::SHAMIR => {
            merge_input_shares::<P, ShamirProtocol<P::ScalarField, ShamirMpcNet>>(inputs, out)?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn run_generate_witness<P: Pairing + CircomArkworksPairingBridge>(
    config: GenerateWitnessConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let input = config.input.clone();
    let circuit = config.circuit.clone();
    let link_library = config.link_library.clone();
    let protocol = config.protocol;
    let out = config.out.clone();

    if protocol != MPCProtocol::REP3 {
        return Err(eyre!(
            "Only REP3 protocol is supported for merging input shares"
        ));
    }
    file_utils::check_file_exists(&input)?;
    let circuit_path = PathBuf::from(&circuit);
    file_utils::check_file_exists(&circuit_path)?;

    // parse input shares
    let input_share_file =
        BufReader::new(File::open(&input).context("while opening input share file")?);
    let input_share = co_circom::parse_shared_input(input_share_file)?;

    // Extend the witness
    let result_witness_share =
        co_circom::generate_witness_rep3::<P>(circuit, link_library, input_share, config)?;

    // write result to output file
    let out_file = BufWriter::new(std::fs::File::create(&out)?);
    bincode::serialize_into(out_file, &result_witness_share)?;
    tracing::info!("Witness successfully written to {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn run_translate_witness<P: Pairing + CircomArkworksPairingBridge>(
    config: TranslateWitnessConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let witness = config.witness;
    let src_protocol = config.src_protocol;
    let target_protocol = config.target_protocol;
    let out = config.out;

    if src_protocol != MPCProtocol::REP3 || target_protocol != MPCProtocol::SHAMIR {
        return Err(eyre!("Only REP3 to SHAMIR translation is supported"));
    }
    file_utils::check_file_exists(&witness)?;

    // parse witness shares
    let witness_file =
        BufReader::new(File::open(witness).context("trying to open witness share file")?);
    let witness_share: SharedWitness<Rep3Protocol<P::ScalarField, Rep3MpcNet>, P> =
        co_circom::parse_witness_share(witness_file)?;

    // connect to network
    let net = Rep3MpcNet::new(config.network)?;
    let id = usize::from(net.get_id());

    // init MPC protocol
    let protocol = Rep3Protocol::new(net)?;
    let mut protocol = protocol.get_shamir_protocol()?;

    // Translate witness to shamir shares
    let start = Instant::now();
    let shamir_witness_share: SharedWitness<ShamirProtocol<P::ScalarField, ShamirMpcNet>, P> =
        SharedWitness {
            public_inputs: witness_share.public_inputs,
            witness: protocol.translate_primefield_repshare_vec(witness_share.witness)?,
        };
    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
    tracing::info!("Party {}: Translating witness took {} ms", id, duration_ms);

    // write result to output file
    let out_file = BufWriter::new(std::fs::File::create(&out)?);
    bincode::serialize_into(out_file, &shamir_witness_share)?;
    tracing::info!("Witness successfully written to {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn run_generate_proof<P: Pairing + CircomArkworksPairingBridge>(
    config: GenerateProofConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let proof_system = config.proof_system;
    let witness = config.witness;
    let zkey = config.zkey;
    let protocol = config.protocol;
    let out = config.out;
    let public_input_filename = config.public_input;
    let t = config.threshold;

    file_utils::check_file_exists(&witness)?;
    file_utils::check_file_exists(&zkey)?;

    // parse witness shares
    let witness_file =
        BufReader::new(File::open(witness).context("trying to open witness share file")?);

    // parse Circom zkey file
    let zkey_file = File::open(zkey)?;

    let public_input = match proof_system {
        ProofSystem::Groth16 => {
            let zkey = Groth16ZKey::<P>::from_reader(zkey_file).context("reading zkey")?;

            let (proof, public_input) = match protocol {
                MPCProtocol::REP3 => {
                    if t != 1 {
                        return Err(eyre!("REP3 only allows the threshold to be 1"));
                    }

                    let witness_share = co_circom::parse_witness_share(witness_file)?;
                    let public_input = witness_share.public_inputs.clone();
                    // connect to network
                    let net = Rep3MpcNet::new(config.network)?;
                    let id = usize::from(net.get_id());

                    // init MPC protocol
                    let protocol = Rep3Protocol::new(net)?;

                    let mut prover = CoGroth16::new(protocol);

                    // execute prover in MPC
                    let start = Instant::now();
                    let proof = prover.prove(&zkey, witness_share)?;
                    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
                    tracing::info!("Party {}: Proof generation took {} ms", id, duration_ms);

                    (proof, public_input)
                }
                MPCProtocol::SHAMIR => {
                    let witness_share = co_circom::parse_witness_share(witness_file)?;
                    let public_input = witness_share.public_inputs.clone();

                    // connect to network
                    let net = ShamirMpcNet::new(config.network)?;
                    let id = net.get_id();

                    // init MPC protocol
                    let protocol = ShamirProtocol::new(t, net)?;

                    let mut prover = CoGroth16::new(protocol);

                    // execute prover in MPC
                    let start = Instant::now();
                    let proof = prover.prove(&zkey, witness_share)?;
                    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
                    tracing::info!("Party {}: Proof generation took {} ms", id, duration_ms);

                    (proof, public_input)
                }
            };

            // write result to output file
            if let Some(out) = out {
                let out_file = BufWriter::new(
                    std::fs::File::create(&out).context("while creating output file")?,
                );

                serde_json::to_writer(out_file, &proof)
                    .context("while serializing proof to JSON file")?;
                tracing::info!("Wrote proof to file {}", out.display());
            }
            public_input
        }
        ProofSystem::Plonk => {
            let pk = PlonkZKey::<P>::from_reader(zkey_file).context("while parsing zkey")?;

            let (proof, public_input) = match protocol {
                MPCProtocol::REP3 => {
                    if t != 1 {
                        return Err(eyre!("REP3 only allows the threshold to be 1"));
                    }

                    let witness_share = co_circom::parse_witness_share(witness_file)?;
                    let public_input = witness_share.public_inputs.clone();
                    // connect to network
                    let net = Rep3MpcNet::new(config.network)?;
                    let id = usize::from(net.get_id());

                    // init MPC protocol
                    let protocol = Rep3Protocol::new(net)?;

                    let prover = CoPlonk::new(protocol);

                    // execute prover in MPC
                    let start = Instant::now();
                    let proof = prover.prove(&pk, witness_share)?;
                    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
                    tracing::info!("Party {}: Proof generation took {} ms", id, duration_ms);
                    (proof, public_input)
                }
                MPCProtocol::SHAMIR => {
                    let witness_share = co_circom::parse_witness_share(witness_file)?;
                    let public_input = witness_share.public_inputs.clone();

                    // connect to network
                    let net = ShamirMpcNet::new(config.network)?;
                    let id = net.get_id();

                    // init MPC protocol
                    let protocol = ShamirProtocol::new(t, net)?;

                    let prover = CoPlonk::new(protocol);

                    // execute prover in MPC
                    let start = Instant::now();
                    let proof = prover.prove(&pk, witness_share)?;
                    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
                    tracing::info!("Party {}: Proof generation took {} ms", id, duration_ms);
                    (proof, public_input)
                }
            };

            // write result to output file
            if let Some(out) = out {
                let out_file = BufWriter::new(
                    std::fs::File::create(&out).context("while creating output file")?,
                );

                serde_json::to_writer(out_file, &proof)
                    .context("while serializing proof to JSON file")?;
                tracing::info!("Wrote proof to file {}", out.display());
            }
            public_input
        }
    };

    // write public input to output file
    if let Some(public_input_filename) = public_input_filename {
        let public_input_as_strings = public_input
            .iter()
            .skip(1) // we skip the constant 1 at position 0
            .map(|f| {
                if f.is_zero() {
                    "0".to_string()
                } else {
                    f.to_string()
                }
            })
            .collect::<Vec<String>>();
        let public_input_file = BufWriter::new(
            std::fs::File::create(&public_input_filename)
                .context("while creating public input file")?,
        );
        serde_json::to_writer(public_input_file, &public_input_as_strings)
            .context("while writing out public inputs to JSON file")?;
        tracing::info!(
            "Wrote public inputs to file {}",
            public_input_filename.display()
        );
    }
    tracing::info!("Proof generation finished successfully");
    Ok(ExitCode::SUCCESS)
}

fn run_verify<P: Pairing + CircomArkworksPairingBridge>(
    config: VerifyConfig,
) -> color_eyre::Result<ExitCode>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
{
    let proofsystem = config.proof_system;
    let proof = config.proof;
    let vk = config.vk;
    let public_input = config.public_input;

    file_utils::check_file_exists(&proof)?;
    file_utils::check_file_exists(&vk)?;
    file_utils::check_file_exists(&public_input)?;

    // parse Circom proof file
    let proof_file = BufReader::new(File::open(&proof).context("while opening proof file")?);

    // parse Circom verification key file
    let vk_file = BufReader::new(File::open(&vk).context("while opening verification key file")?);

    // parse public inputs
    let public_inputs_file =
        BufReader::new(File::open(&public_input).context("while opening public inputs file")?);
    let public_inputs_as_strings: Vec<String> = serde_json::from_reader(public_inputs_file)
        .context(
            "while parsing public inputs, expect them to be array of stringified field elements",
        )?;
    // skip 1 atm
    let public_inputs = public_inputs_as_strings
        .into_iter()
        .map(|s| {
            s.parse::<P::ScalarField>()
                .map_err(|_| eyre!("could not parse as field element: {}", s))
        })
        .collect::<Result<Vec<P::ScalarField>, _>>()
        .context("while converting public input strings to field elements")?;

    // verify proof
    let res = match proofsystem {
        ProofSystem::Groth16 => {
            let proof: Groth16Proof<P> = serde_json::from_reader(proof_file)
                .context("while deserializing proof from file")?;

            let vk: Groth16JsonVerificationKey<P> = serde_json::from_reader(vk_file)
                .context("while deserializing verification key from file")?;

            // The actual verifier
            let start = Instant::now();
            let res = Groth16::<P>::verify(&vk, &proof, &public_inputs)
                .context("while verifying proof")?;
            let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
            tracing::info!("Proof verification took {} ms", duration_ms);
            res
        }
        ProofSystem::Plonk => {
            let proof: PlonkProof<P> = serde_json::from_reader(proof_file)
                .context("while deserializing proof from file")?;

            let vk: PlonkJsonVerificationKey<P> = serde_json::from_reader(vk_file)
                .context("while deserializing verification key from file")?;

            // The actual verifier
            let start = Instant::now();
            let res =
                Plonk::<P>::verify(&vk, &proof, &public_inputs).context("while verifying proof")?;
            let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
            tracing::info!("Proof verification took {} ms", duration_ms);
            res
        }
    };

    if res {
        tracing::info!("Proof verified successfully");
        Ok(ExitCode::SUCCESS)
    } else {
        tracing::error!("Proof verification failed");
        Ok(ExitCode::FAILURE)
    }
}

fn parse_field<F>(val: &serde_json::Value) -> color_eyre::Result<F>
where
    F: std::str::FromStr + PrimeField,
{
    let s = val.as_str().ok_or_else(|| {
        eyre!(
            "expected input to be a field element string, got \"{}\"",
            val
        )
    })?;
    let (is_negative, stripped) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, s)
    };
    let positive_value = if let Some(stripped) = stripped.strip_prefix("0x") {
        let big_int = BigUint::from_str_radix(stripped, 16)
            .map_err(|_| eyre!("could not parse field element: \"{}\"", val))
            .context("while parsing field element")?;
        let big_int: F::BigInt = big_int
            .try_into()
            .map_err(|_| eyre!("could not parse field element: \"{}\"", val))
            .context("while parsing field element")?;
        F::from(big_int)
    } else {
        stripped
            .parse::<F>()
            .map_err(|_| eyre!("could not parse field element: \"{}\"", val))
            .context("while parsing field element")?
    };
    if is_negative {
        Ok(-positive_value)
    } else {
        Ok(positive_value)
    }
}

fn parse_array<F: PrimeField>(val: &serde_json::Value) -> color_eyre::Result<Vec<F>> {
    let json_arr = val.as_array().expect("is an array");
    let mut field_elements = vec![];
    for ele in json_arr {
        if ele.is_array() {
            field_elements.extend(parse_array::<F>(ele)?);
        } else {
            field_elements.push(parse_field(ele)?);
        }
    }
    Ok(field_elements)
}

fn merge_input_shares<P: Pairing, T: PrimeFieldMpcProtocol<P::ScalarField>>(
    inputs: Vec<PathBuf>,
    out: PathBuf,
) -> color_eyre::Result<()> {
    let start = Instant::now();
    let mut input_shares = inputs
        .iter()
        .map(|input| {
            let input_share_file =
                BufReader::new(File::open(input).context("while opening input share file")?);
            let input_share: SharedInput<T, P> = bincode::deserialize_from(input_share_file)
                .context("trying to parse input share file")?;
            color_eyre::Result::<_>::Ok(input_share)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let start_item = input_shares.pop().expect("we have at least two inputs");
    let merged = input_shares.into_iter().try_fold(start_item, |a, b| {
        a.merge(b).context("while merging input shares")
    })?;
    let duration_ms = start.elapsed().as_micros() as f64 / 1000.;
    tracing::info!("Merging took {} ms", duration_ms);

    let out_file = BufWriter::new(File::create(&out).context("while creating output file")?);
    bincode::serialize_into(out_file, &merged).context("while serializing witness share")?;
    tracing::info!("Wrote merged input share to file {}", out.display());
    Ok(())
}
