//! Global modifier-combo monitor (Sentinel, native desktop).
//!
//! Overlay cheat tools are driven by global hotkeys (Cmd+B, Cmd+\,
//! Cmd+Enter, …) that the OS delivers to the overlay and never to our
//! webview. This module runs a *listen-only* system-wide keyboard monitor
//! and records which modifier combos were pressed. The frontend subtracts
//! the combos the webview itself received (and the ones the OS consumes,
//! see [`OS_COMBOS`]) and treats the remainder as "phantom" hotkeys.
//!
//! # Privacy — hard constraints
//!
//! * Only key-down events where **Cmd/Win/Super, Ctrl, or Alt/Option** is
//!   held are recorded. Plain characters and Shift-only combos are dropped
//!   inside the OS callback before anything is stored.
//! * Key-up is never recorded.
//! * What is stored is the normalised combo string (e.g. `cmd+b`,
//!   `ctrl+alt+p`) and a monotonic millisecond timestamp. Nothing else.
//! * The ring buffer is capped at [`RING_CAP`] entries and is cleared on
//!   every [`drain`]. Nothing is persisted here.
//!
//! The monitor is opt-in per platform permission and best-effort: when the
//! probe is unsupported (mobile, Wayland) or lacks permission, [`start`]
//! returns `Err` and nothing runs.
//!
//! # Vocabulary
//!
//! Windows' Win key and Linux's Super key are both reported as `cmd` so
//! the frontend has one vocabulary across platforms. Modifiers are ordered
//! `cmd, ctrl, alt, shift`; keys are lowercase; named keys follow
//! [`normalise_combo`].
//!
//! # Platforms
//!
//! * macOS — `CGEventTapCreate` (session tap, listen-only) on a dedicated
//!   thread's `CFRunLoop`. Requires Accessibility permission
//!   (`AXIsProcessTrusted`).
//! * Windows — `SetWindowsHookExW(WH_KEYBOARD_LL)` with a message loop on
//!   a dedicated thread. No permission prompt.
//! * Linux X11 — XInput2 raw key events on the root window. No permission
//!   prompt. Wayland: unsupported (compositor isolates clients).
//! * iOS / Android — unsupported.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// One recorded modifier combo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyEvent {
    /// Milliseconds since the monitor started (monotonic).
    pub at_ms: u64,
    /// Normalised combo, e.g. `cmd+shift+space`.
    pub combo: String,
}

/// What the monitor can do on this platform, right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyStatus {
    /// A probe exists for this platform / display server.
    pub supported: bool,
    /// The OS lets us listen (macOS Accessibility; always true elsewhere
    /// when supported).
    pub permission_granted: bool,
    /// The monitor thread is active.
    pub running: bool,
    /// `cgeventtap` | `wh_keyboard_ll` | `xinput2` | `unsupported`.
    pub source: String,
}

/// Modifier state for [`normalise_combo`]. `cmd` covers Command, Win and
/// Super.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    /// A combo is worth recording only when a non-Shift modifier is held.
    pub fn is_recordable(self) -> bool {
        self.cmd || self.ctrl || self.alt
    }
}

/// Ring-buffer capacity; the oldest event is dropped past this.
pub const RING_CAP: usize = 256;

/// Combos the OS itself consumes. These never reach any app, so their
/// absence from the webview proves nothing. Win/Super combos are written
/// with the `cmd` prefix (see module doc). Entries are exact matches;
/// Windows/Linux `win+*` / `super+*` are handled by the prefix rule in
/// [`is_os_combo`] because the OS claims the whole family.
pub const OS_COMBOS: &[&str] = &[
    // macOS
    "cmd+tab",
    "cmd+shift+tab",
    "cmd+space",
    "cmd+shift+space",
    "cmd+alt+space",
    "cmd+shift+3",
    "cmd+shift+4",
    "cmd+shift+5",
    "cmd+shift+6",
    "cmd+ctrl+shift+3",
    "cmd+ctrl+shift+4",
    "cmd+q",
    "cmd+h",
    "cmd+alt+h",
    "cmd+m",
    "cmd+grave",
    "cmd+ctrl+q",
    "cmd+alt+escape",
    "cmd+ctrl+space",
    "cmd+ctrl+f",
    "cmd+ctrl+d",
    "ctrl+up",
    "ctrl+down",
    "ctrl+left",
    "ctrl+right",
    "cmd+alt+d",
    // Windows (win → cmd)
    "alt+tab",
    "alt+shift+tab",
    "ctrl+alt+tab",
    "ctrl+alt+delete",
    "ctrl+shift+escape",
    "alt+f4",
    "alt+escape",
    // Linux (super → cmd)
    "ctrl+alt+t",
    "ctrl+alt+l",
    "ctrl+alt+up",
    "ctrl+alt+down",
    "ctrl+alt+left",
    "ctrl+alt+right",
    "ctrl+alt+f1",
    "ctrl+alt+f2",
    "ctrl+alt+f3",
    "ctrl+alt+f4",
    "ctrl+alt+f5",
    "ctrl+alt+f6",
    "ctrl+alt+f7",
    "ctrl+alt+f8",
    "ctrl+alt+f9",
    "ctrl+alt+f10",
    "ctrl+alt+f11",
    "ctrl+alt+f12",
    "ctrl+alt+backspace",
];

