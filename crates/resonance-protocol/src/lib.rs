//! Original protocol primitives. This crate owns wire validation, not accounts,
//! sessions, score acceptance, permissions, or multiplayer state.

pub mod lazer;
pub mod multiplayer;
pub mod replay;
pub mod signalr;
pub mod stable;
pub mod submission;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Truncated,
    Invalid(&'static str),
    Limit(&'static str),
    Utf8,
    TrailingData,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated protocol payload"),
            Self::Invalid(field) => write!(f, "invalid {field}"),
            Self::Limit(field) => write!(f, "{field} exceeds protocol limits"),
            Self::Utf8 => f.write_str("invalid UTF-8"),
            Self::TrailingData => f.write_str("unexpected trailing protocol data"),
        }
    }
}

impl std::error::Error for ProtocolError {}

pub type Result<T> = std::result::Result<T, ProtocolError>;
