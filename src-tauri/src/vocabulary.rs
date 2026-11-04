// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `vocabulary!` macro: a closed set of names with one stable string form
//! each, for values the backend decides and the webview must know.
//!
//! The string is part of the variant, so a variant cannot exist without one;
//! `ALL` lists every variant for the snapshot the webview's types are
//! generated from (`ai_chat::vocab::snapshot_json`) and for round-trip tests.
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