/// Build the canonical combo string. Modifiers in `cmd, ctrl, alt, shift`
/// order; the key lowercased; a few aliases folded to the canonical named
/// keys (`return` → `enter`, `esc` → `escape`, `` ` `` → `grave`, …).
pub fn normalise_combo(mods: Modifiers, key: &str) -> String {
    let mut out = String::with_capacity(24);
    if mods.cmd {
        out.push_str("cmd+");
    }
    if mods.ctrl {
        out.push_str("ctrl+");
    }
    if mods.alt {
        out.push_str("alt+");
    }
    if mods.shift {
        out.push_str("shift+");
    }
    out.push_str(&canonical_key(key));
    out
}

/// Fold key-name aliases to the canonical set.
fn canonical_key(key: &str) -> String {
    let k = key.trim().to_ascii_lowercase();
    match k.as_str() {
        "return" | "kp_enter" | "numpadenter" => "enter".into(),
        "esc" => "escape".into(),
        " " | "spacebar" => "space".into(),
        "`" | "~" | "backquote" => "grave".into(),
        "\\" | "backslash" => "backslash".into(),
        "/" => "slash".into(),
        "," => "comma".into(),
        "." => "period".into(),
        ";" => "semicolon".into(),
        "'" => "quote".into(),
        "[" => "bracketleft".into(),
        "]" => "bracketright".into(),
        "-" => "minus".into(),
        "=" => "equal".into(),
        "arrowup" => "up".into(),
        "arrowdown" => "down".into(),
        "arrowleft" => "left".into(),
        "arrowright" => "right".into(),
        "del" => "delete".into(),
        other => other.to_string(),
    }
}

/// True when the OS consumes this combo itself: an exact [`OS_COMBOS`]
/// match, or a `cmd+` combo on Windows / Linux where the Win / Super key is
/// system-owned (any `cmd+…` on those platforms). On macOS `cmd+…` reaches
/// applications, so only exact matches count there.
pub fn is_os_combo(combo: &str) -> bool {
    is_os_combo_for(combo, cfg!(any(target_os = "windows", target_os = "linux")))
}

/// Platform-independent core of [`is_os_combo`]; `cmd_is_system` is true
/// on Windows and Linux.
pub fn is_os_combo_for(combo: &str, cmd_is_system: bool) -> bool {
    if OS_COMBOS.contains(&combo) {
        return true;
    }
    cmd_is_system && combo.starts_with("cmd+")
}

// ── Shared state ─────────────────────────────────────────────────────

struct Shared {
    ring: Mutex<VecDeque<HotkeyEvent>>,
    running: AtomicBool,
    origin: Mutex<Option<Instant>>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
        running: AtomicBool::new(false),
        origin: Mutex::new(None),
    })
}

/// Push with drop-oldest semantics. Pure, so the cap is unit-testable.
pub fn push_event(ring: &mut VecDeque<HotkeyEvent>, ev: HotkeyEvent) {
    if ring.len() >= RING_CAP {
        ring.pop_front();
    }
    ring.push_back(ev);
}

/// Called by every platform callback. Applies the privacy gate: only
/// recordable modifier combos get in.
fn record(mods: Modifiers, key: &str) {
    if !mods.is_recordable() {
        return;
    }
    let s = shared();
    let at_ms = s
        .origin
        .lock()
        .ok()
        .and_then(|o| o.map(|t| t.elapsed().as_millis() as u64))
        .unwrap_or(0);
    if let Ok(mut ring) = s.ring.lock() {
        push_event(
            &mut ring,
            HotkeyEvent {
                at_ms,
                combo: normalise_combo(mods, key),
            },
        );
    }
}

/// Return-and-clear the recorded combos.
pub fn drain() -> Vec<HotkeyEvent> {
    match shared().ring.lock() {
        Ok(mut ring) => ring.drain(..).collect(),
        Err(_) => Vec::new(),
    }
}

/// Current capability + permission + running state.
pub fn status() -> HotkeyStatus {
    let mut st = imp::status();
    st.running = shared().running.load(Ordering::SeqCst);
    st
}

