#[cfg(unix)]
pub mod broker;
pub mod grants;
pub mod jd_parser;
pub mod learning;
pub mod model;
pub mod provider;
pub mod skills;
pub mod store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("permission_denied: course not found or not authored by you")]
    Permission,
    #[error("not_found: no published course or lesson with that identifier")]
    NotFound,
    #[error("profile_locked: unlock your profile")]
    Locked,
    #[error("conflict: this draft changed; reload before saving")]
    Conflict,
    #[error("invalid_input: {0}")]
    Invalid(String),
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("storage_error: studio data could not be accessed")]
    Storage(#[from] rusqlite::Error),
    #[error("invalid_data: studio data could not be decoded")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
