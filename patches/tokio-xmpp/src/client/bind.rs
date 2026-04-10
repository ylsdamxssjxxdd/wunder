use futures::stream::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use xmpp_parsers::bind::{BindQuery, BindResponse};
use xmpp_parsers::iq::{Iq, IqType};
use xmpp_parsers::jid::{Jid, ResourceRef};

use crate::xmpp_codec::Packet;
use crate::xmpp_stream::XMPPStream;
use crate::{Error, ProtocolError};

const BIND_REQ_ID: &str = "bind";

pub async fn bind<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: XMPPStream<S>,
) -> Result<XMPPStream<S>, Error> {
    if stream.stream_features.can_bind() {
        let resource = stream
            .jid
            .resource()
            .map(|resource: &ResourceRef| resource.to_string());
        let iq = Iq::from_set(BIND_REQ_ID, BindQuery::new(resource));
        stream.send_stanza(iq).await?;

        loop {
            match stream.next().await {
                Some(Ok(Packet::Stanza(stanza))) => match Iq::try_from(stanza) {
                    Ok(iq) if iq.id == BIND_REQ_ID => match iq.payload {
                        IqType::Result(payload) => {
                            if let Some(bind) =
                                payload.and_then(|payload| BindResponse::try_from(payload).ok())
                            {
                                let bound_jid = Jid::from(bind);
                                stream.jid = bound_jid;
                            }
                            return Ok(stream);
                        }
                        _ => return Err(ProtocolError::InvalidBindResponse.into()),
                    },
                    _ => {}
                },
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e),
                None => return Err(Error::Disconnected),
            }
        }
    } else {
        // No resource binding available,
        // return the (probably // usable) stream immediately
        return Ok(stream);
    }
}
