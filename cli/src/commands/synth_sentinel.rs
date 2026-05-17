//! `alex synth-sentinel` — Sentinel synthetic-data toolkit.
//!
//! Generates the JSON adversarial-prior blobs the paste classifier
//! trains against. Inference and training happen elsewhere (Python
//! side-repo bundled with the ONNX export pipeline); this CLI only
//! emits the input data.
//!
//! Output blobs match the schema in `docs/sentinel-adversarial-priors.md`.
//! Same seed → byte-identical bytes (tests assert via Blake2b).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::output;
use crate::synth::generators::{generate_label, AttackLabel, GenerateOptions};
use crate::synth::rng::blake2b_hex;

#[derive(Subcommand)]
pub enum SynthSentinelCommand {
    /// Generate one labeled blob for a single attack class
    Generate(GenerateArgs),

    /// Generate the full training mix (all labels) with default counts
    GenerateAll(GenerateAllArgs),

    /// Generate the holdout mix (disjoint seeds, smaller counts)
    GenerateHoldout(GenerateAllArgs),

    /// Print summary stats for a generated blob — sanity check before publishing
    Stats(StatsArgs),
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Attack class to generate
    #[arg(long, value_enum)]
    pub label: AttackLabel,

    /// Number of samples in the blob
    #[arg(long, default_value_t = 10_000)]
    pub count: usize,

    /// 64-bit deterministic seed
    #[arg(long, default_value_t = 1)]
    pub seed: u64,

    /// Output JSON file path
    #[arg(long)]
    pub out: PathBuf,

    /// Pretty-print JSON. Off by default (smaller, identical bytes)
    #[arg(long, default_value_t = false)]
    pub pretty: bool,
}

#[derive(Args)]
pub struct GenerateAllArgs {
    /// Directory to write per-label .json files to
    #[arg(long)]
    pub out_dir: PathBuf,

    /// Base seed — each label gets `seed_base + index*1009` (1009 is a
    /// prime offset so adjacent labels share no Markov state in the RNG)
    #[arg(long, default_value_t = 1)]
    pub seed_base: u64,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Path to a generated blob
    #[arg(long, short)]
    pub input: PathBuf,
}

pub fn execute(cmd: &SynthSentinelCommand) -> Result<()> {
    match cmd {
        SynthSentinelCommand::Generate(args) => run_generate(args),
        SynthSentinelCommand::GenerateAll(args) => run_generate_set(args, false),
        SynthSentinelCommand::GenerateHoldout(args) => run_generate_set(args, true),
        SynthSentinelCommand::Stats(args) => run_stats(args),
    }
}

const SEED_OFFSET: u64 = 1009;
const HOLDOUT_SEED_BASE: u64 = 100_000;

const TRAIN_PLAN: &[(AttackLabel, usize)] = &[
    (AttackLabel::PasteMacro, 10_000),
    (AttackLabel::TypingBotConstant, 10_000),
    (AttackLabel::TypingBotJitter, 10_000),
    (AttackLabel::LlmPasteEdit, 15_000),
    (AttackLabel::RemoteControl, 7_500),
    (AttackLabel::HumanBaseline, 50_000),
];

const HOLDOUT_PLAN: &[(AttackLabel, usize)] = &[
    (AttackLabel::PasteMacro, 2_000),
    (AttackLabel::TypingBotConstant, 2_000),
    (AttackLabel::TypingBotJitter, 2_000),
    (AttackLabel::LlmPasteEdit, 3_000),
    (AttackLabel::RemoteControl, 1_500),
    (AttackLabel::HumanBaseline, 10_000),
];

