//! Running-process watchlist (Sentinel, native, desktop only).
//!
//! Enumerates the processes on the assessment device and classifies the
//! ones that matter for integrity review: AI "interview assistant"
//! overlays, desktop AI chat clients, remote-desktop hosts (a helper
//! driving the machine), virtual cameras (a replayed or synthetic face),
//! screen-share clients (an off-device watcher), and hypervisor guest
//! agents (the assessment is running inside a VM).
//!
//! This is a fingerprint signal. On macOS and Windows a rule may name a
//! signing identity (team identifier, Authenticode publisher), so renaming
//! a signed tool does not hide it; an unsigned or re-signed binary, and
//! everything on Linux, is matched by name only. Signatures are read once
//! per executable path on a background thread and cached, so a scan never
//! waits on one: a process first seen in this scan matches by name until
//! its read lands. It is cheap, high-precision when it fires, and useless
//! when it misses, so it is one input among several, never a gate on its
//! own. The
//! watchlist is a `const` so it can later move to the model-update channel
//! without touching the matcher.
//!
//! Best-effort per platform; always `None` rather than an error so a
//! missing capability never breaks monitoring.
//!
//! - macOS — `NSWorkspace.runningApplications` (GUI apps: name + bundle
//!   id) merged with a `libproc` pass (`proc_listallpids` + `proc_name` +
//!   `proc_pidpath`) so headless daemons such as `vmtoolsd`, `ollama` or
//!   `x11vnc` are seen too.
//! - Windows — `CreateToolhelp32Snapshot` + `Process32FirstW/NextW`, full
//!   image path via `QueryFullProcessImageNameW` where the process lets us.
//! - Linux — `/proc/<pid>/comm`, `cmdline[0]` and `exe`.
//! - Mobile — `None`; the Android side has its own environment report.
//!
//! The pure core is [`classify`]: case-insensitive matching against
//! [`WATCHLIST`], own pid skipped, one hit per pid.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// One running process, before classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawProcess {
    pub pid: u32,
    /// Executable stem (Windows / Linux) or application name (macOS).
    pub name: String,
    /// Bundle id (macOS), full image path (Windows), `cmdline[0]` or
    /// `exe` link target (Linux). May equal `name`.
    pub identifier: String,
    /// Code-signing identity of the executable: the team identifier on
    /// macOS, the publisher on Windows. `None` when unsigned, not yet read,
    /// or on a platform without a reader (Linux, mobile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
}

/// Why a process is on the watchlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchCategory {
    /// Desktop AI chat client (ChatGPT, Claude, Ollama…). Context, not
    /// misconduct on its own.
    AiAssistant,
    /// Purpose-built interview / assessment cheating overlay.
    InterviewCheat,
    /// Remote-control *host*: someone else can drive this machine.
    RemoteDesktop,
    /// Virtual camera driver or compositor feeding the webcam.
    VirtualCamera,
    /// Screen-share / meeting client: an off-device watcher is plausible.
    ScreenShare,
    /// Hypervisor guest agent: we are running inside a VM.
    VirtualMachine,
}

/// A process that matched the watchlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedProcess {
    pub pid: u32,
    pub name: String,
    pub identifier: String,
    pub category: WatchCategory,
    /// `<match_kind>:<needle>` of the rule that fired, for review.
    pub rule: String,
}

/// One pass over the process table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessScan {
    pub watched: Vec<WatchedProcess>,
    /// Processes inspected, including our own.
    pub scanned: u32,
    /// `nsworkspace` | `toolhelp` | `procfs`.
    pub source: String,
}

/// How a rule's needle is compared (always case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// `name` equals the needle, with or without a trailing `.exe`.
    ExactName,
    /// `identifier` starts with the needle (bundle id families).
    IdentifierPrefix,
    /// `name` contains the needle. Needles must be ≥ 6 chars (tested) so
    /// this never degenerates into `"obs"` matching `"jobs"`.
    NameContains,
    /// The process's signing identity equals the needle. Matches whatever
    /// the file is called, so renaming a signed tool changes nothing.
    Signer,
}

/// One watchlist entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchRule {
    pub category: WatchCategory,
    pub needle: &'static str,
    pub match_kind: MatchKind,
}

const fn rule(category: WatchCategory, needle: &'static str, match_kind: MatchKind) -> WatchRule {
    WatchRule {
        category,
        needle,
        match_kind,
    }
}

use MatchKind::{ExactName, IdentifierPrefix, Signer};
use WatchCategory::*;

