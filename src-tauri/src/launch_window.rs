//! Launch handoff: every native launch surface uses the web first-paint
//! background (`index.html`), so no colour flashes before the opening.
//!
//! On desktop the main window is created hidden (`tauri.conf.json`) and the
//! frontend shows it on its first frame (`src/composables/launchWindow.ts`).
//! [`install`] gives the hidden window the theme background and shows it after
//! [`SHOW_FALLBACK`] if the webview never got that far.
//!
//! The iOS launch screen and the Android splash/window background carry the
//! same colours as native resources; the test below keeps all of them in step.

use tauri::window::Color;
use tauri::Theme;

/// Web first-paint background in light mode (`index.html`).
pub const LIGHT_BACKGROUND: Color = Color(0xf7, 0xf6, 0xf3, 0xff);
/// Web first-paint background in dark mode (`index.html`).
pub const DARK_BACKGROUND: Color = Color(0x11, 0x18, 0x27, 0xff);

/// Shows the window even if the webview never runs its first-paint handoff.
#[cfg(desktop)]
pub const SHOW_FALLBACK: std::time::Duration = std::time::Duration::from_secs(2);

/// The launch background for a window theme.
pub fn background_for(theme: Theme) -> Color {
    match theme {
        Theme::Dark => DARK_BACKGROUND,
        _ => LIGHT_BACKGROUND,
    }
}

/// Gives the hidden main window the theme background and arms the fallback
/// that shows it if the frontend does not.
#[cfg(desktop)]
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // The window theme is the OS appearance until the frontend applies the
    // user's choice; the web first paint makes the same call for `system`.
    let theme = window.theme().unwrap_or(Theme::Light);
    if let Err(e) = window.set_background_color(Some(background_for(theme))) {
        log::warn!("launch window background: {e}");
    }
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_FALLBACK);
        if !window.is_visible().unwrap_or(false) {
            log::warn!("frontend did not show the window; showing it now");
            let _ = window.show();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn hex(color: Color) -> String {
        format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
    }

    fn read(relative: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .to_lowercase()
    }

    /// iOS colour components, as Xcode writes them.
    fn ios_components(color: Color) -> [String; 3] {
        [("red", color.0), ("green", color.1), ("blue", color.2)]
            .map(|(name, c)| format!("\"{name}\" : \"0x{c:02x}\""))
    }

    #[test]
    fn theme_picks_the_matching_background() {
        assert_eq!(background_for(Theme::Light), LIGHT_BACKGROUND);
        assert_eq!(background_for(Theme::Dark), DARK_BACKGROUND);
    }

    #[test]
    fn native_launch_surfaces_match_the_web_first_paint() {
        let first_paint = read("../index.html");
        assert!(first_paint.contains("'#f7f6f3'"), "index.html light paint");
        assert!(first_paint.contains("'#111827'"), "index.html dark paint");
        assert_eq!(hex(LIGHT_BACKGROUND), "#f7f6f3");
        assert_eq!(hex(DARK_BACKGROUND), "#111827");

        let res = "gen/android/app/src/main/res";
        let light = read(&format!("{res}/values/colors.xml"));
        let dark = read(&format!("{res}/values-night/colors.xml"));
        let entry = |c: Color| {
            format!(
                "<color name=\"launch_background\">#ff{}</color>",
                &hex(c)[1..]
            )
        };
        assert!(light.contains(&entry(LIGHT_BACKGROUND)), "Android light");
        assert!(dark.contains(&entry(DARK_BACKGROUND)), "Android night");
        // The night qualifier outranks the API level, so all four theme
        // variants must carry the launch items.
        for dir in ["values", "values-night", "values-v31", "values-night-v31"] {
            let theme = read(&format!("{res}/{dir}/themes.xml"));
            assert!(
                theme.contains(
                    "<item name=\"android:windowbackground\">@color/launch_background</item>"
                ),
                "{dir} window background"
            );
            if dir.ends_with("v31") {
                assert!(
                    theme.contains("<item name=\"android:windowsplashscreenbackground\">@color/launch_background</item>")
                        && theme.contains("<item name=\"android:windowsplashscreenanimatedicon\">@drawable/launch_mark</item>"),
                    "{dir} splash"
                );
            }
        }

        let colorset = read("gen/apple/Assets.xcassets/LaunchBackground.colorset/Contents.json");
        let dark_at = colorset.find("\"dark\"").expect("iOS dark appearance");
        let (any, night) = colorset.split_at(dark_at);
        for c in ios_components(LIGHT_BACKGROUND) {
            assert!(any.contains(&c), "iOS light component {c}");
        }
        for c in ios_components(DARK_BACKGROUND) {
            assert!(night.contains(&c), "iOS dark component {c}");
        }
        let storyboard = read("gen/apple/LaunchScreen.storyboard");
        assert!(storyboard.contains("name=\"launchbackground\""));
        assert!(!storyboard.contains("systembackgroundcolor"));
    }
}
