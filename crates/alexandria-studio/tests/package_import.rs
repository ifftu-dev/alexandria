//! Standards package import: archive bytes in, a package description or a
//! stable refusal code out, plus bounded extraction to a directory.
//!
//! Every archive is assembled here, byte by byte, so the hostile ones can say
//! things no well-behaved zip writer would.

use std::io::Cursor;

use alexandria_studio::package::{
    import_package, LaunchKind, Limits, PackageDescription, Refusal, RefusalCode, Standard,
};

mod fixture {
    //! A zip writer that does what it is told, including lying.

    use std::io::Write;

    pub struct Entry {
        name: Vec<u8>,
        data: Vec<u8>,
        deflate: bool,
        unix_mode: Option<u32>,
        declared_size: Option<u32>,
        method: Option<u16>,
        encrypted: bool,
    }

    pub fn file(name: &str, data: impl AsRef<[u8]>) -> Entry {
        Entry {
            name: name.as_bytes().to_vec(),
            data: data.as_ref().to_vec(),
            deflate: false,
            unix_mode: None,
            declared_size: None,
            method: None,
            encrypted: false,
        }
    }

    impl Entry {
        pub fn deflated(mut self) -> Self {
            self.deflate = true;
            self
        }

        /// Unix file type and permission bits, as `st_mode` holds them.
        pub fn unix_mode(mut self, mode: u32) -> Self {
            self.unix_mode = Some(mode);
            self
        }

        /// State this uncompressed size in both headers, whatever the data is.
        pub fn declaring_size(mut self, size: u32) -> Self {
            self.declared_size = Some(size);
            self
        }

        /// Claim this compression method, whatever was done to the data.
        pub fn claiming_method(mut self, method: u16) -> Self {
            self.method = Some(method);
            self
        }

        /// Set the flag that says the data is encrypted.
        pub fn flagged_encrypted(mut self) -> Self {
            self.encrypted = true;
            self
        }
    }

    pub fn zip(entries: &[Entry]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for entry in entries {
            let mut crc = flate2::Crc::new();
            crc.update(&entry.data);
            let stored = if entry.deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&entry.data).unwrap();
                encoder.finish().unwrap()
            } else {
                entry.data.clone()
            };
            let method = entry.method.unwrap_or(if entry.deflate { 8 } else { 0 });
            // Bit 11: names are UTF-8. Bit 0: the data is encrypted.
            let flags: u16 = 0x0800 | u16::from(entry.encrypted);
            let size = entry.declared_size.unwrap_or(entry.data.len() as u32);
            let offset = out.len() as u32;

            let mut shared = Vec::new();
            shared.extend_from_slice(&20u16.to_le_bytes()); // version needed
            shared.extend_from_slice(&flags.to_le_bytes());
            shared.extend_from_slice(&method.to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes()); // time
            shared.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
            shared.extend_from_slice(&crc.sum().to_le_bytes());
            shared.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            shared.extend_from_slice(&size.to_le_bytes());
            shared.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes()); // extra field length

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&shared);
            out.extend_from_slice(&entry.name);
            out.extend_from_slice(&stored);

            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made on Unix
            directory.extend_from_slice(&shared);
            directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
            directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
            directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            directory.extend_from_slice(&(entry.unix_mode.unwrap_or(0o100644) << 16).to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(&entry.name);
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

use fixture::{file, zip, Entry};

/// A SCORM 1.2 manifest around the given organizations and resources.
fn scorm12_manifest(organizations: &str, resources: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.course" version="1.0"
    xmlns="http://www.imsproject.org/xsd/imscp_rootv1p1p2"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>1.2</schemaversion>
  </metadata>
  {organizations}
  {resources}
</manifest>"#
    )
}

fn single_lesson_manifest() -> String {
    scorm12_manifest(
        r#"<organizations default="org">
             <organization identifier="org">
               <title>Fire safety</title>
               <item identifier="item-1" identifierref="res-1">
                 <title>Using an extinguisher</title>
               </item>
             </organization>
           </organizations>"#,
        r#"<resources>
             <resource identifier="res-1" type="webcontent" adlcp:scormtype="sco" href="index.html">
               <file href="index.html"/>
             </resource>
           </resources>"#,
    )
}

fn import(entries: &[Entry]) -> Result<PackageDescription, Refusal> {
    import_package(Cursor::new(zip(entries)), &Limits::default())
}

fn refusal(entries: &[Entry]) -> Refusal {
    import(entries).expect_err("the package should be refused")
}

#[test]
fn a_single_lesson_scorm12_package_imports_as_one_launchable_activity() {
    let package = import(&[
        file("imsmanifest.xml", single_lesson_manifest()),
        file("index.html", "<html></html>"),
    ])
    .unwrap();

    assert_eq!(package.standard, Standard::Scorm12);
    assert_eq!(package.edition, None);
    assert_eq!(package.identifier, "com.example.course");
    assert_eq!(package.title, "Fire safety");

    assert_eq!(package.tree.id, "org");
    assert_eq!(package.tree.title, "Fire safety");
    assert!(package.tree.launch.is_none());
    assert_eq!(package.tree.children.len(), 1);

    let lesson = &package.tree.children[0];
    assert_eq!(lesson.id, "item-1");
    assert_eq!(lesson.title, "Using an extinguisher");
    assert!(lesson.visible);
    assert!(lesson.children.is_empty());
    let launch = lesson.launch.as_ref().expect("the lesson is launchable");
    assert_eq!(launch.path, "index.html");
    assert_eq!(launch.kind, LaunchKind::Sco);
    assert_eq!(launch.parameters, None);
}