// Provenance (2026-10-09). "verified" = read from an installed bundle's
// Info.plist, a project's package.json, or vendor download metadata;
// "plausible" = the product exists and ships a desktop app, but the exact
// executable / bundle id could not be confirmed, so the rule uses the
// product name as an exact process-name match (the Electron default).
//
// InterviewCheat
//   Cluely                 — Electron app; Windows installer "Cluely Setup.exe"
//                            (third-party mirror listing). Bundle id not
//                            published; `com.cluely` prefix is plausible.
//   Interview Coder        — ibttf/interview-coder, Electron, package name
//                            `interview-coder` (verified). A widely-forked
//                            build (Prat011/free-cluely) ships disguised as
//                            productName "Meeting Notes Coder", appId
//                            `com.electron.meeting-notes` (verified).
//   OpenCluely             — TechyCSR/OpenCluely: productName "OpenCluely",
//                            appId `com.opencluely.app` (verified).
//   Ultracode, Final Round AI, LockedIn AI, Verve AI, Parakeet AI,
//   Sensei AI, InterviewHammer — desktop "stealth" apps exist per vendor
//                            marketing; names plausible, ids unverified.
// AiAssistant
//   Claude `com.anthropic.claudefordesktop` (verified locally), ChatGPT
//   `com.openai.chat` (vendor-documented) and Codex `com.openai.codex`
//   (verified locally), Ollama `com.electron.ollama` (verified locally),
//   Perplexity `ai.perplexity.mac` (plausible), LM Studio / Msty (plausible).
// RemoteDesktop / VirtualCamera / ScreenShare / VirtualMachine
//   Well-known hosts and daemons; bundle ids for Zoom (`us.zoom.xos`),
//   Teams (`com.microsoft.teams2`), Discord (`com.hnc.Discord`, verified
//   locally), OBS (`com.obsproject.obs-studio`, verified locally),
//   TeamViewer (`com.teamviewer.TeamViewer`), AnyDesk
//   (`com.philandro.anydesk`), RustDesk (`com.carriez.RustDesk`), Parsec
//   (`tv.parsec.www`) are the vendors' published ids. Guest-agent daemon
//   names (`vmtoolsd`, `VBoxService`, `prl_tools`, `qemu-ga`,
//   `spice-vdagent`) are the documented service binaries. Hyper-V guest
//   services run inside `svchost` on Windows and are not matchable by name.
//
// Signing identities (2026-11-20). A `Signer` rule sits after its vendor's
// name and id rules, so an unrenamed tool keeps reporting the same rule.
// macOS team identifiers are from appcatalog.cloud's per-app code
// requirements (`certificate leaf[subject.OU]`): TeamViewer H7UGFBUGV6,
// AnyDesk KHRWM533LU, RustDesk HZF9JMC8YN, Parsec Y9MY52XZDB, OBS
// 2MMRE5MTB8, ManyCam 72D53DS677, Zoom BJ4HAAB9B3, Discord 53Q6R32WPB,
// OpenAI (ChatGPT, Codex, Atlas) 2DC432GLL2, Anthropic Q6L2SF6YDW, Ollama
// 3MU9H2V9Y9. Windows publishers: AnyDesk's own incident FAQ names
// "AnyDesk Software GmbH" and its predecessor "philandro Software GmbH";
// "TeamViewer Germany GmbH" is the signer in public sandbox reports of
// current builds. Only vendors whose identity signs nothing outside the
// rule's category are listed: Microsoft (Teams, but also Remote Desktop
// and the OS) and Google (Chrome Remote Desktop, but also Chrome) stay
// name-only, as do the vendors whose identity could not be confirmed.
//
// Deliberately absent: browsers, Slack, Raycast, `mstsc` / Microsoft Remote
// Desktop (clients, not hosts), Google Meet (browser), Hyper-V, WSL.
pub const WATCHLIST: &[WatchRule] = &[
    // InterviewCheat
    rule(InterviewCheat, "cluely", ExactName),
    rule(InterviewCheat, "com.cluely", IdentifierPrefix),
    rule(InterviewCheat, "interview-coder", ExactName),
    rule(InterviewCheat, "interview coder", ExactName),
    rule(InterviewCheat, "meeting notes coder", ExactName),
    rule(
        InterviewCheat,
        "com.electron.meeting-notes",
        IdentifierPrefix,
    ),
    rule(InterviewCheat, "opencluely", ExactName),
    rule(InterviewCheat, "com.opencluely", IdentifierPrefix),
    rule(InterviewCheat, "ultracode", ExactName),
    rule(InterviewCheat, "final round ai", ExactName),
    rule(InterviewCheat, "finalround", ExactName),
    rule(InterviewCheat, "final-round", ExactName),
    rule(InterviewCheat, "lockedin ai", ExactName),
    rule(InterviewCheat, "lockedin", ExactName),
    rule(InterviewCheat, "verve ai", ExactName),
    rule(InterviewCheat, "parakeet ai", ExactName),
    rule(InterviewCheat, "sensei ai", ExactName),
    rule(InterviewCheat, "interviewhammer", ExactName),
    // AiAssistant
    rule(AiAssistant, "com.openai.chat", IdentifierPrefix),
    rule(AiAssistant, "chatgpt", ExactName),
    rule(AiAssistant, "com.openai.codex", IdentifierPrefix),
    rule(AiAssistant, "com.openai.atlas", IdentifierPrefix),
    rule(AiAssistant, "2dc432gll2", Signer),
    rule(
        AiAssistant,
        "com.anthropic.claudefordesktop",
        IdentifierPrefix,
    ),
    rule(AiAssistant, "claude", ExactName),
    rule(AiAssistant, "q6l2sf6ydw", Signer),
    rule(AiAssistant, "ai.perplexity", IdentifierPrefix),
    rule(AiAssistant, "perplexity", ExactName),
    rule(AiAssistant, "com.electron.ollama", IdentifierPrefix),
    rule(AiAssistant, "ollama", ExactName),
    rule(AiAssistant, "3mu9h2v9y9", Signer),
    rule(AiAssistant, "lm studio", ExactName),
    rule(AiAssistant, "lm-studio", ExactName),
    rule(AiAssistant, "msty", ExactName),
    // RemoteDesktop (hosts only)
    rule(RemoteDesktop, "com.teamviewer", IdentifierPrefix),
    rule(RemoteDesktop, "teamviewer", ExactName),
    rule(RemoteDesktop, "teamviewer_service", ExactName),
    rule(RemoteDesktop, "teamviewerd", ExactName),
    rule(RemoteDesktop, "h7ugfbugv6", Signer),
    rule(RemoteDesktop, "teamviewer germany gmbh", Signer),
    rule(RemoteDesktop, "com.philandro.anydesk", IdentifierPrefix),
    rule(RemoteDesktop, "anydesk", ExactName),
    rule(RemoteDesktop, "khrwm533lu", Signer),
    rule(RemoteDesktop, "anydesk software gmbh", Signer),
    rule(RemoteDesktop, "philandro software gmbh", Signer),
    rule(RemoteDesktop, "com.carriez.rustdesk", IdentifierPrefix),
    rule(RemoteDesktop, "rustdesk", ExactName),
    rule(RemoteDesktop, "hzf9jmc8yn", Signer),
    rule(RemoteDesktop, "tv.parsec", IdentifierPrefix),
    rule(RemoteDesktop, "parsec", ExactName),
    rule(RemoteDesktop, "y9my52xzdb", Signer),
    // Not `parsecd`: on macOS that name belongs to Apple's own
    // CoreParsec.framework daemon (Siri / Spotlight), a guaranteed false
    // positive. Parsec's remote host is caught by the `tv.parsec` bundle id
    // on macOS and the `parsec` exe stem elsewhere.
    rule(RemoteDesktop, "remoting_host", ExactName),
    rule(RemoteDesktop, "remote_assistance_host", ExactName),
    rule(RemoteDesktop, "chrome-remote-desktop-host", ExactName),
    rule(RemoteDesktop, "com.splashtop", IdentifierPrefix),
    rule(RemoteDesktop, "splashtop streamer", ExactName),
    rule(RemoteDesktop, "srserver", ExactName),
    rule(RemoteDesktop, "vncserver", ExactName),
    rule(RemoteDesktop, "vncserver-x11", ExactName),
    rule(RemoteDesktop, "realvnc", ExactName),
    rule(RemoteDesktop, "com.realvnc", IdentifierPrefix),
    rule(RemoteDesktop, "tvnserver", ExactName),
    rule(RemoteDesktop, "tightvnc", ExactName),
    rule(RemoteDesktop, "x11vnc", ExactName),
    rule(RemoteDesktop, "jump desktop connect", ExactName),
    rule(RemoteDesktop, "nxnode", ExactName),
    rule(RemoteDesktop, "nxserver", ExactName),
    rule(RemoteDesktop, "zoho assist", ExactName),
    rule(RemoteDesktop, "zohoassist", ExactName),
    rule(RemoteDesktop, "logmein", ExactName),
    rule(RemoteDesktop, "lmiguardiansvc", ExactName),
    // VirtualCamera
    rule(VirtualCamera, "com.obsproject", IdentifierPrefix),
    rule(VirtualCamera, "obs", ExactName),
    rule(VirtualCamera, "obs64", ExactName),
    rule(VirtualCamera, "obs-studio", ExactName),
    rule(VirtualCamera, "2mmre5mtb8", Signer),
    rule(VirtualCamera, "manycam", ExactName),
    rule(VirtualCamera, "72d53ds677", Signer),
    rule(VirtualCamera, "snap camera", ExactName),
    rule(VirtualCamera, "camtwist", ExactName),
    rule(VirtualCamera, "mmhmm", ExactName),
    rule(VirtualCamera, "xsplit vcam", ExactName),
    rule(VirtualCamera, "xsplit.vcam", ExactName),
    rule(VirtualCamera, "splitcam", ExactName),
    rule(VirtualCamera, "youcam", ExactName),
    rule(VirtualCamera, "chromacam", ExactName),
    rule(VirtualCamera, "iriun webcam", ExactName),
    rule(VirtualCamera, "iriunwebcam", ExactName),
    rule(VirtualCamera, "epoccam", ExactName),
    rule(VirtualCamera, "droidcam", ExactName),
    rule(VirtualCamera, "camo", ExactName),
    rule(VirtualCamera, "com.reincubate", IdentifierPrefix),
    rule(VirtualCamera, "ndi virtual input", ExactName),
    // ScreenShare (context)
    rule(ScreenShare, "us.zoom", IdentifierPrefix),
    rule(ScreenShare, "zoom", ExactName),
    rule(ScreenShare, "zoom.us", ExactName),
    rule(ScreenShare, "bj4haab9b3", Signer),
    rule(ScreenShare, "com.microsoft.teams", IdentifierPrefix),
    rule(ScreenShare, "ms-teams", ExactName),
    rule(ScreenShare, "teams", ExactName),
    rule(ScreenShare, "com.hnc.discord", IdentifierPrefix),
    rule(ScreenShare, "discord", ExactName),
    rule(ScreenShare, "53q6r32wpb", Signer),
    rule(ScreenShare, "cisco webex meetings", ExactName),
    rule(ScreenShare, "webex", ExactName),
    rule(ScreenShare, "com.cisco.webex", IdentifierPrefix),
    rule(ScreenShare, "com.loom", IdentifierPrefix),
    rule(ScreenShare, "loom", ExactName),
    rule(ScreenShare, "screen studio", ExactName),
    // VirtualMachine (guest agents)
    rule(VirtualMachine, "vboxservice", ExactName),
    rule(VirtualMachine, "vboxclient", ExactName),
    rule(VirtualMachine, "vmtoolsd", ExactName),
    rule(VirtualMachine, "vmware-tools-daemon", ExactName),
    rule(VirtualMachine, "vgauthservice", ExactName),
    rule(VirtualMachine, "prl_tools", ExactName),
    rule(VirtualMachine, "prl_tools_service", ExactName),
    rule(VirtualMachine, "prl_cc", ExactName),
    rule(VirtualMachine, "qemu-ga", ExactName),
    rule(VirtualMachine, "spice-vdagent", ExactName),
];

