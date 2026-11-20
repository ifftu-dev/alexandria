//! The player's SCORM 2004 browser checks serve the package in
//! `tests/desktop-ui/package-player-2004/package/` and hand the player the
//! description in `description.json` beside it, as the host would. This test
//! keeps the two honest. After changing the package, regenerate the
//! description with
//! `UPDATE_PLAYER_FIXTURE=1 cargo test -p alexandria-studio --test player_fixture_package_2004`.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use alexandria_studio::package::{import_package, Limits, PackageDescription};

#[path = "support/zip_fixture.rs"]
mod fixture;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/desktop-ui/package-player-2004")
}

/// Every file under `dir`, as archive paths, in a stable order.
fn files(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("read fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture names are UTF-8");
        let archived = format!("{prefix}{name}");
        if path.is_dir() {
            files(&path, &format!("{archived}/"), out);
        } else {
            out.push((archived, std::fs::read(&path).expect("read fixture file")));
        }
    }
}

#[test]
fn the_scorm2004_player_fixture_description_is_what_import_makes_of_the_fixture_package() {
    let mut entries = Vec::new();
    files(&fixture_dir().join("package"), "", &mut entries);
    let archive = fixture::zip(
        &entries
            .iter()
            .map(|(name, data)| fixture::file(name, data))
            .collect::<Vec<_>>(),
    );
    let description: PackageDescription =
        import_package(Cursor::new(archive), &Limits::default()).expect("the fixture imports");
    let rendered = serde_json::to_string_pretty(&description).expect("serialise") + "\n";

    let golden = fixture_dir().join("description.json");
    if std::env::var_os("UPDATE_PLAYER_FIXTURE").is_some() {
        std::fs::write(&golden, &rendered).expect("write description");
    }
    let stored = std::fs::read_to_string(&golden).unwrap_or_default();
    assert_eq!(
        stored, rendered,
        "description.json is stale; regenerate it (see this file's header)"
    );
}
