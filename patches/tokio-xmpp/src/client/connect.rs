use sasl::common::Credentials;
use xmpp_parsers::{jid::Jid, ns};

use crate::client::auth::auth;
use crate::client::bind::bind;
use crate::client::session::establish_session;
use crate::connect::ServerConnector;
use crate::{xmpp_stream::XMPPStream, Error};

/// Log into an XMPP server as a client with a jid+pass
/// does channel binding if supported
pub async fn client_login<C: ServerConnector>(
    server: C,
    jid: Jid,
    password: String,
) -> Result<XMPPStream<C::Stream>, Error> {
    let username = jid.node().unwrap().as_str();
    let password = password;

    let xmpp_stream = server.connect(&jid, ns::JABBER_CLIENT).await?;

    let channel_binding = C::channel_binding(xmpp_stream.stream.get_ref())?;

    let creds = Credentials::default()
        .with_username(username)
        .with_password(password)
        .with_channel_binding(channel_binding);
    // Authenticated (unspecified) stream
    let stream = auth(xmpp_stream, creds).await?;
    // Authenticated XMPPStream
    let xmpp_stream = XMPPStream::start(stream, jid, ns::JABBER_CLIENT.to_owned()).await?;

    // XMPPStream bound to user session
    let xmpp_stream = bind(xmpp_stream).await?;

    // Establish explicit session if required (RFC 3921 compatibility)
    let xmpp_stream = establish_session(xmpp_stream).await?;
    Ok(xmpp_stream)
}
