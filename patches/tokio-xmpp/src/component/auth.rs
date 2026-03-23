use futures::stream::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use xmpp_parsers::{component::Handshake, ns};

use crate::xmpp_codec::Packet;
use crate::xmpp_stream::XMPPStream;
use crate::{AuthError, Error};

pub async fn auth<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut XMPPStream<S>,
    password: String,
) -> Result<(), Error> {
    let nonza = Handshake::from_password_and_stream_id(&password, &stream.id);
    stream.send_stanza(nonza).await?;

    loop {
        match stream.next().await {
            Some(Ok(Packet::Stanza(ref stanza)))
                if stanza.is("handshake", ns::COMPONENT_ACCEPT) =>
            {
                return Ok(());
            }
            Some(Ok(Packet::Stanza(ref stanza)))
                if stanza.is("error", "http://etherx.jabber.org/streams") =>
            {
                return Err(AuthError::ComponentFail.into());
            }
            Some(_) => {}
            None => return Err(Error::Disconnected),
        }
    }
}
