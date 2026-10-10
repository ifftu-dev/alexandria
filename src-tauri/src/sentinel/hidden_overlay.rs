//! Hidden-overlay detection (Sentinel, native, desktop only).
//!
//! AI "interview assistant" overlays hide from screen capture by asking the
//! OS to leave their window out of every capture path:
//! `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on Windows and
//! `NSWindow.sharingType = .none` on macOS. They also float above the
//! assessment without ever taking focus, so `active_app` never fires. The
//! window is still visible on the monitor, and both OSes let any process
//! read the exclusion state of every on-screen window — which is exactly
//! what this module does.
//!
//! Best-effort per platform; always `None` rather than an error so a
//! missing capability never breaks monitoring.
//!
//! - macOS — `CGWindowListCopyWindowInfo` with `kCGWindowSharingState == 0`
//!   (`kCGWindowSharingNone`). No permission is required for the owner,
//!   PID, bounds and sharing state; window *titles* need the Screen
//!   Recording permission and are reported as `None` without it.
//! - Windows — `EnumWindows` + `GetWindowDisplayAffinity`, which reads the
//!   affinity "from any process" (Microsoft docs). Layered, click-through,
//!   topmost windows are additionally reported as `clickthrough_topmost`.
//! - Linux (X11) — there is no capture-exclusion flag in X11. The nearest
//!   tell is a viewable override-redirect window (no WM frame, usually
//!   stacked on top), reported as `override_redirect`. Wayland isolates
//!   clients and returns `None`.
//!
//! The pure core is [`classify`]: it drops our own windows, tiny or
//! off-screen ones, and a short allowlist of software that legitimately
//! excludes itself from capture (password managers, OS shell chrome).
//! Everything else that is excluded is reported for review; the caller
//! decides severity.

use serde::{Deserialize, Serialize};

/// Minimum size for a window to count. Anything smaller is a tray hint,
/// a caret, or a tooltip, not an overlay a person can read answers from.
pub const MIN_OVERLAY_SIZE: u32 = 24;

/// A window the OS reports as hidden from capture (or otherwise overlaid).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayWindow {
    pub pid: u32,
    /// Bundle id (macOS), executable stem (Windows) or WM_CLASS (X11).
    pub owner: String,
    /// Always `None` on the wire: titles stay in the backend (privacy).
    pub title: Option<String>,
    pub width: u32,
    pub height: u32,
    pub on_screen: bool,
    /// `capture_excluded` | `override_redirect` | `clickthrough_topmost`.
    pub reason: String,
}

/// One pass over the window list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayScan {
    pub suspicious: Vec<OverlayWindow>,
    /// Excluded windows skipped because their owner is allowlisted.
    pub allowlisted: u32,
    /// Windows inspected, including our own and trivially small ones.
    pub scanned: u32,
    /// `cgwindow` | `win32` | `x11`.
    pub source: String,
}

/// Platform-neutral description of one window, before classification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawWindow {
    pub pid: u32,
    pub owner: String,
    pub title: Option<String>,
    pub width: u32,
    pub height: u32,
    pub on_screen: bool,
    /// The OS will leave this window out of screen capture.
    pub excluded_from_capture: bool,
    /// X11 override-redirect (unmanaged, usually topmost) window.
    pub override_redirect: bool,
    /// Windows: layered + transparent (click-through) + topmost.
    pub clickthrough_topmost: bool,
    /// Window level / layer, kept for review context.
    pub level: i32,
}

/// Owners that legitimately hide from capture. Matched case-insensitively
/// against the whole owner string (a trailing `.exe` is ignored), never as
/// a substring: an overlay names its own process, so `searchhost-ai` or
/// `1password-helper` must not inherit an entry's trust.
pub const ALLOWLIST: &[&str] = &[
    // Password managers (hide secrets from screen share).
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "1password",
    "org.keepassxc.keepassxc",
    "keepassxc",
    "com.bitwarden.desktop",
    "bitwarden",
    "dashlane",
    "lastpass",
    "keeper",
    "enpass",
    "proton pass",
    "protonpass",
    "ch.protonmail.pass",
    // macOS shell chrome — bundle ids (Windows-style owners) and the
    // kCGWindowOwnerName display names CoreGraphics actually reports.
    "control center",
    "notification center",
    "usernotificationcenter",
    "window server",
    "windowmanager",
    "spotlight",
    "dock",
    "loginwindow",
    "systemuiserver",
    "screencaptureui",
    "com.apple.controlcenter",
    "com.apple.dock",
    "com.apple.notificationcenterui",
    "com.apple.windowmanager",
    "com.apple.screencaptureui",
    "com.apple.spotlight",
    "com.apple.loginwindow",
    "com.apple.systemuiserver",
    "com.apple.textinputmenuagent",
    "com.apple.wallpaper",
    // Windows shell chrome.
    "explorer",
    "shellexperiencehost",
    "textinputhost",
    "searchhost",
    "startmenuexperiencehost",
    "lockapp",
    "dwm",
    "applicationframehost",
    "securityhealthsystray",
    "widgets",
    "screenclippinghost",
    "snippingtool",
];

