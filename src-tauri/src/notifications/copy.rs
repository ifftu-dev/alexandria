//! Generic notification text shared with the webview catalogs. Only the kind
//! and selected profile language choose copy; event payloads never format it.
//! The Android channel's name and description come from the same catalogs.

use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;

#[derive(Deserialize)]
struct Catalog {
    kinds: HashMap<String, Copy>,
    channel: Channel,
}

#[derive(Deserialize)]
struct Channel {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct Copy {
    title: String,
    body: String,
}

static CATALOGS: LazyLock<HashMap<&'static str, Catalog>> = LazyLock::new(|| {
    [
        (
            "en",
            include_str!("../../../src/locales/en/notifications.json"),
        ),
        (
            "bn",
            include_str!("../../../src/locales/bn/notifications.json"),
        ),
        (
            "fr",
            include_str!("../../../src/locales/fr/notifications.json"),
        ),
        (
            "te",
            include_str!("../../../src/locales/te/notifications.json"),
        ),
        (
            "es",
            include_str!("../../../src/locales/es/notifications.json"),
        ),
        (
            "ur",
            include_str!("../../../src/locales/ur/notifications.json"),
        ),
        (
            "zh",
            include_str!("../../../src/locales/zh/notifications.json"),
        ),
        (
            "mr",
            include_str!("../../../src/locales/mr/notifications.json"),
        ),
        (
            "hi",
            include_str!("../../../src/locales/hi/notifications.json"),
        ),
    ]
    .into_iter()
    .map(|(locale, json)| {
        (
            locale,
            serde_json::from_str(json).expect("bundled notification catalog"),
        )
    })
    .collect()
});

/// The catalog for a locale tag (`fr`, `FR_ca`, `zh-Hans`), else English.
fn catalog(locale: &str) -> &'static Catalog {
    let normalized = locale.trim().to_ascii_lowercase();
    let primary = normalized.split(['-', '_']).next().unwrap_or("en");
    CATALOGS.get(primary).unwrap_or(&CATALOGS["en"])
}

pub(super) fn localized(
    locale: &str,
    kind: super::NotificationKind,
) -> (&'static str, &'static str) {
    let english = &CATALOGS["en"].kinds;
    let copy = catalog(locale)
        .kinds
        .get(kind.as_str())
        .or_else(|| english.get(kind.as_str()))
        .unwrap_or(&english["generic"]);
    (&copy.title, &copy.body)
}

/// The Android channel's name and description in a locale.
pub(super) fn channel(locale: &str) -> (&'static str, &'static str) {
    let channel = &catalog(locale).channel;
    (&channel.name, &channel.description)
}