fn run_generate(args: &GenerateArgs) -> Result<()> {
    let blob = generate_label(&GenerateOptions {
        label: args.label,
        count: args.count,
        seed: args.seed,
    });
    let bytes = if args.pretty {
        serde_json::to_vec_pretty(&blob)?
    } else {
        serde_json::to_vec(&blob)?
    };
    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
    }
    fs::write(&args.out, &bytes).with_context(|| format!("write {:?}", args.out))?;
    let hash = blake2b_hex(&bytes);
    output::success(&format!(
        "wrote {} samples → {} ({} bytes, blake2b={}…)",
        blob.samples.len(),
        args.out.display(),
        bytes.len(),
        &hash[..16],
    ));
    Ok(())
}

fn run_generate_set(args: &GenerateAllArgs, is_holdout: bool) -> Result<()> {
    fs::create_dir_all(&args.out_dir).with_context(|| format!("create {:?}", args.out_dir))?;

    let (plan, seed_base, kind) = if is_holdout {
        (
            HOLDOUT_PLAN,
            HOLDOUT_SEED_BASE.max(args.seed_base),
            "holdout",
        )
    } else {
        (TRAIN_PLAN, args.seed_base, "train")
    };

    output::info(&format!(
        "generating {} set in {:?} (seed_base={})",
        kind, args.out_dir, seed_base
    ));

    for (idx, (label, count)) in plan.iter().enumerate() {
        let seed = seed_base + (idx as u64) * SEED_OFFSET;
        let opts = GenerateOptions {
            label: *label,
            count: *count,
            seed,
        };
        let blob = generate_label(&opts);
        let bytes = serde_json::to_vec(&blob)?;
        let path = args.out_dir.join(format!("{}.json", label.as_str()));
        fs::write(&path, &bytes).with_context(|| format!("write {:?}", path))?;
        let hash = blake2b_hex(&bytes);
        output::success(&format!(
            "  {} → {} samples, seed={}, blake2b={}…",
            label.as_str(),
            count,
            seed,
            &hash[..16],
        ));
    }
    Ok(())
}

fn run_stats(args: &StatsArgs) -> Result<()> {
    let bytes = fs::read(&args.input).with_context(|| format!("read {:?}", args.input))?;
    let blob: crate::synth::PriorBlob = serde_json::from_slice(&bytes)?;

    let mut total_keystrokes = 0usize;
    let mut all_dwell: Vec<f32> = vec![];
    let mut all_flight: Vec<f32> = vec![];
    for s in &blob.samples {
        total_keystrokes += s.digraphs.len();
        all_dwell.extend(&s.dwell_ms);
        all_flight.extend(&s.flight_ms);
    }

    let mean_d = mean(&all_dwell);
    let mean_f = mean(&all_flight);
    let std_d = std(&all_dwell, mean_d);
    let std_f = std(&all_flight, mean_f);

    output::info(&format!("file:           {}", args.input.display()));
    output::info(&format!("label:          {}", blob.label));
    output::info(&format!("model_kind:     {}", blob.model_kind));
    output::info(&format!("schema_version: {}", blob.schema_version));
    output::info(&format!("synth_version:  {}", blob.synth_version));
    output::info(&format!("synth_seed:     {}", blob.synth_seed));
    output::info(&format!("samples:        {}", blob.samples.len()));
    output::info(&format!("keystrokes:     {}", total_keystrokes));
    output::info(&format!(
        "dwell_ms:       mean={:.1}  std={:.1}  cv={:.3}",
        mean_d,
        std_d,
        if mean_d > 0.001 { std_d / mean_d } else { 0.0 }
    ));
    output::info(&format!(
        "flight_ms:      mean={:.1}  std={:.1}  cv={:.3}",
        mean_f,
        std_f,
        if mean_f > 0.001 { std_f / mean_f } else { 0.0 }
    ));
    output::info(&format!("blake2b:        {}…", &blake2b_hex(&bytes)[..16]));
    Ok(())
}

fn mean(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f32>() / xs.len() as f32
}

fn std(xs: &[f32], m: f32) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    let var = xs.iter().map(|v| (v - m).powi(2)).sum::<f32>() / xs.len() as f32;
    var.sqrt()
}
