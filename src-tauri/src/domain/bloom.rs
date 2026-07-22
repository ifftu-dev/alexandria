//! Bloom's taxonomy cognitive levels.
//!
//! The six levels were already a vocabulary in this codebase — a `&[&str]`
//! ordering in governance used to gate proposal and election eligibility, and
//! a free-text `skills.bloom_level` column that nothing read. This makes them
//! one ordered type, so "does this learner operate at `analyze` or above"
//! means the same thing to governance, to assessment blueprints, and to a
//! capability query.
//!
//! `Ord` derives from declaration order, weakest to strongest — the same
//! pattern [`crate::domain::vc::ProvenanceTier`] uses. Comparisons are the
//! point: `level >= BloomLevel::Analyze` is the operation nearly every caller
//! wants.
//!
//! # Difficulty is a different axis
//!
//! A Bloom level is *what kind of thinking* an item demands; difficulty is
//! *how hard* it is. An easy "create" item and a punishing "remember" item
//! both exist, so the two are stored and stratified separately rather than
//! collapsed into one number.
//!
//! # Parsing is lenient on purpose
//!
//! Taxonomy documents arrive over gossip from peers who may run a different
//! version. An unrecognised level deserializes to the [`BloomLevel::Apply`]
//! default rather than failing the whole document — one unknown string must
//! not cost a peer their entire taxonomy update. Storage stays clean because
//! everything is normalised through this type on the way in.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

/// A Bloom's taxonomy cognitive level, ordered weakest to strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BloomLevel {
    Remember,
    Understand,
    Apply,
    Analyze,
    Evaluate,
    Create,
}

/// The historical column default. Unknown input normalises to this rather
/// than erroring.
impl Default for BloomLevel {
    fn default() -> Self {
        BloomLevel::Apply
    }
}

impl BloomLevel {
    /// Every level, weakest first. Iteration order is the ordering.
    pub const ALL: [BloomLevel; 6] = [
        BloomLevel::Remember,
        BloomLevel::Understand,
        BloomLevel::Apply,
        BloomLevel::Analyze,
        BloomLevel::Evaluate,
        BloomLevel::Create,
    ];

    /// snake_case token used in the database and on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            BloomLevel::Remember => "remember",
            BloomLevel::Understand => "understand",
            BloomLevel::Apply => "apply",
            BloomLevel::Analyze => "analyze",
            BloomLevel::Evaluate => "evaluate",
            BloomLevel::Create => "create",
        }
    }

    /// Parse, falling back to the default on anything unrecognised. Use this
    /// at trust boundaries — gossip, the database, user input — and
    /// [`FromStr`] where an unknown value is genuinely an error worth
    /// reporting.
    pub fn parse_lenient(s: &str) -> Self {
        Self::from_str(s).unwrap_or_default()
    }

    /// Rank in the ordering, 0 (`remember`) through 5 (`create`).
    pub fn rank(&self) -> u8 {
        Self::ALL.iter().position(|l| l == self).unwrap_or(2) as u8
    }
}

impl fmt::Display for BloomLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BloomLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case- and whitespace-insensitive: taxonomy documents are authored
        // by hand and "Analyze" should not be a different level from
        // "analyze".
        match s.trim().to_ascii_lowercase().as_str() {
            "remember" => Ok(BloomLevel::Remember),
            "understand" => Ok(BloomLevel::Understand),
            "apply" => Ok(BloomLevel::Apply),
            "analyze" => Ok(BloomLevel::Analyze),
            "evaluate" => Ok(BloomLevel::Evaluate),
            "create" => Ok(BloomLevel::Create),
            other => Err(format!("unknown Bloom level '{other}'")),
        }
    }
}

/// Lenient deserialization — see the module docs. A peer sending a level this
/// build does not know must not invalidate their whole taxonomy document.
impl<'de> Deserialize<'de> for BloomLevel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(BloomLevel::parse_lenient(&raw))
    }
}

/// Stored as its snake_case token, so the column stays human-readable and
/// existing rows keep their meaning. Because every write goes through this,
/// the database cannot come to hold a level the code does not understand —
/// which is why no CHECK constraint is needed on the columns.
impl rusqlite::types::ToSql for BloomLevel {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(self.as_str()))
    }
}

