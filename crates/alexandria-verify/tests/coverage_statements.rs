//! The coverage statement registry: every statement text ever published
//! resolves from its digest, offline, from this library alone.
//!
//! The digests below are pinned literals. A published digest is named inside
//! signed credentials, so it can never change: a text change adds a version.

use alexandria_verify::coverage;

/// Every version-1 statement, as the app issued them before the registry
/// moved here (no certification has ever been in force).
const VERSION_1: [(&str, &str, &str); 20] = [
    (
        "core",
        "none",
        "blake3:f47ff512a36d079b4c3df51a8f45489141394357f00aeecee2a64a5725001231",
    ),
    (
        "core",
        "process",
        "blake3:7d476081c1b73782ed056f352f027df8094113c88f686b998ad2f63567030df1",
    ),
    (
        "core",
        "probed",
        "blake3:f4646f51eca14bd973bc8e9721cf0a6c31edf9f8d52ad3e25d257861494e4e06",
    ),
    (
        "core",
        "consistent",
        "blake3:c8cf72a8adeb3e7bdcc37a5b0137014e44d2ee79163fe6ad1f7db95ed385ecb8",
    ),
    (
        "core",
        "live",
        "blake3:930f5029edd623f255a30c53ee31519ac7bc82e85fbf4f9d24bf67d9cec2779c",
    ),
    (
        "attested",
        "none",
        "blake3:fb2dc7bcb3cef4a80e3502a7690b98567570a0589176651d0bbc46585ef54fec",
    ),
    (
        "attested",
        "process",
        "blake3:1349b3b8d402a0e99d3e49ee79e2557d066132f1a4dea0d3df3d8898838f0f99",
    ),
    (
        "attested",
        "probed",
        "blake3:2db88768d641a739cb6d321f6d1019329eaf9bcac6f8fc7e2dddc3e21a6cae43",
    ),
    (
        "attested",
        "consistent",
        "blake3:f590e92c289057449bc197544aa56f72821d9566aa44291418f681015ca259f0",
    ),
    (
        "attested",
        "live",
        "blake3:db7f1c654d5adc8752853512e23446599c667cf53c3b8f2dbdd42a00d3257592",
    ),
    (
        "camera",
        "none",
        "blake3:c94b7d2342a017be648059cfe66dc8553d9794dbd45a9c1db94b9fec1287eae7",
    ),
    (
        "camera",
        "process",
        "blake3:3144acc048943848f8e75243112ea2993647ce3a9439966dcf38d57fd0aab943",
    ),
    (
        "camera",
        "probed",
        "blake3:4c32be90134810783abd8b9575bda05bf50f1abee0ab6b970e0d8f8ea6e10c12",
    ),
    (
        "camera",
        "consistent",
        "blake3:ff88f867927c61a96b413cdae5b054174295d9d2d9f6bda3fb77a94d95f044ba",
    ),
    (
        "camera",
        "live",
        "blake3:e34f4ce036d1d01bc997bcd1943a41ed298851643f9948db0beb4e094044ceeb",
    ),
    (
        "full",
        "none",
        "blake3:96584f5dea836452b48aff99132c2c1485ac62bda56b6d998355ad3a065ab7f3",
    ),
    (
        "full",
        "process",
        "blake3:7fceb7f67a0a8ab5c07a1822fc85b065ef69567ca00e60cbacf191ad770e63ed",
    ),
    (
        "full",
        "probed",
        "blake3:1fc0bed57f0fc99957e15453a8596737f4396c08e9b108e4cc29fe89e793f9e1",
    ),
    (
        "full",
        "consistent",
        "blake3:7d40925c77daa328088b6c1a84bbcdf07bd21d16ca98bb780188dfea4bb9a147",
    ),
    (
        "full",
        "live",
        "blake3:02c96f3b9718380b4bf73dbeb3e0ed7c992c86aaad0051ef64707ae781422c15",
    ),
];

