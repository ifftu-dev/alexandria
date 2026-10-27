//! Generic notification text shared with the webview catalogs. Only the kind
//! and selected profile language choose copy; event payloads never format it.

use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;

#[derive(Deserialize)]
struct Catalog {
    kinds: HashMap<String, Copy>,
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

pub(super) fn localized(locale: &str, kind: &str) -> (&'static str, &'static str) {
    let normalized = locale.trim().to_ascii_lowercase();
    let primary = normalized.split(['-', '_']).next().unwrap_or("en");
    let english = &CATALOGS["en"].kinds;
    let catalog = CATALOGS.get(primary).unwrap_or(&CATALOGS["en"]);
    let copy = catalog
        .kinds
        .get(kind)
        .or_else(|| english.get(kind))
        .unwrap_or(&english["generic"]);
    (&copy.title, &copy.body)
}