#[test]
fn bytes_that_are_not_a_zip_archive_are_refused() {
    let refused = import_package(Cursor::new(b"<html>not a zip</html>"), &Limits::default())
        .expect_err("not an archive");
    assert_eq!(refused.code, RefusalCode::NotAnArchive);
    assert_eq!(refused.code.as_str(), "not_an_archive");
}

#[test]
fn an_archive_without_a_manifest_is_refused() {
    let refused = refusal(&[file("index.html", "<html></html>")]);
    assert_eq!(refused.code, RefusalCode::NoManifest);
    assert_eq!(refused.code.as_str(), "no_manifest");
}

fn multi_lesson_manifest() -> String {
    scorm12_manifest(
        r#"<organizations default="main">
             <organization identifier="alternative">
               <title>Short version</title>
               <item identifier="short-1" identifierref="res-intro"><title>Intro only</title></item>
             </organization>
             <organization identifier="main">
               <title>Warehouse induction</title>
               <item identifier="welcome" identifierref="res-intro" parameters="?lang=en">
                 <title>Welcome</title>
               </item>
               <item identifier="chapter-1">
                 <title>Moving loads</title>
                 <item identifier="lifting" identifierref="res-lifting">
                   <title>
                     Lifting &amp;
                     carrying
                   </title>
                   <adlcp:prerequisites type="aicc_script">welcome</adlcp:prerequisites>
                   <adlcp:maxtimeallowed>00:30:00</adlcp:maxtimeallowed>
                   <adlcp:timelimitaction>exit,message</adlcp:timelimitaction>
                   <adlcp:datafromlms>site=north;shift=night</adlcp:datafromlms>
                   <adlcp:masteryscore>80</adlcp:masteryscore>
                 </item>
                 <item identifier="forklift" identifierref="res-forklift" isvisible="false">
                   <title>Forklift quiz</title>
                   <adlcp:masteryscore>62.5</adlcp:masteryscore>
                 </item>
               </item>
               <item identifier="handout" identifierref="res-handout">
                 <title>Handout</title>
               </item>
             </organization>
           </organizations>"#,
        r#"<resources xml:base="content/">
             <resource identifier="res-intro" type="webcontent" adlcp:scormtype="sco" href="intro/index.html"/>
             <resource identifier="res-lifting" type="webcontent" adlcp:scormtype="sco"
                       xml:base="lifting/" href="start%20here.html?mode=normal#top"/>
             <resource identifier="res-forklift" type="webcontent" adlcp:scormtype="sco" href="Forklift/Quiz.html"/>
             <resource identifier="res-handout" type="webcontent" adlcp:scormtype="asset" href="handout.pdf"/>
             <resource identifier="res-unused" type="webcontent" adlcp:scormtype="asset" href="not-in-archive.html"/>
           </resources>"#,
    )
}

fn multi_lesson_package() -> Vec<Entry> {
    vec![
        file("imsmanifest.xml", multi_lesson_manifest()).deflated(),
        file("content/", ""),
        file("content/intro/index.html", "<html>intro</html>").deflated(),
        file("content/lifting/start here.html", "<html>lifting</html>"),
        file("content/forklift/quiz.html", "<html>quiz</html>"),
        file("content/handout.pdf", "%PDF-1.4"),
    ]
}

#[test]
fn a_multi_lesson_scorm12_package_imports_as_a_tree_of_its_default_organization() {
    let package = import(&multi_lesson_package()).unwrap();

    assert_eq!(package.title, "Warehouse induction");
    assert_eq!(package.tree.id, "main");
    let top: Vec<&str> = package
        .tree
        .children
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(top, ["welcome", "chapter-1", "handout"]);

    let chapter = &package.tree.children[1];
    assert_eq!(chapter.title, "Moving loads");
    assert!(chapter.launch.is_none(), "a heading launches nothing");
    let lessons: Vec<&str> = chapter.children.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(lessons, ["lifting", "forklift"]);
    assert_eq!(chapter.children[0].title, "Lifting & carrying");

    let order: Vec<&str> = package
        .launchable_activities()
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(order, ["welcome", "lifting", "forklift", "handout"]);
}

#[test]
fn a_lesson_carries_what_the_manifest_gives_the_run_time() {
    let package = import(&multi_lesson_package()).unwrap();
    let chapter = &package.tree.children[1];

    let lifting = &chapter.children[0];
    assert_eq!(lifting.prerequisites.as_deref(), Some("welcome"));
    assert!(lifting.visible);
    let launch = lifting.launch.as_ref().unwrap();
    assert_eq!(launch.kind, LaunchKind::Sco);
    assert_eq!(
        launch.launch_data.as_deref(),
        Some("site=north;shift=night")
    );
    assert_eq!(launch.mastery_score, Some(80.0));
    assert_eq!(launch.max_time_allowed.as_deref(), Some("00:30:00"));
    assert_eq!(launch.time_limit_action.as_deref(), Some("exit,message"));

    let forklift = &chapter.children[1];
    assert!(
        !forklift.visible,
        "isvisible=false keeps it out of the contents"
    );
    assert_eq!(forklift.launch.as_ref().unwrap().mastery_score, Some(62.5));

    let welcome = package.tree.children[0].launch.as_ref().unwrap();
    assert_eq!(welcome.launch_data, None);
    assert_eq!(welcome.mastery_score, None);
    assert_eq!(welcome.max_time_allowed, None);
    assert_eq!(welcome.time_limit_action, None);

    let handout = package.tree.children[2].launch.as_ref().unwrap();
    assert_eq!(handout.kind, LaunchKind::Asset);
}