/// Every version-2 statement, pinned when version 2 was published.
const VERSION_2: [(&str, &str, &str); 20] = [
    (
        "core",
        "none",
        "blake3:afb5c21c02a8943a22d514c40be452ec96eae2e64402fe70a046b953b1de0fe5",
    ),
    (
        "core",
        "process",
        "blake3:17b6ffedf0cfe387ad935003c549f54fe6a9e28ee1b4fe6813d267235ae6af1f",
    ),
    (
        "core",
        "probed",
        "blake3:ae861e3e33a2956ab22ee985c7f7d9d6c9171cba60a5eac40fb5013427b2d85c",
    ),
    (
        "core",
        "consistent",
        "blake3:cc8820d912efa0ee208515b02aaa784665bf8164c0300a2def8e321dd2d77725",
    ),
    (
        "core",
        "live",
        "blake3:f0295b83c3771112c07296d9814058b8831c586362f869953baff1e5612f8d32",
    ),
    (
        "attested",
        "none",
        "blake3:c906fd3c9b00761d0382dea2ee363ab6a767e8a5c80b746b1a0f5849aef00821",
    ),
    (
        "attested",
        "process",
        "blake3:ed8770eaefd9facb6e37d393033845c6ba8f89cd9ce8c0288f9410307acf642b",
    ),
    (
        "attested",
        "probed",
        "blake3:51339ae82ced1cdedb4ea024a152071fdb864e873c93bd75b764050dda52a34b",
    ),
    (
        "attested",
        "consistent",
        "blake3:042a029a759d703ccd48c7b82e6829c90efa2079e2b0690c415263b60fa3ffbc",
    ),
    (
        "attested",
        "live",
        "blake3:f5e15d6b4c189e657c8e1eef0f4027340b27367ec0f7df266879bef45dcbc8dd",
    ),
    (
        "camera",
        "none",
        "blake3:77ece1e54fbb8ab28dfeb019f99de345b4ecc318777c5459ee46936e59deb2bf",
    ),
    (
        "camera",
        "process",
        "blake3:8fdc46d79ea5076e93b80b966bcb080bcdc255193207902d62feb55b7c5a0a95",
    ),
    (
        "camera",
        "probed",
        "blake3:074046246b9f92eb9ae267fe55b3757a3a7b392a21b7c32df3c1285f6656fd8a",
    ),
    (
        "camera",
        "consistent",
        "blake3:0da1c339e70a6442e95fea948e6e587cc1cb87c0603ea9c74b1ba526fb56b952",
    ),
    (
        "camera",
        "live",
        "blake3:d68db7778e5b325bc8d074607fbe32ff49f39916c5c6ba82f33fea23546a1992",
    ),
    (
        "full",
        "none",
        "blake3:38e3019e9dce6d71e3cac7a192d1babdd96b212be27a09866561d3b7083bcf4f",
    ),
    (
        "full",
        "process",
        "blake3:780d83051a9f0f4272524d75469939f526663d88bd99428cade2abf6f64bdc48",
    ),
    (
        "full",
        "probed",
        "blake3:601d84e961a913ceda630b10e543f87f6a3f5e5ccbca834a9c1d76c00bcf9c5e",
    ),
    (
        "full",
        "consistent",
        "blake3:ce4437fb9d8b7e6721fb940eb1951e51e11adbc51b5d9125c0bcc223dc6e6f63",
    ),
    (
        "full",
        "live",
        "blake3:8d8b926851a007bb4cd4c8b50d309f3637bbaad9a1f989b85d38a16d0147622c",
    ),
];

