//! Vocabularies the backend decides and the webview must know, each with the
//! stable string form that crosses the Tauri bridge.
//!
//! The webview never retypes these. `scripts/chat-vocab.json` is a snapshot
//! of this module, checked by the `chat_vocab_snapshot_is_current` test
//! below and rewritten by `npm run generate:chat-vocab`, which then has
//! `scripts/check-tauri-commands.mjs --write` render it into
//! `src/generated/chat-vocab.ts`. Both guards run in CI, so adding a variant
//! here without regenerating fails the build rather than drifting.

use alexandria_ai::{Location, Protocol};
use serde::Serialize;

/// A closed set of names with one stable string form each. The string is
/// part of the variant, so a variant cannot exist without one; `ALL` lists
/// every variant for the snapshot and the round-trip test.
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
    };
}

vocabulary! {
    /// The code before the first colon of a chat or sign-in error, stored
    /// with a failed answer and translated by the webview. Codes never carry
    /// provider text; `rate_limited` and `http` carry a numeric detail after
    /// a second colon.
    ChatErrorCode {
        // A provider request failed (`ChatErrorCode::of_provider`).
        Invalid => "invalid",
        Unreachable => "unreachable",
        Timeout => "timeout",
        Interrupted => "interrupted",
        Auth => "auth",
        /// `rate_limited:<seconds>` when the provider said how long to wait.
        RateLimited => "rate_limited",
        ModelUnavailable => "model_unavailable",
        /// `http:<status>` for any other non-success status.
        Http => "http",
        Malformed => "malformed",
        TooLarge => "too_large",
        Empty => "empty",
        Provider => "provider",
        Unsupported => "unsupported",
        Cancelled => "cancelled",
        UsageLimit => "usage_limit",
        PlanIneligible => "plan_ineligible",
        // A chat rule refused the request.
        AssessmentActive => "assessment_active",
        AssessmentItem => "assessment_item",
        NoConnection => "no_connection",
        GuardianRequired => "guardian_required",
        GuardianPending => "guardian_pending",
        GuardianDeclined => "guardian_declined",
        NoGuardian => "no_guardian",
        Under13 => "under_13",
        AdultsOnly => "adults_only",
        ContextStale => "context_stale",
        TooManyRuns => "too_many_runs",
        SourceUnavailable => "source_unavailable",
        NotEnrolled => "not_enrolled",
        NoLesson => "no_lesson",
        NeedsPlugins => "needs_plugins",
        OpenCourse => "open_course",
        Conflict => "conflict",
        NotFound => "not_found",
        InvalidInput => "invalid_input",
        StorageError => "storage_error",
        SigninRequired => "signin_required",
        // A provider account sign-in did not finish (`ChatErrorCode::of_sign_in`).
        SigninCancelled => "signin_cancelled",
        SigninTimeout => "signin_timeout",
        SigninDeclined => "signin_declined",
        PlanNotGranted => "plan_not_granted",
        DifferentAccount => "different_account",
        CodeRejected => "code_rejected",
        SigninExpired => "signin_expired",
        SigninFailed => "signin_failed",
        SigninBusy => "signin_busy",
    }
}

impl ChatErrorCode {
    /// The code for a provider failure, with the numeric detail some codes
    /// carry: seconds to wait, or the HTTP status.
    pub fn of_provider(error: &alexandria_ai::Error) -> (Self, Option<u64>) {
        use alexandria_ai::Error as E;
        match error {
            E::Invalid(_) => (Self::Invalid, None),
            E::Unreachable => (Self::Unreachable, None),
            E::Timeout => (Self::Timeout, None),
            E::Interrupted => (Self::Interrupted, None),
            E::Http {
                status: 401 | 403, ..
            } => (Self::Auth, None),
            E::Http {
                status: 429,
                retry_after,
            } => (Self::RateLimited, *retry_after),
            E::Http { status: 404, .. } => (Self::ModelUnavailable, None),
            E::Http { status, .. } => (Self::Http, Some(u64::from(*status))),
            E::Malformed => (Self::Malformed, None),
            E::TooLarge => (Self::TooLarge, None),
            E::Empty => (Self::Empty, None),
            E::Provider => (Self::Provider, None),
            E::Unsupported(_) => (Self::Unsupported, None),
            E::Cancelled => (Self::Cancelled, None),
            E::UsageLimit => (Self::UsageLimit, None),
            E::NotEligible => (Self::PlanIneligible, None),
        }
    }

