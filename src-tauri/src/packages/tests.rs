//! Host-seam tests: a package archive goes in through the public functions
//! of `crate::packages`, and what a learner or a peer could observe comes out:
//! the stored description, the extracted files, the draft course, what the
//! content node holds.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use alexandria_studio::package::{RefusalCode, Standard};
use tempfile::TempDir;

use super::*;
use crate::content_store::content;
use crate::content_store::node::ContentNode;
use crate::db::Database;
use crate::domain::package::ArchiveLocation;

pub(crate) mod fixture {
    //! A stored-only zip writer, enough for well-formed test packages.

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let mut shared = Vec::new();
            shared.extend_from_slice(&20u16.to_le_bytes()); // version needed
            shared.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
            shared.extend_from_slice(&0u16.to_le_bytes()); // stored
            shared.extend_from_slice(&0u16.to_le_bytes()); // time
            shared.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
            shared.extend_from_slice(&crc32(data).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(name.len() as u16).to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes()); // extra field length

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&shared);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made on Unix
            directory.extend_from_slice(&shared);
            directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
            directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
            directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            directory.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.induction" version="1.0"
    xmlns="http://www.imsproject.org/xsd/imscp_rootv1p1p2"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>1.2</schemaversion>
  </metadata>
  <organizations default="org">
    <organization identifier="org">
      <title>Warehouse induction</title>
      <item identifier="welcome" identifierref="res-welcome">
        <title>Welcome</title>
      </item>
      <item identifier="lifting" identifierref="res-lifting">
        <title>Lifting and carrying</title>
      </item>
    </organization>
  </organizations>
  <resources>
    <resource identifier="res-welcome" type="webcontent" adlcp:scormtype="sco" href="welcome/index.html">
      <file href="welcome/index.html"/>
    </resource>
    <resource identifier="res-lifting" type="webcontent" adlcp:scormtype="sco" href="lifting/index.html">
      <file href="lifting/index.html"/>
    </resource>
  </resources>
</manifest>"#;

const WELCOME: &[u8] = b"<html><body>Welcome</body></html>";
const LIFTING: &[u8] = b"<html><body>Lift with your legs</body></html>";
const SCRIPT: &[u8] = b"window.lesson = 'shared';";

pub(crate) fn induction_package() -> Vec<u8> {
    fixture::zip(&[
        ("imsmanifest.xml", MANIFEST.as_bytes()),
        ("welcome/index.html", WELCOME),
        ("lifting/index.html", LIFTING),
        ("shared/script.js", SCRIPT),
    ])
}

/// A profile as the packages module sees it: a migrated database with a
/// local identity, the two package directories and a running content node.
struct Profile {
    db: Mutex<Database>,
    dirs: PackageDirs,
    node: ContentNode,
    _root: TempDir,
}

impl Profile {
    async fn new() -> Self {
        let root = TempDir::new().expect("temp dir");
        let dirs = PackageDirs {
            archives: root.path().join("packages"),
            cache: root.path().join("packagecache"),
        };
        dirs.ensure().expect("package dirs");
        let node = ContentNode::new(&root.path().join("iroh"));
        node.start(None).await.expect("start node");
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1uauthor', 'addr_test1q123')",
                [],
            )
            .expect("identity");
        Self {
            db: Mutex::new(db),
            dirs,
            node,
            _root: root,
        }
    }

    /// A file somewhere outside the profile, as a file picker would hand it over.
    fn outside_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self._root.path().join(name);
        std::fs::write(&path, bytes).expect("write source file");
        path
    }
}

