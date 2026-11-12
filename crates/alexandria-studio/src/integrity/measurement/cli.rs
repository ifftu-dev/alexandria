//! The evaluator's command line, kept in the library so tests drive exactly
//! what CI runs.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use super::corpus::parse;
use super::{
    evaluate, seal, Certification, Corpus, CorpusEntry, CorpusWindow, MeasurementError, Provenance,
};
use crate::integrity::RatioManifest;

pub const USAGE: &str = "\
usage: integrity_evaluate <command> ...
  report <corpus/manifest.json> [--certification <file>] [--json]
         replay the corpus; print per-signal precision, recall, false-positive
         rate and likelihood ratio per profile, and the fusion confusion
         matrix (uncertified ratios unless a certification is given)
  check  <corpus/manifest.json> <certification.json>
         report against the certification, then fail (exit 1) if any admitted
         signal falls below its certified precision or recall
  draft  <corpus/manifest.json> --version <v> --date <yyyy-mm-dd> --threshold <profile>=<llr>...
         print a certification draft for review: every signal seen in attacks
         on each profile given a threshold, at its measured rates
  entry  --capture <session.jsonl> --id <id> --label <clean|attack> --provenance <red_team|consented_clean|synthetic>
         --profile <profile> --platform <platform> --device-class <class>
         print a corpus entry built from a captured session (one window per line)
  seal   <corpus/manifest.json>
         recompute every entry digest and the corpus digest in place";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    /// A certified rate was not met.
    Fail,
}

fn usage() -> MeasurementError {
    MeasurementError::Invalid(USAGE.into())
}

fn emit(out: &mut dyn Write, text: &str) -> Result<(), MeasurementError> {
    out.write_all(text.as_bytes())
        .map_err(|e| MeasurementError::Invalid(format!("writing output: {e}")))
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, MeasurementError> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|e| MeasurementError::Invalid(e.to_string()))?;
    json.push('\n');
    Ok(json)
}

fn report(args: &[String], out: &mut dyn Write) -> Result<Outcome, MeasurementError> {
    let (manifest, mut rest) = args.split_first().ok_or_else(usage)?;
    let mut certification = None;
    let mut json = false;
    while let Some((flag, tail)) = rest.split_first() {
        match (flag.as_str(), tail) {
            ("--json", _) => {
                json = true;
                rest = tail;
            }
            ("--certification", [file, tail @ ..]) => {
                certification = Some(Certification::load(Path::new(file))?.0);
                rest = tail;
            }
            _ => return Err(usage()),
        }
    }
    let corpus = Corpus::load(Path::new(manifest))?;
    let ratios = certification
        .as_ref()
        .map_or_else(RatioManifest::uncertified, Certification::ratio_manifest);
    let report = evaluate(&corpus, &ratios);
    emit(
        out,
        &if json {
            to_json(&report)?
        } else {
            report.render()
        },
    )?;
    Ok(Outcome::Pass)
}

fn draft(args: &[String], out: &mut dyn Write) -> Result<Outcome, MeasurementError> {
    let (manifest, mut rest) = args.split_first().ok_or_else(usage)?;
    let (mut version, mut date) = (None, None);
    let mut thresholds = BTreeMap::new();
    while let [flag, value, tail @ ..] = rest {
        match flag.as_str() {
            "--version" => version = Some(value.as_str()),
            "--date" => date = Some(value.as_str()),
            "--threshold" => {
                let (profile, threshold) = value
                    .split_once('=')
                    .and_then(|(p, t)| Some((p.to_string(), t.parse::<f64>().ok()?)))
                    .ok_or_else(usage)?;
                thresholds.insert(profile, threshold);
            }
            _ => return Err(usage()),
        }
        rest = tail;
    }
    let (Some(version), Some(date), true, true) =
        (version, date, rest.is_empty(), !thresholds.is_empty())
    else {
        return Err(usage());
    };
    let corpus = Corpus::load(Path::new(manifest))?;
    let report = evaluate(&corpus, &RatioManifest::uncertified());
    let draft = Certification::draft(&report, version, date, &thresholds);
    draft.validate().map_err(MeasurementError::Invalid)?;
    emit(out, &to_json(&draft)?)?;
    Ok(Outcome::Pass)
}

fn entry(args: &[String], out: &mut dyn Write) -> Result<Outcome, MeasurementError> {
    let mut fields = BTreeMap::new();
    let mut rest = args;
    while let [flag, value, tail @ ..] = rest {
        let key = flag.strip_prefix("--").ok_or_else(usage)?;
        if fields.insert(key, value.as_str()).is_some() {
            return Err(usage());
        }
        rest = tail;
    }
    let mut take = |key: &str| fields.remove(key).ok_or_else(usage);
    let capture = Path::new(take("capture")?);
    let id = take("id")?.to_string();
    let label = take("label")?.to_string();
    let provenance: Provenance = serde_json::from_value(take("provenance")?.into())
        .map_err(|e| MeasurementError::Invalid(format!("--provenance: {e}")))?;
    let profile = take("profile")?.to_string();
    let platform = take("platform")?.to_string();
    let device_class = take("device-class")?.to_string();
    if !rest.is_empty() || !fields.is_empty() {
        return Err(usage());
    }
    let text = std::fs::read_to_string(capture).map_err(|source| MeasurementError::Io {
        path: capture.to_path_buf(),
        source,
    })?;
    let windows = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse::<CorpusWindow>(capture, line.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    let entry = CorpusEntry {
        id,
        label,
        provenance,
        profile,
        platform,
        device_class,
        interview: false,
        verification_failed: false,
        windows,
    };
    entry.validate().map_err(MeasurementError::Invalid)?;
    emit(out, &to_json(&entry)?)?;
    Ok(Outcome::Pass)
}

fn check(
    manifest: &str,
    certification: &str,
    out: &mut dyn Write,
) -> Result<Outcome, MeasurementError> {
    let (certification, digest) = Certification::load(Path::new(certification))?;
    let corpus = Corpus::load(Path::new(manifest))?;
    let report = evaluate(&corpus, &certification.ratio_manifest());
    let shortfalls = certification.check(&report)?;
    emit(out, &report.render())?;
    emit(
        out,
        &format!("\ncertification {} {digest}\n", certification.version),
    )?;
    for shortfall in &shortfalls {
        emit(out, &format!("{shortfall}\n"))?;
    }
    if shortfalls.is_empty() {
        emit(
            out,
            "PASS every admitted signal meets its certified rates\n",
        )?;
        Ok(Outcome::Pass)
    } else {
        Ok(Outcome::Fail)
    }
}

pub fn run(args: &[String], out: &mut dyn Write) -> Result<Outcome, MeasurementError> {
    let (command, rest) = args.split_first().ok_or_else(usage)?;
    match (command.as_str(), rest) {
        ("report", rest) => report(rest, out),
        ("draft", rest) => draft(rest, out),
        ("entry", rest) => entry(rest, out),
        ("check", [manifest, certification]) => check(manifest, certification, out),
        ("seal", [manifest]) => {
            let sealed = seal(Path::new(manifest))?;
            emit(
                out,
                &format!(
                    "sealed {} entries: {}\n",
                    sealed.entries.len(),
                    sealed.digest
                ),
            )?;
            Ok(Outcome::Pass)
        }
        ("help" | "--help" | "-h", _) => {
            emit(out, USAGE)?;
            emit(out, "\n")?;
            Ok(Outcome::Pass)
        }
        _ => Err(usage()),
    }
}
