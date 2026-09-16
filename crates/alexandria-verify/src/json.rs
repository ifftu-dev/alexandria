//! Bounded parsing for untrusted JSON.
//!
//! Every object Alexandria accepts from a peer, a file, or a service passes
//! through here before any typed decoding, signature work, or storage. The
//! parser enforces explicit limits first and rejects inputs that different
//! JSON implementations could read differently:
//!
//! - documents larger than the byte limit, before any parsing work;
//! - nesting deeper than the depth limit, before serde_json's own recursion
//!   limit could turn hostile input into an opaque error;
//! - arrays, objects, or strings beyond their limits;
//! - duplicate object keys at any depth, which parsers resolve inconsistently;
//! - numbers outside the range JavaScript represents exactly, and non-finite
//!   values, so Rust and JavaScript verifiers read the same number.
//!
//! Trailing bytes after the document are rejected.

use std::cell::RefCell;
use std::fmt;

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::governance::MAX_JCS_SAFE_INTEGER;

/// serde_json refuses to nest deeper than this; the configured depth must stay
/// below it so the explicit limit, not an incidental one, decides.
const SERDE_JSON_RECURSION_LIMIT: usize = 128;

/// Limits for one kind of untrusted document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonLimits {
    /// Maximum encoded document size in bytes.
    pub max_bytes: usize,
    /// Maximum nesting of arrays and objects. The top-level container is 1.
    pub max_depth: usize,
    /// Maximum elements in any one array.
    pub max_array_len: usize,
    /// Maximum entries in any one object.
    pub max_object_entries: usize,
    /// Maximum UTF-8 bytes in any one string or object key.
    pub max_string_bytes: usize,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UntrustedJsonError {
    #[error("JSON document exceeds {max} bytes")]
    TooLarge { max: usize },
    #[error("JSON nesting exceeds depth {max}")]
    TooDeep { max: usize },
    #[error("JSON array exceeds {max} elements")]
    TooManyElements { max: usize },
    #[error("JSON object exceeds {max} entries")]
    TooManyEntries { max: usize },
    #[error("JSON string exceeds {max} bytes")]
    StringTooLong { max: usize },
    #[error("duplicate JSON object key: {0}")]
    DuplicateKey(String),
    #[error("JSON number is outside the exactly representable range: {0}")]
    UnsafeNumber(String),
    #[error("JSON limits are invalid: {0}")]
    InvalidLimits(&'static str),
    #[error("invalid JSON: {0}")]
    Invalid(String),
}

impl JsonLimits {
    fn validate(&self) -> Result<(), UntrustedJsonError> {
        if self.max_depth == 0 || self.max_depth >= SERDE_JSON_RECURSION_LIMIT {
            return Err(UntrustedJsonError::InvalidLimits(
                "max_depth must be between 1 and 127",
            ));
        }
        if self.max_bytes == 0 {
            return Err(UntrustedJsonError::InvalidLimits(
                "max_bytes must be positive",
            ));
        }
        Ok(())
    }
}

/// Parse untrusted bytes into a JSON value under `limits`.
pub fn parse_untrusted(bytes: &[u8], limits: &JsonLimits) -> Result<Value, UntrustedJsonError> {
    limits.validate()?;
    if bytes.len() > limits.max_bytes {
        return Err(UntrustedJsonError::TooLarge {
            max: limits.max_bytes,
        });
    }
    let context = Context {
        limits,
        error: RefCell::new(None),
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = ValueSeed {
        context: &context,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| deserializer.end().map(|()| value));
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => Err(context
            .error
            .into_inner()
            .unwrap_or_else(|| UntrustedJsonError::Invalid(error.to_string()))),
    }
}

/// Parse untrusted bytes under `limits`, then decode them as `T`.
pub fn decode_untrusted<T: DeserializeOwned>(
    bytes: &[u8],
    limits: &JsonLimits,
) -> Result<T, UntrustedJsonError> {
    let value = parse_untrusted(bytes, limits)?;
    serde_json::from_value(value).map_err(|error| UntrustedJsonError::Invalid(error.to_string()))
}

struct Context<'l> {
    limits: &'l JsonLimits,
    /// The first limit violation, kept typed because serde errors are strings.
    error: RefCell<Option<UntrustedJsonError>>,
}

