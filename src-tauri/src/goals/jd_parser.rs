//! On-device job-description → candidate-skill matcher.
//!
//! Pure and deterministic: it takes the free text and a snapshot of the
//! taxonomy's `(skill_id, name, synonyms)` and returns ranked candidate
//! skills whose name or a synonym occurs (as a whole word/phrase) in the
//! text. Deliberately conservative — the caller shows these as *suggestions*
//! the user confirms, never auto-commits them. Keeping it pure makes it
//! trivially testable and swappable for a stronger model later.
//!
//! Also reused by the resume/transcript bootstrap (Phase C), which feeds it
//! document text instead of a JD.

/// One taxonomy skill's matchable surface: its display name plus any
/// synonyms/aliases. Built from the `skills` table (`name` + `synonyms`).
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub id: String,
    pub name: String,
    /// Lowercased alias tokens (from `skills.synonyms`, comma-separated).
    pub synonyms: Vec<String>,
}

/// A suggested skill extracted from free text, with a confidence in
/// `(0, 1]` and the phrase that matched (for a "matched: …" UI hint).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub skill_id: String,
    pub score: f64,
    pub matched: String,
}

/// Normalize free text to a space-padded, lowercased token stream so phrase
/// lookups can require whole-word boundaries (` needle `). Every run of
/// non-alphanumeric characters collapses to a single space.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    let mut prev_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// Confidence for a matched phrase: multi-word / longer phrases are far more
/// specific than a single short token (matching "product" is weak; matching
/// "product management" is strong), so they score higher.
fn phrase_confidence(normalized_phrase: &str) -> f64 {
    let words = normalized_phrase.split_whitespace().count();
    let chars = normalized_phrase.trim().len();
    // Single very short tokens (<= 2 chars, e.g. a stray "ai") are too noisy.
    if words == 1 && chars <= 2 {
        return 0.0;
    }
    let base = 0.45 + 0.18 * (words.saturating_sub(1) as f64);
    base.min(1.0)
}

/// Extract ranked candidate skills from `text`. A skill is a candidate if its
/// name or any synonym appears as a whole phrase in the text; its score is the
/// best matching phrase's confidence. Results are sorted strongest-first, then
/// by skill id for determinism.
pub fn extract_skills(text: &str, skills: &[SkillEntry]) -> Vec<Candidate> {
    let hay = normalize(text);
    let mut out: Vec<Candidate> = Vec::new();

    for skill in skills {
        let mut best = 0.0_f64;
        let mut matched = String::new();
        // The display name plus every synonym are all candidate phrases.
        let phrases =
            std::iter::once(skill.name.as_str()).chain(skill.synonyms.iter().map(|s| s.as_str()));
        for phrase in phrases {
            let needle = normalize(phrase); // already space-padded + lowercased
            if needle.trim().is_empty() {
                continue;
            }
            if hay.contains(&needle) {
                let conf = phrase_confidence(&needle);
                if conf > best {
                    best = conf;
                    matched = phrase.to_string();
                }
            }
        }
        if best > 0.0 {
            out.push(Candidate {
                skill_id: skill.id.clone(),
                score: best,
                matched,
            });
        }
    }

    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.skill_id.cmp(&b.skill_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, syns: &[&str]) -> SkillEntry {
        SkillEntry {
            id: id.into(),
            name: name.into(),
            synonyms: syns.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn skills() -> Vec<SkillEntry> {
        vec![
            entry("skill_js", "JavaScript", &["js", "ecmascript", "node.js"]),
            entry("skill_pm", "Product Management", &["product manager"]),
            entry("skill_k8s", "Kubernetes", &["k8s"]),
            entry("skill_sql", "SQL", &["postgres", "mysql"]),
            entry(
                "skill_leadership",
                "Leadership",
                &["team lead", "people management"],
            ),
        ]
    }

    #[test]
    fn matches_name_and_synonyms_as_whole_words() {
        let jd = "We need a senior engineer strong in JavaScript and Kubernetes (k8s).";
        let got: Vec<_> = extract_skills(jd, &skills())
            .into_iter()
            .map(|c| c.skill_id)
            .collect();
        assert!(got.contains(&"skill_js".to_string()));
        assert!(got.contains(&"skill_k8s".to_string()));
    }

    #[test]
    fn synonym_with_punctuation_matches() {
        // "node.js" normalizes to "node js" and must match in prose.
        let jd = "Backend built on Node.js and Postgres.";
        let ids: Vec<_> = extract_skills(jd, &skills())
            .into_iter()
            .map(|c| c.skill_id)
            .collect();
        assert!(ids.contains(&"skill_js".to_string()));
        assert!(ids.contains(&"skill_sql".to_string()));
    }

    #[test]
    fn multiword_phrases_outrank_single_tokens() {
        let jd = "The Product Management team owns SQL dashboards.";
        let got = extract_skills(jd, &skills());
        let pm = got.iter().find(|c| c.skill_id == "skill_pm").unwrap();
        let sql = got.iter().find(|c| c.skill_id == "skill_sql").unwrap();
        assert!(
            pm.score > sql.score,
            "multi-word 'Product Management' ({}) should outrank single 'SQL' ({})",
            pm.score,
            sql.score
        );
        // Sorted strongest-first.
        assert_eq!(got.first().unwrap().skill_id, "skill_pm");
    }

    #[test]
    fn does_not_match_substrings_across_word_boundaries() {
        // "javascripting" must NOT match "javascript"; "ks" must not match "k8s".
        let jd = "No relevant skills here: javascripting frameworks, thanks.";
        let got = extract_skills(jd, &skills());
        assert!(
            got.iter().all(|c| c.skill_id != "skill_js"),
            "must not match 'javascript' inside 'javascripting'"
        );
    }

    #[test]
    fn empty_or_no_match_returns_empty() {
        assert!(extract_skills("", &skills()).is_empty());
        assert!(extract_skills("cooking and gardening", &skills()).is_empty());
    }

    #[test]
    fn ignores_ultra_short_noise_synonyms() {
        // A 2-char synonym like "js" alone is allowed (explicit alias), but a
        // bare 2-char token isn't auto-promoted: confidence guard drops <=2ch
        // single tokens. "js" is exactly 2 chars → dropped as too noisy.
        let jd = "the js library";
        let got = extract_skills(jd, &skills());
        // "JavaScript" name didn't appear; only the 2-char "js" did → filtered.
        assert!(got.iter().all(|c| c.skill_id != "skill_js"));
    }
}