/// Ask the OS for permission where that is a thing (macOS Accessibility
/// prompt). Elsewhere a no-op that returns [`status`].
pub fn request_permission() -> HotkeyStatus {
    imp::request_permission();
    status()
}

/// The permission gate, factored out so it is unit-testable.
pub fn gate(st: &HotkeyStatus) -> Result<(), String> {
    if !st.supported {
        return Err(format!(
            "global hotkey monitor unsupported here ({})",
            st.source
        ));
    }
    if !st.permission_granted {
        return Err("global hotkey monitor needs input-monitoring permission".into());
    }
    Ok(())
}

/// Start the monitor thread. Idempotent.
pub fn start() -> Result<HotkeyStatus, String> {
    let s = shared();
    if s.running.load(Ordering::SeqCst) {
        return Ok(status());
    }
    let st = status();
    gate(&st)?;
    if let Ok(mut o) = s.origin.lock() {
        *o = Some(Instant::now());
    }
    if let Ok(mut ring) = s.ring.lock() {
        ring.clear();
    }
    imp::start()?;
    s.running.store(true, Ordering::SeqCst);
    Ok(status())
}

/// Stop the monitor thread. Idempotent.
pub fn stop() {
    let s = shared();
    if !s.running.swap(false, Ordering::SeqCst) {
        return;
    }
    imp::stop();
}

// ── macOS: CGEventTap ────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod imp {
    use super::{record, HotkeyStatus, Modifiers};
    use std::ffi::c_void;
    use std::sync::Mutex;

    pub const SOURCE: &str = "cgeventtap";

    type CFTypeRef = *const c_void;
    type CFMachPortRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type CFRunLoopSourceRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;
    type CGEventRef = *const c_void;
    type CGEventTapProxy = *const c_void;
    type CGEventType = u32;
    type CGEventMask = u64;
    type CGEventFlags = u64;
    type CGEventField = u32;

    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
    const K_CG_EVENT_KEY_DOWN: CGEventType = 10;
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: CGEventType = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: CGEventType = 0xFFFF_FFFF;
    const K_CG_KEYBOARD_EVENT_KEYCODE: CGEventField = 9;
    const FLAG_SHIFT: CGEventFlags = 0x0002_0000;
    const FLAG_CONTROL: CGEventFlags = 0x0004_0000;
    const FLAG_ALTERNATE: CGEventFlags = 0x0008_0000;
    const FLAG_COMMAND: CGEventFlags = 0x0010_0000;

    type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: CGEventTapProxy,
        kind: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: CGEventField) -> i64;
        /// Input Monitoring (TCC "ListenEvent"); 10.15+. Key-down taps stay
        /// silent without either this or Accessibility.
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        fn CFRunLoopStop(rl: CFRunLoopRef);
        fn CFRelease(cf: CFTypeRef);
        fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
        static kCFRunLoopCommonModes: CFStringRef;
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    /// Run loop of the monitor thread, so `stop()` can halt it. The raw
    /// pointer is only ever dereferenced by CoreFoundation.
    struct LoopHandle(CFRunLoopRef);
    // SAFETY: CFRunLoopRef is a thread-safe CF object handle; CFRunLoopStop
    // is documented safe to call from any thread.
    unsafe impl Send for LoopHandle {}

    static LOOP: Mutex<Option<LoopHandle>> = Mutex::new(None);
    static TAP: Mutex<Option<TapHandle>> = Mutex::new(None);

    struct TapHandle(CFMachPortRef);
    // SAFETY: the mach port is only used from the monitor thread and the
    // callback that thread runs; the handle is stored so the callback can
    // re-enable the tap after a timeout.
    unsafe impl Send for TapHandle {}

    pub fn status() -> HotkeyStatus {
        // SAFETY: plain queries with no arguments. Either Accessibility or
        // Input Monitoring lets a listen-only key tap receive events.
        let granted = unsafe { AXIsProcessTrusted() || CGPreflightListenEventAccess() };
        HotkeyStatus {
            supported: true,
            permission_granted: granted,
            running: false,
            source: SOURCE.into(),
        }
    }

    pub fn request_permission() {
        // SAFETY: builds a one-entry CFDictionary {kAXTrustedCheckOptionPrompt:
        // true} with the standard CF type callbacks, hands it to
        // AXIsProcessTrustedWithOptions, and releases it afterwards.
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            if !dict.is_null() {
                let _ = AXIsProcessTrustedWithOptions(dict);
                CFRelease(dict);
            }
            // Also surface the Input Monitoring prompt; harmless if already
            // granted or if Accessibility alone suffices.
            let _ = CGRequestListenEventAccess();
        }
    }

    unsafe extern "C" fn callback(
        _proxy: CGEventTapProxy,
        kind: CGEventType,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        if kind == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || kind == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            if let Ok(tap) = TAP.lock() {
                if let Some(t) = tap.as_ref() {
                    // SAFETY: `t.0` is the live tap this callback belongs to.
                    unsafe { CGEventTapEnable(t.0, true) };
                }
            }
            return event;
        }
        if kind != K_CG_EVENT_KEY_DOWN {
            return event;
        }
        // SAFETY: `event` is the live CGEvent CoreGraphics handed us for the
        // duration of this callback.
        let (flags, keycode) = unsafe {
            (
                CGEventGetFlags(event),
                CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE),
            )
        };
        let mods = Modifiers {
            cmd: flags & FLAG_COMMAND != 0,
            ctrl: flags & FLAG_CONTROL != 0,
            alt: flags & FLAG_ALTERNATE != 0,
            shift: flags & FLAG_SHIFT != 0,
        };
        if mods.is_recordable() {
            if let Some(name) = super::mac_keycode_name(keycode as u16) {
                record(mods, name);
            }
        }
        event
    }

    pub fn start() -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("sentinel-hotkeys".into())
            .spawn(move || {
                // SAFETY: creates a listen-only session tap for key-down,
                // attaches its mach-port source to this thread's run loop and
                // runs the loop until CFRunLoopStop. Every CF object created
                // here is released when the loop ends.
                unsafe {
                    let mask: CGEventMask = 1u64 << K_CG_EVENT_KEY_DOWN;
                    let tap = CGEventTapCreate(
                        K_CG_SESSION_EVENT_TAP,
                        K_CG_HEAD_INSERT_EVENT_TAP,
                        K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                        mask,
                        callback,
                        std::ptr::null_mut(),
                    );
                    if tap.is_null() {
                        let _ = tx.send(Err("CGEventTapCreate returned null (permission?)".into()));
                        return;
                    }
                    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                    if source.is_null() {
                        CFRelease(tap);
                        let _ = tx.send(Err("CFMachPortCreateRunLoopSource failed".into()));
                        return;
                    }
                    let rl = CFRunLoopGetCurrent();
                    CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
                    CGEventTapEnable(tap, true);
                    if let Ok(mut l) = LOOP.lock() {
                        *l = Some(LoopHandle(rl));
                    }
                    if let Ok(mut t) = TAP.lock() {
                        *t = Some(TapHandle(tap));
                    }
                    let _ = tx.send(Ok(()));
                    CFRunLoopRun();
                    CGEventTapEnable(tap, false);
                    if let Ok(mut t) = TAP.lock() {
                        *t = None;
                    }
                    if let Ok(mut l) = LOOP.lock() {
                        *l = None;
                    }
                    CFRelease(source);
                    CFRelease(tap);
                }
            })
            .map_err(|e| format!("spawn hotkey monitor: {e}"))?;
        rx.recv()
            .map_err(|_| "hotkey monitor thread exited before reporting".to_string())?
    }

    pub fn stop() {
        if let Ok(l) = LOOP.lock() {
            if let Some(h) = l.as_ref() {
                // SAFETY: stopping a run loop from another thread is supported.
                unsafe { CFRunLoopStop(h.0) };
            }
        }
    }
}

