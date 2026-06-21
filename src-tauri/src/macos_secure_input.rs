//! Clear a WKWebView Secure Event Input leak.
//!
//! macOS enables *Secure Event Input* while a password field is focused —
//! correct behaviour, but WKWebView leaves the per-process enable count
//! unbalanced after a single-page-app navigation away from a focused
//! password field. While Secure Event Input is on, the OS suppresses
//! `CGEventTap`s, which is the mechanism every global-hotkey tool
//! (Hammerspoon, Karabiner, Raycast, BetterTouchTool, skhd, dropdown
//! terminals…) relies on — so the user's hotkeys die whenever the app is
//! foreground. JS `blur()` does not reliably rebalance it.
//!
//! These Carbon calls let us force the count back to zero. The caller
//! must only invoke this when no password entry is actually in progress
//! (the frontend gates on `document.activeElement` not being a password
//! field), so we never weaken protection during real password entry.

// Carbon (HIToolbox) — linked via build.rs on macOS.
extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
    fn DisableSecureEventInput();
}

/// Force Secure Event Input off if it is currently enabled. Returns
/// `true` if it ended up disabled. The enable count is per-process and
/// counted, so we decrement until the state clears (bounded so a
/// genuinely-held lock from elsewhere can't spin forever).
pub fn release_secure_event_input() -> bool {
    // SAFETY: plain Carbon C calls with no arguments; always safe to call.
    unsafe {
        let mut guard = 0;
        while IsSecureEventInputEnabled() && guard < 16 {
            DisableSecureEventInput();
            guard += 1;
        }
        !IsSecureEventInputEnabled()
    }
}
