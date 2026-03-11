/// Install a process-wide rustls crypto provider.
///
/// rustls 0.23 panics when multiple providers are enabled through features
/// unless the application selects one explicitly at startup.
pub fn install_process_default_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
}
