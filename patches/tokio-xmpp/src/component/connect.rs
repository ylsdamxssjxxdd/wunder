use xmpp_parsers::{jid::Jid, ns};

use crate::connect::ServerConnector;
use crate::{xmpp_stream::XMPPStream, Error};

use super::auth::auth;

/// Log into an XMPP server as a client with a jid+pass
pub async fn component_login<C: ServerConnector>(
    connector: C,
    jid: Jid,
    password: String,
) -> Result<XMPPStream<C::Stream>, Error> {
    let password = password;
    let mut xmpp_stream = connector.connect(&jid, ns::COMPONENT).await?;
    auth(&mut xmpp_stream, password).await?;
    Ok(xmpp_stream)
}