fn exact_name_matches(name_lower: &str, needle: &str) -> bool {
    if name_lower == needle {
        return true;
    }
    name_lower
        .strip_suffix(".exe")
        .is_some_and(|stem| stem == needle)
}

/// First watchlist rule the process matches, if any.
fn match_rule(proc_: &RawProcess) -> Option<&'static WatchRule> {
    let name = proc_.name.to_lowercase();
    let identifier = proc_.identifier.to_lowercase();
    let signer = proc_.signer.as_deref().map(str::to_lowercase);
    // Linux `comm` is truncated to 15 bytes by the kernel, so a long needle
    // can only match the executable's basename (from the `exe` link or
    // `cmdline[0]`). On the other platforms this is the same string again.
    let exe_stem = identifier
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&identifier)
        .to_owned();
    WATCHLIST.iter().find(|r| {
        let needle = r.needle.to_lowercase();
        match r.match_kind {
            MatchKind::ExactName => {
                exact_name_matches(&name, &needle) || exact_name_matches(&exe_stem, &needle)
            }
            MatchKind::IdentifierPrefix => identifier.starts_with(&needle),
            MatchKind::NameContains => name.contains(&needle),
            MatchKind::Signer => signer.as_deref() == Some(needle.as_str()),
        }
    })
}

fn kind_label(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::ExactName => "name",
        MatchKind::IdentifierPrefix => "id",
        MatchKind::NameContains => "contains",
        MatchKind::Signer => "signer",
    }
}