impl Context<'_> {
    fn fail<E: de::Error>(&self, error: UntrustedJsonError) -> E {
        let message = error.to_string();
        self.error.borrow_mut().get_or_insert(error);
        E::custom(message)
    }

    fn check_string<E: de::Error>(&self, value: &str) -> Result<(), E> {
        if value.len() > self.limits.max_string_bytes {
            return Err(self.fail(UntrustedJsonError::StringTooLong {
                max: self.limits.max_string_bytes,
            }));
        }
        Ok(())
    }

    fn enter<E: de::Error>(&self, depth: usize) -> Result<usize, E> {
        let nesting = depth + 1;
        if nesting > self.limits.max_depth {
            return Err(self.fail(UntrustedJsonError::TooDeep {
                max: self.limits.max_depth,
            }));
        }
        Ok(nesting)
    }
}

#[derive(Clone, Copy)]
struct ValueSeed<'c, 'l> {
    context: &'c Context<'l>,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        if value.unsigned_abs() > MAX_JCS_SAFE_INTEGER {
            return Err(self
                .context
                .fail(UntrustedJsonError::UnsafeNumber(value.to_string())));
        }
        Ok(Value::from(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        if value > MAX_JCS_SAFE_INTEGER {
            return Err(self
                .context
                .fail(UntrustedJsonError::UnsafeNumber(value.to_string())));
        }
        Ok(Value::from(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        // Without arbitrary precision, an integer literal beyond u64 arrives
        // here already rounded, so magnitude is bounded for floats as well.
        if !value.is_finite() || value.abs() > MAX_JCS_SAFE_INTEGER as f64 {
            return Err(self
                .context
                .fail(UntrustedJsonError::UnsafeNumber(value.to_string())));
        }
        Number::from_f64(value).map(Value::Number).ok_or_else(|| {
            self.context
                .fail(UntrustedJsonError::UnsafeNumber(value.to_string()))
        })
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        self.context.check_string(value)?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        self.context.check_string(&value)?;
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let nesting = self.context.enter(self.depth)?;
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            context: self.context,
            depth: nesting,
        })? {
            if values.len() == self.context.limits.max_array_len {
                return Err(self.context.fail(UntrustedJsonError::TooManyElements {
                    max: self.context.limits.max_array_len,
                }));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let nesting = self.context.enter(self.depth)?;
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            self.context.check_string(&key)?;
            if object.contains_key(&key) {
                return Err(self.context.fail(UntrustedJsonError::DuplicateKey(key)));
            }
            if object.len() == self.context.limits.max_object_entries {
                return Err(self.context.fail(UntrustedJsonError::TooManyEntries {
                    max: self.context.limits.max_object_entries,
                }));
            }
            let value = map.next_value_seed(ValueSeed {
                context: self.context,
                depth: nesting,
            })?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    const LIMITS: JsonLimits = JsonLimits {
        max_bytes: 1024,
        max_depth: 4,
        max_array_len: 3,
        max_object_entries: 3,
        max_string_bytes: 8,
    };

    fn parse(text: &str) -> Result<Value, UntrustedJsonError> {
        parse_untrusted(text.as_bytes(), &LIMITS)
    }

    #[test]
    fn accepts_documents_within_every_limit() {
        let value = parse(r#"{"a":[1,-2,0.5],"b":{"c":[true,null]},"d":"12345678"}"#).unwrap();
        assert_eq!(value["a"][2], serde_json::json!(0.5));
        assert_eq!(value["d"], "12345678");
    }

    #[test]
    fn byte_limit_is_checked_before_parsing() {
        let limits = JsonLimits {
            max_bytes: 7,
            ..LIMITS
        };
        assert!(parse_untrusted(br#""12345""#, &limits).is_ok());
        assert_eq!(
            parse_untrusted(br#""123456""#, &limits),
            Err(UntrustedJsonError::TooLarge { max: 7 })
        );
        // Oversized bytes are refused even when they are not JSON at all.
        assert_eq!(
            parse_untrusted(&[0xff; 8], &limits),
            Err(UntrustedJsonError::TooLarge { max: 7 })
        );
    }

    #[test]
    fn nesting_is_bounded_at_the_exact_depth() {
        assert!(parse("[[[[1]]]]").is_ok());
        assert_eq!(
            parse("[[[[[1]]]]]"),
            Err(UntrustedJsonError::TooDeep { max: 4 })
        );
        assert!(parse(r#"{"a":{"b":{"c":{"d":1}}}}"#).is_ok());
        assert_eq!(
            parse(r#"{"a":{"b":{"c":{"d":{"e":1}}}}}"#),
            Err(UntrustedJsonError::TooDeep { max: 4 })
        );
    }

    #[test]
    fn hostile_nesting_fails_with_the_explicit_limit() {
        let limits = JsonLimits {
            max_bytes: 1 << 20,
            max_depth: 32,
            ..LIMITS
        };
        let hostile = "[".repeat(100_000);
        assert_eq!(
            parse_untrusted(hostile.as_bytes(), &limits),
            Err(UntrustedJsonError::TooDeep { max: 32 })
        );
    }

    #[test]
    fn array_object_and_string_sizes_are_bounded_exactly() {
        assert!(parse("[1,2,3]").is_ok());
        assert_eq!(
            parse("[1,2,3,4]"),
            Err(UntrustedJsonError::TooManyElements { max: 3 })
        );
        assert!(parse(r#"{"a":1,"b":2,"c":3}"#).is_ok());
        assert_eq!(
            parse(r#"{"a":1,"b":2,"c":3,"d":4}"#),
            Err(UntrustedJsonError::TooManyEntries { max: 3 })
        );
        assert!(parse(r#""12345678""#).is_ok());
        assert_eq!(
            parse(r#""123456789""#),
            Err(UntrustedJsonError::StringTooLong { max: 8 })
        );
        assert_eq!(
            parse(r#"{"123456789":1}"#),
            Err(UntrustedJsonError::StringTooLong { max: 8 })
        );
    }

    #[test]
    fn duplicate_keys_are_rejected_at_any_depth() {
        assert_eq!(
            parse(r#"{"a":1,"a":2}"#),
            Err(UntrustedJsonError::DuplicateKey("a".into()))
        );
        assert_eq!(
            parse(r#"{"outer":{"level":2,"level":5}}"#),
            Err(UntrustedJsonError::DuplicateKey("level".into()))
        );
    }

    #[test]
    fn numbers_must_be_exactly_representable_in_javascript() {
        let limits = JsonLimits {
            max_string_bytes: 64,
            ..LIMITS
        };
        let parse = |text: &str| parse_untrusted(text.as_bytes(), &limits);
        assert!(parse("9007199254740991").is_ok());
        assert!(parse("-9007199254740991").is_ok());
        assert!(matches!(
            parse("9007199254740992"),
            Err(UntrustedJsonError::UnsafeNumber(_))
        ));
        assert!(matches!(
            parse("-9007199254740992"),
            Err(UntrustedJsonError::UnsafeNumber(_))
        ));
        assert!(matches!(
            parse("18446744073709551616"),
            Err(UntrustedJsonError::UnsafeNumber(_))
        ));
        assert!(matches!(
            parse("1e300"),
            Err(UntrustedJsonError::UnsafeNumber(_))
        ));
        assert!(parse("0.25").is_ok());
    }

    #[test]
    fn trailing_bytes_and_malformed_input_are_invalid() {
        assert!(matches!(
            parse(r#"{"a":1} {"b":2}"#),
            Err(UntrustedJsonError::Invalid(_))
        ));
        assert!(matches!(
            parse(r#"{"a":"#),
            Err(UntrustedJsonError::Invalid(_))
        ));
        assert_eq!(
            parse_untrusted(
                b"1",
                &JsonLimits {
                    max_depth: 128,
                    ..LIMITS
                }
            ),
            Err(UntrustedJsonError::InvalidLimits(
                "max_depth must be between 1 and 127"
            ))
        );
    }

    #[test]
    fn typed_decoding_happens_only_after_limits_pass() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Pair {
            left: u8,
            right: String,
        }
        assert_eq!(
            decode_untrusted::<Pair>(br#"{"left":1,"right":"x"}"#, &LIMITS).unwrap(),
            Pair {
                left: 1,
                right: "x".into()
            }
        );
        assert_eq!(
            decode_untrusted::<Pair>(br#"{"left":1,"left":2,"right":"x"}"#, &LIMITS),
            Err(UntrustedJsonError::DuplicateKey("left".into()))
        );
        assert!(matches!(
            decode_untrusted::<Pair>(br#"{"left":300,"right":"x"}"#, &LIMITS),
            Err(UntrustedJsonError::Invalid(_))
        ));
    }
}
