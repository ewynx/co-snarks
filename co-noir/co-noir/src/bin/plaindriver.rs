use acir::native_types::{WitnessMap, WitnessStack};
use ark_bn254::Bn254;
use ark_ff::PrimeField;
use clap::Parser;
use co_acvm::{solver::PlainCoSolver, PlainAcvmSolver};
use co_noir::{file_utils, ConfigError, TranscriptHash};
use co_ultrahonk::{
    prelude::{
        CoUltraHonk, PlainUltraHonkDriver, Poseidon2Sponge, ProvingKey, UltraHonk, Utils,
        VerifyingKey,
    },
    PlainCoBuilder,
};
use color_eyre::eyre::{Context, ContextCompat};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use sha3::Keccak256;
use std::{
    io::{BufWriter, Write},
    path::PathBuf,
    process::ExitCode,
};

/// Cli arguments
#[derive(Parser, Debug, Default, Serialize)]
pub struct Cli {
    /// The path to the config file
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub config: Option<PathBuf>,
    /// The path to the prover crs file
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub prover_crs: Option<PathBuf>,
    /// The path to the verifier crs file
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub verifier_crs: Option<PathBuf>,
    /// The path to the input file
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub input: Option<PathBuf>,
    /// The path to the circuit file, generated by Noir
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub circuit: Option<PathBuf>,
    /// The transcript hasher to be used
    #[arg(long, value_enum)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub hasher: Option<TranscriptHash>,
    /// The path to the (existing) output directory
    #[arg(long)]
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub out_dir: Option<PathBuf>,
}

/// Config
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The path to the prover crs file
    pub prover_crs: PathBuf,
    /// The path to the verifier crs file
    pub verifier_crs: PathBuf,
    /// The path to the input file
    pub input: PathBuf,
    /// The path to the circuit file
    pub circuit: PathBuf,
    /// The transcript hasher to be used
    pub hasher: TranscriptHash,
    /// The output file where the final witness share is written to
    pub out_dir: PathBuf,
}

/// Prefix for config env variables
pub const CONFIG_ENV_PREFIX: &str = "CONOIR_";

impl Config {
    /// Parse config from file, env, cli
    pub fn parse(cli: Cli) -> Result<Self, ConfigError> {
        if let Some(path) = &cli.config {
            Ok(Figment::new()
                .merge(Toml::file(path))
                .merge(Env::prefixed(CONFIG_ENV_PREFIX))
                .merge(Serialized::defaults(cli))
                .extract()?)
        } else {
            Ok(Figment::new()
                .merge(Env::prefixed(CONFIG_ENV_PREFIX))
                .merge(Serialized::defaults(cli))
                .extract()?)
        }
    }
}

fn install_tracing() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_line_number(false)
        .with_timer(());
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}

fn witness_map_to_witness_vector<F: PrimeField>(witness_map: WitnessMap<F>) -> Vec<F> {
    let mut wv = Vec::new();
    let mut index = 0;
    for (w, f) in witness_map.into_iter() {
        // ACIR uses a sparse format for WitnessMap where unused witness indices may be left unassigned.
        // To ensure that witnesses sit at the correct indices in the `WitnessVector`, we fill any indices
        // which do not exist within the `WitnessMap` with the dummy value of zero.
        while index < w.0 {
            wv.push(F::zero());
            index += 1;
        }
        wv.push(f);
        index += 1;
    }
    wv
}

fn convert_witness<F: PrimeField>(mut witness_stack: WitnessStack<F>) -> Vec<F> {
    let witness_map = witness_stack
        .pop()
        .expect("Witness should be present")
        .witness;
    witness_map_to_witness_vector(witness_map)
}

fn main() -> color_eyre::Result<ExitCode> {
    install_tracing();

    let args = Cli::parse();
    let config = Config::parse(args)?;

    let prover_crs_path = config.prover_crs;
    let verifier_crs_path = config.verifier_crs;
    let input_path = config.input;
    let circuit_path = config.circuit;
    let hasher = config.hasher;
    let out_dir = config.out_dir;

    file_utils::check_file_exists(&prover_crs_path)?;
    file_utils::check_file_exists(&verifier_crs_path)?;
    file_utils::check_file_exists(&input_path)?;
    file_utils::check_file_exists(&circuit_path)?;
    file_utils::check_dir_exists(&out_dir)?;

    // Read circuit
    let program_artifact = Utils::get_program_artifact_from_file(&circuit_path)
        .context("while parsing program artifact")?;
    let constraint_system = Utils::get_constraint_system_from_artifact(&program_artifact, true);

    // Create witness
    let solver = PlainCoSolver::init_plain_driver(program_artifact, input_path)
        .context("while initializing plain driver")?;
    let witness = solver.solve().context("while solving")?;
    let witness = convert_witness(witness);

    // Build the circuit
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
    .context("while creating the circuit")?;

    // Read the Crs
    let crs = ProvingKey::<PlainUltraHonkDriver, _>::get_crs(
        &builder,
        prover_crs_path
            .to_str()
            .context("while opening prover crs file")?,
        verifier_crs_path
            .to_str()
            .context("while opening verifier crs file")?,
    )?;
    let (prover_crs, verifier_crs) = crs.split();

    // Create the proving key and the barretenberg-compatible verifying key
    let (proving_key, vk_barretenberg) =
        ProvingKey::create_keys_barretenberg(0, builder, prover_crs, &mut driver)
            .context("While creating keys")?;

    // Write the vk to a file
    let out_path = out_dir.join("vk");
    let mut out_file = BufWriter::new(
        std::fs::File::create(&out_path).context("while creating output file for vk")?,
    );
    let vk_u8 = match hasher {
        TranscriptHash::POSEIDON => vk_barretenberg.to_buffer(),
        TranscriptHash::KECCAK => vk_barretenberg.to_buffer_keccak(),
    };
    out_file
        .write(vk_u8.as_slice())
        .context("while writing vk to file")?;
    tracing::info!("Wrote vk to file {}", out_path.display());

    // Create the proof
    let driver = PlainUltraHonkDriver;
    let proof = match hasher {
        TranscriptHash::POSEIDON => {
            let prover = CoUltraHonk::<_, _, Poseidon2Sponge>::new(driver);
            prover.prove(proving_key).context("While creating proof")?
        }
        TranscriptHash::KECCAK => {
            let prover = CoUltraHonk::<_, _, Keccak256>::new(driver);
            prover.prove(proving_key).context("While creating proof")?
        }
    };

    // Write the proof to a file
    let out_path = out_dir.join("proof");
    let mut out_file = BufWriter::new(
        std::fs::File::create(&out_path).context("while creating output file for proof")?,
    );
    let proof_u8 = proof.to_buffer();
    out_file
        .write(proof_u8.as_slice())
        .context("while writing proof to file")?;
    tracing::info!("Wrote proof to file {}", out_path.display());

    // Get the verifying key
    let verifying_key = VerifyingKey::from_barrettenberg_and_crs(vk_barretenberg, verifier_crs);

    // Verify the proof
    let is_valid = match hasher {
        TranscriptHash::POSEIDON => UltraHonk::<_, Poseidon2Sponge>::verify(proof, verifying_key)
            .context("While verifying proof")?,
        TranscriptHash::KECCAK => UltraHonk::<_, Keccak256>::verify(proof, verifying_key)
            .context("While verifying proof")?,
    };

    if is_valid {
        tracing::info!("Proof verified successfully");
        Ok(ExitCode::SUCCESS)
    } else {
        tracing::error!("Proof verification failed");
        Ok(ExitCode::FAILURE)
    }
}
