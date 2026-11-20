//! The SCORM 2004 conformance checks replay the LMS test packages of the
//! standards body's SCORM 2004 4th Edition test suite in the player. Their
//! manifests are vendored, unchanged, under
//! `tests/desktop-ui/scorm2004-conformance/adl/packages/`; the browser checks
//! hand the player the description import makes of each one, kept in
//! `adl/descriptions.json`.
//!
//! This test is the content aggregation half of that replay: every one of the
//! suite's LMS test packages imports, and the stored descriptions are exactly
//! what import makes of them. The suite's own content (Java applets) is not
//! vendored; each file the manifest names is a placeholder here and the
//! browser checks serve a replay lesson in its place. After changing the
//! importer, regenerate with
//! `UPDATE_PLAYER_FIXTURE=1 cargo test -p alexandria-studio --test scorm2004_conformance_packages`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use alexandria_studio::package::{import_package, Limits, PackageDescription, Standard};

#[path = "support/zip_fixture.rs"]
mod fixture;

fn adl_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/desktop-ui/scorm2004-conformance/adl")
}

/// Every value of the attribute `name` (`href`, `xml:base`) in `xml`, however
/// it is spaced.
fn attribute_values(xml: &str, name: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().last();
        rest = &rest[at + name.len()..];
        if !matches!(before, Some(c) if c.is_whitespace()) {
            continue;
        }
        let after = rest.trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if let Some(end) = after[1..].find(quote) {
            found.push(after[1..1 + end].to_string());
        }
    }
    found
}

/// The manifest and a placeholder for every file it could name: each `href`
/// on its own and under each `xml:base`, query and fragment removed.
fn archive_for(manifest: &str) -> Vec<u8> {
    let hrefs = attribute_values(manifest, "href");
    let bases = attribute_values(manifest, "xml:base");
    let mut names = BTreeSet::new();
    for href in &hrefs {
        let path = href.split(['?', '#']).next().unwrap_or_default();
        if path.is_empty() || path.contains("..") || path.contains(':') || path.starts_with('/') {
            continue;
        }
        names.insert(path.to_string());
        for base in &bases {
            if !base.contains("..") && !base.contains(':') && !base.starts_with('/') {
                names.insert(format!("{base}{path}"));
            }
        }
    }
    let mut entries = vec![fixture::file("imsmanifest.xml", manifest)];
    entries.extend(names.iter().map(|name| fixture::file(name, "placeholder")));
    fixture::zip(&entries)
}

#[test]
fn every_lms_test_package_of_the_scorm2004_test_suite_imports_and_its_description_is_stored() {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(adl_dir().join("packages"))
        .expect("read the vendored packages")
        .map(|entry| entry.expect("package entry").path())
        .collect();
    packages.sort();
    assert!(packages.len() > 180, "the suite has 189 LMS test packages");

    let mut descriptions = BTreeMap::new();
    let mut refused = Vec::new();
    for dir in &packages {
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("package names are UTF-8")
            .to_string();
        let manifest =
            std::fs::read_to_string(dir.join("imsmanifest.xml")).expect("read a vendored manifest");
        match import_package(Cursor::new(archive_for(&manifest)), &Limits::default()) {
            Ok(description) => {
                assert_eq!(description.standard, Standard::Scorm2004, "{name}");
                descriptions.insert(name, description);
            }
            Err(refusal) => refused.push(format!("{name}: {refusal}")),
        }
    }
    assert!(refused.is_empty(), "refused: {refused:#?}");

    // One package per line, so a change shows as a change to that package.
    let mut rendered = String::from("{\n");
    for (index, (name, description)) in descriptions.iter().enumerate() {
        let comma = if index + 1 < descriptions.len() {
            ","
        } else {
            ""
        };
        rendered.push_str(&format!(
            "{}: {}{comma}\n",
            serde_json::to_string(name).expect("serialise"),
            serde_json::to_string::<PackageDescription>(description).expect("serialise")
        ));
    }
    rendered.push_str("}\n");

    let golden = adl_dir().join("descriptions.json");
    if std::env::var_os("UPDATE_PLAYER_FIXTURE").is_some() {
        std::fs::write(&golden, &rendered).expect("write descriptions");
    }
    let stored = std::fs::read_to_string(&golden).unwrap_or_default();
    assert_eq!(
        stored, rendered,
        "adl/descriptions.json is stale; regenerate it (see this file's header)"
    );
}
