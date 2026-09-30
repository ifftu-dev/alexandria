use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, ensure, Context, Result};
use num_bigint::BigInt;
use serde::Deserialize;
use serde_json::json;

#[allow(clippy::missing_safety_doc)]
mod generated {
    rust_witness::witness!(aadhaarverifier);
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InputValue {
    Scalar(String),
    Vector(Vec<String>),
}

fn parse_inputs(raw: &str) -> Result<BTreeMap<String, Vec<BigInt>>> {
    let values: BTreeMap<String, InputValue> = serde_json::from_str(raw)?;
    values
        .into_iter()
        .map(|(name, value)| {
            let strings = match value {
                InputValue::Scalar(value) => vec![value],
                InputValue::Vector(values) => values,
            };
            let values = strings
                .into_iter()
                .map(|value| {
                    ensure!(
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
                        "input {name} must contain unsigned decimal integers"
                    );
                    value.parse::<BigInt>().context("invalid circuit integer")
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((name, values))
        })
        .collect()
}

fn max_rss_bytes() -> Result<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    ensure!(status == 0, "getrusage failed");
    let usage = unsafe { usage.assume_init() };
    let value = u64::try_from(usage.ru_maxrss)?;
    #[cfg(target_os = "macos")]
    return Ok(value);
    #[cfg(not(target_os = "macos"))]
    Ok(value * 1024)
}

fn main() -> Result<()> {
    #[cfg(target_os = "android")]
    if let Ok(parent) = std::env::var("PERSONHOOD_PARENT_PID") {
        let parent: libc::pid_t = parent.parse().context("invalid parent process ID")?;
        ensure!(
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } == 0,
            "failed to bind worker lifetime to app"
        );
        ensure!(unsafe { libc::getppid() } == parent, "app process exited");
    }
    let args: Vec<String> = std::env::args().collect();
    if !(args.len() == 5 || args.len() == 6) {
        bail!("usage: personhood-native-bench INPUT_JSON ZKEY VKEY OUTPUT_PREFIX [REFERENCE_WTNS]");
    }
    let started = Instant::now();
    let inputs = parse_inputs(&fs::read_to_string(&args[1])?)?;
    eprintln!("STAGE witness");
    let witness_started = Instant::now();
    let witness_values = generated::aadhaarverifier_witness(inputs);
    let witness_generation_ms = witness_started.elapsed().as_secs_f64() * 1000.0;
    let witness_len = witness_values.len();
    let encode_started = Instant::now();
    let witness = rust_rapidsnark::parse_bigints_to_witness(witness_values)?;
    let witness_encoding_ms = encode_started.elapsed().as_secs_f64() * 1000.0;
    let witness_bytes = witness.len();
    let reference_matches = if let Some(reference) = args.get(5) {
        ensure!(
            witness == fs::read(reference)?,
            "native witness differs from the JavaScript reference witness"
        );
        Some(true)
    } else {
        None
    };
    let witness_peak = max_rss_bytes()?;
    eprintln!("STAGE proving");
    let prove_started = Instant::now();
    let proof = rust_rapidsnark::groth16_prover_zkey_file_wrapper(&args[2], witness)?;
    let prove_ms = prove_started.elapsed().as_secs_f64() * 1000.0;
    eprintln!("STAGE verifying");
    let verify_started = Instant::now();
    let verified = rust_rapidsnark::groth16_verify_wrapper(
        &proof.proof,
        &proof.public_signals,
        &fs::read_to_string(&args[3])?,
    )?;
    ensure!(verified, "native-generated proof did not verify");
    let verify_ms = verify_started.elapsed().as_secs_f64() * 1000.0;
    let result = json!({
        "experimental": true,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "witness_backend": "rust-witness 0.1.6 (published WASM transpiled to native C)",
        "prover_backend": "rust-rapidsnark 0.1.4 / rapidsnark 0.0.8",
        "witness_values": witness_len,
        "witness_bytes": witness_bytes,
        "reference_witness_matches": reference_matches,
        "witness_generation_ms": witness_generation_ms,
        "witness_encoding_ms": witness_encoding_ms,
        "proof_generation_ms": prove_ms,
        "verification_ms": verify_ms,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "witness_peak_rss_bytes": witness_peak,
        "peak_rss_bytes": max_rss_bytes()?,
        "native_verified": verified,
    });
    let output = PathBuf::from(&args[4]);
    fs::write(output.with_extension("proof.json"), proof.proof)?;
    fs::write(output.with_extension("public.json"), proof.public_signals)?;
    let result_json = serde_json::to_string_pretty(&result)?;
    fs::write(output.with_extension("results.json"), &result_json)?;
    println!("{result_json}");
    Ok(())
}
