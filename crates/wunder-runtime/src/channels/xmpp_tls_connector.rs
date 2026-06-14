use futures::{future::select_ok, FutureExt, SinkExt, StreamExt};
use hickory_resolver::config::LookupIpStrategy;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::{IntoName, TokioAsyncResolver};
use sasl::common::ChannelBinding;
use std::error::Error as StdError;
use std::fmt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, InvalidDnsNameError, ServerName, UnixTime};
use tokio_rustls::rustls::{
    self, ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};
use tokio_rustls::TlsConnector;
use tokio_xmpp::connect::{ServerConnector, ServerConnectorError};
use tokio_xmpp::parsers::{jid::Jid, ns};
use tokio_xmpp::xmpp_stream::XMPPStream;
use tokio_xmpp::{Error as XmppError, Packet, ProtocolError};

const XMPP_DEFAULT_SRV_PORT: u16 = 5222;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmppTlsSecurityMode {
    Strict,
    TrustSelfSigned,
    Plain,
}

#[derive(Clone, Debug)]
pub enum XmppTlsServerConfig {
    UseSrv {
        security_mode: XmppTlsSecurityMode,
    },
    Manual {
        host: String,
        port: u16,
        security_mode: XmppTlsSecurityMode,
    },
}

impl XmppTlsServerConfig {
    pub fn use_srv(security_mode: XmppTlsSecurityMode) -> Self {
        Self::UseSrv { security_mode }
    }

    pub fn manual(host: String, port: u16, security_mode: XmppTlsSecurityMode) -> Self {
        Self::Manual {
            host,
            port,
            security_mode,
        }
    }

    fn security_mode(&self) -> XmppTlsSecurityMode {
        match self {
            Self::UseSrv { security_mode } | Self::Manual { security_mode, .. } => *security_mode,
        }
    }

    fn is_plain(&self) -> bool {
        matches!(self.security_mode(), XmppTlsSecurityMode::Plain)
    }

    async fn connect_plain(
        &self,
        jid: &Jid,
        ns_uri: &str,
    ) -> Result<XMPPStream<XmppStream>, XmppTlsConnectorError> {
        let tcp_stream = match self {
            XmppTlsServerConfig::UseSrv { .. } => {
                connect_with_srv(
                    jid.domain().as_str(),
                    "_xmpp-client._tcp",
                    XMPP_DEFAULT_SRV_PORT,
                )
                .await?
            }
            XmppTlsServerConfig::Manual { host, port, .. } => connect_to_host(host, *port).await?,
        };

        // Start XMPP stream directly without STARTTLS for legacy/inner deployments.
        XMPPStream::start(
            XmppStream::Plain(tcp_stream),
            jid.clone(),
            ns_uri.to_owned(),
        )
        .await
        .map_err(XmppTlsConnectorError::from)
    }
}

#[derive(Debug)]
struct TrustSelfSignedVerifier;

impl ServerCertVerifier for TrustSelfSignedVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Unified stream type for both TLS and plaintext XMPP sessions.
pub enum XmppStream {
    Tls(TlsStream<TcpStream>),
    Plain(TcpStream),
}

impl AsyncRead for XmppStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            XmppStream::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
            XmppStream::Plain(tcp) => Pin::new(tcp).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for XmppStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            XmppStream::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
            XmppStream::Plain(tcp) => Pin::new(tcp).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            XmppStream::Tls(tls) => Pin::new(tls).poll_flush(cx),
            XmppStream::Plain(tcp) => Pin::new(tcp).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            XmppStream::Tls(tls) => Pin::new(tls).poll_shutdown(cx),
            XmppStream::Plain(tcp) => Pin::new(tcp).poll_shutdown(cx),
        }
    }
}

#[derive(Debug)]
pub enum XmppTlsConnectorError {
    Idna,
    Dns(hickory_resolver::proto::error::ProtoError),
    Resolve(hickory_resolver::error::ResolveError),
    InvalidDnsName(InvalidDnsNameError),
    Io(std::io::Error),
    Tls(RustlsError),
    Disconnected,
    Xmpp(XmppError),
}

impl fmt::Display for XmppTlsConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idna => write!(f, "IDNA domain conversion failed"),
            Self::Dns(err) => write!(f, "DNS protocol error: {err}"),
            Self::Resolve(err) => write!(f, "DNS resolve error: {err}"),
            Self::InvalidDnsName(err) => write!(f, "invalid TLS SNI DNS name: {err}"),
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::Tls(err) => write!(f, "TLS error: {err}"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Xmpp(err) => write!(f, "TokioXMPP error: {err}"),
        }
    }
}

impl StdError for XmppTlsConnectorError {}

impl ServerConnectorError for XmppTlsConnectorError {}

impl From<XmppError> for XmppTlsConnectorError {
    fn from(value: XmppError) -> Self {
        Self::Xmpp(value)
    }
}

impl From<std::io::Error> for XmppTlsConnectorError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<RustlsError> for XmppTlsConnectorError {
    fn from(value: RustlsError) -> Self {
        Self::Tls(value)
    }
}

impl ServerConnector for XmppTlsServerConfig {
    type Stream = XmppStream;
    type Error = XmppTlsConnectorError;

