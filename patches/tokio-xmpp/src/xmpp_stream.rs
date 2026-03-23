//! `XMPPStream` provides encoding/decoding for XMPP

use futures::sink::Send;
use futures::{sink::SinkExt, task::Poll, Sink, Stream};
use minidom::Element;
use rand::{thread_rng, Rng};
use std::pin::Pin;
use std::task::Context;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;
use xmpp_parsers::jid::Jid;

use crate::stream_features::StreamFeatures;
use crate::stream_start;
use crate::xmpp_codec::{Packet, XmppCodec};
use crate::Error;

fn make_id() -> String {
    let id: u64 = thread_rng().gen();
    format!("{}", id)
}

pub(crate) fn add_stanza_id(mut stanza: Element, default_ns: &str) -> Element {
    if stanza.is("iq", default_ns)
        || stanza.is("message", default_ns)
        || stanza.is("presence", default_ns)
    {
        if stanza.attr("id").is_none() {
            stanza.set_attr("id", make_id());
        }
    }

    stanza
}

/// Wraps a binary stream (tokio's `AsyncRead + AsyncWrite`) to decode
/// and encode XMPP packets.
///
/// Implements `Sink + Stream`
pub struct XMPPStream<S: AsyncRead + AsyncWrite + Unpin> {
    /// The local Jabber-Id
    pub jid: Jid,
    /// Codec instance
    pub stream: Framed<S, XmppCodec>,
    /// `<stream:features/>` for XMPP version 1.0
    pub stream_features: StreamFeatures,
    /// Root namespace
    ///
    /// This is different for either c2s, s2s, or component
    /// connections.
    pub ns: String,
    /// Stream `id` attribute
    pub id: String,
}

impl<S: AsyncRead + AsyncWrite + Unpin> XMPPStream<S> {
    /// Constructor
    pub fn new(
        jid: Jid,
        stream: Framed<S, XmppCodec>,
        ns: String,
        id: String,
        stream_features: Element,
    ) -> Self {
        XMPPStream {
            jid,
            stream,
            stream_features: StreamFeatures::new(stream_features),
            ns,
            id,
        }
    }

    /// Send a `<stream:stream>` start tag
    pub async fn start(stream: S, jid: Jid, ns: String) -> Result<Self, Error> {
        let xmpp_stream = Framed::new(stream, XmppCodec::new());
        stream_start::start(xmpp_stream, jid, ns).await
    }

    /// Unwraps the inner stream
    pub fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    /// Re-run `start()`
    pub async fn restart(self) -> Result<Self, Error> {
        let stream = self.stream.into_inner();
        Self::start(stream, self.jid, self.ns).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> XMPPStream<S> {
    /// Convenience method
    pub fn send_stanza<E: Into<Element>>(&mut self, e: E) -> Send<'_, Self, Packet> {
        self.send(Packet::Stanza(e.into()))
    }
}

/// Proxy to self.stream
impl<S: AsyncRead + AsyncWrite + Unpin> Sink<Packet> for XMPPStream<S> {
    type Error = crate::Error;

    fn poll_ready(self: Pin<&mut Self>, _ctx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // Pin::new(&mut self.stream).poll_ready(ctx)
        //     .map_err(|e| e.into())
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        Pin::new(&mut self.stream)
            .start_send(item)
            .map_err(|e| e.into())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream)
            .poll_flush(cx)
            .map_err(|e| e.into())
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream)
            .poll_close(cx)
            .map_err(|e| e.into())
    }
}

/// Proxy to self.stream
impl<S: AsyncRead + AsyncWrite + Unpin> Stream for XMPPStream<S> {
    type Item = Result<Packet, crate::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream)
            .poll_next(cx)
            .map(|result| result.map(|result| result.map_err(|e| e.into())))
    }
}