#[test]
fn a_launch_names_the_archive_file_and_the_rest_of_its_address() {
    let package = import(&multi_lesson_package()).unwrap();
    let chapter = &package.tree.children[1];

    // xml:base on <resources>, then the href.
    let welcome = package.tree.children[0].launch.as_ref().unwrap();
    assert_eq!(welcome.path, "content/intro/index.html");
    assert_eq!(welcome.parameters.as_deref(), Some("?lang=en"));

    // xml:base on <resources> and on <resource>, an escaped space, and the
    // href's own query and fragment kept apart from the path.
    let lifting = chapter.children[0].launch.as_ref().unwrap();
    assert_eq!(lifting.path, "content/lifting/start here.html");
    assert_eq!(lifting.parameters.as_deref(), Some("?mode=normal#top"));

    // The manifest says Forklift/Quiz.html; the archive holds forklift/quiz.html.
    let forklift = chapter.children[1].launch.as_ref().unwrap();
    assert_eq!(forklift.path, "content/forklift/quiz.html");
    assert_eq!(forklift.parameters, None);
}

#[test]
fn item_parameters_join_the_resource_address_as_the_standard_lays_out() {
    let parameters_for = |href: &str, parameters: &str| {
        let manifest = scorm12_manifest(
            &format!(
                r#"<organizations><organization identifier="org"><title>T</title>
                     <item identifier="i" identifierref="r" parameters="{parameters}"><title>L</title></item>
                   </organization></organizations>"#
            ),
            &format!(
                r#"<resources><resource identifier="r" adlcp:scormtype="sco" href="{href}"/></resources>"#
            ),
        );
        let package = import(&[file("imsmanifest.xml", manifest), file("a.html", "")]).unwrap();
        package.tree.children[0]
            .launch
            .as_ref()
            .unwrap()
            .parameters
            .clone()
    };

    assert_eq!(parameters_for("a.html", ""), None);
    assert_eq!(parameters_for("a.html", "x=1"), Some("?x=1".into()));
    assert_eq!(parameters_for("a.html", "?x=1"), Some("?x=1".into()));
    assert_eq!(parameters_for("a.html", "&amp;x=1"), Some("?x=1".into()));
    assert_eq!(parameters_for("a.html?y=2", "x=1"), Some("?y=2&x=1".into()));
    assert_eq!(parameters_for("a.html", "#end"), Some("#end".into()));
    assert_eq!(parameters_for("a.html#top", "#end"), Some("#top".into()));
    assert_eq!(parameters_for("a.html#top", "x=1"), Some("?x=1#top".into()));
}

#[test]
fn a_description_survives_being_stored_as_json() {
    let package = import(&multi_lesson_package()).unwrap();
    let json = serde_json::to_value(&package).unwrap();

    assert_eq!(json["standard"], "scorm12");
    assert_eq!(json["title"], "Warehouse induction");
    assert_eq!(json["files"], 5);
    assert_eq!(
        json["tree"]["children"][1]["children"][0],
        serde_json::json!({
            "id": "lifting",
            "title": "Lifting & carrying",
            "visible": true,
            "prerequisites": "welcome",
            "launch": {
                "path": "content/lifting/start here.html",
                "parameters": "?mode=normal#top",
                "kind": "sco",
                "launch_data": "site=north;shift=night",
                "mastery_score": 80.0,
                "max_time_allowed": "00:30:00",
                "time_limit_action": "exit,message",
            },
        })
    );

    let restored: PackageDescription = serde_json::from_value(json).unwrap();
    assert_eq!(restored, package);
}

#[test]
fn a_description_reports_what_the_archive_really_holds() {
    let package = import(&[
        file("imsmanifest.xml", single_lesson_manifest()),
        file("index.html", "0123456789"),
        file("media/", ""),
        file("media/clip.bin", vec![7u8; 5000]).deflated(),
    ])
    .unwrap();

    assert_eq!(package.files, 3);
    assert_eq!(
        package.uncompressed_bytes,
        single_lesson_manifest().len() as u64 + 10 + 5000
    );
}

#[test]
fn a_manifest_with_a_byte_order_mark_is_read() {
    let mut manifest = b"\xEF\xBB\xBF".to_vec();
    manifest.extend_from_slice(single_lesson_manifest().as_bytes());
    let package = import(&[file("imsmanifest.xml", manifest), file("index.html", "")]).unwrap();
    assert_eq!(package.title, "Fire safety");
}

#[test]
fn a_manifest_that_declares_latin_1_is_read_as_latin_1() {
    let manifest = single_lesson_manifest()
        .replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"")
        .replace("Fire safety", "S\u{e9}curit\u{e9}");
    let latin_1: Vec<u8> = manifest.chars().map(|c| c as u8).collect();
    let package = import(&[file("imsmanifest.xml", latin_1), file("index.html", "")]).unwrap();
    assert_eq!(package.title, "S\u{e9}curit\u{e9}");
}

// ---- refusals: the manifest -------------------------------------------------