/// ANSI-layout virtual keycode → canonical key name (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mac_keycode_name(code: u16) -> Option<&'static str> {
    MAC_KEYCODES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| *n)
}

/// macOS ANSI virtual keycodes (`Carbon/Events.h` kVK_*).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MAC_KEYCODES: &[(u16, &str)] = &[
    (0x00, "a"),
    (0x01, "s"),
    (0x02, "d"),
    (0x03, "f"),
    (0x04, "h"),
    (0x05, "g"),
    (0x06, "z"),
    (0x07, "x"),
    (0x08, "c"),
    (0x09, "v"),
    (0x0B, "b"),
    (0x0C, "q"),
    (0x0D, "w"),
    (0x0E, "e"),
    (0x0F, "r"),
    (0x10, "y"),
    (0x11, "t"),
    (0x12, "1"),
    (0x13, "2"),
    (0x14, "3"),
    (0x15, "4"),
    (0x16, "6"),
    (0x17, "5"),
    (0x18, "equal"),
    (0x19, "9"),
    (0x1A, "7"),
    (0x1B, "minus"),
    (0x1C, "8"),
    (0x1D, "0"),
    (0x1E, "bracketright"),
    (0x1F, "o"),
    (0x20, "u"),
    (0x21, "bracketleft"),
    (0x22, "i"),
    (0x23, "p"),
    (0x24, "enter"),
    (0x25, "l"),
    (0x26, "j"),
    (0x27, "quote"),
    (0x28, "k"),
    (0x29, "semicolon"),
    (0x2A, "backslash"),
    (0x2B, "comma"),
    (0x2C, "slash"),
    (0x2D, "n"),
    (0x2E, "m"),
    (0x2F, "period"),
    (0x30, "tab"),
    (0x31, "space"),
    (0x32, "grave"),
    (0x33, "backspace"),
    (0x35, "escape"),
    (0x60, "f5"),
    (0x61, "f6"),
    (0x62, "f7"),
    (0x63, "f3"),
    (0x64, "f8"),
    (0x65, "f9"),
    (0x67, "f11"),
    (0x69, "f13"),
    (0x6A, "f16"),
    (0x6B, "f14"),
    (0x6D, "f10"),
    (0x6F, "f12"),
    (0x71, "f15"),
    (0x72, "help"),
    (0x73, "home"),
    (0x74, "pageup"),
    (0x75, "delete"),
    (0x76, "f4"),
    (0x77, "end"),
    (0x78, "f2"),
    (0x79, "pagedown"),
    (0x7A, "f1"),
    (0x7B, "left"),
    (0x7C, "right"),
    (0x7D, "down"),
    (0x7E, "up"),
];

