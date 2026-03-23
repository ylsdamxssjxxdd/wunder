use futures::{sink::SinkExt, Sink, Stream};
use minidom::Element;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::StreamExt;
use xmpp_parsers::{jid::Jid, ns};

use crate::connect::ServerConnector;
use crate::stream_features::StreamFeatures;
use crate::xmpp_codec::Packet;
use crate::xmpp_stream::{add_stanza_id, XMPPStream};
use crate::Error;

use super::connect::client_login;

/// A simple XMPP client connection
///
/// This implements the `futures` crate's [`Stream`](#impl-Stream) and
/// [`Sink`](#impl-Sink<Packet>) traits.
pub struct Client<C: ServerConnector> {
    stream: XMPPStream<C::Stream>,
}

impl<C: ServerConnector> Client<C> {
    /// Start a new client given that the JID is already parsed.
    pub async fn new_with_jid_connector(
        connector: C,
        jid: Jid,
        password: String,
    ) -> Result<Self, Error> {
        let stream = client_login(connector, jid, password).await?;
        Ok(Client { stream })
    }

    /// Get direct access to inner XMPP Stream
    pub fn into_inner(self) -> XMPPStream<C::Stream> {
        self.stream
    }

    /// Get the client's bound JID (the one reported by the XMPP
    /// server).
    pub fn bound_jid(&self) -> &Jid {
        &self.stream.jid
    }

    /// Send stanza
    pub async fn send_stanza<E>(&mut self, stanza: E) -> Result<(), Error>
    where
        E: Into<Element>,
    {
        self.send(Packet::Stanza(add_stanza_id(
            stanza.into(),
            ns::JABBER_CLIENT,
        )))
        .await
    }

    /// Get the stream features (`<stream:features/>`) of the underlying stream
    pub fn get_stream_features(&self) -> &StreamFeatures {
        &self.stream.stream_features
    }

    /// End connection by sending `</stream:stream>`
    ///
    /// You may expect the server to respond with the same. This
    /// client will then drop its connection.
    pub async fn end(mut self) -> Result<(), Error> {
        self.send(Packet::StreamEnd).await?;

        // Wait for stream end from server
        while let Some(Ok(_)) = self.next().await {}

        Ok(())
    }
}

/// Incoming XMPP events
///
/// In an `async fn` you may want to use this with `use
/// futures::stream::StreamExt;`
impl<C: ServerConnector> Stream for Client<C> {
    type Item = Result<Element, Error>;

    /// Low-level read on the XMPP stream
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(Packet::Stanza(stanza)))) => {
                    return Poll::Ready(Some(Ok(stanza)))
                }
                Poll::Ready(Some(Ok(Packet::Text(_)))) => {
                    // Ignore, retry
                }
                Poll::Ready(_) =>
                // Unexpected and errors, just end
                {
                    return Poll::Ready(None)
                }
            }
        }
    }
}

/// Outgoing XMPP packets
///
/// See `send_stanza()` for an `async fn`
impl<C: ServerConnector> Sink<Packet> for Client<C> {
    type Error = Error;

    fn start_send(mut self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        Pin::new(&mut self.stream).start_send(item)
    }

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream).poll_ready(cx)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream).poll_close(cx)
    }
}