#[test]
fn a_manifest_one_folder_down_is_no_manifest_and_the_refusal_says_where_it_is() {
    let refused = refusal(&[
        file("course/imsmanifest.xml", single_lesson_manifest()),
        file("course/index.html", ""),
    ]);
    assert_eq!(refused.code, RefusalCode::NoManifest);
    assert!(
        refused.detail.contains("course/imsmanifest.xml"),
        "{}",
        refused.detail
    );
}

#[test]
fn a_manifest_that_is_not_well_formed_is_refused_as_unparseable() {
    let cut_short = single_lesson_manifest().replace("</manifest>", "");
    let refused = refusal(&[file("imsmanifest.xml", cut_short), file("index.html", "")]);
    assert_eq!(refused.code, RefusalCode::ManifestUnparseable);
    assert_eq!(refused.code.as_str(), "manifest_unparseable");
}

#[test]
fn a_manifest_that_is_well_formed_but_broken_is_refused_as_unparseable() {
    let broken = [
        // Nothing to play.
        single_lesson_manifest().replace("organization", "organisation"),
        // Run-time state is keyed by item identifier: no identifier, no key.
        single_lesson_manifest().replace(r#"<item identifier="item-1""#, "<item"),
        // An item pointing at a resource that is not defined.
        single_lesson_manifest().replace(r#"identifierref="res-1""#, r#"identifierref="res-9""#),
        // A passing score that is not a score.
        single_lesson_manifest().replace(
            "</item>",
            "<adlcp:masteryscore>most of it</adlcp:masteryscore></item>",
        ),
        single_lesson_manifest().replace(
            "</item>",
            "<adlcp:masteryscore>180</adlcp:masteryscore></item>",
        ),
        // A resource that is neither of the two things it can be.
        single_lesson_manifest().replace(r#"scormtype="sco""#, r#"scormtype="quiz""#),
        // An entity nobody defined.
        single_lesson_manifest().replace("Fire safety", "Fire &safety;"),
        // An entity the document defines for itself, which is how expansion
        // attacks start. Declarations are not read, so it is undefined too.
        single_lesson_manifest()
            .replace(
                "<manifest ",
                r#"<!DOCTYPE manifest [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;">]><manifest "#,
            )
            .replace("Fire safety", "&b;&b;&b;&b;"),
    ];
    for manifest in broken {
        let refused = refusal(&[
            file("imsmanifest.xml", manifest.as_str()),
            file("index.html", ""),
        ]);
        assert_eq!(
            refused.code,
            RefusalCode::ManifestUnparseable,
            "{}\n{manifest}",
            refused.detail
        );
    }
}

#[test]
fn items_nested_deeper_than_any_table_of_contents_are_refused() {
    let nested = |depth: usize| {
        let open: String = (0..depth)
            .map(|n| format!(r#"<item identifier="level-{n}"><title>Level {n}</title>"#))
            .collect();
        let manifest = scorm12_manifest(
            &format!(
                r#"<organizations><organization identifier="org"><title>T</title>
                     {open}<item identifier="leaf" identifierref="r"><title>Leaf</title></item>{}
                   </organization></organizations>"#,
                "</item>".repeat(depth)
            ),
            r#"<resources><resource identifier="r" adlcp:scormtype="sco" href="a.html"/></resources>"#,
        );
        import(&[file("imsmanifest.xml", manifest), file("a.html", "")])
    };

    let package = nested(12).expect("a dozen levels is deep but fine");
    assert_eq!(package.launchable_activities()[0].id, "leaf");

    let refused = nested(200).expect_err("two hundred levels is not a course");
    assert_eq!(refused.code, RefusalCode::ManifestUnparseable);
}

#[test]
fn two_items_with_one_identifier_are_refused() {
    let manifest = scorm12_manifest(
        r#"<organizations><organization identifier="org"><title>T</title>
             <item identifier="same" identifierref="r"><title>One</title></item>
             <item identifier="same" identifierref="r"><title>Two</title></item>
           </organization></organizations>"#,
        r#"<resources><resource identifier="r" adlcp:scormtype="sco" href="a.html"/></resources>"#,
    );
    let refused = refusal(&[file("imsmanifest.xml", manifest), file("a.html", "")]);
    assert_eq!(refused.code, RefusalCode::ManifestUnparseable);
    assert!(refused.detail.contains("same"), "{}", refused.detail);
}

// ---- refusals: the standard ------------------------------------------------

/// A SCORM 2004 manifest declaring the given schema version.
fn scorm2004_manifest(schemaversion: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.2004" version="1"
    xmlns="http://www.imsglobal.org/xsd/imscp_v1p1"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_v1p3"
    xmlns:adlseq="http://www.adlnet.org/xsd/adlseq_v1p3"
    xmlns:adlnav="http://www.adlnet.org/xsd/adlnav_v1p3"
    xmlns:imsss="http://www.imsglobal.org/xsd/imsss">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>{schemaversion}</schemaversion>
  </metadata>
  <organizations default="org">
    <organization identifier="org">
      <title>Sequenced course</title>
      <item identifier="item-1" identifierref="res-1">
        <title>Lesson</title>
        <imsss:sequencing><imsss:controlMode choice="true" flow="true"/></imsss:sequencing>
      </item>
    </organization>
  </organizations>
  <resources>
    <resource identifier="res-1" type="webcontent" adlcp:scormType="sco" href="index.html"/>
  </resources>
</manifest>"#
    )
}

#[test]
fn scorm_2004_is_recognised_with_its_edition_and_refused_as_unsupported() {
    use alexandria_studio::package::Edition;

    let editions = [
        ("CAM 1.3", Some(Edition::Second)),
        ("2004 3rd Edition", Some(Edition::Third)),
        ("2004 4th Edition", Some(Edition::Fourth)),
        // An edition this code has never heard of is still SCORM 2004.
        ("2004 9th Edition", None),
    ];
    for (schemaversion, edition) in editions {
        let refused = refusal(&[
            file("imsmanifest.xml", scorm2004_manifest(schemaversion)),
            file("index.html", ""),
        ]);
        assert_eq!(refused.code, RefusalCode::UnsupportedStandard);
        assert_eq!(refused.code.as_str(), "unsupported_standard");
        assert_eq!(
            refused.standard,
            Some(Standard::Scorm2004),
            "{schemaversion}"
        );
        assert_eq!(refused.edition, edition, "{schemaversion}");
    }
}

#[test]
fn a_cmi5_package_is_recognised_and_refused_as_unsupported() {
    let refused = refusal(&[
        file(
            "cmi5.xml",
            r#"<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
                 <course id="https://example.org/course"><title><langstring lang="en">C</langstring></title></course>
                 <au id="https://example.org/au" launchMethod="AnyWindow"><url>index.html</url></au>
               </courseStructure>"#,
        ),
        file("index.html", ""),
    ]);
    assert_eq!(refused.code, RefusalCode::UnsupportedStandard);
    assert_eq!(refused.standard, Some(Standard::Cmi5));
    assert_eq!(refused.edition, None);
}

#[test]
fn a_content_package_that_claims_no_scorm_version_is_refused_as_unsupported() {
    let manifest = r#"<?xml version="1.0"?>
<manifest identifier="plain" xmlns="http://www.imsglobal.org/xsd/imscp_v1p1">
  <organizations><organization identifier="org"><title>T</title>
    <item identifier="i" identifierref="r"><title>L</title></item>
  </organization></organizations>
  <resources><resource identifier="r" type="webcontent" href="index.html"/></resources>
</manifest>"#;
    let refused = refusal(&[file("imsmanifest.xml", manifest), file("index.html", "")]);
    assert_eq!(refused.code, RefusalCode::UnsupportedStandard);
    assert_eq!(refused.standard, None);
}

#[test]
fn a_scorm12_manifest_is_recognised_by_its_namespace_when_it_declares_no_version() {
    let manifest = single_lesson_manifest().replace("<schemaversion>1.2</schemaversion>", "");
    let package = import(&[file("imsmanifest.xml", manifest), file("index.html", "")]).unwrap();
    assert_eq!(package.standard, Standard::Scorm12);
}

// ---- refusals: what an activity launches -----------------------------------

#[test]
fn a_lesson_whose_file_is_not_in_the_archive_is_refused() {
    let refused = refusal(&[
        file("imsmanifest.xml", single_lesson_manifest()),
        file("other.html", ""),
    ]);
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
    assert_eq!(refused.code.as_str(), "launch_resource_missing");
    assert_eq!(refused.standard, Some(Standard::Scorm12));
    assert!(refused.detail.contains("item-1"), "{}", refused.detail);
    assert!(refused.detail.contains("index.html"), "{}", refused.detail);
}

#[test]
fn a_lesson_whose_address_climbs_out_of_the_package_is_refused_as_missing() {
    let manifest =
        single_lesson_manifest().replace(r#"href="index.html""#, r#"href="../index.html""#);
    let refused = refusal(&[file("imsmanifest.xml", manifest), file("index.html", "")]);
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
}

#[test]
fn a_lesson_whose_resource_names_no_file_is_refused_as_missing() {
    let manifest = single_lesson_manifest().replace(r#" href="index.html">"#, ">");
    let refused = refusal(&[file("imsmanifest.xml", manifest), file("index.html", "")]);
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
}

#[test]
fn a_package_with_nothing_to_launch_is_refused_as_missing() {
    let manifest = single_lesson_manifest().replace(r#" identifierref="res-1""#, "");
    let refused = refusal(&[file("imsmanifest.xml", manifest), file("index.html", "")]);
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
}

#[test]
fn a_lesson_that_launches_something_outside_the_archive_is_refused_as_remote() {
    let remote = [
        r#"href="https://cdn.example.org/course/index.html""#,
        r#"href="//cdn.example.org/course/index.html""#,
        r#"href="file:///etc/passwd""#,
        r#"href="javascript:alert(1)""#,
        // The file is in the archive, but the base moves the address off it.
        r#"xml:base="https://cdn.example.org/course/" href="index.html""#,
    ];
    for address in remote {
        let manifest =
            single_lesson_manifest().replace(r#"href="index.html">"#, &format!("{address}>"));
        let refused = refusal(&[file("imsmanifest.xml", manifest), file("index.html", "")]);
        assert_eq!(refused.code, RefusalCode::LaunchResourceRemote, "{address}");
        assert_eq!(refused.code.as_str(), "launch_resource_remote");
    }
}

#[test]
fn a_resource_no_activity_launches_may_be_missing_or_remote() {
    let manifest = single_lesson_manifest().replace(
        "</resources>",
        r#"<resource identifier="spare" type="webcontent" adlcp:scormtype="asset" href="https://example.org/x"/>
           <resource identifier="gone" type="webcontent" adlcp:scormtype="asset" href="gone.html"/>
           </resources>"#,
    );
    import(&[file("imsmanifest.xml", manifest), file("index.html", "")]).unwrap();
}

// ---- refusals: the archive --------------------------------------------------

/// A sound single-lesson package with one more entry added to it.
fn package_with(extra: Entry) -> Vec<Entry> {
    vec![
        file("imsmanifest.xml", single_lesson_manifest()),
        file("index.html", "<html></html>"),
        extra,
    ]
}

const SYMLINK: u32 = 0o120777;
const FIFO: u32 = 0o010644;

fn unsafe_entries() -> Vec<(&'static str, Entry)> {
    vec![
        ("absolute", file("/etc/cron.d/owned", "x")),
        ("parent", file("../owned.txt", "x")),
        ("parent, further in", file("media/../../owned.txt", "x")),
        (
            "parent, Windows separators",
            file("media\\..\\..\\owned.txt", "x"),
        ),
        ("absolute, Windows separator", file("\\owned.txt", "x")),
        ("drive", file("C:\\Users\\Public\\owned.txt", "x")),
        ("drive, relative", file("C:owned.txt", "x")),
        ("control character", file("media/own\ned.txt", "x")),
        (
            "link out",
            file("media/link", "/etc/passwd").unix_mode(SYMLINK),
        ),
        (
            "link within",
            file("media/link", "../index.html").unix_mode(SYMLINK),
        ),
        ("device or pipe", file("media/pipe", "").unix_mode(FIFO)),
        ("same file twice, by case", file("INDEX.html", "x")),
        (
            "a file where a folder is",
            file("index.html/inside.txt", "x"),
        ),
        (
            "a name no filesystem holds",
            file(&format!("media/{}.txt", "n".repeat(300)), "x"),
        ),
    ]
}

#[test]
fn an_archive_with_an_unsafe_entry_is_refused() {
    for (what, entry) in unsafe_entries() {
        let refused = refusal(&package_with(entry));
        assert_eq!(
            refused.code,
            RefusalCode::UnsafeEntryPath,
            "{what}: {}",
            refused.detail
        );
        assert_eq!(refused.code.as_str(), "unsafe_entry_path");
    }
}

#[test]
fn entry_names_that_only_look_odd_are_accepted_and_tidied() {
    let package = import(&[
        file("./imsmanifest.xml", single_lesson_manifest()),
        file(".\\index.html", "<html></html>"),
        file("media//clip..mp4", "x"),
        file("media/...", "x"),
    ])
    .unwrap();
    assert_eq!(
        package.tree.children[0].launch.as_ref().unwrap().path,
        "index.html"
    );
    assert_eq!(package.files, 4);
}

#[test]
fn the_starting_limits_are_the_ones_the_spec_names() {
    let limits = Limits::default();
    assert_eq!(limits.max_uncompressed_bytes, 1024 * 1024 * 1024);
    assert_eq!(limits.max_entries, 20_000);
    assert_eq!(limits.max_compression_ratio, 200);
}

fn package_of_entries(count: usize) -> Vec<Entry> {
    let mut entries = vec![
        file("imsmanifest.xml", single_lesson_manifest()),
        file("index.html", "<html></html>"),
    ];
    entries.extend((entries.len()..count).map(|n| file(&format!("media/{n}.txt"), "")));
    entries
}

#[test]
fn an_archive_with_more_entries_than_the_limit_is_refused() {
    import(&package_of_entries(20_000)).expect("exactly at the limit is allowed");

    let refused = refusal(&package_of_entries(20_001));
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
    assert_eq!(refused.code.as_str(), "limit_exceeded");
    assert!(refused.detail.contains("20000"), "{}", refused.detail);
}

#[test]
fn a_directory_far_past_the_entry_limit_is_refused_before_it_is_loaded() {
    let limits = Limits {
        max_entries: 10,
        ..Limits::default()
    };
    let archive = zip(&package_of_entries(60_000));
    let refused = import_package(Cursor::new(archive), &limits).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
}

#[test]
fn an_archive_that_holds_more_than_the_size_limit_is_refused() {
    let limits = Limits {
        max_uncompressed_bytes: 64 * 1024,
        ..Limits::default()
    };
    let noise = |seed: u32, len: usize| -> Vec<u8> {
        // Incompressible enough that the ratio limit stays out of the way.
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    };

    let under = package_with(file("media/a.bin", noise(1, 60 * 1024)).deflated());
    import_package(Cursor::new(zip(&under)), &limits).expect("under the limit");

    let over = package_with(file("media/a.bin", noise(1, 70 * 1024)).deflated());
    let refused = import_package(Cursor::new(zip(&over)), &limits).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);

    // No single file is over, but together they are.
    let mut spread = package_with(file("media/a.bin", noise(1, 30 * 1024)));
    spread.push(file("media/b.bin", noise(2, 30 * 1024)));
    spread.push(file("media/c.bin", noise(3, 30 * 1024)));
    let refused = import_package(Cursor::new(zip(&spread)), &limits).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
}

#[test]
fn an_archive_that_decompresses_past_the_ratio_limit_is_refused() {
    // 8 MiB of zeros deflates to a few kilobytes: far past 200 to 1.
    let bomb = package_with(file("media/blank.bin", vec![0u8; 8 << 20]).deflated());
    let archive = zip(&bomb);
    assert!(archive.len() < 32 * 1024, "the fixture should be tiny");

    let refused = import_package(Cursor::new(archive), &Limits::default()).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
    assert!(refused.detail.contains("200"), "{}", refused.detail);
}

#[test]
fn a_file_that_holds_more_than_it_declares_is_refused() {
    // Declared sizes are what the limits are first checked against, so a
    // file must not be allowed to turn out larger than it said.
    let bomb = package_with(
        file("media/blank.bin", vec![0u8; 8 << 20])
            .deflated()
            .declaring_size(12),
    );
    let refused = refusal(&bomb);
    assert_eq!(refused.code, RefusalCode::NotAnArchive, "{refused}");
    assert!(refused.detail.contains("media/blank.bin"), "{refused}");
}

#[test]
fn an_archive_with_a_damaged_file_is_refused() {
    let mut archive = zip(&package_with(file("media/a.txt", "the quick brown fox")));
    let at = archive
        .windows(5)
        .position(|window| window == b"quick")
        .unwrap();
    archive[at] = b'Q';

    let refused = import_package(Cursor::new(archive), &Limits::default()).unwrap_err();
    assert_eq!(refused.code, RefusalCode::NotAnArchive);
    assert!(refused.detail.contains("media/a.txt"), "{}", refused.detail);
}

#[test]
fn an_archive_with_a_file_this_build_cannot_decode_is_refused() {
    let unreadable = [
        (
            "encrypted",
            file("media/a.bin", "secret").flagged_encrypted(),
        ),
        // Method 14 is LZMA, which is not built in.
        ("lzma", file("media/a.bin", "data").claiming_method(14)),
    ];
    for (what, entry) in unreadable {
        let refused = refusal(&package_with(entry));
        assert_eq!(refused.code, RefusalCode::NotAnArchive, "{what}");
        assert!(refused.detail.contains("media/a.bin"), "{what}: {refused}");
    }
}

#[test]
fn a_manifest_too_large_to_be_a_table_of_contents_is_refused() {
    // Padding compresses to almost nothing, so lift the ratio limit out of
    // the way to reach the manifest's own cap.
    let limits = Limits {
        max_compression_ratio: u64::MAX,
        ..Limits::default()
    };
    let padded = single_lesson_manifest().replace(
        "</manifest>",
        &format!("{}</manifest>", " ".repeat(17 << 20)),
    );
    let archive = zip(&[
        file("imsmanifest.xml", padded).deflated(),
        file("index.html", ""),
    ]);
    let refused = import_package(Cursor::new(archive), &limits).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
    assert!(refused.detail.contains("imsmanifest.xml"), "{refused}");
}

#[test]
fn a_refusal_reads_as_its_code_then_its_reason_and_stores_as_json() {
    let refused = refusal(&[
        file("imsmanifest.xml", single_lesson_manifest()),
        file("other.html", ""),
    ]);

    assert!(
        refused.to_string().starts_with("launch_resource_missing: "),
        "{refused}"
    );
    let json = serde_json::to_value(&refused).unwrap();
    assert_eq!(json["code"], "launch_resource_missing");
    assert_eq!(json["standard"], "scorm12");
    assert_eq!(json["detail"], refused.detail);
    assert!(json.get("edition").is_none());
    assert_eq!(serde_json::from_value::<Refusal>(json).unwrap(), refused);
}

// ---- extraction ---------------------------------------------------------------

mod extraction {
    use super::*;
    use alexandria_studio::package::{extract_package, ExtractError, ExtractProgress};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn extract(entries: &[Entry], target: &Path) -> Result<(u64, u64), ExtractError> {
        extract_package(
            Cursor::new(zip(entries)),
            target,
            &Limits::default(),
            &mut |_| {},
        )
        .map(|summary| (summary.files, summary.bytes))
    }

    /// Every file under `root`, as sorted `/`-separated relative paths.
    fn files_under(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, found: &mut Vec<String>) {
            let Ok(children) = fs::read_dir(dir) else {
                return;
            };
            for child in children {
                let path = child.unwrap().path();
                // `symlink_metadata` so that a link would be reported, not followed.
                if fs::symlink_metadata(&path).unwrap().is_dir() {
                    walk(&path, root, found);
                } else {
                    let relative: PathBuf = path.strip_prefix(root).unwrap().into();
                    found.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut found = Vec::new();
        walk(root, root, &mut found);
        found.sort();
        found
    }

    #[test]
    fn a_package_extracts_to_the_paths_its_launches_name() {
        let scratch = tempfile::tempdir().unwrap();
        let target = scratch.path().join("cache").join("package");
        let entries = multi_lesson_package();

        let (files, bytes) = extract(&entries, &target).unwrap();

        let package = import(&entries).unwrap();
        assert_eq!(files, package.files);
        assert_eq!(bytes, package.uncompressed_bytes);
        for activity in package.launchable_activities() {
            let launch = activity.launch.as_ref().unwrap();
            assert!(target.join(&launch.path).is_file(), "{}", launch.path);
        }
        assert_eq!(
            fs::read_to_string(target.join("content/intro/index.html")).unwrap(),
            "<html>intro</html>"
        );
        assert_eq!(
            files_under(&target),
            [
                "content/forklift/quiz.html",
                "content/handout.pdf",
                "content/intro/index.html",
                "content/lifting/start here.html",
                "imsmanifest.xml",
            ]
        );
    }

    #[test]
    fn extraction_reports_each_file_as_it_lands_and_large_files_as_they_grow() {
        let scratch = tempfile::tempdir().unwrap();
        let big = vec![0x5au8; 3 << 20];
        let entries = [
            file("imsmanifest.xml", single_lesson_manifest()),
            file("index.html", "<html></html>"),
            file("media/video.bin", &big),
        ];
        let mut reports: Vec<(String, bool, u64, u64)> = Vec::new();

        let summary = extract_package(
            Cursor::new(zip(&entries)),
            scratch.path(),
            &Limits::default(),
            &mut |progress: &ExtractProgress<'_>| {
                reports.push((
                    progress.path.to_owned(),
                    progress.file_complete,
                    progress.files_done,
                    progress.bytes_done,
                ));
            },
        )
        .unwrap();

        let completed: Vec<&str> = reports
            .iter()
            .filter(|report| report.1)
            .map(|report| report.0.as_str())
            .collect();
        assert_eq!(
            completed,
            ["imsmanifest.xml", "index.html", "media/video.bin"]
        );
        let partial = reports.iter().filter(|report| !report.1).count();
        assert!(
            partial >= 2,
            "a 3 MiB file reports along the way: {partial}"
        );
        assert!(reports.windows(2).all(|pair| pair[0].3 <= pair[1].3));
        assert_eq!(
            reports.last().unwrap(),
            &("media/video.bin".to_owned(), true, 3, summary.bytes)
        );
        assert_eq!(summary.files, 3);
        assert_eq!(
            summary.bytes,
            (single_lesson_manifest().len() + "<html></html>".len() + big.len()) as u64
        );
    }

    #[test]
    fn a_hostile_archive_writes_nothing_inside_or_outside_the_target() {
        for (what, entry) in unsafe_entries() {
            let scratch = tempfile::tempdir().unwrap();
            let target = scratch.path().join("a").join("b").join("target");

            let error = extract(&package_with(entry), &target).unwrap_err();

            match error {
                ExtractError::Refused(refused) => {
                    assert_eq!(refused.code, RefusalCode::UnsafeEntryPath, "{what}")
                }
                other => panic!("{what}: {other}"),
            }
            // The sound entries ahead of the unsafe one were not written
            // either, and nothing appeared beside or above the target.
            assert_eq!(files_under(scratch.path()), Vec::<String>::new(), "{what}");
        }
    }

    #[test]
    fn extraction_never_creates_a_link() {
        // Without Unix mode bits there is nothing to mark an entry as a link,
        // so its content is written as an ordinary file.
        let scratch = tempfile::tempdir().unwrap();
        let mut entries = package_with(file("media/shortcut", "/etc/passwd"));
        entries.push(file("media/shortcut.lnk", "../../outside"));

        extract(&entries, scratch.path()).unwrap();

        let shortcut = scratch.path().join("media/shortcut");
        assert!(fs::symlink_metadata(&shortcut).unwrap().is_file());
        assert_eq!(fs::read_to_string(&shortcut).unwrap(), "/etc/passwd");
    }

    #[test]
    fn a_file_that_holds_more_than_it_declares_stops_extraction_and_leaves_the_target_empty() {
        let scratch = tempfile::tempdir().unwrap();
        let target = scratch.path().join("target");
        // The declared size passes the check made before writing starts, and
        // two sound files are on disk by the time the third gives itself away.
        let bomb = package_with(
            file("media/blank.bin", vec![0u8; 8 << 20])
                .deflated()
                .declaring_size(12),
        );

        let error = extract(&bomb, &target).unwrap_err();

        assert!(
            matches!(
                &error,
                ExtractError::Refused(refused) if refused.code == RefusalCode::NotAnArchive
            ),
            "{error}"
        );
        assert!(target.is_dir());
        assert_eq!(files_under(scratch.path()), Vec::<String>::new());
    }

    #[test]
    fn extraction_holds_the_size_limit_it_is_given() {
        let scratch = tempfile::tempdir().unwrap();
        let limits = Limits {
            max_uncompressed_bytes: 1024,
            ..Limits::default()
        };
        let entries = package_with(file("media/a.bin", vec![1u8; 4096]));

        let error = extract_package(
            Cursor::new(zip(&entries)),
            scratch.path(),
            &limits,
            &mut |_| {},
        )
        .unwrap_err();

        assert!(
            matches!(&error, ExtractError::Refused(refused) if refused.code == RefusalCode::LimitExceeded),
            "{error}"
        );
        assert_eq!(files_under(scratch.path()), Vec::<String>::new());
    }

    #[test]
    fn extraction_refuses_a_directory_that_already_holds_something() {
        let scratch = tempfile::tempdir().unwrap();
        fs::write(scratch.path().join("index.html"), "mine").unwrap();
        let entries = [
            file("imsmanifest.xml", single_lesson_manifest()),
            file("index.html", "theirs"),
        ];

        let error = extract(&entries, scratch.path()).unwrap_err();

        assert!(matches!(error, ExtractError::TargetNotEmpty), "{error}");
        assert_eq!(files_under(scratch.path()), ["index.html"]);
        assert_eq!(
            fs::read_to_string(scratch.path().join("index.html")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn extraction_does_not_need_a_manifest() {
        // Extraction is about the archive alone; whether it is a package is
        // import's question.
        let scratch = tempfile::tempdir().unwrap();
        let (files, _) = extract(&[file("notes/readme.txt", "hello")], scratch.path()).unwrap();
        assert_eq!(files, 1);
        assert_eq!(files_under(scratch.path()), ["notes/readme.txt"]);
    }
}
