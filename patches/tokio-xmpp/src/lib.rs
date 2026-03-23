//! XMPP implementation with asynchronous I/O using Tokio.

#![deny(unsafe_code, missing_docs, bare_trait_objects)]

#[cfg(all(
    not(xmpprs_doc_build),
    not(doc),
    feature = "tls-native",
    feature = "tls-rust"
))]
compile_error!("Both tls-native and tls-rust features can't be enabled at the same time.");

#[cfg(all(
    feature = "starttls",
    not(feature = "tls-native"),
    not(feature = "tls-rust")
))]
compile_error!(
    "when starttls feature enabled one of tls-native and tls-rust features must be enabled."
);

#[cfg(feature = "starttls")]
pub mod starttls;
mod stream_start;
#[cfg(feature = "insecure-tcp")]
pub mod tcp;
mod xmpp_codec;
pub use crate::xmpp_codec::{Packet, XmppCodec};
mod event;
pub use event::Event;
mod client;
pub mod connect;
pub mod stream_features;
pub mod xmpp_stream;

pub use client::{
    async_client::{Client as AsyncClient, Config as AsyncConfig},
    simple_client::Client as SimpleClient,
};
mod component;
pub use crate::component::Component;
mod error;
pub use crate::error::{AuthError, Error, ParseError, ProtocolError};

// Re-exports
pub use minidom;
pub use xmpp_parsers as parsers;
pub use xmpp_parsers::jid;