/// Reads are lenient for the same reason parsing is: a row written by an
/// older build, or by hand, must not break a query.
impl rusqlite::types::FromSql for BloomLevel {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_str().map(BloomLevel::parse_lenient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_runs_weakest_to_strongest() {
        // The whole reason this is an enum and not a string.
        assert!(BloomLevel::Remember < BloomLevel::Understand);
        assert!(BloomLevel::Understand < BloomLevel::Apply);
        assert!(BloomLevel::Apply < BloomLevel::Analyze);
        assert!(BloomLevel::Analyze < BloomLevel::Evaluate);
        assert!(BloomLevel::Evaluate < BloomLevel::Create);
        assert!(BloomLevel::Create > BloomLevel::Remember);
    }

    #[test]
    fn all_is_sorted_and_complete() {
        let mut sorted = BloomLevel::ALL;
        sorted.sort();
        assert_eq!(sorted, BloomLevel::ALL, "ALL must already be in rank order");
        assert_eq!(BloomLevel::ALL.len(), 6);
    }

    #[test]
    fn rank_matches_position() {
        for (i, level) in BloomLevel::ALL.iter().enumerate() {
            assert_eq!(level.rank(), i as u8);
        }
    }

    #[test]
    fn round_trips_through_its_token() {
        for level in BloomLevel::ALL {
            assert_eq!(BloomLevel::from_str(level.as_str()).unwrap(), level);
            assert_eq!(level.to_string(), level.as_str());
        }
    }

    #[test]
    fn parsing_tolerates_case_and_whitespace() {
        assert_eq!(
            BloomLevel::from_str("Analyze").unwrap(),
            BloomLevel::Analyze
        );
        assert_eq!(
            BloomLevel::from_str("  CREATE ").unwrap(),
            BloomLevel::Create
        );
    }

    #[test]
    fn strict_parsing_rejects_unknown_levels() {
        assert!(BloomLevel::from_str("synthesize").is_err());
        assert!(BloomLevel::from_str("").is_err());
    }

    #[test]
    fn lenient_parsing_falls_back_to_the_column_default() {
        // Historical rows and older peers default to `apply`; matching that
        // keeps this change invisible to existing data.
        assert_eq!(BloomLevel::parse_lenient("synthesize"), BloomLevel::Apply);
        assert_eq!(BloomLevel::parse_lenient(""), BloomLevel::Apply);
        assert_eq!(BloomLevel::default(), BloomLevel::Apply);
    }

    #[test]
    fn serializes_as_the_wire_token() {
        // The stored and gossiped representation must not change, or every
        // existing taxonomy document would be re-encoded.
        let json = serde_json::to_string(&BloomLevel::Analyze).unwrap();
        assert_eq!(json, "\"analyze\"");
    }

    #[test]
    fn deserializing_an_unknown_level_does_not_fail_the_document() {
        // A peer on a newer build sends a level this one has never heard of.
        // Losing that field is acceptable; losing the taxonomy update is not.
        #[derive(Deserialize)]
        struct Doc {
            bloom_level: BloomLevel,
            name: String,
        }
        let doc: Doc =
            serde_json::from_str(r#"{"bloom_level":"transcend","name":"Some skill"}"#).unwrap();
        assert_eq!(doc.bloom_level, BloomLevel::Apply);
        assert_eq!(doc.name, "Some skill");
    }

    #[test]
    fn deserializes_every_known_level() {
        for level in BloomLevel::ALL {
            let json = format!("\"{}\"", level.as_str());
            let parsed: BloomLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, level);
        }
    }

    #[test]
    fn frontend_ordering_matches_this_one() {
        // `src/utils/bloom.ts` carries its own copy of the ordering, used for
        // badge colours and graph node sizing. Drift would mean the UI ranks
        // a learner differently from the gate that admits them — silently,
        // and only for levels near the boundary. Cheaper to pin it here than
        // to discover it in a governance dispute.
        let ts = include_str!("../../../src/utils/bloom.ts");
        let start = ts
            .find("BLOOM_ORDER = [")
            .expect("src/utils/bloom.ts should declare BLOOM_ORDER");
        let end = ts[start..]
            .find(']')
            .map(|i| start + i)
            .expect("BLOOM_ORDER should be a closed array");

        let frontend: Vec<String> = ts[start..end]
            .split('\'')
            .filter(|s| BloomLevel::from_str(s).is_ok())
            .map(|s| s.to_string())
            .collect();

        let backend: Vec<String> = BloomLevel::ALL
            .iter()
            .map(|l| l.as_str().to_string())
            .collect();

        assert_eq!(
            frontend, backend,
            "src/utils/bloom.ts BLOOM_ORDER has drifted from BloomLevel::ALL"
        );
    }

    #[test]
    fn round_trips_through_sqlite() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, level TEXT NOT NULL)")
            .unwrap();

        for (i, level) in BloomLevel::ALL.iter().enumerate() {
            conn.execute(
                "INSERT INTO t (id, level) VALUES (?1, ?2)",
                rusqlite::params![i as i64, level],
            )
            .unwrap();
        }

        for (i, expected) in BloomLevel::ALL.iter().enumerate() {
            let got: BloomLevel = conn
                .query_row("SELECT level FROM t WHERE id = ?1", [i as i64], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(&got, expected);
        }
    }

    #[test]
    fn stores_the_same_token_the_column_already_held() {
        // Existing rows are lowercase tokens. If storage changed shape, every
        // historical `skills.bloom_level` row would stop matching.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (level TEXT NOT NULL)")
            .unwrap();
        conn.execute(
            "INSERT INTO t (level) VALUES (?1)",
            rusqlite::params![BloomLevel::Analyze],
        )
        .unwrap();

        let raw: String = conn
            .query_row("SELECT level FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, "analyze");
    }

    #[test]
    fn reading_a_legacy_or_hand_written_row_does_not_fail() {
        // Rows predating this type, or written by hand, must still read.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER, level TEXT);
             INSERT INTO t VALUES (1, 'Analyze'), (2, 'synthesize'), (3, '');",
        )
        .unwrap();

        let read = |id: i64| -> BloomLevel {
            conn.query_row("SELECT level FROM t WHERE id = ?1", [id], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(read(1), BloomLevel::Analyze, "case is normalised");
        assert_eq!(read(2), BloomLevel::Apply, "unknown falls back");
        assert_eq!(read(3), BloomLevel::Apply, "empty falls back");
    }
}
