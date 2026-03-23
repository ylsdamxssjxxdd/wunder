use sasl::client::MechanismError as SaslMechanismError;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::io::Error as IoError;
use std::str::Utf8Error;

use xmpp_parsers::sasl::DefinedCondition as SaslDefinedCondition;
use xmpp_parsers::{jid::Error as JidParseError, Error as ParsersError};

use crate::connect::ServerConnectorError;

/// Top-level error type
#[derive(Debug)]
pub enum Error {
    /// I/O error
    Io(IoError),
    /// Error parsing Jabber-Id
    JidParse(JidParseError),
    /// Protocol-level error
    Protocol(ProtocolError),
    /// Authentication error
    Auth(AuthError),
    /// Connection closed
    Disconnected,
    /// Should never happen
    InvalidState,
    /// Fmt error
    Fmt(fmt::Error),
    /// Utf8 error
    Utf8(Utf8Error),
    /// Error resolving DNS and/or establishing a connection, returned by a ServerConnector impl
    Connection(Box<dyn ServerConnectorError>),
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => write!(fmt, "IO error: {}", e),
            Error::Connection(e) => write!(fmt, "connection error: {}", e),
            Error::JidParse(e) => write!(fmt, "jid parse error: {}", e),
            Error::Protocol(e) => write!(fmt, "protocol error: {}", e),
            Error::Auth(e) => write!(fmt, "authentication error: {}", e),
            Error::Disconnected => write!(fmt, "disconnected"),
            Error::InvalidState => write!(fmt, "invalid state"),
            Error::Fmt(e) => write!(fmt, "Fmt error: {}", e),
            Error::Utf8(e) => write!(fmt, "Utf8 error: {}", e),
        }
    }
}

impl StdError for Error {}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Error::Io(e)
    }
}

impl<T: ServerConnectorError + 'static> From<T> for Error {
    fn from(e: T) -> Self {
        Error::Connection(Box::new(e))
    }
}

impl From<JidParseError> for Error {
    fn from(e: JidParseError) -> Self {
        Error::JidParse(e)
    }
}

impl From<ProtocolError> for Error {
    fn from(e: ProtocolError) -> Self {
        Error::Protocol(e)
    }
}

impl From<AuthError> for Error {
    fn from(e: AuthError) -> Self {
        Error::Auth(e)
    }
}

impl From<fmt::Error> for Error {
    fn from(e: fmt::Error) -> Self {
        Error::Fmt(e)
    }
}

impl From<Utf8Error> for Error {
    fn from(e: Utf8Error) -> Self {
        Error::Utf8(e)
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

/// XMPP protocol-level error
#[derive(Debug)]
pub enum ProtocolError {
    /// XML parser error
    Parser(minidom::Error),
    /// Error with expected stanza schema
    Parsers(ParsersError),
    /// No TLS available
    NoTls,
    /// Invalid response to resource binding
    InvalidBindResponse,
    /// Invalid response to session establishment
    InvalidSessionResponse,
    /// No xmlns attribute in <stream:stream>
    NoStreamNamespace,
    /// No id attribute in <stream:stream>
    NoStreamId,
    /// Encountered an unexpected XML token
    InvalidToken,
    /// Unexpected <stream:stream> (shouldn't occur)
    InvalidStreamStart,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProtocolError::Parser(e) => write!(fmt, "XML parser error: {}", e),
            ProtocolError::Parsers(e) => write!(fmt, "error with expected stanza schema: {}", e),
            ProtocolError::NoTls => write!(fmt, "no TLS available"),
            ProtocolError::InvalidBindResponse => {
                write!(fmt, "invalid response to resource binding")
            }
            ProtocolError::InvalidSessionResponse => {
                write!(fmt, "invalid response to session establishment")
            }
            ProtocolError::NoStreamNamespace => {
                write!(fmt, "no xmlns attribute in <stream:stream>")
            }
            ProtocolError::NoStreamId => write!(fmt, "no id attribute in <stream:stream>"),
            ProtocolError::InvalidToken => write!(fmt, "encountered an unexpected XML token"),
            ProtocolError::InvalidStreamStart => write!(fmt, "unexpected <stream:stream>"),
        }
    }
}

impl StdError for ProtocolError {}

impl From<minidom::Error> for ProtocolError {
    fn from(e: minidom::Error) -> Self {
        ProtocolError::Parser(e)
    }
}

impl From<minidom::Error> for Error {
    fn from(e: minidom::Error) -> Self {
        ProtocolError::Parser(e).into()
    }
}

impl From<ParsersError> for ProtocolError {
    fn from(e: ParsersError) -> Self {
        ProtocolError::Parsers(e)
    }
}

/// Authentication error
#[derive(Debug)]
pub enum AuthError {
    /// No matching SASL mechanism available
    NoMechanism,
    /// Local SASL implementation error
    Sasl(SaslMechanismError),
    /// Failure from server
    Fail(SaslDefinedCondition),
    /// Component authentication failure
    ComponentFail,
}

impl StdError for AuthError {}

impl fmt::Display for AuthError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AuthError::NoMechanism => write!(fmt, "no matching SASL mechanism available"),
            AuthError::Sasl(s) => write!(fmt, "local SASL implementation error: {}", s),
            AuthError::Fail(c) => write!(fmt, "failure from the server: {:?}", c),
            AuthError::ComponentFail => write!(fmt, "component authentication failure"),
        }
    }
}
