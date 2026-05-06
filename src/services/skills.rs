// Skills 加载与执行：解析 SKILL.md 元信息，并提供统一执行入口。
use crate::config::Config;
use crate::core::python_runtime;
use crate::i18n;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const SKILL_FILE_NAME: &str = "SKILL.md";
const ENTRY_FILES: [&str; 3] = ["run.py", "skill.py", "main.py"];
const SKILL_RUNNER_PATH_ENV: &str = "WUNDER_SKILL_RUNNER_PATH";

#[derive(Clone, Debug)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub path: String,
    pub input_schema: Value,
    pub frontmatter: String,
    pub root: PathBuf,
    pub entrypoint: Option<PathBuf>,
}

#[derive(Default, Clone)]
pub struct SkillRegistry {
    specs: Vec<SkillSpec>,
}

impl SkillRegistry {
    pub fn list_specs(&self) -> Vec<SkillSpec> {
        self.specs.clone()
    }

    pub fn get(&self, name: &str) -> Option<SkillSpec> {
        self.specs.iter().find(|spec| spec.name == name).cloned()
    }

    #[cfg(test)]
    pub fn add_spec_for_test(&mut self, spec: SkillSpec) {
        self.specs.push(spec);
    }
}

pub fn load_skills(
    config: &Config,
    load_entrypoints: bool,
    only_enabled: bool,
    include_repo_skills: bool,
) -> SkillRegistry {
    let mut registry = SkillRegistry::default();
    let enabled: Vec<String> = config
        .skills
        .enabled
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if only_enabled && enabled.is_empty() {
        return registry;
    }

    let mut scan_paths = config.skills.paths.clone();
    if include_repo_skills {
        let skills_root = Path::new("skills");
        if skills_root.exists() && !scan_paths.iter().any(|item| item == "skills") {
            scan_paths.push("skills".to_string());
        }
    }

    let mut remaining = enabled.clone();
    let mut seen_roots: HashSet<String> = HashSet::new();
    let mut seen_dirs: HashSet<String> = HashSet::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    for raw_path in scan_paths {
        if only_enabled && remaining.is_empty() {
            break;
        }
        let raw_path = raw_path.trim();
        if raw_path.is_empty() {
            continue;
        }
        let base = PathBuf::from(raw_path);
        let root_key = canonical_key(&base);
        if !seen_roots.insert(root_key) {
            continue;
        }
        for skill_dir in discover_skill_dirs(&base) {
            let key = canonical_key(&skill_dir);
            if !seen_dirs.insert(key) {
                continue;
            }
            let skill_file = skill_dir.join(SKILL_FILE_NAME);
            let content = match std::fs::read_to_string(&skill_file) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let (meta, frontmatter) = parse_frontmatter(&content).unwrap_or_default();
            let name = extract_skill_name(&meta, &content, &skill_dir);
            if name.is_empty() {
                continue;
            }
            if only_enabled && !enabled.contains(&name) {
                continue;
            }
            if !seen_names.insert(name.clone()) {
                continue;
            }
            let description = extract_skill_description(&meta, &content);
            let input_schema = build_input_schema(&meta);
            let entrypoint = if load_entrypoints {
                find_entrypoint(&skill_dir)
            } else {
                None
            };
            registry.specs.push(SkillSpec {
                name: name.clone(),
                description,
                path: skill_file.to_string_lossy().to_string(),
                input_schema,
                frontmatter,
                root: skill_dir.clone(),
                entrypoint,
            });
            if only_enabled {
                remaining.retain(|value| value != &name);
            }
        }
    }

    registry
}

/// 执行技能入口脚本，返回技能输出 JSON。
pub async fn execute_skill(spec: &SkillSpec, args: &Value, timeout_s: u64) -> Result<Value> {
    let entrypoint = spec.entrypoint.clone().ok_or_else(|| {
        anyhow!(i18n::t_with_params(
            "error.skill_not_executable",
            &HashMap::from([("name".to_string(), spec.name.clone())]),
        ))
    })?;
    let runner = resolve_skill_runner_path();
    if !runner.exists() {
        return Err(anyhow!(i18n::t_with_params(
            "tool.invoke.skill_failed",
            &HashMap::from([("detail".to_string(), i18n::t("error.skill_file_not_found"),)]),
        )));
    }
    let runtime = python_runtime::resolve_python_runtime();
    let python_bin = runtime
        .as_ref()
        .map(|value| value.bin.to_string_lossy().to_string())
        .unwrap_or_else(|| "python".to_string());
    let mut command = Command::new(python_bin);
    command
        .arg(runner)
        .arg(&entrypoint)
        .current_dir(&spec.root)
        .env("PYTHONIOENCODING", "utf-8")
        .env("WUNDER_LANGUAGE", i18n::get_language())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(runtime) = runtime.as_ref() {
        python_runtime::apply_python_env(&mut command, runtime);
    }
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(args).unwrap_or_default();
        stdin.write_all(&payload).await.ok();
    }
    let output = if timeout_s > 0 {
        tokio::time::timeout(
            std::time::Duration::from_secs(timeout_s),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            anyhow!(i18n::t_with_params(
                "tool.invoke.skill_failed",
                &HashMap::from([("detail".to_string(), skill_timeout_message(),)]),
            ))
        })??
    } else {
        child.wait_with_output().await?
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(i18n::t_with_params(
            "tool.invoke.skill_failed",
            &HashMap::from([("detail".to_string(), stderr.to_string())]),
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.trim().to_string()));
    Ok(value)
}