/// Every version-3 statement, pinned when version 3 was published.
const VERSION_3: [(&str, &str, &str); 20] = [
    (
        "core",
        "none",
        "blake3:d61f0af610ccb1d6db46d29e1fd12b5cc325008f6f3d3dc7c96e2f95bfedeb09",
    ),
    (
        "core",
        "process",
        "blake3:1fe5fce8c6ea7d96b31078d298cc703200282e143ac64a58917321acbcca1bdb",
    ),
    (
        "core",
        "probed",
        "blake3:9524dd9bc1753dff56e82ba7dddf08189f4221d0ffdd5697df1bb7caed706b1f",
    ),
    (
        "core",
        "consistent",
        "blake3:1b65415d3bf273a5167997897d48f5d0f47e7453b8f0c1c76a5b2fdc4e1b0292",
    ),
    (
        "core",
        "live",
        "blake3:c87267f048b402c51643890c9b37ddefa85cf7c44c8ad520f59f83435d2d938f",
    ),
    (
        "attested",
        "none",
        "blake3:17c7e0626872cf03771e4e8d9fc495729e2fad29954a0ead97a5c17dbe35b850",
    ),
    (
        "attested",
        "process",
        "blake3:94368ae1a4f80ed88e2933993e7e9595dc2daa6fcf1d12a4f0c88dfdc81650e8",
    ),
    (
        "attested",
        "probed",
        "blake3:01c3d28b127f6cda7974f147d474f252b2e6ff0516b2c95119ea55622c17c45f",
    ),
    (
        "attested",
        "consistent",
        "blake3:c7c426e8db9344a188a740feb71a6d255d4155a1690988ca7754b1b808b75a10",
    ),
    (
        "attested",
        "live",
        "blake3:3b41d32cc250d61b808629f7fd74cd3257cf058e47fbe8a6b9df5285f9245fd3",
    ),
    (
        "camera",
        "none",
        "blake3:83e7c082aaa4c0d707c4ef06e0841f3f7da39b59f8e20d0ab6439459b7b841aa",
    ),
    (
        "camera",
        "process",
        "blake3:221c078394fae0280aec1529ff2529f9f581616b23187a4f490ce993e803c02a",
    ),
    (
        "camera",
        "probed",
        "blake3:39f4ba070d3ab4021a92c6c49bb1b17a9b90c98cb703bce9121bc6340bb1a229",
    ),
    (
        "camera",
        "consistent",
        "blake3:0e2f14cb88d52209a702fe19dfdec2086c63f482fafc72e345a1c2216e39a39e",
    ),
    (
        "camera",
        "live",
        "blake3:1bbca5f574c03e8e35791c3c6f325683ff34c2a7168bb458948329c4903669a1",
    ),
    (
        "full",
        "none",
        "blake3:b2ebc4be449c75a0605b438d75bc17c9effed51b08009deec4525ab246598ce1",
    ),
    (
        "full",
        "process",
        "blake3:9266b0a7d9f6c35be6809f2025a90f3e04d9136a9bafa27813c77806ba6d232e",
    ),
    (
        "full",
        "probed",
        "blake3:5c48a5834a0a7b6d725737a936c61efa0b2fee3743517c94a851ff99ccb17f70",
    ),
    (
        "full",
        "consistent",
        "blake3:62128aa771014ca369b040166970623b14b780ab15af422bda2dec4a6990bf11",
    ),
    (
        "full",
        "live",
        "blake3:c1add9f20ee153248b9ae0d4958e6c52dc417b962ef8bb8c104f55229c490d51",
    ),
];

/// A version's pinned table. A new version adds an arm and a table here.
fn pinned(version: &str) -> &'static [(&'static str, &'static str, &'static str); 20] {
    match version {
        "1" => &VERSION_1,
        "2" => &VERSION_2,
        "3" => &VERSION_3,
        other => panic!("version {other} is published but its digests are not pinned here"),
    }
}

#[test]
fn no_published_version_changes_a_digest() {
    for version in coverage::VERSIONS {
        for (profile, rung, digest) in pinned(version) {
            let statement = coverage::statement(version, profile, rung, None)
                .unwrap_or_else(|| panic!("version {version} {profile}/{rung} is gone"));
            assert_eq!(
                &statement.digest(),
                digest,
                "version {version} {profile}/{rung}"
            );
            assert_eq!(coverage::resolve(digest, None), Some(statement));
        }
    }
}

