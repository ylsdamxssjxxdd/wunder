// Config store: load base config, apply overrides, and persist diffs.
use crate::config::{load_base_config_value, load_config, Config};
use crate::i18n;
use anyhow::{Context, Result};
use serde_yaml::Value;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;

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
        let updated_value = serde_yaml::to_value(config).unwrap_or(Value::Null);
        let base_value = load_base_config_value();
        let diff_value = diff_yaml(&base_value, &updated_value);
        let target = self.override_path.clone();
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("create config override dir failed: {}", parent.display())
            })?;
        }
        match diff_value {
            Some(value) => {
                let text = serde_yaml::to_string(&value).unwrap_or_default();
                tokio::fs::write(&target, text).await.with_context(|| {
                    format!("write override config failed: {}", target.display())
                })?;
            }
            None => {
                if let Err(err) = tokio::fs::remove_file(&target).await {
                    if err.kind() != ErrorKind::NotFound {
                        return Err(err).with_context(|| {
                            format!("remove override config failed: {}", target.display())
                        });
                    }
                }
            }
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

fn diff_yaml(base: &Value, updated: &Value) -> Option<Value> {
    if base == updated {
        return None;
    }
    match (base, updated) {
        (Value::Mapping(base_map), Value::Mapping(updated_map)) => {
            let mut diff_map = serde_yaml::Mapping::new();
            for (key, updated_value) in updated_map {
                let base_value = base_map.get(key).unwrap_or(&Value::Null);
                if let Some(value) = diff_yaml(base_value, updated_value) {
                    diff_map.insert(key.clone(), value);
                }
            }
            if diff_map.is_empty() {
                None
            } else {
                Some(Value::Mapping(diff_map))
            }
        }
        _ => Some(updated.clone()),
    }
}
