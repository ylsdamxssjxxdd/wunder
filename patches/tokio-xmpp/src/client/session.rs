use futures::stream::StreamExt;
use minidom::Element;
use tokio::io::{AsyncRead, AsyncWrite};
use xmpp_parsers::iq::{Iq, IqType};

use crate::xmpp_codec::Packet;
use crate::xmpp_stream::XMPPStream;
use crate::{Error, ProtocolError};

const SESSION_REQ_ID: &str = "session";
const NS_SESSION: &str = "urn:ietf:params:xml:ns:xmpp-session";

/// Establish XMPP session after resource binding (RFC 3921 compatible).
pub async fn establish_session<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: XMPPStream<S>,
) -> Result<XMPPStream<S>, Error> {
    // Check if server requires explicit session establishment
    if stream.stream_features.can_session() {
        // Build session IQ: <iq id="session" type="set"><session xmlns="urn:ietf:params:xml:ns:xmpp-session"/></iq>
        let session_elem = Element::builder("session", NS_SESSION).build();
        let iq = Iq {
            from: None,
            to: None,
            id: SESSION_REQ_ID.to_string(),
            payload: IqType::Set(session_elem),
        };
        let iq_elem: Element = iq.into();
        stream.send_stanza(iq_elem).await?;

        loop {
            match stream.next().await {
                Some(Ok(Packet::Stanza(stanza))) => match Iq::try_from(stanza) {
                    Ok(iq) if iq.id == SESSION_REQ_ID => match iq.payload {
                        IqType::Result(_) => {
                            return Ok(stream);
                        }
                        _ => return Err(ProtocolError::InvalidSessionResponse.into()),
                    },
                    _ => {}
                },
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e),
                None => return Err(Error::Disconnected),
            }
        }
    } else {
        // Server doesn't require explicit session (RFC 6121+)
        Ok(stream)
    }
}