// ── Windows: WH_KEYBOARD_LL ──────────────────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use super::{record, HotkeyStatus, Modifiers};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_QUIT, WM_SYSKEYDOWN,
    };

    pub const SOURCE: &str = "wh_keyboard_ll";

    static THREAD_ID: AtomicU32 = AtomicU32::new(0);
    struct Hook(HHOOK);
    // SAFETY: HHOOK is an opaque handle; it is only unhooked from the
    // monitor thread that installed it.
    unsafe impl Send for Hook {}
    static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

    pub fn status() -> HotkeyStatus {
        HotkeyStatus {
            supported: true,
            permission_granted: true,
            running: false,
            source: SOURCE.into(),
        }
    }

    pub fn request_permission() {}

    fn down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
        // SAFETY: GetAsyncKeyState has no preconditions.
        unsafe { (GetAsyncKeyState(vk.0 as i32) as u16) & 0x8000 != 0 }
    }

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
            // SAFETY: for WH_KEYBOARD_LL, lparam points at a KBDLLHOOKSTRUCT
            // owned by the system for the duration of this call.
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let mods = Modifiers {
                cmd: down(VK_LWIN) || down(VK_RWIN),
                ctrl: down(VK_CONTROL),
                alt: down(VK_MENU),
                shift: down(VK_SHIFT),
            };
            if mods.is_recordable() {
                if let Some(name) = super::win_vk_name(kb.vkCode as u16) {
                    record(mods, name);
                }
            }
        }
        // SAFETY: forwarding the hook chain with the arguments we received.
        unsafe { CallNextHookEx(HHOOK::default(), code, wparam, lparam) }
    }

    pub fn start() -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("sentinel-hotkeys".into())
            .spawn(move || {
                // SAFETY: installs a low-level keyboard hook owned by this
                // thread and pumps its message queue until WM_QUIT; the hook
                // is removed before the thread exits.
                unsafe {
                    THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
                    let hook = match SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(proc),
                        HINSTANCE::default(),
                        0,
                    ) {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = tx.send(Err(format!("SetWindowsHookExW: {e}")));
                            return;
                        }
                    };
                    if let Ok(mut h) = HOOK.lock() {
                        *h = Some(Hook(hook));
                    }
                    // A thread has no message queue until it first asks for
                    // one, and `stop()` targets this queue with WM_QUIT. Create
                    // it before reporting ready so a stop() that follows
                    // start() immediately cannot post into the void and leave
                    // the hook installed.
                    let mut msg = MSG::default();
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let _ = tx.send(Ok(()));
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        if msg.message == WM_QUIT {
                            break;
                        }
                    }
                    let _ = UnhookWindowsHookEx(hook);
                    if let Ok(mut h) = HOOK.lock() {
                        *h = None;
                    }
                    THREAD_ID.store(0, Ordering::SeqCst);
                }
            })
            .map_err(|e| format!("spawn hotkey monitor: {e}"))?;
        rx.recv()
            .map_err(|_| "hotkey monitor thread exited before reporting".to_string())?
    }

    pub fn stop() {
        let tid = THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            // SAFETY: posting WM_QUIT to our own monitor thread's queue.
            let _ = unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }
}

