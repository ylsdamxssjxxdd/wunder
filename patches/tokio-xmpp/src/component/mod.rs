//! Components in XMPP are services/gateways that are logged into an
//! XMPP server under a JID consisting of just a domain name. They are
//! allowed to use any user and resource identifiers in their stanzas.
use futures::{sink::SinkExt, task::Poll, Sink, Stream};
use minidom::Element;
use std::pin::Pin;
use std::str::FromStr;
use std::task::Context;
use xmpp_parsers::{jid::Jid, ns};

use self::connect::component_login;

use super::xmpp_codec::Packet;
use super::Error;
use crate::connect::ServerConnector;
use crate::xmpp_stream::add_stanza_id;
use crate::xmpp_stream::XMPPStream;

mod auth;

pub(crate) mod connect;

/// Component connection to an XMPP server
///
/// This simplifies the `XMPPStream` to a `Stream`/`Sink` of `Element`
/// (stanzas). Connection handling however is up to the user.
pub struct Component<C: ServerConnector> {
    /// The component's Jabber-Id
    pub jid: Jid,
    stream: XMPPStream<C::Stream>,
}

impl<C: ServerConnector> Component<C> {
    /// Start a new XMPP component
    pub async fn new_with_connector(
        jid: &str,
        password: &str,
        connector: C,
    ) -> Result<Self, Error> {
        let jid = Jid::from_str(jid)?;
        let password = password.to_owned();
        let stream = component_login(connector, jid.clone(), password).await?;
        Ok(Component { jid, stream })
    }

    /// Send stanza
    pub async fn send_stanza(&mut self, stanza: Element) -> Result<(), Error> {
        self.send(add_stanza_id(stanza, ns::COMPONENT_ACCEPT)).await
    }

    /// End connection
    pub async fn send_end(&mut self) -> Result<(), Error> {
        self.close().await
    }
}

impl<C: ServerConnector> Stream for Component<C> {
    type Item = Element;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(Packet::Stanza(stanza)))) => return Poll::Ready(Some(stanza)),
                Poll::Ready(Some(Ok(Packet::Text(_)))) => {
                    // retry
                }
                Poll::Ready(Some(Ok(_))) =>
                // unexpected
                {
                    return Poll::Ready(None)
                }
                Poll::Ready(Some(Err(_))) => return Poll::Ready(None),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<C: ServerConnector> Sink<Element> for Component<C> {
    type Error = Error;

    fn start_send(mut self: Pin<&mut Self>, item: Element) -> Result<(), Self::Error> {
        Pin::new(&mut self.stream)
            .start_send(Packet::Stanza(item))
            .map_err(|e| e.into())
    }

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.stream)
            .poll_ready(cx)
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
