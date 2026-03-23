use super::error::{ConnectorError, Error};
use futures::{future::select_ok, FutureExt};
use hickory_resolver::{
    config::LookupIpStrategy, name_server::TokioConnectionProvider, IntoName, TokioAsyncResolver,
};
use log::debug;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub async fn connect_to_host(domain: &str, port: u16) -> Result<TcpStream, Error> {
    let ascii_domain = idna::domain_to_ascii(&domain).map_err(|_| Error::Idna)?;

    if let Ok(ip) = ascii_domain.parse() {
        return Ok(TcpStream::connect(&SocketAddr::new(ip, port))
            .await
            .map_err(|e| Error::from(crate::Error::Io(e)))?);
    }

    let (config, mut options) =
        hickory_resolver::system_conf::read_system_conf().map_err(ConnectorError::Resolve)?;
    options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    let resolver = TokioAsyncResolver::new(config, options, TokioConnectionProvider::default());

    let ips = resolver
        .lookup_ip(ascii_domain)
        .await
        .map_err(ConnectorError::Resolve)?;
    // Happy Eyeballs: connect to all records in parallel, return the
    // first to succeed
    select_ok(
        ips.into_iter()
            .map(|ip| TcpStream::connect(SocketAddr::new(ip, port)).boxed()),
    )
    .await
    .map(|(result, _)| result)
    .map_err(|_| crate::Error::Disconnected.into())
}

pub async fn connect_with_srv(
    domain: &str,
    srv: &str,
    fallback_port: u16,
) -> Result<TcpStream, Error> {
    let ascii_domain = idna::domain_to_ascii(&domain).map_err(|_| Error::Idna)?;

    if let Ok(ip) = ascii_domain.parse() {
        debug!("Attempting connection to {ip}:{fallback_port}");
        return Ok(TcpStream::connect(&SocketAddr::new(ip, fallback_port))
            .await
            .map_err(|e| Error::from(crate::Error::Io(e)))?);
    }

    let resolver = TokioAsyncResolver::tokio_from_system_conf().map_err(ConnectorError::Resolve)?;

    let srv_domain = format!("{}.{}.", srv, ascii_domain)
        .into_name()
        .map_err(ConnectorError::Dns)?;
    let srv_records = resolver.srv_lookup(srv_domain.clone()).await.ok();

    match srv_records {
        Some(lookup) => {
            // TODO: sort lookup records by priority/weight
            for srv in lookup.iter() {
                debug!("Attempting connection to {srv_domain} {srv}");
                match connect_to_host(&srv.target().to_ascii(), srv.port()).await {
                    Ok(stream) => return Ok(stream),
                    Err(_) => {}
                }
            }
            Err(crate::Error::Disconnected.into())
        }
        None => {
            // SRV lookup error, retry with hostname
            debug!("Attempting connection to {domain}:{fallback_port}");
            connect_to_host(domain, fallback_port).await
        }
    }
}
