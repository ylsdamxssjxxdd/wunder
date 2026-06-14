// Config store: read and persist a single config file.
use crate::config::{
    config_path_default as resolve_default_config_path, load_config_from_path, resolve_config_path,
    Config,
};
use crate::i18n;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<Config>>,
    config_path: PathBuf,
    version: Arc<AtomicU64>,
}

impl ConfigStore {
    pub fn new(config_path: PathBuf) -> Self {
        let config_path = resolve_config_path(&config_path);
        let config = load_config_from_path(&config_path);
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
        let mut guard = self.inner.write().await;
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
        tokio::fs::write(&target, text)
            .await
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
