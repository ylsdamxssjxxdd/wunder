//! User-facing configuration (`~/.wunder/config.toml`).
//!
//! The engine owns a generated YAML file; users own one small TOML file. This
//! module reads the TOML layers (user → profile → project), reports which layer
//! supplied each effective value, and hands the result to the runtime, which
//! projects it onto the engine config. Writes go through `toml_edit` so a user's
//! comments and ordering survive an edit made from inside the CLI.
//!
//! Layers, lowest priority first: built-in defaults, `~/.wunder/config.toml`,
//! `~/.wunder/profiles/<name>.toml`, `<workspace>/.wunder/config.toml`, and
//! finally command-line flags.

use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Value};

pub const USER_CONFIG_FILE_NAME: &str = "config.toml";
pub const PROFILE_DIR_NAME: &str = "profiles";
pub const PROJECT_CONFIG_DIR_NAME: &str = ".wunder";

/// Keys a user file may set. Anything else is a typo and, in strict mode, an
/// error rather than a silently ignored line.
const KNOWN_KEYS: &[&str] = &[
    "model",
    "model_reasoning_effort",
    "approval_policy",
    "sandbox_mode",
    "project_doc",
    "project_doc_max_bytes",
    "language",
    "notify",
    "notify_when",
    "provider",
];

pub const USER_CONFIG_TEMPLATE: &str = r#"# wunder 舵机用户配置（codex 形态）
# 本文件是用户层；引擎自身的 YAML 由程序生成，不需要手工编辑。
# 优先级（低 → 高）：内置默认 < 本文件 < profiles/<name>.toml < 工作区 .wunder/config.toml < 命令行参数。

# 默认模型名；留空则使用引擎配置里的默认模型。
# model = ""

# 推理强度：low | medium | high（写入所选模型的推理档位）。
# model_reasoning_effort = "medium"

# 审批策略：
#   never        从不询问（默认；本地形态由工作区边界兜底）
#   on-request   写入免批、执行与受控操作需批准
#   suggest      写入与执行都需要批准
# approval_policy = "never"

# 沙箱范围：
#   workspace-write     只能读写工作区目录（默认）
#   read-only           写入与执行都需要批准（非交互下等于拒绝）
#   danger-full-access  放开全盘访问，慎用
# sandbox_mode = "workspace-write"

# 是否把工作区（及项目根到工作区沿途）的 AGENTS.md 快照进新线程。
# project_doc = true
# project_doc_max_bytes = 32768

# 界面语言：zh-CN | en-US
# language = "zh-CN"

# 回合完成通知：off | bell | osc9 | <命令...>（默认 off）。
# notify = "bell"
# 通知时机：always | unfocused（默认 always，仅在终端失焦时提醒）。
# notify_when = "unfocused"

# 模型服务（等价于 /config <base_url> <api_key> <model> 的一次性配置）。
# [provider]
# base_url = "https://api.example.com/v1"
# api_key = ""
# max_context = 128000
"#;

/// Values a user layer may set. Absent fields fall through to the next layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserConfigValues {
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub project_doc: Option<bool>,
    pub project_doc_max_bytes: Option<usize>,
    pub language: Option<String>,
    pub notify: Option<String>,
    pub notify_when: Option<String>,
    pub provider: Option<ProviderValues>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderValues {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub max_context: Option<u32>,
}

impl UserConfigValues {
    /// Overlay `higher` on top of `self`; `None` never erases a value.
    pub fn overlay(&mut self, higher: &UserConfigValues) {
        if higher.model.is_some() {
            self.model = higher.model.clone();
        }
        if higher.model_reasoning_effort.is_some() {
            self.model_reasoning_effort = higher.model_reasoning_effort.clone();
        }
        if higher.approval_policy.is_some() {
            self.approval_policy = higher.approval_policy.clone();
        }
        if higher.sandbox_mode.is_some() {
            self.sandbox_mode = higher.sandbox_mode.clone();
        }
        if higher.project_doc.is_some() {
            self.project_doc = higher.project_doc;
        }
        if higher.project_doc_max_bytes.is_some() {
            self.project_doc_max_bytes = higher.project_doc_max_bytes;
        }
        if higher.language.is_some() {
            self.language = higher.language.clone();
        }
        if higher.notify.is_some() {
            self.notify = higher.notify.clone();
        }
        if higher.notify_when.is_some() {
            self.notify_when = higher.notify_when.clone();
        }
        if let Some(provider) = higher.provider.as_ref() {
            let target = self.provider.get_or_insert_with(ProviderValues::default);
            if provider.base_url.is_some() {
                target.base_url = provider.base_url.clone();
            }
            if provider.api_key.is_some() {
                target.api_key = provider.api_key.clone();
            }
            if provider.max_context.is_some() {
                target.max_context = provider.max_context;
            }
        }
    }
}