    /// The code for a saved connection that Studio's gate refuses before
    /// anything is sent: a disabled one, or one whose settings are not valid.
    pub fn of_connection(error: &alexandria_studio::Error) -> Self {
        match error {
            alexandria_studio::Error::Unavailable(_) => Self::NoConnection,
            _ => Self::Invalid,
        }
    }

    /// The code a sign-in failure's display string starts with.
    pub fn of_sign_in(error: &crate::ai_auth::SignInError) -> Self {
        use crate::ai_auth::SignInError as S;
        match error {
            S::Cancelled => Self::SigninCancelled,
            S::Timeout => Self::SigninTimeout,
            S::Declined => Self::SigninDeclined,
            S::PlanNotGranted => Self::PlanNotGranted,
            S::DifferentAccount => Self::DifferentAccount,
            S::Unreachable => Self::Unreachable,
            S::CodeRejected => Self::CodeRejected,
            S::Expired => Self::SigninExpired,
            S::Failed(_) => Self::SigninFailed,
        }
    }
}

/// A refusal as commands return it: the code, a colon, and a reason written
/// for people. The webview reads the code and translates it; the reason is
/// shown only where the message says it is.
pub fn refuse(code: ChatErrorCode, reason: impl std::fmt::Display) -> String {
    format!("{code}: {reason}")
}

/// A refusal from the AI access policy (`crate::ai_access`) as chat returns
/// it: the typed reason's code from this vocabulary, then its reason. A
/// Studio error the policy passed on reads as it always has.
pub fn refused(refusal: alexandria_studio::access::Refused) -> String {
    use alexandria_studio::access::Refused as R;
    let code = match &refusal {
        R::TooYoungForRemote => ChatErrorCode::Under13,
        R::AdultsOnly(_) => ChatErrorCode::AdultsOnly,
        R::GuardianRequired => ChatErrorCode::GuardianRequired,
        R::GuardianPending => ChatErrorCode::GuardianPending,
        R::GuardianDeclined => ChatErrorCode::GuardianDeclined,
        R::NoGuardian => ChatErrorCode::NoGuardian,
        R::NotRemote | R::LessonWithheld | R::Invalid(_) => ChatErrorCode::InvalidInput,
        R::ConnectionDisabled | R::WrongCapability => ChatErrorCode::NoConnection,
        R::AttemptInProgress => ChatErrorCode::AssessmentActive,
        R::LessonNotFound => ChatErrorCode::NotFound,
        R::Storage(_) => ChatErrorCode::StorageError,
        R::Studio(error) => return error.to_string(),
    };
    refuse(code, refusal.reason())
}

vocabulary! {
    /// What a chat may propose to save or change; the `kind` tag of
    /// `artifact::Body`.
    ProposalKind {
        Note => "note",
        Flashcards => "flashcards",
        Goal => "goal",
        GoalChange => "goal_change",
        CourseNote => "course_note",
        Enroll => "enroll",
    }
}

vocabulary! {
    /// The tag of a fenced block a reply may end with. Each becomes a card;
    /// the webview hides a reply from the first such fence while it streams.
    ReplyFence {
        Practice => "practice",
        Memory => "memory",
        Artifact => "artifact",
    }
}

vocabulary! {
    /// Where a saved connection's endpoint runs, in the Studio connection's
    /// own names. Chat describes the same places as [`Location`].
    ConnectionLocation {
        Local => "local",
        PrivateNetwork => "private_network",
        Cloud => "cloud",
        /// Never saved: the placeholder on a run step an external assistant
        /// proposed through the local API, which chat cannot use.
        External => "external",
    }
}

impl ConnectionLocation {
    /// The place chat names for this connection, if chat can use it. The
    /// mapping lives with [`Location::from_legacy`]; this only names it.
    pub fn chat_location(self) -> Option<Location> {
        Location::from_legacy(self.as_str())
    }
}

/// Every place chat names, in the order the webview lists them.
pub const CHAT_LOCATIONS: [Location; 3] =
    [Location::Device, Location::PrivateNetwork, Location::Remote];

/// Every wire protocol a connection may speak.
pub const PROTOCOLS: [Protocol; 3] = [
    Protocol::Compatible,
    Protocol::Anthropic,
    Protocol::Responses,
];

