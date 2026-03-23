//! TCP ServerConnector Error

use core::fmt;

/// TCP ServerConnector Error
#[derive(Debug)]
pub enum Error {
    /// tokio-xmpp error
    TokioXMPP(crate::error::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::TokioXMPP(e) => write!(fmt, "TokioXMPP error: {}", e),
        }
    }
}

impl From<crate::error::Error> for Error {
    fn from(e: crate::error::Error) -> Self {
        Error::TokioXMPP(e)
    }
}
