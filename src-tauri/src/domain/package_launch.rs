//! An open launch of a standards package, as the player receives it.
//! Mirrored in `src/types/index.ts`.

use serde::{Deserialize, Serialize};

/// A package bound to an unguessable path prefix for as long as the launch
/// is open. The player loads pages from `base_url`; once the launch closes,
/// nothing under it is served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenedPackageLaunch {
    /// The launch's path prefix, 32 lowercase hex characters. Closing the
    /// launch takes it.
    pub prefix: String,
    /// The package the prefix serves.
    pub package_hash: String,
    /// `<origin>/<prefix>/` for this platform: `package://localhost/…` on
    /// macOS, iOS and Linux, `http://package.localhost/…` on Windows and
    /// Android. A package file `<path>` is at `base_url + path`.
    pub base_url: String,
}
