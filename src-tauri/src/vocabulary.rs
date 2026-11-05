// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `vocabulary!` macro: a closed set of names with one stable string form
//! each, for values the backend decides and the webview must know.
//!
//! The string is part of the variant, so a variant cannot exist without one;
//! `ALL` lists every variant for the snapshots the webview's types are
//! generated from (`ai_chat::vocab::snapshot_json` for chat,
//! `notifications::vocabulary::snapshot_json` for notifications; both are
//! checked by [`assert_snapshot_current`]) and for round-trip tests.
//! Each vocabulary serialises as its string and refuses any other string when
//! deserialised; a boundary that must tolerate unknown text calls `parse`.

macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str> as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::parse(&text).ok_or_else(|| {
                    ::serde::de::Error::unknown_variant(&text, &[$($text),+])
                })
            }
        }
    };
}

pub(crate) use vocabulary;

/// Fails unless the checked-in snapshot at `relative` (from the crate root)
/// equals `expected`, naming the lines that differ and the `command` that
/// rewrites it. With `update_var=1` (set by that command) the snapshot is
/// rewritten instead.
#[cfg(test)]
pub(crate) fn assert_snapshot_current(
    relative: &str,
    expected: &str,
    update_var: &str,
    command: &str,
) {
    use std::collections::HashSet;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if std::env::var_os(update_var).is_some_and(|v| v == "1") {
        std::fs::write(&path, expected)
            .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    if actual == expected {
        return;
    }
    let expected_lines: HashSet<&str> = expected.lines().collect();
    let actual_lines: HashSet<&str> = actual.lines().collect();
    let missing: Vec<&str> = expected
        .lines()
        .filter(|l| !actual_lines.contains(l))
        .collect();
    let extra: Vec<&str> = actual
        .lines()
        .filter(|l| !expected_lines.contains(l))
        .collect();
    panic!(
        "{} is stale; run `{command}` and commit the result.\n  missing from the snapshot: {missing:?}\n  no longer in the vocabulary: {extra:?}",
        path.display()
    );
}

/// Every variant of a vocabulary parses back from its string, and the
/// strings are distinct snake_case names.
#[cfg(test)]
pub(crate) fn assert_round_trips<T: Copy + PartialEq + std::fmt::Debug>(
    all: &[T],
    as_str: fn(T) -> &'static str,
    parse: fn(&str) -> Option<T>,
) {
    let mut seen = std::collections::HashSet::new();
    for &item in all {
        let text = as_str(item);
        assert_eq!(parse(text), Some(item), "{text}");
        assert!(seen.insert(text), "{text} is listed twice");
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit()),
            "{text} is not a stable snake_case name"
        );
    }
}