/// Classify a process list: skip our own pid, one hit per pid, first
/// matching rule wins. Pure and deterministic.
pub fn classify(raw: &[RawProcess], own_pid: u32, source: &str) -> ProcessScan {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut watched = Vec::new();
    for p in raw {
        if p.pid == own_pid || !seen.insert(p.pid) {
            continue;
        }
        if let Some(r) = match_rule(p) {
            watched.push(WatchedProcess {
                pid: p.pid,
                name: p.name.clone(),
                identifier: p.identifier.clone(),
                category: r.category,
                rule: format!("{}:{}", kind_label(r.match_kind), r.needle),
            });
        }
    }
    ProcessScan {
        watched,
        scanned: raw.len() as u32,
        source: source.to_owned(),
    }
}

/// Signing identities by executable path. Each path is read once; until
/// its read lands the process is matched by name only.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Default)]
struct SignerCache {
    known: std::collections::HashMap<String, Option<String>>,
    queued: HashSet<String>,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl SignerCache {
    /// The identity read for `path` so far, and whether `path` has just
    /// been queued for its one read.
    fn lookup(&mut self, path: &str) -> (Option<String>, bool) {
        if let Some(signer) = self.known.get(path) {
            return (signer.clone(), false);
        }
        (None, self.queued.insert(path.to_owned()))
    }

    fn record(&mut self, path: String, signer: Option<String>) {
        self.queued.remove(&path);
        self.known.insert(path, signer);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod signers {
    use std::sync::{mpsc, LazyLock, Mutex, PoisonError};

    use super::SignerCache;

    static CACHE: LazyLock<Mutex<SignerCache>> = LazyLock::new(Default::default);

    /// One background thread reads signatures, so a scan never waits on
    /// one. `None` if the thread could not be started: name matching only.
    static READER: LazyLock<Option<mpsc::Sender<String>>> = LazyLock::new(|| {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("sentinel-signer".into())
            .spawn(move || {
                for path in rx {
                    let signer = super::imp::read_signer(&path);
                    CACHE
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .record(path, signer);
                }
            })
            .ok()?;
        Some(tx)
    });

    /// The cached signing identity of the executable at `path`. A path
    /// seen for the first time is queued for the reader and yields `None`
    /// on this scan.
    pub fn cached(path: &str) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        let (signer, queued) = CACHE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lookup(path);
        if queued {
            if let Some(tx) = READER.as_ref() {
                let _ = tx.send(path.to_owned());
            }
        }
        signer
    }
}

/// Enumerate and classify, or `None` when the platform has no probe or
/// enumeration failed outright.
pub fn scan() -> Option<ProcessScan> {
    let raw = imp::enumerate()?;
    Some(classify(&raw, std::process::id(), imp::SOURCE))
}

/// Raw process table, for diagnostics.
pub fn raw_processes() -> Option<Vec<RawProcess>> {
    imp::enumerate()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::BTreeMap;

    use std::ffi::c_void;

    use super::RawProcess;

    pub const SOURCE: &str = "nsworkspace";

    unsafe extern "C" {
        // libproc.h — shipped in libSystem, no explicit link needed.
        fn proc_listallpids(
            buffer: *mut std::ffi::c_void,
            buffersize: std::ffi::c_int,
        ) -> std::ffi::c_int;
        fn proc_name(
            pid: std::ffi::c_int,
            buffer: *mut std::ffi::c_void,
            buffersize: u32,
        ) -> std::ffi::c_int;
        fn proc_pidpath(
            pid: std::ffi::c_int,
            buffer: *mut std::ffi::c_void,
            buffersize: u32,
        ) -> std::ffi::c_int;
    }

    // Security.framework / CoreFoundation, for the code signature's team
    // identifier. Raw FFI: four calls do not justify another binding crate.
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecStaticCodeCreateWithPath(
            path: *const c_void,
            flags: u32,
            static_code: *mut *const c_void,
        ) -> i32;
        fn SecCodeCopySigningInformation(
            code: *const c_void,
            flags: u32,
            information: *mut *const c_void,
        ) -> i32;
        static kSecCodeInfoTeamIdentifier: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            buf_len: isize,
            is_directory: u8,
        ) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut std::ffi::c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    const K_SEC_CS_DEFAULT_FLAGS: u32 = 0;
    /// `kSecCSSigningInformation`: read the embedded signature without
    /// validating it (validation hashes the whole binary).
    const K_SEC_CS_SIGNING_INFORMATION: u32 = 1 << 1;
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// Team identifier from the code signature of the executable at
    /// `path`, or `None` when unsigned, ad-hoc signed or unreadable. The
    /// signature is not validated: this is a fingerprint, and a tool that
    /// strips or re-signs itself falls back to name matching.
    pub fn read_signer(path: &str) -> Option<String> {
        // SAFETY: every CF object created here (`url`, `code`, `info`) is
        // null-checked and released exactly once. `team` is borrowed from
        // `info` (Get rule), type-checked as a CFString and read before
        // `info` is released. `buf` is a fixed stack buffer whose size is
        // passed to CFStringGetCString, which NUL-terminates on success.
        unsafe {
            let url = CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                path.as_ptr(),
                path.len() as isize,
                0,
            );
            if url.is_null() {
                return None;
            }
            let mut code: *const c_void = std::ptr::null();
            let status = SecStaticCodeCreateWithPath(url, K_SEC_CS_DEFAULT_FLAGS, &mut code);
            CFRelease(url);
            if status != 0 || code.is_null() {
                return None;
            }
            let mut info: *const c_void = std::ptr::null();
            let status =
                SecCodeCopySigningInformation(code, K_SEC_CS_SIGNING_INFORMATION, &mut info);
            CFRelease(code);
            if status != 0 || info.is_null() {
                return None;
            }
            let team = CFDictionaryGetValue(info, kSecCodeInfoTeamIdentifier);
            let mut out = None;
            if !team.is_null() && CFGetTypeID(team) == CFStringGetTypeID() {
                let mut buf = [0 as std::ffi::c_char; 64];
                if CFStringGetCString(
                    team,
                    buf.as_mut_ptr(),
                    buf.len() as isize,
                    K_CF_STRING_ENCODING_UTF8,
                ) != 0
                {
                    out = std::ffi::CStr::from_ptr(buf.as_ptr())
                        .to_str()
                        .ok()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                }
            }
            CFRelease(info);
            out
        }
    }