/// One parsed layer plus where it came from.
#[derive(Debug, Clone)]
pub struct ConfigLayer {
    pub name: &'static str,
    pub values: UserConfigValues,
}

/// The merged user configuration and the layer that decided each value.
#[derive(Debug, Clone, Default)]
pub struct UserConfigReport {
    pub user_path: PathBuf,
    pub profile_path: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
    pub values: UserConfigValues,
    /// Effective key → layer name (`default` when nothing set it).
    pub sources: BTreeMap<String, String>,
}

impl UserConfigReport {
    pub fn layer_paths(&self) -> Vec<(&'static str, &Path)> {
        let mut out = vec![("user", self.user_path.as_path())];
        if let Some(path) = self.profile_path.as_deref() {
            out.push(("profile", path));
        }
        if let Some(path) = self.project_path.as_deref() {
            out.push(("project", path));
        }
        out
    }
}

pub fn user_config_path(wunder_home: &Path) -> PathBuf {
    wunder_home.join(USER_CONFIG_FILE_NAME)
}

pub fn profile_config_path(wunder_home: &Path, profile: &str) -> PathBuf {
    wunder_home
        .join(PROFILE_DIR_NAME)
        .join(format!("{profile}.toml"))
}

pub fn project_config_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(PROJECT_CONFIG_DIR_NAME)
        .join(USER_CONFIG_FILE_NAME)
}

/// Write the annotated template when no user file exists yet. Never overwrites.
pub fn ensure_user_config_template(wunder_home: &Path) -> Result<PathBuf> {
    let path = user_config_path(wunder_home);
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir failed: {}", parent.display()))?;
    }
    std::fs::write(&path, USER_CONFIG_TEMPLATE)
        .with_context(|| format!("write config template failed: {}", path.display()))?;
    Ok(path)
}

/// Read and merge the layers that exist. Missing files are fine; a malformed
/// file is reported with its path so the user can fix it.
pub fn load_report(
    wunder_home: &Path,
    workspace_root: &Path,
    profile: Option<&str>,
    strict: bool,
) -> Result<UserConfigReport> {
    let user_path = user_config_path(wunder_home);
    let profile_path = profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| profile_config_path(wunder_home, value));
    let project_path = Some(project_config_path(workspace_root));

    let mut layers: Vec<ConfigLayer> = Vec::new();
    for (name, path) in [
        ("user", Some(user_path.clone())),
        ("profile", profile_path.clone()),
        ("project", project_path.clone()),
    ] {
        let Some(path) = path else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read config failed: {}", path.display()))?;
        let values = parse_values(&text, &path, strict)?;
        layers.push(ConfigLayer { name, values });
    }

    if let Some(profile) = profile.map(str::trim).filter(|value| !value.is_empty()) {
        let path = profile_config_path(wunder_home, profile);
        if !path.exists() {
            return Err(anyhow!("profile not found: {}", path.display()));
        }
    }

    let mut values = UserConfigValues::default();
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    for key in KNOWN_KEYS {
        sources.insert((*key).to_string(), "default".to_string());
    }
    for layer in &layers {
        values.overlay(&layer.values);
        for key in keys_set_by(&layer.values) {
            sources.insert(key, layer.name.to_string());
        }
    }
    // A profile that exists but sets nothing still counts as loaded.
    if let Some(path) = profile_path.clone() {
        if path.exists() {
            sources
                .entry("profile".to_string())
                .or_insert_with(|| "profile".to_string());
        }
    }

    Ok(UserConfigReport {
        user_path,
        profile_path: profile_path.filter(|path| path.exists()),
        project_path: project_path.filter(|path| path.exists()),
        values,
        sources,
    })
}

