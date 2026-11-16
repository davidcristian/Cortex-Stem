//! The error every [`Platform`](crate::Platform) method can return.

use stem_core::WindowId;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The feature is not implemented in this build. The payload names the
    /// missing feature (e.g. `"icon extraction"`).
    #[error("{0} is not implemented on this platform")]
    Unsupported(&'static str),
    #[error("window {0:?} no longer exists")]
    WindowNotFound(WindowId),
    #[error("access denied (elevated or protected process)")]
    AccessDenied,
    #[error("operation timed out")]
    Timeout,
    #[error("OS error {code:#010x}: {message}")]
    Os { code: u32, message: String },
    #[error("{0}")]
    Other(String),
}

pub type PlatformResult<T> = Result<T, PlatformError>;
