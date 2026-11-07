//! Outbound HTTP clients shared by every runtime surface.
//!
//! On Android, reqwest verifies TLS through the system trust store with
//! `rustls-platform-verifier`, which needs the hosting app to hand over its
//! JVM/activity over JNI. Termux is an Android userland without a JVM, so that
//! verifier can only panic. There we use the roots compiled into the binary.
//! `WUNDER_TLS_ROOTS=webpki|platform` overrides the automatic choice.

use std::sync::Arc;

/// Where `rustls` loads trusted server certificates from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsRoots {
    Platform,
    Webpki,
}

pub const TLS_ROOTS_ENV: &str = "WUNDER_TLS_ROOTS";

/// Termux sets `TERMUX_VERSION` and a `/data/data/com.termux/...` prefix.
pub fn is_termux_env(
    termux_version: Option<&std::ffi::OsStr>,
    prefix: Option<&std::ffi::OsStr>,
) -> bool {
    termux_version.is_some_and(|value| !value.is_empty())
        || prefix.is_some_and(|value| value.to_string_lossy().contains("/com.termux/"))
}

pub fn is_termux() -> bool {
    is_termux_env(
        std::env::var_os("TERMUX_VERSION").as_deref(),
        std::env::var_os("PREFIX").as_deref(),
    )
}

pub fn tls_roots() -> TlsRoots {
    match std::env::var(TLS_ROOTS_ENV)
        .ok()
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webpki") => TlsRoots::Webpki,
        Some("platform") => TlsRoots::Platform,
        // Termux has no JVM, so the platform verifier would panic on the first
        // handshake. Desktop and the 茧 Android app keep the platform store.
        _ if is_termux() => TlsRoots::Webpki,
        _ => TlsRoots::Platform,
    }
}

pub fn tls_roots_label() -> &'static str {
    match tls_roots() {
        TlsRoots::Platform => "platform",
        TlsRoots::Webpki => "webpki",
    }
}

/// A reqwest builder that uses the selected trust roots.
pub fn builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    match tls_roots() {
        TlsRoots::Platform => builder,
        TlsRoots::Webpki => builder.use_preconfigured_tls(webpki_tls_config()),
    }
}

pub fn new() -> reqwest::Client {
    builder().build().expect("build wunder http client")
}

pub fn webpki_tls_config() -> rustls::ClientConfig {
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()));
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("rustls default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    // The win7 build has no HTTP/2 support; offering h2 there would let the
    // server pick a protocol the client cannot speak.
    #[cfg(not(target_vendor = "win7"))]
    {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    }
    #[cfg(target_vendor = "win7")]
    {
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn termux_is_detected_from_version_or_prefix() {
        use std::ffi::OsStr;
        assert!(is_termux_env(Some(OsStr::new("0.118.3")), None));
        assert!(is_termux_env(
            None,
            Some(OsStr::new("/data/data/com.termux/files/usr"))
        ));
        assert!(!is_termux_env(None, Some(OsStr::new("/usr"))));
        assert!(!is_termux_env(Some(OsStr::new("")), None));
    }

    #[test]
    fn webpki_config_uses_bundled_roots() {
        assert!(webpki_roots::TLS_SERVER_ROOTS.len() > 0);
        let config = webpki_tls_config();
        assert!(!config.alpn_protocols.is_empty());
    }

    #[test]
    fn webpki_builder_builds_a_client() {
        // Force the branch regardless of the host environment.
        let builder = reqwest::Client::builder().use_preconfigured_tls(webpki_tls_config());
        assert!(builder.build().is_ok());
    }
}