/// Windows virtual-key code → canonical key name.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn win_vk_name(vk: u16) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FKEYS: [&str; 24] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14",
        "f15", "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23", "f24",
    ];
    Some(match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x70..=0x87 => FKEYS[(vk - 0x70) as usize],
        0x08 => "backspace",
        0x09 => "tab",
        0x0D => "enter",
        0x1B => "escape",
        0x20 => "space",
        0x21 => "pageup",
        0x22 => "pagedown",
        0x23 => "end",
        0x24 => "home",
        0x25 => "left",
        0x26 => "up",
        0x27 => "right",
        0x28 => "down",
        0x2E => "delete",
        0xBA => "semicolon",
        0xBB => "equal",
        0xBC => "comma",
        0xBD => "minus",
        0xBE => "period",
        0xBF => "slash",
        0xC0 => "grave",
        0xDB => "bracketleft",
        0xDC => "backslash",
        0xDD => "bracketright",
        0xDE => "quote",
        _ => return None,
    })
}

// ── Linux: XInput2 raw key events ────────────────────────────────────

#[cfg(target_os = "linux")]
mod imp {
    use super::{record, HotkeyStatus, Modifiers};
    use std::sync::atomic::{AtomicBool, Ordering};

    pub const SOURCE: &str = "xinput2";

    static STOP: AtomicBool = AtomicBool::new(false);

    fn has_x11() -> bool {
        std::env::var_os("DISPLAY").is_some()
    }

    pub fn status() -> HotkeyStatus {
        let supported = has_x11();
        HotkeyStatus {
            supported,
            permission_granted: supported,
            running: false,
            source: if supported { SOURCE } else { "unsupported" }.into(),
        }
    }

    pub fn request_permission() {}

    pub fn start() -> Result<(), String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xinput::{self, ConnectionExt as _, EventMask, XIEventMask};
        use x11rb::protocol::xproto::ConnectionExt as _;
        use x11rb::protocol::Event;

        STOP.store(false, Ordering::SeqCst);
        let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("X11 connect: {e}"))?;
        let root = conn
            .setup()
            .roots
            .get(screen_num)
            .ok_or("no X11 screen")?
            .root;
        let min_kc = conn.setup().min_keycode;
        let max_kc = conn.setup().max_keycode;
        conn.xinput_xi_query_version(2, 0)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| format!("XInput2 unavailable: {e}"))?;
        let mapping = conn
            .get_keyboard_mapping(min_kc, max_kc.saturating_sub(min_kc).saturating_add(1))
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        conn.xinput_xi_select_events(
            root,
            &[EventMask {
                deviceid: xinput::Device::ALL_MASTER.into(),
                mask: vec![XIEventMask::RAW_KEY_PRESS | XIEventMask::RAW_KEY_RELEASE],
            }],
        )
        .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;

        std::thread::Builder::new()
            .name("sentinel-hotkeys".into())
            .spawn(move || {
                let mut mods = Modifiers::default();
                let per = mapping.keysyms_per_keycode.max(1) as usize;
                let keysym_of = |kc: u32| -> Option<u32> {
                    let idx = (kc as usize).checked_sub(min_kc as usize)? * per;
                    mapping.keysyms.get(idx).copied()
                };
                loop {
                    if STOP.load(Ordering::SeqCst) {
                        break;
                    }
                    let ev = match conn.poll_for_event() {
                        Ok(Some(ev)) => ev,
                        Ok(None) => {
                            std::thread::sleep(std::time::Duration::from_millis(15));
                            continue;
                        }
                        Err(_) => break,
                    };
                    match ev {
                        Event::XinputRawKeyPress(e) => {
                            let Some(sym) = keysym_of(e.detail) else {
                                continue;
                            };
                            if super::x11_apply_modifier(sym, true, &mut mods) {
                                continue;
                            }
                            if mods.is_recordable() {
                                if let Some(name) = super::x11_keysym_name(sym) {
                                    record(mods, name);
                                }
                            }
                        }
                        Event::XinputRawKeyRelease(e) => {
                            if let Some(sym) = keysym_of(e.detail) {
                                super::x11_apply_modifier(sym, false, &mut mods);
                            }
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| format!("spawn hotkey monitor: {e}"))?;
        Ok(())
    }

    pub fn stop() {
        STOP.store(true, Ordering::SeqCst);
    }
}

/// Track modifier state from raw X11 keysyms. Returns true when the keysym
/// *was* a modifier (so the caller does not record it as a key).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn x11_apply_modifier(keysym: u32, pressed: bool, mods: &mut Modifiers) -> bool {
    match keysym {
        0xFFE3 | 0xFFE4 => mods.ctrl = pressed,         // Control_L/R
        0xFFE9 | 0xFFEA | 0xFF7E => mods.alt = pressed, // Alt_L/R, Mode_switch
        0xFFEB | 0xFFEC | 0xFFE7 | 0xFFE8 => mods.cmd = pressed, // Super_L/R, Meta_L/R
        0xFFE1 | 0xFFE2 => mods.shift = pressed,        // Shift_L/R
        _ => return false,
    }
    true
}

/// X11 keysym → canonical key name (Latin-1 printable + common named keys).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn x11_keysym_name(keysym: u32) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FKEYS: [&str; 24] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14",
        "f15", "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23", "f24",
    ];
    Some(match keysym {
        0x61..=0x7A => LETTERS[(keysym - 0x61) as usize],
        0x41..=0x5A => LETTERS[(keysym - 0x41) as usize],
        0x30..=0x39 => DIGITS[(keysym - 0x30) as usize],
        0xFFBE..=0xFFD5 => FKEYS[(keysym - 0xFFBE) as usize],
        0x20 => "space",
        0xFF0D | 0xFF8D => "enter",
        0xFF09 => "tab",
        0xFF1B => "escape",
        0xFF08 => "backspace",
        0xFFFF => "delete",
        0xFF52 => "up",
        0xFF54 => "down",
        0xFF51 => "left",
        0xFF53 => "right",
        0xFF50 => "home",
        0xFF57 => "end",
        0xFF55 => "pageup",
        0xFF56 => "pagedown",
        0x5C => "backslash",
        0x2F => "slash",
        0x2C => "comma",
        0x2E => "period",
        0x3B => "semicolon",
        0x27 => "quote",
        0x5B => "bracketleft",
        0x5D => "bracketright",
        0x2D => "minus",
        0x3D => "equal",
        0x60 => "grave",
        _ => return None,
    })
}

