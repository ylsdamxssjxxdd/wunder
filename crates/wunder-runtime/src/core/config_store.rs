// Config store: read and persist a single config file.
use crate::config::{
    config_path_default as resolve_default_config_path, load_config_from_path, resolve_config_path,
    try_load_config_from_path, Config,
};
use crate::core::atomic_write::atomic_write_text;
use crate::i18n;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::ErrorKind;
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
        Self {
            inner: Arc::new(RwLock::new(config)),
            config_path,
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn get(&self) -> Config {
        self.inner.read().await.clone()
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
        let _lock = acquire_config_lock(&lock_path)?;
        // Another client may have persisted a newer snapshot since this
        // process initialized. Re-read while holding the cross-process lock so
        // field updates are applied to the current file, never to a stale
        // in-memory copy.
        let mut guard = self.inner.write().await;
        if self.config_path.exists() {
            *guard = try_load_config_from_path(&self.config_path)
                .with_context(|| format!("reload config failed: {}", self.config_path.display()))?;
        }
        updater(&mut guard);
        let updated = guard.clone();
        drop(guard);
        self.version.fetch_add(1, Ordering::SeqCst);
        i18n::configure_i18n(
            Some(updated.i18n.default_language.clone()),
            Some(updated.i18n.supported_languages.clone()),
            Some(updated.i18n.aliases.clone()),
        );
        self.persist(&updated).await?;
        Ok(updated)
    }

    async fn persist(&self, config: &Config) -> Result<()> {
        let target = self.config_path.clone();
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create config dir failed: {}", parent.display()))?;
        }
        let text = serde_yaml::to_string(config).context("serialize config failed")?;
        let target_for_write = target.clone();
        tokio::task::spawn_blocking(move || atomic_write_text(&target_for_write, &text))
            .await
            .context("config write task cancelled")?
            .with_context(|| format!("write config failed: {}", target.display()))?;
        Ok(())
    }

    pub fn config_path_default() -> PathBuf {
        resolve_default_config_path()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
}

struct ConfigFileLock {
    path: PathBuf,
    _file: std::fs::File,
}

impl Drop for ConfigFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acquire_config_lock(path: &std::path::Path) -> Result<ConfigFileLock> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => {
                return Ok(ConfigFileLock {
                    path: path.to_path_buf(),
                    _file: file,
                })
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                // A killed client cannot run `Drop`; reclaim only a clearly
                // stale marker so a live writer is never interrupted.
                if std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age > Duration::from_secs(60))
                {
                    let _ = std::fs::remove_file(path);
                    continue;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("配置正在被另一个蛹或舵机实例更新，请稍后重试")
}