#[test]
fn every_version_1_digest_resolves_to_its_profile_and_rung() {
    for (profile, rung, digest) in VERSION_1 {
        let statement = coverage::resolve(digest, None)
            .unwrap_or_else(|| panic!("{profile}/{rung} {digest} does not resolve"));
        assert_eq!(statement.version, "1");
        assert_eq!(
            (statement.profile.as_str(), statement.rung.as_str()),
            (profile, rung)
        );
        assert_eq!(statement.digest(), digest);
    }
    assert_eq!(coverage::resolve("blake3:00", None), None);
}

#[test]
fn version_2_attestation_line_says_who_verified() {
    let full = coverage::statement("2", "full", "live", None).expect("v2 full/live");
    let line = full
        .checked
        .iter()
        .find(|l| l.contains("platform attestation"))
        .expect("attestation line");
    assert!(line.contains("verified by the issuing device"), "{line}");
    assert!(
        line.contains("relies on it only as far as the credential's attestation evidence verifies"),
        "{line}"
    );
    let v1 = coverage::statement("1", "full", "live", None).expect("v1 full/live");
    assert!(v1.checked.iter().any(|l| l.contains("verified again")));

    let resolved = coverage::resolve(&full.digest(), None).expect("v2 resolves");
    assert_eq!(resolved, full);
}

/// Version 3: below `live`, every statement says a second device or person
/// was not ruled out, since only a person asking unscripted questions rules
/// it out; and the probe line says the follow-up questions were ones the
/// learner had not already answered.
#[test]
fn new_sessions_use_version_3_which_says_below_live_a_second_device_was_not_ruled_out() {
    assert_eq!(coverage::CURRENT_VERSION, "3");
    let not_ruled_out = |s: &coverage::CoverageStatement| {
        s.not_checked
            .iter()
            .any(|l| l.contains("not ruled out") && l.contains("second device or person"))
    };
    for profile in coverage::PROFILES {
        for rung in coverage::RUNGS {
            let v3 = coverage::statement("3", profile, rung, None).expect("v3 statement");
            assert_eq!(not_ruled_out(&v3), rung != "live", "{profile}/{rung}");
            let v2 = coverage::statement("2", profile, rung, None).expect("v2 statement");
            assert!(!not_ruled_out(&v2), "v2 {profile}/{rung} is unchanged");
        }
    }
    let probed = coverage::statement("3", "core", "probed", None).expect("v3 core/probed");
    assert!(probed
        .checked
        .iter()
        .any(|l| l.contains("had not already answered")));
    let process = coverage::statement("3", "core", "process", None).expect("v3 core/process");
    assert!(process
        .not_checked
        .iter()
        .any(|l| l.contains("had not already answered")));
}

#[test]
fn every_published_version_resolves_every_profile_and_rung_and_digests_are_distinct() {
    let mut digests = Vec::new();
    for version in coverage::VERSIONS {
        let statements = coverage::statements(version, None);
        assert_eq!(statements.len(), 4 * 5, "version {version}");
        for statement in statements {
            assert!(statement
                .not_checked
                .iter()
                .any(|l| l.contains("second person") && l.contains("second device")));
            assert_eq!(
                coverage::resolve(&statement.digest(), None),
                Some(statement.clone())
            );
            digests.push(statement.digest());
        }
    }
    let total = digests.len();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), total, "no two statements share a digest");
}

#[test]
fn unknown_versions_profiles_and_rungs_have_no_statement() {
    assert_eq!(coverage::statement("0", "core", "none", None), None);
    assert_eq!(coverage::statement("2", "remote", "none", None), None);
    assert_eq!(coverage::statement("2", "core", "proctored", None), None);
    assert_eq!(coverage::statement("4", "core", "none", None), None);
}
