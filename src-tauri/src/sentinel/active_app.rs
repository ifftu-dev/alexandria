//! Frontmost-application detection (Sentinel, native).
//!
//! When the Alexandria window loses focus during an assessment, this
//! reports which OS application took the foreground — a signal the
//! webview cannot see. Best-effort per platform; always returns `None`
//! rather than failing, so a missing capability never breaks monitoring.
//!
//! - macOS  — `NSWorkspace.frontmostApplication` (name + bundle id).
//! - Windows — `GetForegroundWindow` → process image file stem.
//! - Linux (X11) — `_NET_ACTIVE_WINDOW` → `WM_CLASS`. Wayland → `None`.

use serde::Serialize;

/// The currently-frontmost application, as seen by the OS.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveApp {
    /// Human-readable application name (e.g. "Google Chrome").
    pub name: String,
    /// Stable identifier where available — bundle id (macOS), process
    /// image name (Windows), or WM_CLASS (Linux). May equal `name`.
    pub identifier: String,
}

/// Resolve the frontmost application, or `None` if it can't be
/// determined (unsupported platform, Wayland, permission, or our own
/// window).
pub fn frontmost_app() -> Option<ActiveApp> {
    imp::frontmost_app()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ActiveApp;

    pub fn frontmost_app() -> Option<ActiveApp> {
        use objc2_app_kit::NSWorkspace;
        // NSWorkspace + NSRunningApplication are main-thread-affine for
        // some members, but frontmostApplication / localizedName /
        // bundleIdentifier are safe to read off-main in practice; we
        // keep the access minimal and within an autorelease scope.
        objc2::rc::autoreleasepool(|_| {
            let ws = NSWorkspace::sharedWorkspace();
            let app = ws.frontmostApplication()?;
            let name = app
                .localizedName()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let identifier = app
                .bundleIdentifier()
                .map(|s| s.to_string())
                .unwrap_or_else(|| name.clone());
            if name.is_empty() && identifier.is_empty() {
                return None;
            }
            Some(ActiveApp { name, identifier })
        })
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::ActiveApp;

    pub fn frontmost_app() -> Option<ActiveApp> {
        use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return None;
            }
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
            if !ok {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            // File stem as the friendly name (e.g. "chrome").
            let name = full
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(&full)
                .trim_end_matches(".exe")
                .to_string();
            if name.is_empty() {
                return None;
            }
            Some(ActiveApp {
                name,
                identifier: full,
            })
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ActiveApp;

    pub fn frontmost_app() -> Option<ActiveApp> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        // No X11 display (e.g. Wayland) → can't resolve.
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;

        let active_atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let reply = conn
            .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let win = reply.value32().and_then(|mut it| it.next())?;
        if win == 0 {
            return None;
        }

        // WM_CLASS = "instance\0class\0".
        let class = conn
            .get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        let raw = class.value;
        let parts: Vec<&[u8]> = raw.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
        let instance = parts
            .first()
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default();
        let class_name = parts
            .get(1)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_else(|| instance.clone());
        if class_name.is_empty() {
            return None;
        }
        Some(ActiveApp {
            name: class_name.clone(),
            identifier: if instance.is_empty() {
                class_name
            } else {
                instance
            },
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::ActiveApp;
    pub fn frontmost_app() -> Option<ActiveApp> {
        None
    }
}
