//! Running-process watchlist (Sentinel, native, desktop only).
//!
//! Enumerates the processes on the assessment device and classifies the
//! ones that matter for integrity review: AI "interview assistant"
//! overlays, desktop AI chat clients, remote-desktop hosts (a helper
//! driving the machine), virtual cameras (a replayed or synthetic face),
//! screen-share clients (an off-device watcher), and hypervisor guest
//! agents (the assessment is running inside a VM).
//!
//! This is a fingerprint signal: a renamed binary walks straight past it.
//! It is cheap, high-precision when it fires, and useless when it misses,
//! so it is one input among several, never a gate on its own. The
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

use MatchKind::{ExactName, IdentifierPrefix};
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
    rule(
        AiAssistant,
        "com.anthropic.claudefordesktop",
        IdentifierPrefix,
    ),
    rule(AiAssistant, "claude", ExactName),
    rule(AiAssistant, "ai.perplexity", IdentifierPrefix),
    rule(AiAssistant, "perplexity", ExactName),
    rule(AiAssistant, "com.electron.ollama", IdentifierPrefix),
    rule(AiAssistant, "ollama", ExactName),
    rule(AiAssistant, "lm studio", ExactName),
    rule(AiAssistant, "lm-studio", ExactName),
    rule(AiAssistant, "msty", ExactName),
    // RemoteDesktop (hosts only)
    rule(RemoteDesktop, "com.teamviewer", IdentifierPrefix),
    rule(RemoteDesktop, "teamviewer", ExactName),
    rule(RemoteDesktop, "teamviewer_service", ExactName),
    rule(RemoteDesktop, "teamviewerd", ExactName),
    rule(RemoteDesktop, "com.philandro.anydesk", IdentifierPrefix),
    rule(RemoteDesktop, "anydesk", ExactName),
    rule(RemoteDesktop, "com.carriez.rustdesk", IdentifierPrefix),
    rule(RemoteDesktop, "rustdesk", ExactName),
    rule(RemoteDesktop, "tv.parsec", IdentifierPrefix),
    rule(RemoteDesktop, "parsec", ExactName),
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
    rule(VirtualCamera, "manycam", ExactName),
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
    rule(ScreenShare, "com.microsoft.teams", IdentifierPrefix),
    rule(ScreenShare, "ms-teams", ExactName),
    rule(ScreenShare, "teams", ExactName),
    rule(ScreenShare, "com.hnc.discord", IdentifierPrefix),
    rule(ScreenShare, "discord", ExactName),
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
        }
    })
}

fn kind_label(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::ExactName => "name",
        MatchKind::IdentifierPrefix => "id",
        MatchKind::NameContains => "contains",
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
                let identifier = if path.is_empty() { name.clone() } else { path };
                out.push(RawProcess {
                    pid: pid as u32,
                    name,
                    identifier,
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
                });
            }
            out
        })
    }

    pub fn enumerate() -> Option<Vec<RawProcess>> {
        // libproc first, then let NSWorkspace's richer info win per pid.
        let mut merged: BTreeMap<u32, RawProcess> = BTreeMap::new();
        for p in libproc_pass() {
            merged.insert(p.pid, p);
        }
        for p in workspace_pass() {
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
                        let identifier = image_path(pid).unwrap_or_else(|| exe.clone());
                        out.push(RawProcess {
                            pid,
                            name,
                            identifier,
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
        }
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