/// Whether `owner` is on the allowlist (see [`ALLOWLIST`] for the rule).
pub fn is_allowlisted(owner: &str) -> bool {
    let owner = owner.trim().to_ascii_lowercase();
    if owner.is_empty() {
        return false;
    }
    let stem = owner.strip_suffix(".exe").unwrap_or(&owner);
    ALLOWLIST.contains(&stem)
}

/// Pure classification: which raw windows are worth a reviewer's eyes.
pub fn classify(raw: &[RawWindow], own_pid: u32, source: &str) -> OverlayScan {
    let mut suspicious = Vec::new();
    let mut allowlisted = 0u32;
    for w in raw {
        // X11 override-redirect alone is how every tooltip, menu, IME
        // candidate list and combo popup is drawn; it is context for a
        // reviewer, not a tell. Capture exclusion and click-through topmost
        // are the tells.
        let flagged = w.excluded_from_capture || w.clickthrough_topmost;
        if !flagged {
            continue;
        }
        if w.pid == own_pid {
            continue;
        }
        if !w.on_screen || w.width < MIN_OVERLAY_SIZE || w.height < MIN_OVERLAY_SIZE {
            continue;
        }
        if is_allowlisted(&w.owner) {
            allowlisted += 1;
            continue;
        }
        let reason = if w.excluded_from_capture {
            "capture_excluded"
        } else if w.override_redirect {
            "override_redirect"
        } else {
            "clickthrough_topmost"
        };
        // Titles can carry user content (and on macOS need Screen Recording
        // to read at all); the reviewer gets owner + reason, nothing more.
        suspicious.push(OverlayWindow {
            pid: w.pid,
            owner: w.owner.clone(),
            title: None,
            width: w.width,
            height: w.height,
            on_screen: w.on_screen,
            reason: reason.to_owned(),
        });
    }
    OverlayScan {
        suspicious,
        allowlisted,
        scanned: raw.len() as u32,
        source: source.to_owned(),
    }
}

/// Unclassified window list, for diagnostics (dev live view, probes).
/// Same `None` semantics as [`scan`].
pub fn raw_windows() -> Option<Vec<RawWindow>> {
    imp::enumerate()
}