    /// Headless pass: every pid with its short name and executable path.
    fn libproc_pass() -> Vec<RawProcess> {
        // SAFETY: `proc_listallpids(NULL, 0)` returns the pid count only;
        // the second call receives a buffer of exactly that many i32 slots
        // (plus slack for processes spawned in between). `proc_name` and
        // `proc_pidpath` write at most `buffersize` bytes into stack
        // buffers of that size and return the bytes written (0 on failure).
        unsafe {
            let count = proc_listallpids(std::ptr::null_mut(), 0);
            if count <= 0 {
                return Vec::new();
            }
            let mut pids = vec![0i32; count as usize + 64];
            let bytes = (pids.len() * std::mem::size_of::<i32>()) as std::ffi::c_int;
            let got = proc_listallpids(pids.as_mut_ptr().cast(), bytes);
            if got <= 0 {
                return Vec::new();
            }
            pids.truncate(got as usize);

            let mut out = Vec::with_capacity(pids.len());
            for pid in pids {
                if pid <= 0 {
                    continue;
                }
                let mut name_buf = [0u8; 256];
                let n = proc_name(pid, name_buf.as_mut_ptr().cast(), name_buf.len() as u32);
                let mut path_buf = [0u8; 4096];
                let p = proc_pidpath(pid, path_buf.as_mut_ptr().cast(), path_buf.len() as u32);
                let name = if n > 0 {
                    String::from_utf8_lossy(&name_buf[..n as usize]).into_owned()
                } else {
                    String::new()
                };
                let path = if p > 0 {
                    String::from_utf8_lossy(&path_buf[..p as usize]).into_owned()
                } else {
                    String::new()
                };
                if name.is_empty() && path.is_empty() {
                    continue;
                }
                let name = if name.is_empty() {
                    path.rsplit('/').next().unwrap_or(&path).to_owned()
                } else {
                    name
                };
                let signer = super::signers::cached(&path);
                let identifier = if path.is_empty() { name.clone() } else { path };
                out.push(RawProcess {
                    pid: pid as u32,
                    name,
                    identifier,
                    signer,
                });
            }
            out
        }
    }