// ── Unsupported platforms ────────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::HotkeyStatus;

    pub fn status() -> HotkeyStatus {
        HotkeyStatus {
            supported: false,
            permission_granted: false,
            running: false,
            source: "unsupported".into(),
        }
    }
    pub fn request_permission() {}
    pub fn start() -> Result<(), String> {
        Err("global hotkey monitor unsupported on this platform".into())
    }
    pub fn stop() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(cmd: bool, ctrl: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            cmd,
            ctrl,
            alt,
            shift,
        }
    }

    #[test]
    fn normalise_orders_modifiers_and_lowercases() {
        assert_eq!(
            normalise_combo(m(true, true, true, true), "B"),
            "cmd+ctrl+alt+shift+b"
        );
        assert_eq!(normalise_combo(m(false, true, false, false), "P"), "ctrl+p");
        assert_eq!(
            normalise_combo(m(true, false, false, true), "Space"),
            "cmd+shift+space"
        );
    }

    #[test]
    fn normalise_folds_named_key_aliases() {
        assert_eq!(
            normalise_combo(m(true, false, false, false), "Return"),
            "cmd+enter"
        );
        assert_eq!(
            normalise_combo(m(true, false, false, false), "Esc"),
            "cmd+escape"
        );
        assert_eq!(
            normalise_combo(m(true, false, false, false), "\\"),
            "cmd+backslash"
        );
        assert_eq!(
            normalise_combo(m(true, false, false, false), "`"),
            "cmd+grave"
        );
        assert_eq!(
            normalise_combo(m(false, false, true, false), "ArrowLeft"),
            "alt+left"
        );
        assert_eq!(
            normalise_combo(m(false, true, false, false), "F11"),
            "ctrl+f11"
        );
    }

    #[test]
    fn shift_only_and_plain_keys_are_not_recordable() {
        assert!(!m(false, false, false, false).is_recordable());
        assert!(!m(false, false, false, true).is_recordable());
        assert!(m(true, false, false, false).is_recordable());
        assert!(m(false, true, false, false).is_recordable());
        assert!(m(false, false, true, false).is_recordable());
    }

    #[test]
    fn os_combos_are_normalised_strings() {
        for c in OS_COMBOS {
            assert_eq!(*c, c.to_ascii_lowercase(), "{c}");
            assert!(!c.contains(' '), "{c}");
            assert!(
                c.starts_with("cmd+") || c.starts_with("ctrl+") || c.starts_with("alt+"),
                "{c}"
            );
            let (mods, key) = c.rsplit_once('+').unwrap();
            for part in mods.split('+') {
                assert!(matches!(part, "cmd" | "ctrl" | "alt" | "shift"), "{c}");
            }
            assert_eq!(canonical_key(key), key, "{c}");
        }
        let mut sorted: Vec<&str> = OS_COMBOS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), OS_COMBOS.len(), "duplicates in OS_COMBOS");
    }

    #[test]
    fn is_os_combo_exact_and_prefix_rules() {
        assert!(is_os_combo_for("cmd+tab", false));
        assert!(is_os_combo_for("alt+tab", false));
        assert!(!is_os_combo_for("cmd+b", false)); // reaches apps on macOS
        assert!(is_os_combo_for("cmd+b", true)); // win+b is system-owned
        assert!(is_os_combo_for("cmd+shift+s", true));
        assert!(!is_os_combo_for("ctrl+b", true));
        assert!(!is_os_combo_for("ctrl+alt+p", true));
    }

    #[test]
    fn ring_buffer_caps_and_drops_oldest() {
        let mut ring = VecDeque::new();
        for i in 0..(RING_CAP as u64 + 10) {
            push_event(
                &mut ring,
                HotkeyEvent {
                    at_ms: i,
                    combo: "cmd+b".into(),
                },
            );
        }
        assert_eq!(ring.len(), RING_CAP);
        assert_eq!(ring.front().map(|e| e.at_ms), Some(10));
        assert_eq!(ring.back().map(|e| e.at_ms), Some(RING_CAP as u64 + 9));
    }

    #[test]
    fn drain_clears_shared_ring() {
        // `record` applies the privacy gate; a plain key must not land.
        record(m(false, false, false, false), "a");
        record(m(false, false, false, true), "a");
        record(m(true, false, false, false), "b");
        let got = drain();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].combo, "cmd+b");
        assert!(drain().is_empty());
    }

    #[test]
    fn gate_rejects_unsupported_and_unpermitted() {
        let unsupported = HotkeyStatus {
            supported: false,
            permission_granted: false,
            running: false,
            source: "unsupported".into(),
        };
        assert!(gate(&unsupported).is_err());
        let no_perm = HotkeyStatus {
            supported: true,
            permission_granted: false,
            running: false,
            source: "cgeventtap".into(),
        };
        assert!(gate(&no_perm).unwrap_err().contains("permission"));
        let ok = HotkeyStatus {
            permission_granted: true,
            ..no_perm
        };
        assert!(gate(&ok).is_ok());
    }

    #[test]
    fn status_reports_a_known_source() {
        let st = status();
        assert!(matches!(
            st.source.as_str(),
            "cgeventtap" | "wh_keyboard_ll" | "xinput2" | "unsupported"
        ));
        if !st.supported {
            assert!(!st.permission_granted);
        }
        assert!(!st.running);
    }

    #[test]
    fn mac_keycode_table_is_unique_and_covers_alnum() {
        let mut names: Vec<&str> = MAC_KEYCODES.iter().map(|(_, n)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate key names");
        let mut codes: Vec<u16> = MAC_KEYCODES.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "duplicate keycodes");
        for ch in 'a'..='z' {
            let s = ch.to_string();
            assert!(names.contains(&s.as_str()), "missing {s}");
        }
        for d in '0'..='9' {
            let s = d.to_string();
            assert!(names.contains(&s.as_str()), "missing {s}");
        }
        assert_eq!(mac_keycode_name(0x0B), Some("b"));
        assert_eq!(mac_keycode_name(0x31), Some("space"));
        assert_eq!(mac_keycode_name(0x2A), Some("backslash"));
        assert_eq!(mac_keycode_name(0xFF), None);
    }

    #[test]
    fn win_vk_and_x11_keysym_tables() {
        assert_eq!(win_vk_name(0x42), Some("b"));
        assert_eq!(win_vk_name(0x30), Some("0"));
        assert_eq!(win_vk_name(0x70), Some("f1"));
        assert_eq!(win_vk_name(0x87), Some("f24"));
        assert_eq!(win_vk_name(0xDC), Some("backslash"));
        assert_eq!(win_vk_name(0xA0), None); // VK_LSHIFT is a modifier, not a key
        assert_eq!(x11_keysym_name(0x62), Some("b"));
        assert_eq!(x11_keysym_name(0x42), Some("b"));
        assert_eq!(x11_keysym_name(0xFFBE), Some("f1"));
        assert_eq!(x11_keysym_name(0xFF0D), Some("enter"));
        assert_eq!(x11_keysym_name(0xFFE3), None); // Control_L is a modifier
    }

    #[test]
    fn x11_modifier_tracking() {
        let mut mods = Modifiers::default();
        assert!(x11_apply_modifier(0xFFEB, true, &mut mods)); // Super_L
        assert!(mods.cmd);
        assert!(x11_apply_modifier(0xFFE3, true, &mut mods)); // Control_L
        assert!(mods.ctrl);
        assert!(!x11_apply_modifier(0x62, true, &mut mods)); // 'b' is not a modifier
        assert!(x11_apply_modifier(0xFFEB, false, &mut mods));
        assert!(!mods.cmd && mods.ctrl);
    }

    #[test]
    fn serde_field_names() {
        let ev = HotkeyEvent {
            at_ms: 5,
            combo: "cmd+b".into(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["at_ms"], 5);
        assert_eq!(v["combo"], "cmd+b");
        let st = serde_json::to_value(status()).unwrap();
        for k in ["supported", "permission_granted", "running", "source"] {
            assert!(st.get(k).is_some(), "{k}");
        }
    }
}