/// Scan the current window list, or `None` when the platform offers no
/// probe (mobile, Wayland) or the probe failed.
pub fn scan() -> Option<OverlayScan> {
    let raw = imp::enumerate()?;
    Some(classify(&raw, std::process::id(), imp::SOURCE))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::RawWindow;
    use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFNumber, CFString, CFType};
    use objc2_core_graphics::{
        kCGNullWindowID, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowName,
        kCGWindowOwnerName, kCGWindowOwnerPID, kCGWindowSharingState,
        CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    };
    use std::ffi::c_void;

    pub const SOURCE: &str = "cgwindow";

    /// `kCGWindowSharingNone`.
    const SHARING_NONE: i32 = 0;

    /// Look a key up in an untyped window-info dictionary.
    ///
    /// SAFETY (callers): `dict` must be a live `CFDictionary` from
    /// `CGWindowListCopyWindowInfo`; the returned reference is only valid
    /// while that array is retained, which the caller guarantees by
    /// holding the `CFRetained<CFArray>` for the whole loop.
    unsafe fn lookup<'a>(dict: &'a CFDictionary, key: &CFString) -> Option<&'a CFType> {
        let key_ptr: *const CFString = key;
        let value = dict.value(key_ptr.cast::<c_void>());
        if value.is_null() {
            return None;
        }
        // SAFETY: a non-null value in a CF dictionary is a live CF object.
        Some(unsafe { &*value.cast::<CFType>() })
    }

    unsafe fn number(dict: &CFDictionary, key: &CFString) -> Option<i64> {
        lookup(dict, key)?.downcast_ref::<CFNumber>()?.as_i64()
    }

    unsafe fn string(dict: &CFDictionary, key: &CFString) -> Option<String> {
        Some(lookup(dict, key)?.downcast_ref::<CFString>()?.to_string())
    }

    unsafe fn boolean(dict: &CFDictionary, key: &CFString) -> Option<bool> {
        Some(lookup(dict, key)?.downcast_ref::<CFBoolean>()?.value())
    }

    pub fn enumerate() -> Option<Vec<RawWindow>> {
        let options =
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
        let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID)?;
        let list: &CFArray = &list;
        let count = list.count();
        let mut out = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            // SAFETY: `i` is within `0..count` of a live array we retain
            // for this whole function; every element of a window-info
            // list is a CFDictionary (documented by CoreGraphics). All
            // `lookup`-based reads borrow from that dictionary and end
            // before the next iteration.
            unsafe {
                let ptr = list.value_at_index(i);
                if ptr.is_null() {
                    continue;
                }
                let dict: &CFDictionary = &*ptr.cast::<CFDictionary>();

                let pid = number(dict, kCGWindowOwnerPID).unwrap_or(0).max(0) as u32;
                let owner = string(dict, kCGWindowOwnerName).unwrap_or_default();
                let title = string(dict, kCGWindowName).filter(|t| !t.is_empty());
                let sharing = number(dict, kCGWindowSharingState).unwrap_or(1) as i32;
                let level = number(dict, kCGWindowLayer).unwrap_or(0) as i32;
                let on_screen = boolean(dict, kCGWindowIsOnscreen).unwrap_or(true);

                let (width, height) = match lookup(dict, kCGWindowBounds)
                    .and_then(|b| b.downcast_ref::<CFDictionary>())
                {
                    Some(bounds) => {
                        let mut rect = objc2_core_foundation::CGRect::default();
                        if CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut rect) {
                            (
                                rect.size.width.max(0.0) as u32,
                                rect.size.height.max(0.0) as u32,
                            )
                        } else {
                            (0, 0)
                        }
                    }
                    None => (0, 0),
                };

                out.push(RawWindow {
                    pid,
                    owner,
                    title,
                    width,
                    height,
                    on_screen,
                    excluded_from_capture: sharing == SHARING_NONE,
                    override_redirect: false,
                    clickthrough_topmost: false,
                    level,
                });
            }
        }
        Some(out)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::RawWindow;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, MAX_PATH, RECT};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowDisplayAffinity, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE,
        WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    pub const SOURCE: &str = "win32";

    /// Click-through overlays below this size are ignored unless they are
    /// also capture-excluded; small transparent topmost windows are common
    /// (tooltips, drag images).
    const CLICKTHROUGH_MIN_W: u32 = 200;
    const CLICKTHROUGH_MIN_H: u32 = 100;

    fn process_image(pid: u32) -> Option<String> {
        // SAFETY: mirrors `active_app.rs`. `OpenProcess` is checked before
        // use and its HANDLE closed on every path after acquisition; the
        // buffer is initialised to its full length.
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
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            if !ok {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            let stem = full
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(&full)
                .trim_end_matches(".exe")
                .to_string();
            if stem.is_empty() {
                None
            } else {
                Some(stem)
            }
        }
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `*mut Vec<RawWindow>` we passed to
        // `EnumWindows`, which only invokes this callback synchronously
        // during that call, so the Vec outlives every invocation.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<RawWindow>) };

        // SAFETY: every Win32 call receives a valid HWND handed to us by
        // the enumerator plus initialised out-parameters.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return BOOL(1);
            }
            let width = (rect.right - rect.left).max(0) as u32;
            let height = (rect.bottom - rect.top).max(0) as u32;

            let mut affinity: u32 = 0;
            // Fails for non-layered windows or without DWM: treat as "not
            // excluded", which is the truthful reading of that failure.
            let excluded = GetWindowDisplayAffinity(hwnd, &mut affinity).is_ok()
                && affinity == WDA_EXCLUDEFROMCAPTURE.0;

            let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
            let clickthrough = ex.contains(WS_EX_LAYERED)
                && ex.contains(WS_EX_TRANSPARENT)
                && ex.contains(WS_EX_TOPMOST)
                && (excluded || (width >= CLICKTHROUGH_MIN_W && height >= CLICKTHROUGH_MIN_H));

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));

            let mut title_buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut title_buf);
            let title = if n > 0 {
                Some(String::from_utf16_lossy(&title_buf[..n as usize]))
            } else {
                None
            };

            // Only resolve the image name for windows we might report —
            // OpenProcess per window is the expensive part of this scan.
            // A protected or elevated process refuses OpenProcess; name it
            // "unknown" rather than "" so the report says so and the
            // allowlist cannot be slipped past with an empty owner.
            let owner = if excluded || clickthrough {
                process_image(pid).unwrap_or_else(|| "unknown".to_string())
            } else {
                String::new()
            };

            out.push(RawWindow {
                pid,
                owner,
                title,
                width,
                height,
                on_screen: true,
                excluded_from_capture: excluded,
                override_redirect: false,
                clickthrough_topmost: clickthrough,
                level: if ex.contains(WS_EX_TOPMOST) { 1 } else { 0 },
            });
        }
        BOOL(1)
    }

    pub fn enumerate() -> Option<Vec<RawWindow>> {
        let mut out: Vec<RawWindow> = Vec::new();
        // SAFETY: `visit` only dereferences `lparam` as the `Vec` pointer
        // passed here, and `EnumWindows` returns before `out` drops.
        let ok = unsafe { EnumWindows(Some(visit), LPARAM(&mut out as *mut _ as isize)) };
        ok.ok()?;
        Some(out)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::RawWindow;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState, Window};

    pub const SOURCE: &str = "x11";

    fn string_prop(
        conn: &impl Connection,
        win: Window,
        prop: impl Into<x11rb::protocol::xproto::Atom>,
        ty: impl Into<x11rb::protocol::xproto::Atom>,
    ) -> Option<String> {
        let reply = conn
            .get_property(false, win, prop, ty, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&reply.value).into_owned())
    }

    pub fn enumerate() -> Option<Vec<RawWindow>> {
        // No X11 display (Wayland, headless) → no probe.
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;

        let atom = |name: &[u8]| -> Option<u32> {
            Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
        };
        let net_wm_pid = atom(b"_NET_WM_PID")?;
        let net_wm_name = atom(b"_NET_WM_NAME")?;
        let utf8_string = atom(b"UTF8_STRING")?;

        let tree = conn.query_tree(root).ok()?.reply().ok()?;
        let mut out = Vec::with_capacity(tree.children.len());
        for win in tree.children {
            let Some(attrs) = conn
                .get_window_attributes(win)
                .ok()
                .and_then(|c| c.reply().ok())
            else {
                continue;
            };
            if attrs.map_state != MapState::VIEWABLE {
                continue;
            }
            let Some(geom) = conn.get_geometry(win).ok().and_then(|c| c.reply().ok()) else {
                continue;
            };

            let pid = conn
                .get_property(false, win, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32().and_then(|mut it| it.next()))
                .unwrap_or(0);

            // WM_CLASS = "instance\0class\0"; prefer the class.
            let owner = string_prop(&conn, win, AtomEnum::WM_CLASS, AtomEnum::STRING)
                .map(|raw| {
                    let parts: Vec<&str> = raw.split('\0').filter(|s| !s.is_empty()).collect();
                    parts
                        .get(1)
                        .or(parts.first())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let title = string_prop(&conn, win, net_wm_name, utf8_string)
                .or_else(|| string_prop(&conn, win, AtomEnum::WM_NAME, AtomEnum::STRING))
                .filter(|t| !t.is_empty());

            out.push(RawWindow {
                pid,
                owner,
                title,
                width: geom.width as u32,
                height: geom.height as u32,
                on_screen: true,
                excluded_from_capture: false,
                override_redirect: attrs.override_redirect,
                clickthrough_topmost: false,
                level: 0,
            });
        }
        Some(out)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::RawWindow;
    pub const SOURCE: &str = "none";
    pub fn enumerate() -> Option<Vec<RawWindow>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(pid: u32, owner: &str, w: u32, h: u32) -> RawWindow {
        RawWindow {
            pid,
            owner: owner.to_owned(),
            title: Some("t".to_owned()),
            width: w,
            height: h,
            on_screen: true,
            excluded_from_capture: true,
            override_redirect: false,
            clickthrough_topmost: false,
            level: 0,
        }
    }

    #[test]
    fn allowlist_matches_whole_owner_case_insensitively() {
        assert!(is_allowlisted("com.1password.1password"));
        assert!(is_allowlisted("1Password"));
        assert!(is_allowlisted("1Password.exe"));
        assert!(is_allowlisted("org.keepassxc.KeePassXC"));
        assert!(is_allowlisted("Bitwarden"));
        assert!(is_allowlisted("com.apple.controlcenter"));
        assert!(is_allowlisted("ShellExperienceHost"));
        assert!(is_allowlisted("Dock"));
        assert!(is_allowlisted("Control Center"));
        assert!(!is_allowlisted("Docker Desktop"));
        assert!(!is_allowlisted("Keeper Overlay Tool"));
        // An overlay picks its own name; a substring of a trusted one buys nothing.
        assert!(!is_allowlisted("searchhost-ai"));
        assert!(!is_allowlisted("1password-helper"));
        assert!(!is_allowlisted("com.1password.1password.overlay"));
        assert!(!is_allowlisted("Cluely"));
        assert!(!is_allowlisted("us.zoom.xos"));
        assert!(!is_allowlisted(""));
    }

    #[test]
    fn own_pid_is_skipped() {
        let scan = classify(&[win(42, "cluely", 400, 300)], 42, "test");
        assert!(scan.suspicious.is_empty());
        assert_eq!(scan.scanned, 1);
        assert_eq!(scan.allowlisted, 0);
    }

    #[test]
    fn allowlisted_owner_is_counted_not_reported() {
        let scan = classify(&[win(7, "com.1password.1password", 400, 300)], 1, "test");
        assert!(scan.suspicious.is_empty());
        assert_eq!(scan.allowlisted, 1);
    }

    #[test]
    fn tiny_and_offscreen_windows_are_skipped() {
        let mut off = win(7, "cluely", 400, 300);
        off.on_screen = false;
        let scan = classify(
            &[win(7, "cluely", 23, 300), win(7, "cluely", 300, 10), off],
            1,
            "test",
        );
        assert!(scan.suspicious.is_empty());
        assert_eq!(scan.scanned, 3);
    }

    #[test]
    fn excluded_window_is_reported_with_reason() {
        let scan = classify(&[win(7, "Cluely", 640, 220)], 1, "cgwindow");
        assert_eq!(scan.suspicious.len(), 1);
        let s = &scan.suspicious[0];
        assert_eq!(s.reason, "capture_excluded");
        assert_eq!(s.owner, "Cluely");
        assert_eq!(s.pid, 7);
        assert_eq!((s.width, s.height), (640, 220));
        assert_eq!(scan.source, "cgwindow");
    }

    #[test]
    fn override_redirect_alone_is_not_reported() {
        // Tooltips, menus and IME popups are all override-redirect.
        let mut w = win(9, "some-overlay", 300, 200);
        w.excluded_from_capture = false;
        w.override_redirect = true;
        let scan = classify(&[w], 1, "x11");
        assert!(scan.suspicious.is_empty());
        assert_eq!(scan.scanned, 1);
    }

    #[test]
    fn titles_never_cross_ipc() {
        let scan = classify(&[win(7, "Cluely", 640, 220)], 1, "cgwindow");
        assert_eq!(scan.suspicious[0].title, None);
    }

    #[test]
    fn clickthrough_topmost_is_reported_and_excluded_wins() {
        let mut both = win(9, "overlay", 300, 200);
        both.clickthrough_topmost = true;
        let mut only = win(10, "overlay2", 300, 200);
        only.excluded_from_capture = false;
        only.clickthrough_topmost = true;
        let scan = classify(&[both, only], 1, "win32");
        assert_eq!(scan.suspicious[0].reason, "capture_excluded");
        assert_eq!(scan.suspicious[1].reason, "clickthrough_topmost");
    }

    #[test]
    fn ordinary_windows_are_not_reported() {
        let mut plain = win(9, "com.google.Chrome", 1200, 800);
        plain.excluded_from_capture = false;
        let scan = classify(&[plain], 1, "test");
        assert!(scan.suspicious.is_empty());
        assert_eq!(scan.scanned, 1);
    }

    #[test]
    fn serializes_with_snake_case_fields() {
        let scan = classify(&[win(7, "Cluely", 640, 220)], 1, "cgwindow");
        let v = serde_json::to_value(&scan).unwrap();
        assert_eq!(v["scanned"], 1);
        assert_eq!(v["allowlisted"], 0);
        assert_eq!(v["source"], "cgwindow");
        assert_eq!(v["suspicious"][0]["reason"], "capture_excluded");
        assert_eq!(v["suspicious"][0]["on_screen"], true);
        assert!(v["suspicious"][0]["title"].is_null());
    }
}