fn skill_timeout_message() -> String {
    let language = i18n::get_language().to_lowercase();
    if language.starts_with("en") {
        "Skill execution timed out".to_string()
    } else {
        "技能执行超时".to_string()
    }
}

fn resolve_skill_runner_path() -> PathBuf {
    std::env::var(SKILL_RUNNER_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("scripts/skill_runner.py"))
}

fn discover_skill_dirs(base: &Path) -> Vec<PathBuf> {
    if !base.exists() || base.is_file() {
        return Vec::new();
    }
    if base.join(SKILL_FILE_NAME).exists() {
        return vec![base.to_path_buf()];
    }
    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(SKILL_FILE_NAME).exists() {
                dirs.push(path);
            }
        }
    }
    dirs
}

fn canonical_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
        .to_lowercase()
}

fn find_entrypoint(skill_dir: &Path) -> Option<PathBuf> {
    for name in ENTRY_FILES {
        let candidate = skill_dir.join(name);
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_frontmatter(text: &str) -> Option<(HashMap<String, YamlValue>, String)> {
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches('\u{feff}')
        .to_string();
    let mut lines = normalized.lines();
    let first = lines.next()?.trim();
    if first != "---" {
        return None;
    }
    let mut body_lines = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        body_lines.push(line);
    }
    let body = body_lines.join("\n");
    let meta: HashMap<String, YamlValue> = serde_yaml::from_str(&body).ok()?;
    Some((meta, body))
}

fn extract_meta_name(meta: &HashMap<String, YamlValue>) -> String {
    for key in ["name", "名称", "技能名称"] {
        if let Some(value) = meta.get(key).and_then(|value| value.as_str()) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
    }
    "".to_string()
}

fn extract_skill_name(meta: &HashMap<String, YamlValue>, content: &str, skill_dir: &Path) -> String {
    let meta_name = extract_meta_name(meta);
    if !meta_name.is_empty() {
        return meta_name;
    }
    let heading_name = extract_first_heading(content);
    if !heading_name.is_empty() {
        return heading_name;
    }
    skill_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

fn extract_meta_description(meta: &HashMap<String, YamlValue>) -> String {
    for key in ["description", "描述", "技能描述"] {
        if let Some(value) = meta.get(key).and_then(|value| value.as_str()) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
    }
    i18n::t("skill.description.missing")
}

fn extract_skill_description(meta: &HashMap<String, YamlValue>, content: &str) -> String {
    let missing = i18n::t("skill.description.missing");
    let meta_description = extract_meta_description(meta);
    if meta_description != missing {
        return meta_description;
    }
    let body_description = extract_first_body_paragraph(content);
    if !body_description.is_empty() {
        return body_description;
    }
    missing
}

fn extract_first_heading(content: &str) -> String {
    let normalized = content
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let trimmed = normalized.trim_start_matches('\u{feff}');
    for line in trimmed.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix('#') {
            let heading = value.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return heading.to_string();
            }
        }
    }
    String::new()
}

fn extract_first_body_paragraph(content: &str) -> String {
    let normalized = content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches('\u{feff}')
        .to_string();
    let mut lines = normalized.lines().peekable();
    if matches!(lines.peek().map(|line| line.trim()), Some("---")) {
        lines.next();
        while let Some(line) = lines.next() {
            if line.trim() == "---" {
                break;
            }
        }
    }

    let mut paragraph = String::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    paragraph
}

fn build_input_schema(meta: &HashMap<String, YamlValue>) -> Value {
    for key in ["input_schema", "args_schema", "输入结构", "参数结构"] {
        if let Some(value) = meta.get(key) {
            return serde_json::to_value(value)
                .unwrap_or(json!({"type": "object", "properties": {}}));
        }
    }
    json!({"type": "object", "properties": {}})
}
