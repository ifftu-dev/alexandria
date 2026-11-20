//! Serving standards package content to the player, in isolation.
//!
//! Package script is untrusted. Its files are served under a dedicated
//! scheme ([`SCHEME`]), separate from the plugin scheme and the app's own
//! origin, and only under a launch prefix:
//!
//! - **Launch-scoped addresses.** [`open_launch`] extracts a package if
//!   needed and binds a fresh, unguessable path prefix to its extracted
//!   files. [`Launches::close`] retires the prefix. The handler answers
//!   nothing outside an open launch, so a package cannot read another
//!   package's files, or its own after the launch ends, even on the
//!   platforms where every package shares one web origin.
//! - **The handler** ([`serve`]) is asynchronous, reads only the requested
//!   byte range from disk and caps whole-file responses ([`ServeLimits`]).
//!   It is a plain request-to-response function, so it is tested without a
//!   webview.
//! - **Every response** carries the package content policy and refuses
//!   device capabilities by header. Every HTML response links the bootstrap
//!   (`bootstrap.js`) as its first element; it removes the command bridge and
//!   the network-capable interfaces the policy does not govern.
//! - **A reserved path** under every launch prefix, [`RESERVED_SEGMENT`], is
//!   answered from files built into the app, never from the package: the
//!   bootstrap, the storage-clearing page and the vendored run-time library.
//!   A package file at that path is unreachable.
//!
//! URL shapes, from Tauri's custom-scheme rules: `package://localhost/<prefix>/<path>`
//! on macOS, iOS and Linux, `http://package.localhost/<prefix>/<path>` on
//! Windows and Android. Both reach the handler as the first form.

mod handler;
mod launches;

#[cfg(test)]
mod tests;

pub use handler::{serve, ServeLimits};
pub use launches::Launches;

use crate::content_store::node::ContentNode;
use crate::domain::package::PackageExtractProgress;
use crate::domain::package_launch::OpenedPackageLaunch;
use crate::packages::{self, PackageDb, PackageDirs, PackageError};

/// The URI scheme package content is served under.
pub const SCHEME: &str = "package";

/// The first path segment under a launch prefix that serves the app's own
/// files instead of the package's.
pub const RESERVED_SEGMENT: &str = "_alexandria";

/// The origin package pages see on this platform.
pub fn origin() -> &'static str {
    if cfg!(any(target_os = "android", target_os = "windows")) {
        "http://package.localhost"
    } else {
        "package://localhost"
    }
}

/// Opens a launch of the package with this hash: extracts its files if the
/// cache does not hold them, reporting progress, then binds a new launch
/// prefix to them. Two launches of the same package never extract twice at
/// once.
pub async fn open_launch(
    launches: &Launches,
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    hash: &str,
    progress: impl FnMut(PackageExtractProgress) + Send + 'static,
) -> Result<OpenedPackageLaunch, PackageError> {
    let hash = hash.to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PackageError::InvalidHash(hash));
    }
    let _extracting = launches.lock_extraction(&hash).await;
    let extracted = packages::ensure_extracted(db, node, dirs, &hash, progress).await?;
    Ok(launches.open(&extracted.package.hash, &extracted.root)?)
}

/// Drops a package's extracted files unless a launch of it is open; returns
/// whether anything was dropped. It holds the extraction lock [`open_launch`]
/// holds, so a launch being opened either registered before the check (and
/// the files stay) or extracts again after the drop.
pub async fn drop_unless_open(
    launches: &Launches,
    dirs: &PackageDirs,
    hash: &str,
) -> Result<bool, PackageError> {
    let hash = hash.to_ascii_lowercase();
    let _extracting = launches.lock_extraction(&hash).await;
    if launches.is_package_open(&hash) {
        return Ok(false);
    }
    packages::drop_extracted(dirs, &hash).await
}
