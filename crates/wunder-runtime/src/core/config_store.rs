// Config store: read and persist a single config file.
use super::process_file_lock::ProcessFileLock;
use crate::config::{
    config_path_default as resolve_default_config_path, load_config_from_path, resolve_config_path,
    try_load_config_from_path, Config,
};
use crate::core::atomic_write::atomic_write_text;
use crate::i18n;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<Config>>,
    config_path: PathBuf,
    version: Arc<AtomicU64>,
    disk_fingerprint: Arc<std::sync::Mutex<Option<(u64, Option<SystemTime>)>>>,
}

impl ConfigStore {
    pub fn try_new(config_path: PathBuf) -> Result<Self> {
        let config_path = resolve_config_path(&config_path);
        let config = try_load_config_from_path(&config_path)?;
        Ok(Self::from_value(config_path, config))
    }

    pub fn new(config_path: PathBuf) -> Self {
        let config_path = resolve_config_path(&config_path);
        let config = load_config_from_path(&config_path);
        Self::from_value(config_path, config)
    }

    /// A store whose initial value comes from the caller instead of the file it
    /// persists to. The local forms generate their YAML from a shipped template
    /// on every start, so a value the user deleted from their own config file
    /// cannot survive in the generated one.
    pub fn from_config(config_path: PathBuf, config: Config) -> Self {
        Self::from_value(resolve_config_path(&config_path), config)
    }

    fn from_value(config_path: PathBuf, config: Config) -> Self {
        i18n::configure_i18n(
            Some(config.i18n.default_language.clone()),
            Some(config.i18n.supported_languages.clone()),
            Some(config.i18n.aliases.clone()),
        );
        let fingerprint = file_fingerprint(&config_path);
        Self {
            inner: Arc::new(RwLock::new(config)),
            config_path,
            version: Arc::new(AtomicU64::new(0)),
            disk_fingerprint: Arc::new(std::sync::Mutex::new(fingerprint)),
        }
    }

    pub async fn get(&self) -> Config {
        {
            let guard = self.inner.read().await;
            if file_fingerprint(&self.config_path)
                == *self
                    .disk_fingerprint
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
            {
                return guard.clone();
            }
        }
        // Serialize reload publication with local updates. Otherwise an old
        // disk read can overwrite the snapshot just published by update().
        let mut guard = self.inner.write().await;
        match self.read_disk_if_changed() {
            Ok(Some(config)) => {
                i18n::configure_i18n(
                    Some(config.i18n.default_language.clone()),
                    Some(config.i18n.supported_languages.clone()),
                    Some(config.i18n.aliases.clone()),
                );
                *guard = config;
                self.version.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                tracing::warn!("shared configuration reload failed; retaining last valid snapshot")
            }
            _ => {}
        }
        guard.clone()
    }

    pub async fn update<F>(&self, updater: F) -> Result<Config>
    where
        F: FnOnce(&mut Config),
    {
        let lock_path = self
            .config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("config.lock");
        let _lock = ProcessFileLock::acquire(&lock_path, Duration::from_millis(2500)).await?;
        // Another client may have persisted a newer snapshot since this
        // process initialized. Re-read while holding the cross-process lock so
        // field updates are applied to the current file, never to a stale
        // in-memory copy.
        let mut guard = self.inner.write().await;
        let disk_fingerprint = file_fingerprint(&self.config_path);
        let current = if disk_fingerprint.is_some() {
            try_load_config_from_path(&self.config_path)
                .with_context(|| format!("reload config failed: {}", self.config_path.display()))?
        } else {
            guard.clone()
        };
        // Most callers project their defaults on every process start. Keep the
        // cross-process lock and fresh disk read for consistency, but avoid an
        // atomic replace (and its filesystem flush) when the projection is a
        // no-op. YAML serialization is bounded by the config size and is much
        // cheaper than rewriting the shared file.
        let current_yaml =
            serde_yaml::to_string(&current).context("serialize current config failed")?;
        let mut updated = current;
        updater(&mut updated);
        let updated_yaml =
            serde_yaml::to_string(&updated).context("serialize updated config failed")?;
        if disk_fingerprint.is_some() && updated_yaml == current_yaml {
            let mut cached_fingerprint = self
                .disk_fingerprint
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if *cached_fingerprint != disk_fingerprint {
                // A no-op against disk may still replace a stale local
                // snapshot. Publish its version so dependent caches refresh.
                self.version.fetch_add(1, Ordering::SeqCst);
            }
            *cached_fingerprint = disk_fingerprint;
            drop(cached_fingerprint);
            *guard = updated.clone();
            i18n::configure_i18n(
                Some(updated.i18n.default_language.clone()),
                Some(updated.i18n.supported_languages.clone()),
                Some(updated.i18n.aliases.clone()),
            );
            return Ok(updated);
        }
        // Persist first. A write error must never leave memory claiming a
        // configuration state that another process cannot observe on disk.
        self.persist_text(&updated_yaml)?;
        *guard = updated.clone();
        self.version.fetch_add(1, Ordering::SeqCst);
        i18n::configure_i18n(
            Some(updated.i18n.default_language.clone()),
            Some(updated.i18n.supported_languages.clone()),
            Some(updated.i18n.aliases.clone()),
        );
        Ok(updated)
    }

