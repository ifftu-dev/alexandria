//! Offline evaluation of saved responses. This module never calls a provider.
use std::collections::BTreeSet;

use alexandria_learning_contracts::{
    BloomLevel, DecisionRecord, Task, TaxonomySnapshot, RELATIONS,
};
use serde::{Deserialize, Serialize};

use crate::{learning, Error, Response, MODEL};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub dataset_id: String,
    pub provenance: String,
    pub split: String,
    pub independently_labelled: bool,
    pub language: String,
    pub task: Task,
    pub taxonomy_digest: String,
    pub model: String,
    pub rubric_version: String,
    pub thresholds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub skill_id: String,
    pub relation: String,
    pub bloom: Option<BloomLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub id: String,
    pub source: String,
    pub candidates: Vec<String>,
    pub expected: Vec<Label>,
    pub baseline: Vec<Label>,
    pub response: Option<Response>,
    #[serde(default)]
    pub record: Option<DecisionRecord>,
    pub latency_ms: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Metrics {
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub necessity_errors: usize,
    pub bloom_overstatements: usize,
    pub bloom_absolute_error: u64,
    pub bloom_pairs: usize,
    pub predictions: usize,
}

impl Metrics {
    fn add(&mut self, expected: &[Label], predicted: &[Label]) {
        let positive = |r: &str| matches!(r, "required" | "preferred");
        self.predictions += predicted.len();
        for p in predicted {
            let expected = expected.iter().find(|e| e.skill_id == p.skill_id);
            if positive(&p.relation) {
                if expected.is_some_and(|e| positive(&e.relation)) {
                    self.true_positives += 1;
                } else {
                    self.false_positives += 1;
                }
            }
            if let Some(e) = expected {
                if e.relation != p.relation {
                    self.necessity_errors += 1;
                }
                if let (Some(actual), Some(wanted)) = (p.bloom, e.bloom) {
                    self.bloom_pairs += 1;
                    self.bloom_absolute_error += u64::from(actual.rank().abs_diff(wanted.rank()));
                    if actual > wanted {
                        self.bloom_overstatements += 1;
                    }
                }
            }
        }
        self.false_negatives += expected
            .iter()
            .filter(|e| {
                positive(&e.relation)
                    && !predicted
                        .iter()
                        .any(|p| p.skill_id == e.skill_id && positive(&p.relation))
            })
            .count();
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub manifest: Manifest,
    pub examples: usize,
    pub invalid_or_failed: usize,
    pub candidate_hits: usize,
    pub expected_positive_skills: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub baseline: Metrics,
    pub thresholds: Vec<(f64, Metrics)>,
    /// A report is evidence for an owner review, never permission to enable assist.
    pub rollout_approved: bool,
}

pub fn evaluate(
    manifest: Manifest,
    taxonomy: &TaxonomySnapshot,
    examples: &[Example],
) -> Result<Report, Error> {
    taxonomy
        .validate(&taxonomy.network_id)
        .map_err(|_| Error::Invalid)?;
    if manifest.model != MODEL
        || manifest.rubric_version != learning::RUBRIC
        || manifest.taxonomy_digest != taxonomy.digest
        || manifest.dataset_id.is_empty()
        || manifest.provenance.is_empty()
        || manifest.language.is_empty()
        || !["development", "held_out", "synthetic"].contains(&manifest.split.as_str())
        || manifest.thresholds.is_empty()
        || manifest.thresholds.len() > 20
        || manifest
            .thresholds
            .iter()
            .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
        || examples.is_empty()
    {
        return Err(Error::Invalid);
    }
    let thresholds = manifest
        .thresholds
        .iter()
        .map(|n| (*n, Metrics::default()))
        .collect();
    let mut report = Report {
        manifest,
        examples: examples.len(),
        invalid_or_failed: 0,
        candidate_hits: 0,
        expected_positive_skills: 0,
        input_tokens: 0,
        output_tokens: 0,
        p50_ms: 0,
        p95_ms: 0,
        baseline: Metrics::default(),
        thresholds,
        rollout_approved: false,
    };
    let mut ids = BTreeSet::new();
    let mut latencies = Vec::new();
    for example in examples {
        if !ids.insert(&example.id) || example.id.is_empty() {
            return Err(Error::Invalid);
        }
        for labels in [&example.expected, &example.baseline] {
            let mut skills = BTreeSet::new();
            if labels.iter().any(|l| {
                !skills.insert(&l.skill_id)
                    || !taxonomy.skills.iter().any(|s| s.skill_id == l.skill_id)
                    || !RELATIONS.contains(&l.relation.as_str())
            }) {
                return Err(Error::Invalid);
            }
        }
        report.baseline.add(&example.expected, &example.baseline);
        for expected in &example.expected {
            if matches!(expected.relation.as_str(), "required" | "preferred") {
                report.expected_positive_skills += 1;
                report.candidate_hits +=
                    usize::from(example.candidates.contains(&expected.skill_id));
            }
        }
        latencies.push(example.latency_ms);
        let record = example
            .response
            .as_ref()
            .and_then(|response| {
                learning::record(
                    &example.source,
                    taxonomy,
                    &example.candidates,
                    report.manifest.task,
                    response,
                )
                .ok()
            })
            .or_else(|| {
                example
                    .record
                    .as_ref()
                    .filter(|record| {
                        record.task == report.manifest.task
                            && record.model == MODEL
                            && record.rubric_version == learning::RUBRIC
                            && record.validate(&example.source, taxonomy).is_ok()
                    })
                    .cloned()
            });
        if let Some(response) = &example.response {
            report.input_tokens = report
                .input_tokens
                .saturating_add(response.usage.input_tokens);
            report.output_tokens = report
                .output_tokens
                .saturating_add(response.usage.output_tokens);
        }
        if record.is_none() {
            report.invalid_or_failed += 1;
        }
        for (threshold, metrics) in &mut report.thresholds {
            let predictions: Vec<_> = record
                .as_ref()
                .map(|r| {
                    r.decisions
                        .iter()
                        .filter(|d| {
                            d.relation.confidence >= *threshold && d.relation.value != "uncertain"
                        })
                        .map(|d| Label {
                            skill_id: d.skill_id.clone(),
                            relation: d.relation.value.clone(),
                            bloom: if d.bloom.confidence >= *threshold {
                                d.bloom.value.parse().ok()
                            } else {
                                None
                            },
                        })
                        .collect()
                })
                .unwrap_or_default();
            metrics.add(&example.expected, &predictions);
        }
    }
    latencies.sort();
    report.p50_ms = latencies[(latencies.len() * 50).div_ceil(100) - 1];
    report.p95_ms = latencies[(latencies.len() * 95).div_ceil(100) - 1];
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_predictions_are_not_perfect_precision_or_recall() {
        let expected = vec![Label {
            skill_id: "s".into(),
            relation: "required".into(),
            bloom: Some(BloomLevel::Apply),
        }];
        let mut metrics = Metrics::default();
        metrics.add(&expected, &[]);
        assert_eq!(metrics.false_negatives, 1);
        assert_eq!(metrics.true_positives, 0);
        metrics.add(
            &expected,
            &[Label {
                skill_id: "s".into(),
                relation: "preferred".into(),
                bloom: Some(BloomLevel::Create),
            }],
        );
        assert_eq!(metrics.bloom_overstatements, 1);
        assert_eq!(metrics.necessity_errors, 1);
        assert_eq!(metrics.bloom_absolute_error, 3);
    }
}