/// Defaults-only view used when a user file cannot be parsed: the paths stay
/// visible for diagnostics and every value falls back to the engine default.
pub fn fallback_report(
    wunder_home: &Path,
    workspace_root: &Path,
    profile: Option<&str>,
) -> UserConfigReport {
    let profile_path = profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| profile_config_path(wunder_home, value))
        .filter(|path| path.exists());
    let project_path = Some(project_config_path(workspace_root));
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    for key in KNOWN_KEYS {
        sources.insert((*key).to_string(), "default".to_string());
    }
    UserConfigReport {
        user_path: user_config_path(wunder_home),
        profile_path,
        project_path: project_path.filter(|path| path.exists()),
        values: UserConfigValues::default(),
        sources,
    }
}

fn keys_set_by(values: &UserConfigValues) -> Vec<String> {
    let mut keys = Vec::new();
    if values.model.is_some() {
        keys.push("model".to_string());
    }
    if values.model_reasoning_effort.is_some() {
        keys.push("model_reasoning_effort".to_string());
    }
    if values.approval_policy.is_some() {
        keys.push("approval_policy".to_string());
    }
    if values.sandbox_mode.is_some() {
        keys.push("sandbox_mode".to_string());
    }
    if values.project_doc.is_some() {
        keys.push("project_doc".to_string());
    }
    if values.project_doc_max_bytes.is_some() {
        keys.push("project_doc_max_bytes".to_string());
    }
    if values.language.is_some() {
        keys.push("language".to_string());
    }
    if values.notify.is_some() {
        keys.push("notify".to_string());
    }
    if values.notify_when.is_some() {
        keys.push("notify_when".to_string());
    }
    if values.provider.is_some() {
        keys.push("provider".to_string());
    }
    keys
}

fn parse_values(text: &str, path: &Path, strict: bool) -> Result<UserConfigValues> {
    let document: DocumentMut = text
        .parse()
        .map_err(|err| anyhow!("invalid TOML in {}: {err}", path.display()))?;
    if strict {
        for (key, _) in document.iter() {
            if !KNOWN_KEYS.contains(&key) {
                return Err(anyhow!(
                    "unknown config key `{key}` in {} (strict mode)",
                    path.display()
                ));
            }
        }
    }

    let mut values = UserConfigValues::default();
    values.model = optional_string(&document, "model", path)?;
    values.model_reasoning_effort = optional_string(&document, "model_reasoning_effort", path)?;
    values.approval_policy = optional_string(&document, "approval_policy", path)?;
    values.sandbox_mode = optional_string(&document, "sandbox_mode", path)?;
    values.project_doc = optional_bool(&document, "project_doc", path)?;
    values.project_doc_max_bytes = optional_usize(&document, "project_doc_max_bytes", path)?;
    values.language = optional_string(&document, "language", path)?;
    values.notify = optional_string(&document, "notify", path)?;
    values.notify_when = optional_string(&document, "notify_when", path)?;

    // A typo in a word-list key is a real mistake, not a value to ignore: the
    // sandbox and approval words decide what the agent may touch.
    if let Some(raw) = values.sandbox_mode.as_deref() {
        if crate::args::SandboxModeArg::from_word(raw).is_none() {
            return Err(anyhow!(
                "`sandbox_mode` must be read-only|workspace-write|danger-full-access in {}",
                path.display()
            ));
        }
    }
    if let Some(raw) = values.approval_policy.as_deref() {
        if crate::map_approval_policy(raw).is_none() {
            return Err(anyhow!(
                "`approval_policy` must be never|on-request|suggest in {}",
                path.display()
            ));
        }
    }
    if values.notify.is_some() || values.notify_when.is_some() {
        crate::runtime::notify_config_for(&values)
            .map_err(|err| anyhow!("{err} in {}", path.display()))?;
    }

    if let Some(item) = document.get("provider") {
        let table = item.as_table_like().ok_or_else(|| {
            anyhow!(
                "`provider` must be a table in {} (use [provider])",
                path.display()
            )
        })?;
        let mut provider = ProviderValues::default();
        if let Some(value) = table.get("base_url") {
            provider.base_url = Some(as_string(value, "provider.base_url", path)?);
        }
        if let Some(value) = table.get("api_key") {
            provider.api_key = Some(as_string(value, "provider.api_key", path)?);
        }
        if let Some(value) = table.get("max_context") {
            let raw = value.as_integer().ok_or_else(|| {
                anyhow!(
                    "`provider.max_context` must be an integer in {}",
                    path.display()
                )
            })?;
            if raw <= 0 {
                return Err(anyhow!(
                    "`provider.max_context` must be positive in {}",
                    path.display()
                ));
            }
            provider.max_context = Some(raw.min(u32::MAX as i64) as u32);
        }
        values.provider = Some(provider);
    }

    Ok(values)
}