    fn read_disk_if_changed(&self) -> Result<Option<Config>> {
        let Some(fingerprint) = file_fingerprint(&self.config_path) else {
            return Ok(None);
        };
        if *self
            .disk_fingerprint
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            == Some(fingerprint)
        {
            return Ok(None);
        }
        let disk = try_load_config_from_path(&self.config_path)
            .with_context(|| format!("reload config failed: {}", self.config_path.display()))?;
        *self
            .disk_fingerprint
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(fingerprint);
        Ok(Some(disk))
    }

    fn persist_text(&self, text: &str) -> Result<()> {
        // No detached blocking write: cancellation must not release the file
        // lock while a worker continues replacing the shared file.
        atomic_write_text(&self.config_path, text).context("write config failed")?;
        *self
            .disk_fingerprint
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = file_fingerprint(&self.config_path);
        Ok(())
    }

    pub fn config_path_default() -> PathBuf {
        resolve_default_config_path()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn independent_stores_merge_concurrent_updates_and_refresh_external_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let first = ConfigStore::new(path.clone());
        let second = ConfigStore::new(path.clone());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let left_barrier = barrier.clone();
        let left = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            left_barrier.wait();
            rt.block_on(first.update(|config| config.server.mode = "desktop".into()))
                .unwrap();
        });
        let right = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            barrier.wait();
            rt.block_on(second.update(|config| config.server.host = "127.0.0.2".into()))
                .unwrap();
        });
        left.join().unwrap();
        right.join().unwrap();
        let reread = ConfigStore::try_new(path.clone()).unwrap().get().await;
        assert_eq!(reread.server.mode, "desktop");
        assert_eq!(reread.server.host, "127.0.0.2");

        let external = ConfigStore::new(path.clone());
        let writer = ConfigStore::new(path.clone());
        writer
            .update(|config| config.server.mode = "cli".into())
            .await
            .unwrap();
        assert_eq!(external.get().await.server.mode, "cli");
        assert!(external.version() > 0);
    }

    #[tokio::test]
    async fn failed_persist_does_not_publish_updated_memory_snapshot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let store = ConfigStore::new(path.clone());
        let result = store
            .update(|config| {
                config.server.mode = "changed".into();
                std::fs::create_dir(&path).unwrap(); // replacement fails after mutation
            })
            .await;
        assert!(result.is_err());
        assert_ne!(store.get().await.server.mode, "changed");
    }

    #[tokio::test]
    async fn noop_update_keeps_the_existing_config_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let store = ConfigStore::new(path.clone());
        store
            .update(|config| config.server.mode = "desktop".into())
            .await
            .unwrap();
        // A replacement would lose this disk-only comment even if all
        // deserialized config fields and the serialized bytes were identical.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\n# retained user comment\n");
        std::fs::write(&path, &text).unwrap();
        let store = ConfigStore::try_new(path.clone()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let version_before = store.version();

        let current = store.update(|_| {}).await.unwrap();

        assert_eq!(current.server.mode, "desktop");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(store.version(), version_before);
    }

    #[tokio::test]
    async fn noop_update_publishes_an_external_change_and_materializes_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let store = ConfigStore::new(path.clone());
        store.update(|_| {}).await.unwrap();
        assert!(path.is_file(), "initial defaults must still be persisted");
        let writer = ConfigStore::try_new(path.clone()).unwrap();
        writer
            .update(|config| config.server.mode = "external-mode".into())
            .await
            .unwrap();
        let version_before = store.version();

        let current = store.update(|_| {}).await.unwrap();

        assert_eq!(current.server.mode, "external-mode");
        assert!(store.version() > version_before);
        let refreshed_version = store.version();
        assert_eq!(store.get().await.server.mode, "external-mode");
        assert_eq!(
            store.version(),
            refreshed_version,
            "no redundant disk reload"
        );
    }
}

fn file_fingerprint(path: &std::path::Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
