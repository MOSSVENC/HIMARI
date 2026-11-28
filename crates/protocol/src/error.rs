//! Transport error type shared by every hardware backend.
//!
//! Kept in its own module so callers can name `DchuError` without pulling in the
//! DCHU implementation, and so the platform backends agree on one error shape.

use std::fmt;

#[derive(Debug)]
pub enum DchuError {
    UnsupportedPlatform,
    Status {
        operation: &'static str,
        code: i32,
    },
    InvalidArgument(String),
    ReadbackMismatch {
        operation: &'static str,
        expected: u8,
        actual: Option<u8>,
        rollback: Option<String>,
    },
    ControlFailed {
        operation: &'static str,
        detail: String,
    },
    QuickToggleMismatch {
        expected: [u8; 5],
        actual: Option<[u8; 5]>,
        rollback: Option<String>,
    },
    ThreadUnavailable,
}

impl fmt::Display for DchuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(f, "DCHU hardware access is only available on Windows")
            }
            Self::Status { operation, code } => {
                write!(f, "{operation} returned transport status {code}")
            }
            Self::InvalidArgument(message) => f.write_str(message),
            Self::ReadbackMismatch {
                operation,
                expected,
                actual,
                rollback,
            } => {
                write!(
                    f,
                    "{operation} did not read back: expected {expected}, got {}",
                    actual
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "no value".to_owned())
                )?;
                if let Some(rollback) = rollback {
                    write!(f, "; rollback: {rollback}")?;
                }
                Ok(())
            }
            Self::ControlFailed { operation, detail } => {
                write!(f, "{operation} failed: {detail}")
            }
            Self::QuickToggleMismatch {
                expected,
                actual,
                rollback,
            } => {
                write!(
                    f,
                    "quick-toggle readback mismatch: expected {expected:?}, got {actual:?}"
                )?;
                if let Some(rollback) = rollback {
                    write!(f, "; rollback: {rollback}")?;
                }
                Ok(())
            }
            Self::ThreadUnavailable => write!(f, "the hardware worker stopped unexpectedly"),
        }
    }
}

impl std::error::Error for DchuError {}

pub type Result<T> = std::result::Result<T, DchuError>;
