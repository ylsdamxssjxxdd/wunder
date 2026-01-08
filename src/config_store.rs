// 配置存储：加载基础配置与覆盖配置，支持运行时更新并写回覆盖文件。
use crate::config::{load_config, Config};
use crate::i18n;
use anyhow::Result;
use serde_yaml::Value;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<Config>>,
    override_path: PathBuf,
    version: Arc<AtomicU64>,
}

impl ConfigStore {
    pub fn new(override_path: PathBuf) -> Self {
        let config = load_config();
        i18n::configure_i18n(
            Some(config.i18n.default_language.clone()),
            Some(config.i18n.supported_languages.clone()),
            Some(config.i18n.aliases.clone()),
        );
        Self {
            inner: Arc::new(RwLock::new(config)),
            override_path,
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
        let value = serde_yaml::to_value(config).unwrap_or(Value::Null);
        let text = serde_yaml::to_string(&value).unwrap_or_default();
        let target = self.override_path.clone();
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        if let Err(err) = tokio::fs::write(&target, text).await {
            warn!("写入覆盖配置失败: {}: {err}", target.display());
        }
        Ok(())
    }

    pub fn override_path_default() -> PathBuf {
        let path = std::env::var("WUNDER_CONFIG_OVERRIDE_PATH")
            .unwrap_or_else(|_| "data/config/wunder.override.yaml".to_string());
        Path::new(&path).to_path_buf()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
}
