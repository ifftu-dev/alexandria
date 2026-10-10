use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u32 = 1;
pub const BLOOM_LEVELS: [&str; 6] = [
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BloomLevel {
    Remember,
    Understand,
    Apply,
    Analyze,
    Evaluate,
    Create,
}

impl BloomLevel {
    pub fn rank(self) -> u8 {
        self as u8
    }
    pub fn as_str(self) -> &'static str {
        BLOOM_LEVELS[self.rank() as usize]
    }
}

impl FromStr for BloomLevel {
    type Err = ContractError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "remember" => Ok(Self::Remember),
            "understand" => Ok(Self::Understand),
            "apply" => Ok(Self::Apply),
            "analyze" => Ok(Self::Analyze),
            "evaluate" => Ok(Self::Evaluate),
            "create" => Ok(Self::Create),
            _ => Err(ContractError::Invalid("unknown Bloom level")),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ContractError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("could not encode contract: {0}")]
    Encoding(String),
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub bloom_level: BloomLevel,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Prerequisite {
    pub skill_id: String,
    pub prerequisite_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaxonomySnapshot {
    pub schema_version: u32,
    pub network_id: String,
    pub revision: String,
    pub provenance: String,
    pub skills: Vec<Skill>,
    pub prerequisites: Vec<Prerequisite>,
    pub digest: String,
}

impl TaxonomySnapshot {
    /// Convert the application's reference data without copying provider or database code.
    pub fn from_reference(
        network: &str,
        revision: &str,
        json: &str,
    ) -> Result<Self, ContractError> {
        #[derive(Deserialize)]
        struct Reference {
            skills: Vec<ReferenceSkill>,
            skill_prerequisites: Vec<Prerequisite>,
        }
        #[derive(Deserialize)]
        struct ReferenceSkill {
            id: String,
            name: String,
            description: String,
            bloom_level: BloomLevel,
        }
        let reference: Reference =
            serde_json::from_str(json).map_err(|e| ContractError::Encoding(e.to_string()))?;
        Self {
            schema_version: SCHEMA_VERSION,
            network_id: network.into(),
            revision: revision.into(),
            provenance: "alexandria-bundled".into(),
            skills: reference
                .skills
                .into_iter()
                .map(|s| Skill {
                    skill_id: s.id,
                    name: s.name,
                    description: s.description,
                    bloom_level: s.bloom_level,
                    aliases: vec![],
                })
                .collect(),
            prerequisites: reference.skill_prerequisites,
            digest: String::new(),
        }
        .seal()
    }

    pub fn seal(mut self) -> Result<Self, ContractError> {
        self.skills.sort_by(|a, b| a.skill_id.cmp(&b.skill_id));
        for skill in &mut self.skills {
            skill.aliases.sort();
            skill.aliases.dedup();
        }
        self.prerequisites.sort();
        self.digest = self.content_digest()?;
        self.validate(&self.network_id)?;
        Ok(self)
    }

    pub fn content_digest(&self) -> Result<String, ContractError> {
        let mut content = self.clone();
        content.digest.clear();
        serde_json::to_vec(&content)
            .map(|bytes| hash(&bytes))
            .map_err(|e| ContractError::Encoding(e.to_string()))
    }

    pub fn validate(&self, network_id: &str) -> Result<(), ContractError> {
        if self.schema_version != SCHEMA_VERSION
            || self.network_id != network_id
            || network_id.is_empty()
            || self.revision.is_empty()
            || self.provenance.is_empty()
            || self.skills.is_empty()
            || self.skills.len() > 20_000
        {
            return Err(ContractError::Invalid("incompatible taxonomy metadata"));
        }
        let ids: BTreeSet<_> = self.skills.iter().map(|s| s.skill_id.as_str()).collect();
        if ids.len() != self.skills.len()
            || self.skills.iter().any(|s| {
                s.skill_id.is_empty() || s.name.trim().is_empty() || s.description.len() > 16_000
            })
        {
            return Err(ContractError::Invalid("invalid or duplicate skill"));
        }
        let mut indegree: BTreeMap<&str, usize> = ids.iter().map(|id| (*id, 0)).collect();
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for edge in &self.prerequisites {
            if edge.skill_id == edge.prerequisite_id
                || !ids.contains(edge.skill_id.as_str())
                || !ids.contains(edge.prerequisite_id.as_str())
                || !seen.insert(edge)
            {
                return Err(ContractError::Invalid("invalid prerequisite"));
            }
            *indegree
                .get_mut(edge.skill_id.as_str())
                .ok_or(ContractError::Invalid("missing skill"))? += 1;
            children
                .entry(&edge.prerequisite_id)
                .or_default()
                .push(&edge.skill_id);
        }
        let mut ready: Vec<_> = indegree
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut visited = 0;
        while let Some(id) = ready.pop() {
            visited += 1;
            for child in children.get(id).into_iter().flatten() {
                let n = indegree
                    .get_mut(child)
                    .ok_or(ContractError::Invalid("missing skill"))?;
                *n -= 1;
                if *n == 0 {
                    ready.push(child);
                }
            }
        }
        if visited != ids.len() {
            return Err(ContractError::Invalid("cyclic prerequisites"));
        }
        if self.digest != self.content_digest()? {
            return Err(ContractError::Invalid("taxonomy digest mismatch"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Shadow,
    Assist,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    JobDescription,
    LearningGoal,
    DocumentClaim,
    StudioReview,
    Search,
    Tutor,
    Incident,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Required,
    Preferred,
    Mentioned,
    Excluded,
    Unsupported,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub start: usize,
    pub end: usize,
}

impl Evidence {
    pub fn text<'a>(&self, source: &'a str) -> Result<&'a str, ContractError> {
        source
            .get(self.start..self.end)
            .filter(|s| !s.trim().is_empty())
            .ok_or(ContractError::Invalid("invalid evidence range"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub value: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
}

impl Judgment {
    pub fn validate(&self, options: &[&str]) -> Result<(), ContractError> {
        if self.probabilities.len() != options.len()
            || !options.contains(&self.value.as_str())
            || !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || options
                .iter()
                .any(|key| !self.probabilities.contains_key(*key))
            || self
                .probabilities
                .values()
                .any(|p| !p.is_finite() || !(0.0..=1.0).contains(p))
            || (self.probabilities.values().sum::<f64>() - 1.0).abs() > 0.001
            || self
                .probabilities
                .values()
                .any(|p| *p > self.probabilities[&self.value] + 0.000_001)
        {
            return Err(ContractError::Invalid("invalid choice distribution"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDecision {
    pub skill_id: String,
    pub relation: Judgment,
    pub bloom: Judgment,
    pub evidence: Option<Evidence>,
}

pub const RELATIONS: [&str; 6] = [
    "required",
    "preferred",
    "mentioned",
    "excluded",
    "unsupported",
    "uncertain",
];
pub const BLOOM_CHOICES: [&str; 7] = [
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
    "unknown",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub schema_version: u32,
    pub task: Task,
    pub taxonomy_digest: String,
    pub taxonomy_revision: String,
    pub source_hash: String,
    pub model: String,
    pub rubric_version: String,
    pub decisions: Vec<SkillDecision>,
}

impl DecisionRecord {
    pub fn validate(&self, source: &str, taxonomy: &TaxonomySnapshot) -> Result<(), ContractError> {
        if self.schema_version != SCHEMA_VERSION
            || self.source_hash != hash(source.as_bytes())
            || self.taxonomy_digest != taxonomy.digest
            || self.taxonomy_revision != taxonomy.revision
            || self.model.is_empty()
            || self.rubric_version.is_empty()
        {
            return Err(ContractError::Invalid("stale or incompatible decision"));
        }
        let mut seen = BTreeSet::new();
        for d in &self.decisions {
            if !seen.insert(&d.skill_id)
                || !taxonomy.skills.iter().any(|s| s.skill_id == d.skill_id)
            {
                return Err(ContractError::Invalid(
                    "unknown or duplicate decision skill",
                ));
            }
            d.relation.validate(&RELATIONS)?;
            d.bloom.validate(&BLOOM_CHOICES)?;
            if let Some(e) = &d.evidence {
                e.text(source)?;
            } else if matches!(d.relation.value.as_str(), "required" | "preferred") {
                return Err(ContractError::Invalid(
                    "supported decision needs source evidence",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> TaxonomySnapshot {
        TaxonomySnapshot {
            schema_version: 1,
            network_id: "test".into(),
            revision: "v1".into(),
            provenance: "fixture".into(),
            digest: String::new(),
            prerequisites: vec![],
            skills: ["a", "b"]
                .into_iter()
                .map(|id| Skill {
                    skill_id: id.into(),
                    name: id.into(),
                    description: String::new(),
                    bloom_level: BloomLevel::Apply,
                    aliases: vec![],
                })
                .collect(),
        }
        .seal()
        .unwrap()
    }
    #[test]
    fn strict_bloom_preserves_wire_order() {
        for (i, name) in BLOOM_LEVELS.iter().enumerate() {
            assert_eq!(name.parse::<BloomLevel>().unwrap().rank(), i as u8);
        }
        assert!("unknown".parse::<BloomLevel>().is_err());
        assert!(serde_json::from_str::<BloomLevel>("\"Apply\"").is_err());
    }
    #[test]
    fn taxonomy_rejects_tampering_network_duplicates_and_cycles() {
        let good = snapshot();
        assert!(good.validate("test").is_ok());
        assert!(good.validate("other").is_err());
        let mut bad = good.clone();
        bad.skills[0].name = "changed".into();
        assert!(bad.validate("test").is_err());
        bad = good.clone();
        bad.skills.push(bad.skills[0].clone());
        assert!(bad.seal().is_err());
        bad = good;
        bad.prerequisites = vec![
            Prerequisite {
                skill_id: "a".into(),
                prerequisite_id: "b".into(),
            },
            Prerequisite {
                skill_id: "b".into(),
                prerequisite_id: "a".into(),
            },
        ];
        assert!(bad.seal().is_err());
    }
    #[test]
    fn evidence_cannot_split_utf8_or_escape_source() {
        assert!(Evidence { start: 1, end: 2 }.text("é").is_err());
        assert!(Evidence { start: 0, end: 99 }.text("source").is_err());
    }
    #[test]
    fn choice_checks_completeness_and_winner() {
        let mut j = Judgment {
            value: "yes".into(),
            probabilities: BTreeMap::from([("yes".into(), 0.1), ("no".into(), 0.9)]),
            confidence: 0.8,
        };
        assert!(j.validate(&["yes", "no"]).is_err());
        j.value = "no".into();
        assert!(j.validate(&["yes", "no"]).is_ok());
        assert!(j.validate(&["yes", "no", "unknown"]).is_err());
    }
}