fn optional_string(document: &DocumentMut, key: &str, path: &Path) -> Result<Option<String>> {
    let Some(item) = document.get(key) else {
        return Ok(None);
    };
    let raw = as_string(item, key, path)?;
    let cleaned = raw.trim().to_string();
    if cleaned.is_empty() {
        return Ok(None);
    }
    Ok(Some(cleaned))
}

fn as_string(item: &Item, key: &str, path: &Path) -> Result<String> {
    item.as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("`{key}` must be a string in {}", path.display()))
}

fn optional_bool(document: &DocumentMut, key: &str, path: &Path) -> Result<Option<bool>> {
    let Some(item) = document.get(key) else {
        return Ok(None);
    };
    item.as_bool()
        .map(Some)
        .ok_or_else(|| anyhow!("`{key}` must be true or false in {}", path.display()))
}

fn optional_usize(document: &DocumentMut, key: &str, path: &Path) -> Result<Option<usize>> {
    let Some(item) = document.get(key) else {
        return Ok(None);
    };
    let raw = item
        .as_integer()
        .ok_or_else(|| anyhow!("`{key}` must be an integer in {}", path.display()))?;
    if raw <= 0 {
        return Err(anyhow!("`{key}` must be positive in {}", path.display()));
    }
    Ok(Some(raw as usize))
}

/// Set one key in the user file, preserving comments and ordering. Used by the
/// in-session commands (`/approvals`) so the choice survives a restart.
pub fn save_user_value(wunder_home: &Path, key: &str, value: &str) -> Result<PathBuf> {
    if !KNOWN_KEYS.contains(&key) {
        return Err(anyhow!("unknown config key: {key}"));
    }
    let path = ensure_user_config_template(wunder_home)?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read config failed: {}", path.display()))?;
    let mut document: DocumentMut = text
        .parse()
        .map_err(|err| anyhow!("invalid TOML in {}: {err}", path.display()))?;
    document[key] = Item::Value(Value::from(value));
    std::fs::write(&path, document.to_string())
        .with_context(|| format!("write config failed: {}", path.display()))?;
    Ok(path)
}

