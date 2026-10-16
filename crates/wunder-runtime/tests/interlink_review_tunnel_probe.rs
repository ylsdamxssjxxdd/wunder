//! Kernel lease regression: repeated local instances cannot fight over a tunnel.
use std::time::Duration;
use wunder_server::process_file_lock::ProcessFileLock;

#[tokio::test]
async fn one_local_form_holds_one_tunnel_lease() {
    let dir = tempfile::tempdir().unwrap();
    let desktop = dir.path().join("interlink.desktop.lock");
    let cli = dir.path().join("interlink.cli.lock");
    let first = ProcessFileLock::acquire(&desktop, Duration::ZERO)
        .await
        .unwrap();
    assert!(ProcessFileLock::acquire(&desktop, Duration::ZERO)
        .await
        .is_err());
    let companion = ProcessFileLock::acquire(&cli, Duration::ZERO)
        .await
        .unwrap();
    drop(first);
    let restarted = ProcessFileLock::acquire(&desktop, Duration::ZERO)
        .await
        .unwrap();
    drop((restarted, companion));
}
