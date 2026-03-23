//! `starttls::ServerConfig` provides a `ServerConnector` for starttls connections

use futures::{sink::SinkExt, stream::StreamExt};

#[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
use {
    std::sync::Arc,
    tokio_rustls::{
        client::TlsStream,
        rustls::pki_types::ServerName,
        rustls::{ClientConfig, RootCertStore},
        TlsConnector,
    },
};

#[cfg(feature = "tls-native")]
use {
    native_tls::TlsConnector as NativeTlsConnector,
    tokio_native_tls::{TlsConnector, TlsStream},
};

use minidom::Element;
use sasl::common::ChannelBinding;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use xmpp_parsers::{jid::Jid, ns};

use crate::{connect::ServerConnector, xmpp_codec::Packet, AsyncClient, SimpleClient};
use crate::{connect::ServerConnectorError, xmpp_stream::XMPPStream};

use self::error::Error;
use self::happy_eyeballs::{connect_to_host, connect_with_srv};

mod client;
pub mod error;
mod happy_eyeballs;

/// AsyncClient that connects over StartTls
pub type StartTlsAsyncClient = AsyncClient<ServerConfig>;
/// SimpleClient that connects over StartTls
pub type StartTlsSimpleClient = SimpleClient<ServerConfig>;

/// StartTLS XMPP server connection configuration
#[derive(Clone, Debug)]
pub enum ServerConfig {
    /// Use SRV record to find server host
    UseSrv,
    #[allow(unused)]
    /// Manually define server host and port
    Manual {
        /// Server host name
        host: String,
        /// Server port
        port: u16,
    },
}

impl ServerConnectorError for Error {}

impl ServerConnector for ServerConfig {
    type Stream = TlsStream<TcpStream>;
    type Error = Error;
    async fn connect(&self, jid: &Jid, ns: &str) -> Result<XMPPStream<Self::Stream>, Error> {
        // TCP connection
        let tcp_stream = match self {
            ServerConfig::UseSrv => {
                connect_with_srv(jid.domain().as_str(), "_xmpp-client._tcp", 5222).await?
            }
            ServerConfig::Manual { host, port } => connect_to_host(host.as_str(), *port).await?,
        };

        // Unencryped XMPPStream
        let xmpp_stream = XMPPStream::start(tcp_stream, jid.clone(), ns.to_owned()).await?;

        if xmpp_stream.stream_features.can_starttls() {
            // TlsStream
            let tls_stream = starttls(xmpp_stream).await?;
            // Encrypted XMPPStream
            Ok(XMPPStream::start(tls_stream, jid.clone(), ns.to_owned()).await?)
        } else {
            return Err(crate::Error::Protocol(crate::ProtocolError::NoTls).into());
        }
    }

    fn channel_binding(
        #[allow(unused_variables)] stream: &Self::Stream,
    ) -> Result<sasl::common::ChannelBinding, Error> {
        #[cfg(feature = "tls-native")]
        {
            log::warn!("tls-native doesn’t support channel binding, please use tls-rust if you want this feature!");
            Ok(ChannelBinding::None)
        }
        #[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
        {
            let (_, connection) = stream.get_ref();
            Ok(match connection.protocol_version() {
                // TODO: Add support for TLS 1.2 and earlier.
                Some(tokio_rustls::rustls::ProtocolVersion::TLSv1_3) => {
                    let data = vec![0u8; 32];
                    let data = connection.export_keying_material(
                        data,
                        b"EXPORTER-Channel-Binding",
                        None,
                    )?;
                    ChannelBinding::TlsExporter(data)
                }
                _ => ChannelBinding::None,
            })
        }
    }
}

#[cfg(feature = "tls-native")]
async fn get_tls_stream<S: AsyncRead + AsyncWrite + Unpin>(
    xmpp_stream: XMPPStream<S>,
) -> Result<TlsStream<S>, Error> {
    let domain = xmpp_stream.jid.domain().to_owned();
    let stream = xmpp_stream.into_inner();
    let tls_stream = TlsConnector::from(NativeTlsConnector::builder().build().unwrap())
        .connect(&domain, stream)
        .await?;
    Ok(tls_stream)
}

#[cfg(all(feature = "tls-rust", not(feature = "tls-native")))]
async fn get_tls_stream<S: AsyncRead + AsyncWrite + Unpin>(
    xmpp_stream: XMPPStream<S>,
) -> Result<TlsStream<S>, Error> {
    let domain = xmpp_stream.jid.domain().to_string();
    let domain = ServerName::try_from(domain)?;
    let stream = xmpp_stream.into_inner();
    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let tls_stream = TlsConnector::from(Arc::new(config))
        .connect(domain, stream)
        .await
        .map_err(|e| Error::from(crate::Error::Io(e)))?;
    Ok(tls_stream)
}

/// Performs `<starttls/>` on an XMPPStream and returns a binary
/// TlsStream.
pub async fn starttls<S: AsyncRead + AsyncWrite + Unpin>(
    mut xmpp_stream: XMPPStream<S>,
) -> Result<TlsStream<S>, Error> {
    let nonza = Element::builder("starttls", ns::TLS).build();
    let packet = Packet::Stanza(nonza);
    xmpp_stream.send(packet).await?;

    loop {
        match xmpp_stream.next().await {
            Some(Ok(Packet::Stanza(ref stanza))) if stanza.name() == "proceed" => break,
            Some(Ok(Packet::Text(_))) => {}
            Some(Err(e)) => return Err(e.into()),
            _ => {
                return Err(crate::Error::Protocol(crate::ProtocolError::NoTls).into());
            }
        }
    }

    get_tls_stream(xmpp_stream).await
}