/// Write the `[provider]` table. The model wizard resolves an endpoint once, so
/// its three values belong to one edit; other tables and comments survive.
pub fn save_user_provider(
    wunder_home: &Path,
    base_url: &str,
    api_key: &str,
    max_context: Option<u32>,
) -> Result<PathBuf> {
    let path = ensure_user_config_template(wunder_home)?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read config failed: {}", path.display()))?;
    let mut document: DocumentMut = text
        .parse()
        .map_err(|err| anyhow!("invalid TOML in {}: {err}", path.display()))?;
    let mut table = toml_edit::Table::new();
    table.insert("base_url", Item::Value(Value::from(base_url)));
    table.insert("api_key", Item::Value(Value::from(api_key)));
    if let Some(max_context) = max_context {
        table.insert(
            "max_context",
            Item::Value(Value::from(i64::from(max_context.max(1)))),
        );
    }
    document["provider"] = Item::Table(table);
    std::fs::write(&path, document.to_string())
        .with_context(|| format!("write config failed: {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let mut root = std::env::temp_dir();
            root.push(format!(
                "wunder_cli_cfg_{tag}_{}_{}",
                std::process::id(),
                stamp
            ));
            std::fs::create_dir_all(&root).expect("create temp home");
            Self(root)
        }

        fn path(&self) -> &Path {
            self.0.as_path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn template_is_written_once_and_never_overwritten() {
        let home = TempHome::new("template");
        let path = ensure_user_config_template(home.path()).expect("write template");
        assert!(path.exists());
        std::fs::write(&path, "model = \"custom\"\n").expect("user edit");
        ensure_user_config_template(home.path()).expect("second call");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(
            text.contains("custom"),
            "the template must not clobber edits"
        );
    }

    #[test]
    fn layers_apply_from_user_to_profile_to_project() {
        let home = TempHome::new("layers");
        let workspace = home.path().join("work");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::write(
            user_config_path(home.path()),
            "model = \"user-model\"\napproval_policy = \"never\"\n",
        )
        .expect("user layer");
        std::fs::create_dir_all(home.path().join(PROFILE_DIR_NAME)).expect("profile dir");
        std::fs::write(
            profile_config_path(home.path(), "work"),
            "model = \"profile-model\"\n",
        )
        .expect("profile layer");
        std::fs::create_dir_all(project_config_path(&workspace).parent().unwrap())
            .expect("project dir");
        std::fs::write(project_config_path(&workspace), "project_doc = false\n")
            .expect("project layer");

        let report = load_report(home.path(), &workspace, Some("work"), false).expect("load");

        assert_eq!(report.values.model.as_deref(), Some("profile-model"));
        assert_eq!(report.values.approval_policy.as_deref(), Some("never"));
        assert_eq!(report.values.project_doc, Some(false));
        assert_eq!(
            report.sources.get("model").map(String::as_str),
            Some("profile")
        );
        assert_eq!(
            report.sources.get("approval_policy").map(String::as_str),
            Some("user")
        );
        assert_eq!(
            report.sources.get("project_doc").map(String::as_str),
            Some("project")
        );
    }

    #[test]
    fn strict_mode_rejects_unknown_keys() {
        let home = TempHome::new("strict");
        std::fs::write(user_config_path(home.path()), "modle = \"typo\"\n").expect("write");
        let workspace = home.path().join("work");
        std::fs::create_dir_all(&workspace).expect("workspace");

        let lenient = load_report(home.path(), &workspace, None, false);
        assert!(lenient.is_ok(), "unknown keys are ignored by default");

        let strict = load_report(home.path(), &workspace, None, true);
        let err = strict.expect_err("strict mode must fail");
        assert!(
            err.to_string().contains("modle"),
            "the error names the offending key: {err}"
        );
    }

    #[test]
    fn a_missing_profile_is_an_error_but_a_missing_user_file_is_not() {
        let home = TempHome::new("profile");
        let workspace = home.path().join("work");
        std::fs::create_dir_all(&workspace).expect("workspace");

        let report = load_report(home.path(), &workspace, None, false).expect("no files is fine");
        assert_eq!(report.values, UserConfigValues::default());

        let missing = load_report(home.path(), &workspace, Some("nope"), false);
        assert!(
            missing.is_err(),
            "an explicitly requested profile must exist"
        );
    }

    #[test]
    fn saving_a_value_keeps_comments_and_other_keys() {
        let home = TempHome::new("save");
        let path = ensure_user_config_template(home.path()).expect("template");
        std::fs::write(&path, "# my note\nmodel = \"keep-me\"\n").expect("write");

        save_user_value(home.path(), "approval_policy", "on-request").expect("save");

        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("# my note"), "comments survive: {text}");
        assert!(text.contains("keep-me"), "other keys survive: {text}");
        assert!(text.contains("on-request"));
    }

    #[test]
    fn provider_table_parses_and_validates() {
        let home = TempHome::new("provider");
        std::fs::write(
            user_config_path(home.path()),
            "model = \"demo\"\n[provider]\nbase_url = \"https://example.test/v1\"\napi_key = \"k\"\nmax_context = 64000\n",
        )
        .expect("write");
        let workspace = home.path().join("work");
        std::fs::create_dir_all(&workspace).expect("workspace");

        let report = load_report(home.path(), &workspace, None, true).expect("load");
        let provider = report.values.provider.expect("provider parsed");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://example.test/v1")
        );
        assert_eq!(provider.max_context, Some(64000));
    }
}
