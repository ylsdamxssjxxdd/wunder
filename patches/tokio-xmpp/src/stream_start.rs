use futures::{sink::SinkExt, stream::StreamExt};
use minidom::Element;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;
use xmpp_parsers::{jid::Jid, ns};

use crate::xmpp_codec::{Packet, XmppCodec};
use crate::xmpp_stream::XMPPStream;
use crate::{Error, ProtocolError};

/// Sends a `<stream:stream>`, then wait for one from the server, and
/// construct an XMPPStream.
pub async fn start<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: Framed<S, XmppCodec>,
    jid: Jid,
    ns: String,
) -> Result<XMPPStream<S>, Error> {
    let attrs = [
        ("to".to_owned(), jid.domain().to_string()),
        ("version".to_owned(), "1.0".to_owned()),
        ("xmlns".to_owned(), ns.clone()),
        ("xmlns:stream".to_owned(), ns::STREAM.to_owned()),
    ]
    .iter()
    .cloned()
    .collect();
    stream.send(Packet::StreamStart(attrs)).await?;

    let stream_attrs;
    loop {
        match stream.next().await {
            Some(Ok(Packet::StreamStart(attrs))) => {
                stream_attrs = attrs;
                break;
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
            None => return Err(Error::Disconnected),
        }
    }

    let stream_ns = stream_attrs
        .get("xmlns")
        .ok_or(ProtocolError::NoStreamNamespace)?
        .clone();
    let stream_id = stream_attrs
        .get("id")
        .ok_or(ProtocolError::NoStreamId)?
        .clone();
    let stream = if stream_ns == "jabber:client" && stream_attrs.get("version").is_some() {
        let stream_features;
        loop {
            match stream.next().await {
                Some(Ok(Packet::Stanza(stanza))) if stanza.is("features", ns::STREAM) => {
                    stream_features = stanza;
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => return Err(Error::Disconnected),
            }
        }
        XMPPStream::new(jid, stream, ns, stream_id, stream_features)
    } else {
        // FIXME: huge hack, shouldn’t be an element!
        XMPPStream::new(
            jid,
            stream,
            ns,
            stream_id.clone(),
            Element::builder(stream_id, ns::STREAM).build(),
        )
    };
    Ok(stream)
}