    /// GUI pass: localized name + bundle id for every running application.
    fn workspace_pass() -> Vec<RawProcess> {
        use objc2_app_kit::NSWorkspace;
        objc2::rc::autoreleasepool(|_| {
            let ws = NSWorkspace::sharedWorkspace();
            let apps = ws.runningApplications();
            let mut out = Vec::with_capacity(apps.len());
            for app in apps.iter() {
                let pid = app.processIdentifier();
                if pid <= 0 {
                    continue;
                }
                let name = app
                    .localizedName()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let identifier = app
                    .bundleIdentifier()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        app.executableURL()
                            .and_then(|u| u.lastPathComponent())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| name.clone());
                if name.is_empty() && identifier.is_empty() {
                    continue;
                }
                let name = if name.is_empty() {
                    identifier.clone()
                } else {
                    name
                };
                out.push(RawProcess {
                    pid: pid as u32,
                    name,
                    identifier,
                    signer: None,
                });
            }
            out
        })
    }

    pub fn enumerate() -> Option<Vec<RawProcess>> {
        // libproc first, then let NSWorkspace's richer info win per pid.
        // Only libproc knows the executable path, so its signer carries over.
        let mut merged: BTreeMap<u32, RawProcess> = BTreeMap::new();
        for p in libproc_pass() {
            merged.insert(p.pid, p);
        }
        for mut p in workspace_pass() {
            if let Some(seen) = merged.get_mut(&p.pid) {
                p.signer = seen.signer.take();
            }
            merged.insert(p.pid, p);
        }
        if merged.is_empty() {
            return None;
        }
        Some(merged.into_values().collect())
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::RawProcess;

    pub const SOURCE: &str = "toolhelp";

    fn image_path(pid: u32) -> Option<String> {
        use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: `OpenProcess` is checked before use and its HANDLE is
        // closed on every path; the output buffer is MAX_PATH u16s and
        // `len` carries its capacity in/out as the API requires.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; MAX_PATH as usize];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Publisher (signer certificate's simple display name) of the
    /// Authenticode signature embedded in the file at `path`, or `None`
    /// when unsigned or catalog-signed. The signature is not verified (no
    /// chain build, no revocation fetch): this is a fingerprint, and a
    /// tool that strips its signature falls back to name matching.
    pub fn read_signer(path: &str) -> Option<String> {
        use std::ffi::c_void;
        use windows::Win32::Security::Cryptography::{
            CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext,
            CertGetNameStringW, CryptMsgClose, CryptMsgGetParam, CryptQueryObject,
            CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_NAME_SIMPLE_DISPLAY_TYPE,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
            CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM, HCERTSTORE,
            PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
        };
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the
        // CryptQueryObject call. The store and message it returns are
        // closed on every path below. The signer info is copied into a
        // u64-aligned buffer of the size CryptMsgGetParam reported, and
        // its issuer/serial blobs point into that buffer, which outlives
        // the CertFindCertificateInStore call. The certificate context is
        // freed after its name is read into a stack buffer whose length
        // CertGetNameStringW receives.
        unsafe {
            let mut store = HCERTSTORE::default();
            let mut msg: *mut c_void = std::ptr::null_mut();
            CryptQueryObject(
                CERT_QUERY_OBJECT_FILE,
                wide.as_ptr().cast(),
                CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
                CERT_QUERY_FORMAT_FLAG_BINARY,
                0,
                None,
                None,
                None,
                Some(&mut store),
                Some(&mut msg),
                None,
            )
            .ok()?;
            let name = (|| {
                let mut len = 0u32;
                CryptMsgGetParam(msg, CMSG_SIGNER_INFO_PARAM, 0, None, &mut len).ok()?;
                if (len as usize) < std::mem::size_of::<CMSG_SIGNER_INFO>() {
                    return None;
                }
                let mut buf = vec![0u64; (len as usize).div_ceil(8)];
                CryptMsgGetParam(
                    msg,
                    CMSG_SIGNER_INFO_PARAM,
                    0,
                    Some(buf.as_mut_ptr().cast()),
                    &mut len,
                )
                .ok()?;
                let signer = &*(buf.as_ptr() as *const CMSG_SIGNER_INFO);
                let wanted = CERT_INFO {
                    Issuer: signer.Issuer,
                    SerialNumber: signer.SerialNumber,
                    ..Default::default()
                };
                let cert = CertFindCertificateInStore(
                    store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    0,
                    CERT_FIND_SUBJECT_CERT,
                    Some((&wanted as *const CERT_INFO).cast()),
                    None,
                );
                if cert.is_null() {
                    return None;
                }
                let mut name = [0u16; 256];
                let n = CertGetNameStringW(
                    cert,
                    CERT_NAME_SIMPLE_DISPLAY_TYPE,
                    0,
                    None,
                    Some(&mut name),
                );
                let _ = CertFreeCertificateContext(Some(cert));
                // `n` counts the trailing NUL; 1 means an empty name.
                (n > 1).then(|| String::from_utf16_lossy(&name[..n as usize - 1]))
            })();
            let _ = CryptMsgClose(Some(msg));
            let _ = CertCloseStore(store, 0);
            name
        }
    }

    pub fn enumerate() -> Option<Vec<RawProcess>> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: the snapshot HANDLE is validated by the crate wrapper and
        // closed before returning; `entry` is a zeroed PROCESSENTRY32W with
        // `dwSize` set as Process32FirstW/NextW require, and `szExeFile` is
        // read only up to its first NUL.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut out = Vec::new();
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    let pid = entry.th32ProcessID;
                    if pid != 0 && !exe.is_empty() {
                        let name = exe.trim_end_matches(".exe").to_owned();
                        let path = image_path(pid);
                        let signer = path.as_deref().and_then(super::signers::cached);
                        let identifier = path.unwrap_or_else(|| exe.clone());
                        out.push(RawProcess {
                            pid,
                            name,
                            identifier,
                            signer,
                        });
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::RawProcess;

    pub const SOURCE: &str = "procfs";

    pub fn enumerate() -> Option<Vec<RawProcess>> {
        let dir = std::fs::read_dir("/proc").ok()?;
        let mut out = Vec::new();
        for entry in dir.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let base = entry.path();
            let comm = std::fs::read_to_string(base.join("comm"))
                .map(|s| s.trim().to_owned())
                .unwrap_or_default();
            let cmd0 = std::fs::read(base.join("cmdline")).ok().and_then(|b| {
                b.split(|&c| c == 0)
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).into_owned())
            });
            let exe = std::fs::read_link(base.join("exe"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let identifier = exe.or(cmd0).unwrap_or_else(|| comm.clone());
            if comm.is_empty() && identifier.is_empty() {
                continue;
            }
            let name = if comm.is_empty() {
                identifier
                    .rsplit('/')
                    .next()
                    .unwrap_or(&identifier)
                    .to_owned()
            } else {
                comm
            };
            out.push(RawProcess {
                pid,
                name,
                identifier,
                signer: None,
            });
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::RawProcess;
    pub const SOURCE: &str = "none";
    pub fn enumerate() -> Option<Vec<RawProcess>> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_truncated_linux_comm_still_matches_on_the_exe_basename() {
        // The kernel cuts `comm` at 15 bytes; the exe link carries the rest.
        let p = super::RawProcess {
            pid: 4242,
            name: "chrome-remote-d".into(),
            identifier: "/opt/google/chrome-remote-desktop/chrome-remote-desktop-host".into(),
            signer: None,
        };
        let scan = super::classify(&[p], 1, "procfs");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(
            scan.watched[0].category,
            super::WatchCategory::RemoteDesktop
        );
    }

    use super::*;

    fn p(pid: u32, name: &str, identifier: &str) -> RawProcess {
        RawProcess {
            pid,
            name: name.into(),
            identifier: identifier.into(),
            signer: None,
        }
    }

    fn signed(pid: u32, name: &str, identifier: &str, signer: &str) -> RawProcess {
        RawProcess {
            signer: Some(signer.into()),
            ..p(pid, name, identifier)
        }
    }

    #[test]
    fn a_renamed_tool_signed_by_a_watched_identity_still_matches() {
        // TeamViewer's macOS team identifier, copied and renamed.
        let raw = [signed(
            1,
            "Notes Helper",
            "/Users/x/Downloads/notes-helper",
            "H7UGFBUGV6",
        )];
        let scan = classify(&raw, 0, "nsworkspace");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::RemoteDesktop);
        assert_eq!(scan.watched[0].rule, "signer:h7ugfbugv6");
    }

    #[test]
    fn an_unsigned_process_falls_back_to_its_name() {
        let raw = [p(
            1,
            "AnyDesk",
            "/Applications/AnyDesk.app/Contents/MacOS/AnyDesk",
        )];
        let scan = classify(&raw, 0, "nsworkspace");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::RemoteDesktop);
        assert_eq!(scan.watched[0].rule, "name:anydesk");
    }

    #[test]
    fn an_unwatched_identity_does_not_hide_a_watched_name() {
        let raw = [signed(
            1,
            "Cluely",
            "C:\\Users\\x\\AppData\\Local\\cluely\\Cluely.exe",
            "Some Unrelated Vendor Ltd",
        )];
        let scan = classify(&raw, 0, "toolhelp");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::InterviewCheat);
        assert_eq!(scan.watched[0].rule, "name:cluely");
    }

    #[test]
    fn a_windows_publisher_matches_whatever_the_exe_is_called() {
        let raw = [signed(
            1,
            "svc-helper",
            "C:\\Temp\\svc-helper.exe",
            "AnyDesk Software GmbH",
        )];
        let scan = classify(&raw, 0, "toolhelp");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::RemoteDesktop);
        assert_eq!(scan.watched[0].rule, "signer:anydesk software gmbh");
    }

    #[test]
    fn a_signed_tool_keeps_its_name_rule_label() {
        // Signer rules sit after a vendor's name and id rules, so an
        // unrenamed tool reports the same rule as before.
        let raw = [signed(
            1,
            "TeamViewer",
            "com.teamviewer.TeamViewer",
            "H7UGFBUGV6",
        )];
        assert_eq!(
            classify(&raw, 0, "nsworkspace").watched[0].rule,
            "id:com.teamviewer"
        );
    }

    #[test]
    fn each_path_is_read_once_and_then_served_from_the_cache() {
        let mut cache = SignerCache::default();
        assert_eq!(
            cache.lookup("/a"),
            (None, true),
            "first sight queues a read"
        );
        assert_eq!(
            cache.lookup("/a"),
            (None, false),
            "a queued path is not re-queued"
        );
        cache.record("/a".into(), Some("TEAMID1234".into()));
        assert_eq!(cache.lookup("/a"), (Some("TEAMID1234".into()), false));
        cache.record("/b".into(), None);
        assert_eq!(
            cache.lookup("/b"),
            (None, false),
            "unsigned is remembered too"
        );
    }

    #[test]
    fn own_pid_is_skipped() {
        let raw = [p(7, "Cluely", "com.cluely.app")];
        let scan = classify(&raw, 7, "t");
        assert!(scan.watched.is_empty());
        assert_eq!(scan.scanned, 1);
    }

    #[test]
    fn exact_name_is_case_insensitive_and_accepts_exe_suffix() {
        let raw = [
            p(1, "CLUELY", "x"),
            p(2, "cluely.exe", "y"),
            p(3, "Cluely Helper", "z"),
        ];
        let scan = classify(&raw, 0, "t");
        let pids: Vec<u32> = scan.watched.iter().map(|w| w.pid).collect();
        assert_eq!(pids, vec![1, 2]);
        assert!(scan
            .watched
            .iter()
            .all(|w| w.category == WatchCategory::InterviewCheat));
    }

    #[test]
    fn identifier_prefix_matches_bundle_family() {
        let raw = [p(1, "Whatever", "com.anthropic.claudefordesktop.helper")];
        let scan = classify(&raw, 0, "t");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::AiAssistant);
        assert_eq!(scan.watched[0].rule, "id:com.anthropic.claudefordesktop");
    }

    #[test]
    fn short_names_do_not_over_match() {
        // "obs" is ExactName; "jobs", "Observer" must not fire.
        let raw = [
            p(1, "jobs", "/usr/bin/jobs"),
            p(2, "Observer", "obs.observer"),
            p(3, "obs", "/usr/bin/obs"),
        ];
        let scan = classify(&raw, 0, "t");
        let pids: Vec<u32> = scan.watched.iter().map(|w| w.pid).collect();
        assert_eq!(pids, vec![3]);
        assert_eq!(scan.watched[0].category, WatchCategory::VirtualCamera);
    }

    #[test]
    fn categories_are_assigned_per_rule() {
        let raw = [
            p(1, "TeamViewer", "com.teamviewer.TeamViewer"),
            p(2, "vmtoolsd", "/usr/bin/vmtoolsd"),
            p(3, "zoom.us", "us.zoom.xos"),
            p(4, "ManyCam", "ManyCam"),
            p(5, "ollama", "/usr/local/bin/ollama"),
        ];
        let scan = classify(&raw, 0, "t");
        let cats: Vec<WatchCategory> = scan.watched.iter().map(|w| w.category).collect();
        assert_eq!(
            cats,
            vec![
                WatchCategory::RemoteDesktop,
                WatchCategory::VirtualMachine,
                WatchCategory::ScreenShare,
                WatchCategory::VirtualCamera,
                WatchCategory::AiAssistant,
            ]
        );
    }

    #[test]
    fn duplicate_pids_are_reported_once() {
        let raw = [
            p(9, "Discord", "com.hnc.Discord"),
            p(9, "Discord", "com.hnc.Discord"),
        ];
        let scan = classify(&raw, 0, "t");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.scanned, 2);
    }

    #[test]
    fn unrelated_processes_are_not_watched() {
        let raw = [
            p(1, "Google Chrome", "com.google.Chrome"),
            p(2, "Slack", "com.tinyspeck.slackmacgap"),
            p(3, "Raycast", "com.raycast.macos"),
            p(4, "mstsc", "C:\\Windows\\System32\\mstsc.exe"),
            p(5, "Microsoft Remote Desktop", "com.microsoft.rdc.macos"),
        ];
        let scan = classify(&raw, 0, "t");
        assert!(scan.watched.is_empty());
        assert_eq!(scan.scanned, 5);
        assert_eq!(scan.source, "t");
    }

    #[test]
    fn apple_coreparsec_daemon_is_not_remote_desktop() {
        let raw = [p(
            1093,
            "parsecd",
            "/System/Library/PrivateFrameworks/CoreParsec.framework/parsecd",
        )];
        assert!(classify(&raw, 0, "nsworkspace").watched.is_empty());
        let real = [p(2, "Parsec", "tv.parsec.www")];
        assert_eq!(
            classify(&real, 0, "nsworkspace").watched[0].category,
            WatchCategory::RemoteDesktop
        );
    }

    #[test]
    fn disguised_interview_coder_build_is_caught() {
        let raw = [p(1, "Meeting Notes Coder", "com.electron.meeting-notes")];
        let scan = classify(&raw, 0, "t");
        assert_eq!(scan.watched.len(), 1);
        assert_eq!(scan.watched[0].category, WatchCategory::InterviewCheat);
    }

    #[test]
    fn serializes_with_snake_case_fields_and_categories() {
        let scan = classify(&[p(1, "RustDesk", "com.carriez.RustDesk")], 0, "procfs");
        let v = serde_json::to_value(&scan).unwrap();
        assert_eq!(v["scanned"], 1);
        assert_eq!(v["source"], "procfs");
        assert_eq!(v["watched"][0]["category"], "remote_desktop");
        assert_eq!(v["watched"][0]["rule"], "id:com.carriez.rustdesk");
        assert_eq!(v["watched"][0]["pid"], 1);
        let back: ProcessScan = serde_json::from_value(v).unwrap();
        assert_eq!(back, scan);
    }

    #[test]
    fn watchlist_has_no_duplicate_rules() {
        let mut seen = HashSet::new();
        for r in WATCHLIST {
            assert!(
                seen.insert((r.category, r.needle.to_lowercase(), r.match_kind as u8)),
                "duplicate rule {:?} {}",
                r.category,
                r.needle
            );
        }
    }

    #[test]
    fn watchlist_needles_are_long_enough() {
        for r in WATCHLIST {
            assert!(r.needle.len() >= 3, "needle too short: {}", r.needle);
            if r.match_kind == MatchKind::NameContains {
                assert!(
                    r.needle.len() >= 6,
                    "contains needle too short: {}",
                    r.needle
                );
            }
            assert_eq!(
                r.needle,
                r.needle.to_lowercase(),
                "needles are stored lowercase"
            );
        }
    }

    #[test]
    fn name_contains_requires_substring() {
        let raw = [p(1, "Interview Coder Helper", "x")];
        // No NameContains rule exists for this today, so it must not fire.
        assert!(classify(&raw, 0, "t").watched.is_empty());
        let r = WatchRule {
            category: WatchCategory::InterviewCheat,
            needle: "interview coder",
            match_kind: MatchKind::NameContains,
        };
        assert!(raw[0].name.to_lowercase().contains(r.needle));
    }
}