#[derive(Serialize)]
struct ConnectionLocationEntry {
    name: &'static str,
    chat_location: Option<&'static str>,
}

/// The whole vocabulary as the webview generator reads it.
#[derive(Serialize)]
struct Snapshot {
    _comment: &'static str,
    chat_error_codes: Vec<&'static str>,
    proposal_kinds: Vec<&'static str>,
    reply_fence_tags: Vec<&'static str>,
    connection_locations: Vec<ConnectionLocationEntry>,
    chat_locations: Vec<&'static str>,
    protocols: Vec<&'static str>,
    restricts_apps_for_minors_hosts: Vec<&'static str>,
}

/// The snapshot `scripts/check-tauri-commands.mjs` renders into TypeScript.
pub fn snapshot_json() -> String {
    let snapshot = Snapshot {
        _comment: "Written by `npm run generate:chat-vocab` from src-tauri/src/ai_chat/vocab.rs and read by scripts/check-tauri-commands.mjs --write. Do not edit by hand.",
        chat_error_codes: ChatErrorCode::ALL.iter().map(|c| c.as_str()).collect(),
        proposal_kinds: ProposalKind::ALL.iter().map(|k| k.as_str()).collect(),
        reply_fence_tags: ReplyFence::ALL.iter().map(|f| f.as_str()).collect(),
        connection_locations: ConnectionLocation::ALL
            .iter()
            .map(|l| ConnectionLocationEntry {
                name: l.as_str(),
                chat_location: l.chat_location().map(Location::as_str),
            })
            .collect(),
        chat_locations: CHAT_LOCATIONS.iter().map(|l| l.as_str()).collect(),
        protocols: PROTOCOLS.iter().map(|p| p.as_str()).collect(),
        restricts_apps_for_minors_hosts: alexandria_ai::terms::RESTRICTS_APPS_FOR_MINORS.to_vec(),
    };
    let mut json = serde_json::to_string_pretty(&snapshot).expect("vocabulary serialises");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn every_policy_refusal_reads_as_the_code_the_webview_translates() {
        use super::refused;
        use alexandria_studio::access::Refused;
        use alexandria_studio::Error;
        let cases: [(Refused, &str); 13] = [
            (Refused::TooYoungForRemote, "under_13: learners under 13 can use models on this device or their own network only"),
            (Refused::AdultsOnly("OpenRouter"), "adults_only: OpenRouter requires users to be 18 or older"),
            (Refused::GuardianRequired, "guardian_required: remote models need a guardian's approval; use a model on this device or your own network"),
            (Refused::GuardianPending, "guardian_pending: waiting for a guardian to approve this model"),
            (Refused::GuardianDeclined, "guardian_declined: a guardian did not approve this model"),
            (Refused::NoGuardian, "no_guardian: link a guardian in Settings first"),
            (Refused::NotRemote, "invalid_input: only remote models need a guardian's approval"),
            (Refused::ConnectionDisabled, "no_connection: the chosen model connection is disabled"),
            (Refused::AttemptInProgress, &Error::AssessmentActive.to_string()),
            (Refused::LessonNotFound, &Error::NotFound.to_string()),
            (Refused::LessonWithheld, "invalid_input: only text lessons can be attached"),
            (Refused::Storage("profile"), "storage_error: profile could not be read"),
            (Refused::Studio(Error::Conflict), &Error::Conflict.to_string()),
        ];
        for (refusal, expected) in cases {
            assert_eq!(refused(refusal), expected);
        }
    }

    use super::*;

    fn round_trips<T: Copy + PartialEq + std::fmt::Debug>(
        all: &[T],
        as_str: fn(T) -> &'static str,
        parse: fn(&str) -> Option<T>,
    ) {
        let mut seen = HashSet::new();
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

    #[test]
    fn every_vocabulary_round_trips_through_its_string_form() {
        round_trips(
            ChatErrorCode::ALL,
            ChatErrorCode::as_str,
            ChatErrorCode::parse,
        );
        round_trips(ProposalKind::ALL, ProposalKind::as_str, ProposalKind::parse);
        round_trips(ReplyFence::ALL, ReplyFence::as_str, ReplyFence::parse);
        round_trips(
            ConnectionLocation::ALL,
            ConnectionLocation::as_str,
            ConnectionLocation::parse,
        );
    }

    #[test]
    fn chat_locations_and_protocols_list_every_variant_in_their_serde_form() {
        // A new variant fails these matches until it is listed above.
        for location in CHAT_LOCATIONS {
            match location {
                Location::Device | Location::PrivateNetwork | Location::Remote => {}
            }
            assert_eq!(
                serde_json::to_value(location).unwrap(),
                location.as_str(),
                "{location:?}"
            );
            assert_eq!(Location::from_legacy(location.as_str()), Some(location));
        }
        for protocol in PROTOCOLS {
            match protocol {
                Protocol::Compatible | Protocol::Anthropic | Protocol::Responses => {}
            }
            assert_eq!(
                serde_json::to_value(protocol).unwrap(),
                protocol.as_str(),
                "{protocol:?}"
            );
            assert_eq!(Protocol::parse(protocol.as_str()), Some(protocol));
        }
        assert_eq!(
            CHAT_LOCATIONS
                .iter()
                .map(|l| l.as_str())
                .collect::<HashSet<_>>()
                .len(),
            CHAT_LOCATIONS.len()
        );
        assert_eq!(
            PROTOCOLS
                .iter()
                .map(|p| p.as_str())
                .collect::<HashSet<_>>()
                .len(),
            PROTOCOLS.len()
        );
    }

    #[test]
    fn connection_locations_agree_with_the_studio_crate() {
        for &location in ConnectionLocation::ALL {
            let connection = alexandria_studio::model::StudioConnection {
                id: "c".into(),
                name: "c".into(),
                endpoint: String::new(),
                model: "m".into(),
                location: location.as_str().into(),
                capability: "text".into(),
                enabled: true,
                has_key: false,
                context_tokens: None,
                protocol: Default::default(),
                auth: Default::default(),
            };
            let studio = alexandria_studio::provider::location(&connection).ok();
            assert_eq!(studio, location.chat_location(), "{location}");
            assert_eq!(
                Location::from_legacy(location.as_str()),
                location.chat_location(),
                "{location}"
            );
        }
    }

    #[test]
    fn proposal_kinds_are_the_body_tags() {
        use super::super::artifact::{Body, Card};
        let bodies = [
            Body::Note { text: "t".into() },
            Body::Flashcards {
                cards: vec![Card {
                    front: "f".into(),
                    back: "b".into(),
                }],
            },
            Body::Goal {
                text: "t".into(),
                goal_id: None,
            },
            Body::GoalChange {
                goal_id: "g".into(),
                rename: None,
                add: vec![],
                remove: vec![],
                text: "t".into(),
            },
            Body::CourseNote {
                course_id: "c".into(),
                element_id: "e".into(),
                lesson: "l".into(),
                text: "t".into(),
            },
            Body::Enroll {
                course_id: "c".into(),
                goal_id: "g".into(),
                text: "t".into(),
            },
        ];
        let mut kinds = HashSet::new();
        for body in &bodies {
            let tag = serde_json::to_value(body).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(tag, body.kind().as_str());
            kinds.insert(body.kind());
        }
        assert_eq!(
            kinds.len(),
            ProposalKind::ALL.len(),
            "a kind has no body above"
        );
    }

    #[test]
    fn provider_codes_keep_their_wire_strings() {
        use alexandria_ai::Error as E;
        let cases: Vec<(E, &str)> = vec![
            (E::Invalid("secret".into()), "invalid"),
            (E::Unreachable, "unreachable"),
            (E::Timeout, "timeout"),
            (E::Interrupted, "interrupted"),
            (
                E::Http {
                    status: 401,
                    retry_after: None,
                },
                "auth",
            ),
            (
                E::Http {
                    status: 429,
                    retry_after: Some(7),
                },
                "rate_limited:7",
            ),
            (
                E::Http {
                    status: 429,
                    retry_after: None,
                },
                "rate_limited",
            ),
            (
                E::Http {
                    status: 404,
                    retry_after: None,
                },
                "model_unavailable",
            ),
            (
                E::Http {
                    status: 503,
                    retry_after: None,
                },
                "http:503",
            ),
            (E::Malformed, "malformed"),
            (E::TooLarge, "too_large"),
            (E::Empty, "empty"),
            (E::Provider, "provider"),
            (E::Unsupported("tools"), "unsupported"),
            (E::Cancelled, "cancelled"),
            (E::UsageLimit, "usage_limit"),
            (E::NotEligible, "plan_ineligible"),
        ];
        for (error, expected) in cases {
            assert_eq!(super::super::error_code(&error), expected);
        }
    }

    #[test]
    fn sign_in_failures_start_with_their_code() {
        use crate::ai_auth::SignInError as S;
        for error in [
            S::Cancelled,
            S::Timeout,
            S::Declined,
            S::PlanNotGranted,
            S::DifferentAccount,
            S::Unreachable,
            S::CodeRejected,
            S::Expired,
            S::Failed("busy".into()),
        ] {
            let code = ChatErrorCode::of_sign_in(&error);
            let text = error.to_string();
            assert!(
                text.starts_with(&format!("{code}: ")),
                "{text:?} does not start with {code}"
            );
        }
    }

    fn app_source(relative: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Every `.rs` file under `relative`, as paths relative to the crate.
    fn rust_files(relative: &str) -> Vec<String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "rs"))
            .map(|entry| format!("{relative}/{}", entry.file_name().to_string_lossy()))
            .collect();
        files.sort();
        files
    }

    /// Files that take part in chat: the chat and sign-in modules, and every
    /// command file that uses them. Commands chat shares with other screens
    /// (skills, guardian sync) are not chat-owned; their codes fall to the
    /// generic message, which is the designed fallback.
    fn chat_source_files() -> Vec<String> {
        let mut files = rust_files("src/ai_chat");
        files.extend(rust_files("src/ai_auth"));
        files.extend(
            rust_files("src/commands")
                .into_iter()
                .filter(|file| app_source(file).contains("ai_chat")),
        );
        files
    }

    /// Commands that still write refusals as `"code: reason"` literals, rather
    /// than through [`refuse`], must still use a code the webview can
    /// translate. Only refusal contexts count; log labels and fixtures do not.
    #[test]
    fn every_literal_error_prefix_in_chat_code_is_a_known_code() {
        let is_log = |line: &str| {
            ["log::", "warn!(", "info!(", "debug!(", "error!(", "trace!("]
                .iter()
                .any(|m| line.contains(m))
        };
        let is_refusal = |window: &str| {
            [
                "Err(",
                "ok_or",
                "map_err",
                "write_str(",
                "write!(",
                "#[error(",
            ]
            .iter()
            .any(|m| window.contains(m))
        };
        let mut unknown = Vec::new();
        for file in chat_source_files() {
            let source = app_source(&file);
            let lines: Vec<&str> = source.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if is_log(line) {
                    continue;
                }
                let window = lines[index.saturating_sub(1)..=index].join(" ");
                if !is_refusal(&window) {
                    continue;
                }
                for (start, _) in line.match_indices('"') {
                    let rest = &line[start + 1..];
                    let Some(colon) = rest.find(": ") else {
                        continue;
                    };
                    let prefix = &rest[..colon];
                    let is_name = !prefix.is_empty()
                        && prefix
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit());
                    if is_name && ChatErrorCode::parse(prefix).is_none() {
                        unknown.push(format!("{file}:{}: {prefix}", index + 1));
                    }
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "error prefixes missing from ChatErrorCode: {unknown:?}"
        );
    }

    #[test]
    fn the_scanner_covers_the_chat_commands_and_modules() {
        let files = chat_source_files();
        for expected in [
            "src/ai_chat/vocab.rs",
            "src/ai_auth/mod.rs",
            "src/commands/ai_chat.rs",
            "src/commands/ai_signin.rs",
            "src/commands/course_notes.rs",
        ] {
            assert!(
                files.iter().any(|f| f == expected),
                "{expected} not scanned"
            );
        }
    }

    fn snapshot_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/chat-vocab.json")
    }

    /// The checked-in snapshot equals this module. With `UPDATE_CHAT_VOCAB=1`
    /// (set by `npm run generate:chat-vocab`) the snapshot is rewritten
    /// instead; otherwise a stale snapshot fails with the lines that differ.
    #[test]
    fn chat_vocab_snapshot_is_current() {
        let expected = snapshot_json();
        let path = snapshot_path();
        if std::env::var_os("UPDATE_CHAT_VOCAB").is_some_and(|v| v == "1") {
            std::fs::write(&path, &expected)
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
            "{} is stale; run `npm run generate:chat-vocab` and commit the result.\n  missing from the snapshot: {missing:?}\n  no longer in the vocabulary: {extra:?}",
            path.display()
        );
    }
}
