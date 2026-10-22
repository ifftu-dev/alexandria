// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // linuxdeploy's GTK hook pins a startup-only light/dark variant, which
        // overrides later GtkSettings changes from the app's theme switcher.
        // Clear only its default override, before GTK or worker threads start.
        // An exported APPIMAGE_GTK_THEME remains an explicit user override.
        if uses_bundled_gtk_theme_override(
            std::env::var_os("APPIMAGE").is_some(),
            std::env::var_os("APPIMAGE_GTK_THEME").is_some(),
            std::env::var("GTK_THEME").ok().as_deref(),
        ) {
            std::env::remove_var("GTK_THEME");
        }
    }
    app_lib::run();
}

#[cfg(any(target_os = "linux", test))]
fn uses_bundled_gtk_theme_override(
    is_appimage: bool,
    has_explicit_override: bool,
    gtk_theme: Option<&str>,
) -> bool {
    is_appimage
        && !has_explicit_override
        && matches!(gtk_theme, Some("Adwaita:light" | "Adwaita:dark"))
}

#[cfg(test)]
mod tests {
    use super::uses_bundled_gtk_theme_override;

    #[test]
    fn appimage_default_variants_allow_runtime_theme_changes() {
        for theme in ["Adwaita:light", "Adwaita:dark"] {
            assert!(uses_bundled_gtk_theme_override(true, false, Some(theme)));
        }
    }

    #[test]
    fn explicit_appimage_theme_is_preserved() {
        for theme in ["Adwaita:light", "Adwaita:dark", "HighContrast"] {
            assert!(!uses_bundled_gtk_theme_override(true, true, Some(theme)));
        }
    }

    #[test]
    fn native_packages_and_other_themes_are_untouched() {
        assert!(!uses_bundled_gtk_theme_override(
            false,
            false,
            Some("Adwaita:dark")
        ));
        for theme in [None, Some("Adwaita"), Some("HighContrast"), Some("")] {
            assert!(!uses_bundled_gtk_theme_override(true, false, theme));
        }
    }
}