fn hash_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn read(root: &Path, path: &str) -> Vec<u8> {
    std::fs::read(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[tokio::test]
async fn an_imported_package_is_kept_under_its_own_hash_and_described() {
    let profile = Profile::new().await;
    let bytes = induction_package();
    let source = profile.outside_file("induction.zip", &bytes);

    let package = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import");

    assert_eq!(package.hash, hash_of(&bytes));
    assert_eq!(package.standard, Standard::Scorm12);
    assert_eq!(package.edition, None);
    assert_eq!(package.title, "Warehouse induction");
    assert_eq!(package.archive_bytes, bytes.len() as u64);
    assert_eq!(package.location, ArchiveLocation::Draft);

    let found = get(&profile.db, &package.hash)
        .await
        .expect("get")
        .expect("the package is recorded");
    assert_eq!(found, package);
    let lessons: Vec<_> = found
        .description
        .launchable_activities()
        .into_iter()
        .map(|activity| activity.launch.as_ref().expect("launch").path.clone())
        .collect();
    assert_eq!(lessons, ["welcome/index.html", "lifting/index.html"]);
    assert_eq!(found.description.files, 4);
}

#[tokio::test]
async fn a_refused_archive_reports_its_code_and_leaves_nothing_behind() {
    let profile = Profile::new().await;
    let source = profile.outside_file("notes.zip", b"these are not a zip archive");

    match import(&profile.db, &profile.dirs, &source).await {
        Err(PackageError::Refused(refusal)) => {
            assert_eq!(refusal.code, RefusalCode::NotAnArchive)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_dir(&profile.dirs.archives).unwrap().count(),
        0,
        "a refused archive is not kept"
    );
}

/// Progress reports with the thread each arrived on.
type Reports = std::sync::Arc<Mutex<Vec<(PackageExtractProgress, std::thread::ThreadId)>>>;

/// Collects progress reports and the threads they arrived on.
fn recorder() -> (Reports, impl FnMut(PackageExtractProgress) + Send + 'static) {
    let reports = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = reports.clone();
    (reports, move |report| {
        sink.lock()
            .unwrap()
            .push((report, std::thread::current().id()))
    })
}

fn assert_induction_files(root: &Path) {
    assert_eq!(read(root, "welcome/index.html"), WELCOME);
    assert_eq!(read(root, "lifting/index.html"), LIFTING);
    assert_eq!(read(root, "shared/script.js"), SCRIPT);
}

#[tokio::test]
async fn dropping_the_extracted_files_keeps_the_archive_and_extraction_rebuilds_them() {
    let profile = Profile::new().await;
    let bytes = induction_package();
    let source = profile.outside_file("induction.zip", &bytes);
    let package = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import");
    std::fs::remove_file(&source).expect("the learner deletes the download");

    let (reports, progress) = recorder();
    let first = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract");
    assert_induction_files(&first.root);
    assert!(
        first.package.last_opened_at.is_some(),
        "a launch is recorded"
    );

    {
        let reports = reports.lock().unwrap();
        let (last, _) = reports.last().expect("progress was reported");
        assert_eq!(last.hash, package.hash);
        assert_eq!((last.files_done, last.files_total), (4, 4));
        assert_eq!(last.bytes_total, package.description.uncompressed_bytes);
        assert_eq!(last.bytes_done, last.bytes_total);
        let caller = std::thread::current().id();
        assert!(
            reports.iter().all(|(_, thread)| *thread != caller),
            "extraction runs off the calling thread"
        );
    }

    assert!(drop_extracted(&profile.dirs, &package.hash)
        .await
        .expect("drop"));
    assert!(!first.root.exists(), "the extracted files are gone");
    assert!(
        !drop_extracted(&profile.dirs, &package.hash)
            .await
            .expect("drop again"),
        "nothing left to drop"
    );

    let (_, progress) = recorder();
    let again = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract again");
    assert_eq!(again.root, first.root);
    assert_induction_files(&again.root);
}

#[tokio::test]
async fn an_extracted_package_is_not_extracted_twice() {
    let profile = Profile::new().await;
    let source = profile.outside_file("induction.zip", &induction_package());
    let package = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import");
    let (_, progress) = recorder();
    ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract");

    let (reports, progress) = recorder();
    let second = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract again");
    assert_induction_files(&second.root);
    assert!(reports.lock().unwrap().is_empty(), "no second extraction");
}

#[tokio::test]
async fn extracting_an_unknown_package_is_refused_without_touching_the_cache() {
    let profile = Profile::new().await;
    let (_, progress) = recorder();
    let unknown = "ab".repeat(32);
    match ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &unknown,
        progress,
    )
    .await
    {
        Err(PackageError::NotFound(hash)) => assert_eq!(hash, unknown),
        other => panic!("expected not found, got {other:?}"),
    }
    let (_, progress) = recorder();
    assert!(matches!(
        ensure_extracted(
            &profile.db,
            &profile.node,
            &profile.dirs,
            "../../etc",
            progress
        )
        .await,
        Err(PackageError::InvalidHash(_))
    ));
    assert_eq!(std::fs::read_dir(&profile.dirs.cache).unwrap().count(), 0);
}

#[tokio::test]
async fn publishing_moves_the_archive_into_the_served_store_where_extraction_finds_it() {
    let profile = Profile::new().await;
    let bytes = induction_package();
    let source = profile.outside_file("induction.zip", &bytes);
    let package = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import");

    let published = publish(&profile.db, &profile.node, &profile.dirs, &package.hash)
        .await
        .expect("publish");

    assert_eq!(published.location, ArchiveLocation::Store);
    assert_eq!(
        content::get_bytes(&profile.node, &package.hash)
            .await
            .expect("the store holds the archive under the same hash"),
        bytes
    );
    let pinned: (String, i64) = profile
        .db
        .lock()
        .unwrap()
        .conn()
        .query_row(
            "SELECT pin_type, auto_unpin FROM pins WHERE cid = ?1",
            [&package.hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the archive is pinned");
    assert_eq!(pinned, ("course".to_string(), 0), "never evicted");
    assert_eq!(
        std::fs::read_dir(&profile.dirs.archives).unwrap().count(),
        0,
        "one copy: the draft file is gone"
    );

    let (_, progress) = recorder();
    let extracted = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract from the store");
    assert_induction_files(&extracted.root);
    let leftovers: Vec<_> = std::fs::read_dir(&profile.dirs.cache)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        leftovers,
        std::slice::from_ref(&package.hash),
        "no exported copy is left behind"
    );

    // Importing the same file again after publishing keeps the one copy.
    let again = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import again");
    assert_eq!(again.location, ArchiveLocation::Store);
    assert_eq!(
        std::fs::read_dir(&profile.dirs.archives).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn a_package_received_into_the_store_is_described_and_extracted_from_there() {
    let profile = Profile::new().await;
    let bytes = induction_package();
    let added = content::add_bytes_unencrypted(&profile.node, &bytes)
        .await
        .expect("received with a course");

    let package = register_stored(&profile.db, &profile.node, &profile.dirs, &added.hash)
        .await
        .expect("register");
    assert_eq!(package.hash, hash_of(&bytes));
    assert_eq!(package.location, ArchiveLocation::Store);
    assert_eq!(package.title, "Warehouse induction");

    let (_, progress) = recorder();
    let extracted = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract");
    assert_induction_files(&extracted.root);
}

#[tokio::test]
async fn a_received_blob_that_is_not_a_package_is_refused() {
    let profile = Profile::new().await;
    let added = content::add_bytes_unencrypted(&profile.node, b"a text lesson")
        .await
        .expect("add");
    match register_stored(&profile.db, &profile.node, &profile.dirs, &added.hash).await {
        Err(PackageError::Refused(refusal)) => assert_eq!(refusal.code, RefusalCode::NotAnArchive),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(get(&profile.db, &added.hash).await.expect("get"), None);
}

#[tokio::test]
async fn the_idle_check_drops_only_packages_not_launched_within_the_period() {
    let profile = Profile::new().await;
    let induction = import(
        &profile.db,
        &profile.dirs,
        &profile.outside_file("induction.zip", &induction_package()),
    )
    .await
    .expect("import");
    let other_bytes = fixture::zip(&[
        (
            "imsmanifest.xml",
            MANIFEST.replace("Warehouse", "Office").as_bytes(),
        ),
        ("welcome/index.html", WELCOME),
        ("lifting/index.html", LIFTING),
    ]);
    let other = import(
        &profile.db,
        &profile.dirs,
        &profile.outside_file("office.zip", &other_bytes),
    )
    .await
    .expect("import");
    for hash in [&induction.hash, &other.hash] {
        let (_, progress) = recorder();
        ensure_extracted(&profile.db, &profile.node, &profile.dirs, hash, progress)
            .await
            .expect("extract");
    }
    let opened = chrono::Utc::now();

    let dropped = drop_idle(&profile.db, &profile.dirs, opened + IDLE_DROP_AFTER / 2)
        .await
        .expect("idle check");
    assert!(dropped.is_empty(), "both were launched within the period");

    // The induction is launched again later; the other one is not.
    profile
        .db
        .lock()
        .unwrap()
        .conn()
        .execute(
            "UPDATE packages SET last_opened_at = ?2 WHERE hash = ?1",
            rusqlite::params![
                induction.hash,
                (opened + IDLE_DROP_AFTER).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ],
        )
        .unwrap();
    let dropped = drop_idle(
        &profile.db,
        &profile.dirs,
        opened + IDLE_DROP_AFTER + chrono::Duration::days(1),
    )
    .await
    .expect("idle check");
    assert_eq!(dropped, std::slice::from_ref(&other.hash));
    assert!(profile.dirs.cache.join(&induction.hash).is_dir());
    assert!(!profile.dirs.cache.join(&other.hash).exists());
    assert_eq!(
        get(&profile.db, &other.hash)
            .await
            .expect("get")
            .map(|p| p.hash),
        Some(other.hash.clone()),
        "the package itself is kept"
    );
}

#[tokio::test]
async fn a_package_opened_from_disk_becomes_a_draft_course_that_is_never_served() {
    let profile = Profile::new().await;
    let bytes = induction_package();
    let source = profile.outside_file("induction.zip", &bytes);

    let opened = open_file(&profile.db, &profile.dirs, &source)
        .await
        .expect("open");

    assert_eq!(opened.package.hash, hash_of(&bytes));
    let (title, status, in_catalog, elements) = {
        let db = profile.db.lock().unwrap();
        let conn = db.conn();
        let (title, status): (String, String) = conn
            .query_row(
                "SELECT title, status FROM courses WHERE id = ?1",
                [&opened.course_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the draft course exists");
        let in_catalog: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog WHERE course_id = ?1",
                [&opened.course_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut statement = conn
            .prepare(
                "SELECT e.id, e.element_type, e.content_cid FROM course_elements e \
                 JOIN course_chapters ch ON ch.id = e.chapter_id WHERE ch.course_id = ?1",
            )
            .unwrap();
        let elements: Vec<(String, String, String)> = statement
            .query_map([&opened.course_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        (title, status, in_catalog, elements)
    };
    assert_eq!(title, "Warehouse induction");
    assert_eq!(status, "draft");
    assert_eq!(in_catalog, 0, "a draft is not in the catalog");
    assert_eq!(
        elements,
        [(
            opened.element_id.clone(),
            "package".to_string(),
            opened.package.hash.clone()
        )],
        "one package element carrying the archive's hash"
    );

    // The node serves and announces only what its store holds, and the
    // archive is not there: it is a draft file that only this device reads.
    assert!(
        !content::has(&profile.node, &opened.package.hash)
            .await
            .expect("has"),
        "the content node cannot serve the archive"
    );
    let pinned: i64 = profile
        .db
        .lock()
        .unwrap()
        .conn()
        .query_row("SELECT COUNT(*) FROM pins", [], |row| row.get(0))
        .unwrap();
    assert_eq!(pinned, 0, "nothing is tracked as served content");

    // It still launches: the files come from the draft archive.
    let (_, progress) = recorder();
    let extracted = ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &opened.package.hash,
        progress,
    )
    .await
    .expect("extract");
    assert_induction_files(&extracted.root);

    let again = open_file(&profile.db, &profile.dirs, &source)
        .await
        .expect("open again");
    assert_eq!(
        (again.course_id, again.element_id),
        (opened.course_id, opened.element_id),
        "opening the same file again returns the same draft"
    );
}

#[tokio::test]
async fn opening_a_file_that_is_not_a_package_makes_no_course() {
    let profile = Profile::new().await;
    let source = profile.outside_file("photo.zip", &fixture::zip(&[("photo.jpg", b"jpeg")]));
    match open_file(&profile.db, &profile.dirs, &source).await {
        Err(PackageError::Refused(refusal)) => assert_eq!(refusal.code, RefusalCode::NoManifest),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let courses: i64 = profile
        .db
        .lock()
        .unwrap()
        .conn()
        .query_row("SELECT COUNT(*) FROM courses", [], |row| row.get(0))
        .unwrap();
    assert_eq!(courses, 0);
}

#[tokio::test]
async fn profile_start_clears_what_an_interrupted_extraction_left_behind() {
    let profile = Profile::new().await;
    let source = profile.outside_file("induction.zip", &induction_package());
    let package = import(&profile.db, &profile.dirs, &source)
        .await
        .expect("import");
    let (_, progress) = recorder();
    ensure_extracted(
        &profile.db,
        &profile.node,
        &profile.dirs,
        &package.hash,
        progress,
    )
    .await
    .expect("extract");
    // What a crash in the middle of each step leaves.
    let half_extracted = profile
        .dirs
        .cache
        .join(format!(".extract-{}-crashed", package.hash));
    std::fs::create_dir_all(half_extracted.join("welcome")).unwrap();
    std::fs::write(half_extracted.join("welcome/index.html"), b"<ht").unwrap();
    std::fs::write(profile.dirs.cache.join(".archive-crashed.zip"), b"PK").unwrap();
    std::fs::write(profile.dirs.archives.join(".import-crashed.zip"), b"PK").unwrap();

    clear_scratch(&profile.dirs).expect("clear");
    drop_idle(&profile.db, &profile.dirs, chrono::Utc::now())
        .await
        .expect("idle check");

    let names = |dir: &Path| -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        names(&profile.dirs.cache),
        std::slice::from_ref(&package.hash)
    );
    assert_eq!(
        names(&profile.dirs.archives),
        [format!("{}.zip", package.hash)]
    );
}
