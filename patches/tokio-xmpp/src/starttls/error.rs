//! StartTLS ServerConnector Error

use hickory_resolver::{error::ResolveError, proto::error::ProtoError};
#[cfg(feature = "tls-native")]
use native_tls::Error as TlsError;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
#[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;
#[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
use tokio_rustls::rustls::Error as TlsError;

/// StartTLS ServerConnector Error
#[derive(Debug)]
pub enum Error {
    /// Error resolving DNS and establishing a connection
    Connection(ConnectorError),
    /// DNS label conversion error, no details available from module
    /// `idna`
    Idna,
    /// TLS error
    Tls(TlsError),
    #[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
    /// DNS name parsing error
    DnsNameError(InvalidDnsNameError),
    /// tokio-xmpp error
    TokioXMPP(crate::error::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Connection(e) => write!(fmt, "connection error: {}", e),
            Error::Idna => write!(fmt, "IDNA error"),
            Error::Tls(e) => write!(fmt, "TLS error: {}", e),
            #[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
            Error::DnsNameError(e) => write!(fmt, "DNS name error: {}", e),
            Error::TokioXMPP(e) => write!(fmt, "TokioXMPP error: {}", e),
        }
    }
}

impl StdError for Error {}

impl From<crate::error::Error> for Error {
    fn from(e: crate::error::Error) -> Self {
        Error::TokioXMPP(e)
    }
}

impl From<ConnectorError> for Error {
    fn from(e: ConnectorError) -> Self {
        Error::Connection(e)
    }
}

impl From<TlsError> for Error {
    fn from(e: TlsError) -> Self {
        Error::Tls(e)
    }
}

#[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
impl From<InvalidDnsNameError> for Error {
    fn from(e: InvalidDnsNameError) -> Self {
        Error::DnsNameError(e)
    }
}

/// XML parse error wrapper type
#[derive(Debug)]
pub struct ParseError(pub Cow<'static, str>);

impl StdError for ParseError {
    fn description(&self) -> &str {
        self.0.as_ref()
    }
    fn cause(&self) -> Option<&dyn StdError> {
        None
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Error establishing connection
#[derive(Debug)]
pub enum ConnectorError {
    /// All attempts failed, no error available
    AllFailed,
    /// DNS protocol error
    Dns(ProtoError),
    /// DNS resolution error
    Resolve(ResolveError),
}

impl StdError for ConnectorError {}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(fmt, "{:?}", self)
    }
}