    async fn connect(
        &self,
        jid: &Jid,
        ns_uri: &str,
    ) -> Result<XMPPStream<Self::Stream>, Self::Error> {
        if self.is_plain() {
            return self.connect_plain(jid, ns_uri).await;
        }

        let tcp_stream = match self {
            XmppTlsServerConfig::UseSrv { .. } => {
                connect_with_srv(
                    jid.domain().as_str(),
                    "_xmpp-client._tcp",
                    XMPP_DEFAULT_SRV_PORT,
                )
                .await?
            }
            XmppTlsServerConfig::Manual { host, port, .. } => connect_to_host(host, *port).await?,
        };

        // Explicitly run STARTTLS so we can switch cert verification policy per account.
        let mut xmpp_stream = XMPPStream::start(tcp_stream, jid.clone(), ns_uri.to_owned()).await?;
        if !xmpp_stream.stream_features.can_starttls() {
            return Err(no_tls_error());
        }
        xmpp_stream
            .send(Packet::Stanza(
                tokio_xmpp::minidom::Element::builder("starttls", ns::TLS).build(),
            ))
            .await
            .map_err(XmppTlsConnectorError::from)?;

        loop {
            match xmpp_stream.next().await {
                Some(Ok(Packet::Stanza(stanza))) if stanza.name() == "proceed" => break,
                Some(Ok(Packet::Text(_))) => {}
                Some(Err(err)) => return Err(XmppTlsConnectorError::from(err)),
                _ => return Err(no_tls_error()),
            }
        }

        let server_name = ServerName::try_from(jid.domain().to_string())
            .map_err(XmppTlsConnectorError::InvalidDnsName)?;
        let tls_config = build_tls_config(self.security_mode());
        let tls_stream = TlsConnector::from(Arc::new(tls_config))
            .connect(server_name, xmpp_stream.into_inner())
            .await
            .map_err(XmppTlsConnectorError::Io)?;
        XMPPStream::start(XmppStream::Tls(tls_stream), jid.clone(), ns_uri.to_owned())
            .await
            .map_err(XmppTlsConnectorError::from)
    }

    fn channel_binding(stream: &Self::Stream) -> Result<ChannelBinding, Self::Error> {
        match stream {
            XmppStream::Tls(tls) => {
                let (_, connection) = tls.get_ref();
                Ok(match connection.protocol_version() {
                    Some(tokio_rustls::rustls::ProtocolVersion::TLSv1_3) => {
                        let data = vec![0_u8; 32];
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
            XmppStream::Plain(_) => Ok(ChannelBinding::None),
        }
    }
}

fn build_tls_config(security_mode: XmppTlsSecurityMode) -> ClientConfig {
    match security_mode {
        XmppTlsSecurityMode::Strict => {
            let root_store = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.into(),
            };
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        }
        XmppTlsSecurityMode::TrustSelfSigned => ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(TrustSelfSignedVerifier))
            .with_no_client_auth(),
        XmppTlsSecurityMode::Plain => ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth(),
    }
}

fn no_tls_error() -> XmppTlsConnectorError {
    XmppTlsConnectorError::Xmpp(XmppError::Protocol(ProtocolError::NoTls))
}

async fn connect_to_host(domain: &str, port: u16) -> Result<TcpStream, XmppTlsConnectorError> {
    let ascii_domain = idna::domain_to_ascii(domain).map_err(|_| XmppTlsConnectorError::Idna)?;
    if let Ok(ip) = ascii_domain.parse() {
        return TcpStream::connect(SocketAddr::new(ip, port))
            .await
            .map_err(XmppTlsConnectorError::Io);
    }

    let (config, mut options) = hickory_resolver::system_conf::read_system_conf()
        .map_err(XmppTlsConnectorError::Resolve)?;
    options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    let resolver = TokioAsyncResolver::new(config, options, TokioConnectionProvider::default());
    let ips = resolver
        .lookup_ip(ascii_domain)
        .await
        .map_err(XmppTlsConnectorError::Resolve)?;
    let attempts = ips
        .into_iter()
        .map(|ip| TcpStream::connect(SocketAddr::new(ip, port)).boxed())
        .collect::<Vec<_>>();
    if attempts.is_empty() {
        return Err(XmppTlsConnectorError::Disconnected);
    }
    select_ok(attempts)
        .await
        .map(|(stream, _)| stream)
        .map_err(XmppTlsConnectorError::Io)
}

async fn connect_with_srv(
    domain: &str,
    srv: &str,
    fallback_port: u16,
) -> Result<TcpStream, XmppTlsConnectorError> {
    let ascii_domain = idna::domain_to_ascii(domain).map_err(|_| XmppTlsConnectorError::Idna)?;
    if let Ok(ip) = ascii_domain.parse() {
        return TcpStream::connect(SocketAddr::new(ip, fallback_port))
            .await
            .map_err(XmppTlsConnectorError::Io);
    }

    let resolver =
        TokioAsyncResolver::tokio_from_system_conf().map_err(XmppTlsConnectorError::Resolve)?;
    let srv_domain = format!("{srv}.{ascii_domain}.")
        .into_name()
        .map_err(XmppTlsConnectorError::Dns)?;
    let srv_records = resolver.srv_lookup(srv_domain).await.ok();
    if let Some(lookup) = srv_records {
        for record in lookup.iter() {
            if let Ok(stream) = connect_to_host(&record.target().to_ascii(), record.port()).await {
                return Ok(stream);
            }
        }
        return Err(XmppTlsConnectorError::Disconnected);
    }
    connect_to_host(domain, fallback_port).await
}
