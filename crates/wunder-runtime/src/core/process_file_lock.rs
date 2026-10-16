//! Kernel-owned file locks. The marker is persistent: unlinking it would
//! create two lock domains for the same path on Unix. Process exit releases
//! ownership automatically, irrespective of marker age or PID reuse.
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

pub(crate) struct ProcessFileLock {
    _file: File,
}
impl ProcessFileLock {
    pub(crate) async fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .context("open shared lock failed")?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if started.elapsed() < timeout => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(TryLockError::WouldBlock) => anyhow::bail!("shared file is busy; retry later"),
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_child_process_fixture() {
        let Some(root) = std::env::var_os("WUNDER_LOCK_TEST_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _lock = runtime
            .block_on(ProcessFileLock::acquire(
                &root.join("fixture.lock"),
                Duration::ZERO,
            ))
            .unwrap();
        std::fs::write(root.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(15));
    }

    #[tokio::test]
    async fn killed_process_releases_kernel_lock_without_removing_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "core::process_file_lock::tests::lock_child_process_fixture",
                "--nocapture",
            ])
            .env("WUNDER_LOCK_TEST_ROOT", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let start = Instant::now();
        while !dir.path().join("ready").exists() && start.elapsed() < Duration::from_secs(5) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let ready = dir.path().join("ready").exists();
        let blocked = ProcessFileLock::acquire(&dir.path().join("fixture.lock"), Duration::ZERO)
            .await
            .is_err();
        let _ = child.kill();
        child.wait().unwrap();
        assert!(ready && blocked);
        let _recovered = ProcessFileLock::acquire(&dir.path().join("fixture.lock"), Duration::ZERO)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn abandoned_marker_is_reusable_and_live_owner_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.lock");
        std::fs::write(&path, b"abandoned marker").unwrap();
        let first = ProcessFileLock::acquire(&path, Duration::ZERO)
            .await
            .unwrap();
        assert!(ProcessFileLock::acquire(&path, Duration::from_millis(40))
            .await
            .is_err());
        assert!(path.is_file());
        drop(first);
        let _next = ProcessFileLock::acquire(&path, Duration::ZERO)
            .await
            .unwrap();
    }
}
