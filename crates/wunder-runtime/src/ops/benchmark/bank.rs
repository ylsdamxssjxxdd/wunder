use super::loader::{default_tasks_dir, load_task_specs, load_task_specs_with_asset_root};
use super::spec::{BenchmarkGradingType, BenchmarkTaskSpec, WorkspaceFileSpec};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::ZipArchive;

pub const BANK_PROTOCOL: &str = "wunderbench.question_bank";
pub const BANK_SCHEMA_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "wunderbench.json";
const BANKS_DIR: &str = "config/benchmark/banks";
const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: usize = 96 * 1024 * 1024;
const MAX_FILES: usize = 256;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionBankManifest {
    protocol: String,
    schema_version: u32,
    id: String,
    version: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default = "default_tasks_path")]
    tasks_path: String,
    #[serde(default = "default_assets_path")]
    assets_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionBankSummary {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub subject: String,
    pub languages: Vec<String>,
    pub task_count: usize,
    pub task_ids: Vec<String>,
    pub suites: Vec<String>,
    pub has_executable_grading: bool,
    pub checksum: String,
    pub built_in: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedQuestionBank {
    pub summary: QuestionBankSummary,
    pub tasks: Vec<BenchmarkTaskSpec>,
}

fn default_tasks_path() -> String {
    "tasks".to_string()
}

fn default_assets_path() -> String {
    "assets".to_string()
}

pub fn default_banks_dir() -> PathBuf {
    resolve_config_path(BANKS_DIR)
}

pub fn list_question_banks() -> Result<Vec<QuestionBankSummary>> {
    let mut banks = vec![built_in_bank()?.summary];
    let root = default_banks_dir();
    if !root.exists() {
        return Ok(banks);
    }
    for entry in std::fs::read_dir(&root)
        .with_context(|| format!("read question bank directory failed: {root:?}"))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        for version in std::fs::read_dir(entry.path())? {
            let version = version?;
            if !version.file_type()?.is_dir() {
                continue;
            }
            if version.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            banks.push(load_imported_bank(version.path())?.summary);
        }
    }
    banks.sort_by(|left, right| {
        left.built_in
            .cmp(&right.built_in)
            .reverse()
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.version.cmp(&right.version))
    });
    Ok(banks)
}

pub fn load_question_bank(id: Option<&str>, version: Option<&str>) -> Result<LoadedQuestionBank> {
    let id = id.map(str::trim).unwrap_or("");
    if id.is_empty() || id == "builtin" {
        return built_in_bank();
    }
    let safe_id = validate_identifier(id, "bank id")?;
    let version = version.map(str::trim).unwrap_or("");
    if version.is_empty() {
        return Err(anyhow!("question_bank_version is required for imported banks"));
    }
    let safe_version = validate_identifier(version, "bank version")?;
    load_imported_bank(default_banks_dir().join(safe_id).join(safe_version))
}

pub fn import_question_bank(data: &[u8], allow_executable_grading: bool) -> Result<QuestionBankSummary> {
    if data.is_empty() || data.len() > MAX_PACKAGE_BYTES {
        return Err(anyhow!("question bank package must be between 1 byte and {MAX_PACKAGE_BYTES} bytes"));
    }
    let staging = default_banks_dir().join(format!(".import-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&staging)?;
    let result = import_question_bank_into(data, allow_executable_grading, &staging);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn import_question_bank_into(
    data: &[u8],
    allow_executable_grading: bool,
    staging: &Path,
) -> Result<QuestionBankSummary> {
    extract_package(data, staging)?;
    let loaded = load_imported_bank(staging.to_path_buf())?;
    if loaded.summary.has_executable_grading && !allow_executable_grading {
        return Err(anyhow!(
            "question bank contains automated Python grading; confirm allow_executable_grading to import it"
        ));
    }
    let destination = default_banks_dir()
        .join(&loaded.summary.id)
        .join(&loaded.summary.version);
    if destination.exists() {
        return Err(anyhow!("question bank {}@{} already exists", loaded.summary.id, loaded.summary.version));
    }
    std::fs::create_dir_all(destination.parent().expect("bank destination parent"))?;
    std::fs::rename(staging, &destination).with_context(|| "activate imported question bank failed")?;
    Ok(loaded.summary)
}

fn built_in_bank() -> Result<LoadedQuestionBank> {
    let tasks = load_task_specs(&default_tasks_dir())?;
    let summary = summarize(
        "builtin".to_string(),
        "1".to_string(),
        "Built-in WunderBench tasks".to_string(),
        "Tasks bundled with the current Wunder deployment.".to_string(),
        String::new(),
        Vec::new(),
        &tasks,
        checksum_directory(&default_tasks_dir())?,
        true,
    );
    Ok(LoadedQuestionBank { summary, tasks })
}

fn load_imported_bank(root: PathBuf) -> Result<LoadedQuestionBank> {
    let manifest_path = root.join(MANIFEST_FILE);
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("question bank manifest missing: {manifest_path:?}"))?;
    let manifest: QuestionBankManifest = serde_json::from_str(&manifest_text)
        .with_context(|| "invalid wunderbench.json")?;
    validate_manifest(&manifest)?;
    let tasks_dir = resolve_relative_directory(&root, &manifest.tasks_path, "tasks_path")?;
    let assets_dir = resolve_relative_path(&root, &manifest.assets_path, "assets_path")?;
    if assets_dir.exists() && !assets_dir.is_dir() {
        return Err(anyhow!("assets_path must be a directory when present"));
    }
    let tasks = load_task_specs_with_asset_root(&tasks_dir, Some(&assets_dir))?;
    if tasks.is_empty() {
        return Err(anyhow!("question bank contains no tasks"));
    }
    validate_unique_task_ids(&tasks, &assets_dir)?;
    validate_task_paths(&tasks)?;
    let checksum = checksum_directory(&root)?;
    let summary = summarize(
        manifest.id,
        manifest.version,
        manifest.name,
        manifest.description,
        manifest.subject,
        manifest.languages,
        &tasks,
        checksum,
        false,
    );
    Ok(LoadedQuestionBank { summary, tasks })
}

fn summarize(
    id: String,
    version: String,
    name: String,
    description: String,
    subject: String,
    languages: Vec<String>,
    tasks: &[BenchmarkTaskSpec],
    checksum: String,
    built_in: bool,
) -> QuestionBankSummary {
    let mut task_ids = tasks.iter().map(|task| task.id().to_string()).collect::<Vec<_>>();
    task_ids.sort();
    let mut suites = tasks.iter().map(|task| task.suite().to_string()).collect::<Vec<_>>();
    suites.sort();
    suites.dedup();
    QuestionBankSummary {
        id,
        version,
        name,
        description,
        subject,
        languages,
        task_count: tasks.len(),
        task_ids,
        suites,
        has_executable_grading: tasks.iter().any(BenchmarkTaskSpec::has_automated_checks),
        checksum,
        built_in,
    }
}

fn validate_manifest(manifest: &QuestionBankManifest) -> Result<()> {
    if manifest.protocol != BANK_PROTOCOL {
        return Err(anyhow!("unsupported question bank protocol"));
    }
    if manifest.schema_version != BANK_SCHEMA_VERSION {
        return Err(anyhow!("unsupported question bank schema version"));
    }
    validate_identifier(&manifest.id, "bank id")?;
    validate_identifier(&manifest.version, "bank version")?;
    if manifest.name.trim().is_empty() {
        return Err(anyhow!("question bank name is required"));
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(anyhow!("{label} must use only letters, numbers, '.', '_' or '-'"));
    }
    Ok(value.to_string())
}

fn resolve_relative_directory(root: &Path, value: &str, label: &str) -> Result<PathBuf> {
    let target = resolve_relative_path(root, value, label)?;
    if !target.is_dir() {
        return Err(anyhow!("{label} directory is missing"));
    }
    Ok(target)
}

fn resolve_relative_path(root: &Path, value: &str, label: &str) -> Result<PathBuf> {
    let relative = Path::new(value.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(anyhow!("{label} must be a package-relative path"));
    }
    Ok(root.join(relative))
}

fn validate_unique_task_ids(tasks: &[BenchmarkTaskSpec], assets_dir: &Path) -> Result<()> {
    let mut seen = HashSet::new();
    for task in tasks {
        let id = task.id().to_string();
        if !seen.insert(id.clone()) {
            return Err(anyhow!("duplicate task id in question bank: {id}"));
        }
        for file in &task.frontmatter.workspace_files {
            if let WorkspaceFileSpec::Asset { source, .. } = file {
                validate_asset_reference(source)?;
                if !assets_dir.is_dir() || !assets_dir.join(source).is_file() {
                    return Err(anyhow!("question bank asset is missing: {source}"));
                }
            }
        }
    }
    Ok(())
}

fn validate_asset_reference(source: &str) -> Result<()> {
    let source = Path::new(source);
    if source.is_absolute()
        || source.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(anyhow!("asset source must stay inside the question bank assets directory"));
    }
    Ok(())
}

fn validate_task_paths(tasks: &[BenchmarkTaskSpec]) -> Result<()> {
    for task in tasks {
        for file in &task.frontmatter.workspace_files {
            let path = match file {
                WorkspaceFileSpec::Asset { dest, .. } => dest,
                WorkspaceFileSpec::Inline { path, .. } => path,
            };
            validate_workspace_relative_path(path)?;
        }
    }
    Ok(())
}

fn validate_workspace_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value.trim());
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(anyhow!("workspace file path must stay inside the attempt workspace"));
    }
    Ok(())
}

pub fn build_import_preview(data: &[u8]) -> Result<QuestionBankSummary> {
    if data.is_empty() || data.len() > MAX_PACKAGE_BYTES {
        return Err(anyhow!("question bank package must be between 1 byte and {MAX_PACKAGE_BYTES} bytes"));
    }
    let staging = default_banks_dir().join(format!(".preview-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&staging)?;
    let result = (|| {
        extract_package(data, &staging)?;
        Ok(load_imported_bank(staging.clone())?.summary)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn extract_package(data: &[u8], target: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).context("question bank package must be a ZIP file")?;
    if archive.len() > MAX_FILES {
        return Err(anyhow!("question bank package has too many files"));
    }
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("question bank package contains an unsafe path"))?
            .to_path_buf();
        if enclosed.as_os_str().is_empty() {
            continue;
        }
        let output = target.join(enclosed);
        if entry.is_dir() {
            std::fs::create_dir_all(output)?;
            continue;
        }
        total = total.saturating_add(entry.size() as usize);
        if total > MAX_EXTRACTED_BYTES {
            return Err(anyhow!("question bank package expands beyond the size limit"));
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(output)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

fn checksum_directory(root: &Path) -> Result<String> {
    let mut files = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    files.sort();
    let mut hasher = Sha256::new();
    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        hasher.update(relative.as_bytes());
        let mut file = File::open(path)?;
        let mut buffer = [0_u8; 8192];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

fn resolve_config_path(relative: &str) -> PathBuf {
    let cwd_path = PathBuf::from(relative);
    if cwd_path.exists() {
        return cwd_path;
    }
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        for ancestor in Path::new(manifest_dir).ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    cwd_path
}

pub fn bank_snapshot(bank: &QuestionBankSummary) -> Value {
    json!(bank)
}

pub fn task_specs_snapshot(tasks: &[BenchmarkTaskSpec]) -> Value {
    let entries = tasks
        .iter()
        .map(|task| {
            (
                task.id().to_string(),
                json!({
                    "id": task.frontmatter.id,
                    "name": task.frontmatter.name,
                    "suite": task.frontmatter.suite,
                    "category": task.frontmatter.category,
                    "grading_type": grading_type_name(&task.frontmatter.grading_type),
                    "timeout_seconds": task.frontmatter.timeout_seconds,
                    "runs_recommended": task.frontmatter.runs_recommended,
                    "difficulty": task.frontmatter.difficulty,
                    "required_tools": task.frontmatter.required_tools,
                    "tags": task.frontmatter.tags,
                    "languages": task.frontmatter.languages,
                    "workspace_files": task.frontmatter.workspace_files,
                    "prompt": task.prompt,
                    "expected_behavior": task.expected_behavior,
                    "grading_criteria": task.grading_criteria,
                    "automated_checks": task.automated_checks,
                    "llm_judge_rubric": task.llm_judge_rubric,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Value::Object(entries)
}

fn grading_type_name(value: &BenchmarkGradingType) -> &'static str {
    match value {
        BenchmarkGradingType::Automated => "automated",
        BenchmarkGradingType::LlmJudge => "llm_judge",
        BenchmarkGradingType::Hybrid => "hybrid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_package_traversal() {
        assert!(validate_asset_reference("../outside.txt").is_err());
        assert!(validate_asset_reference("assets/ok.txt").is_ok());
        assert!(validate_workspace_relative_path("../outside.txt").is_err());
    }

    #[test]
    fn accepts_stable_identifiers() {
        assert_eq!(validate_identifier("sample-bank_1.0", "bank id").unwrap(), "sample-bank_1.0");
        assert!(validate_identifier("not/allowed", "bank id").is_err());
    }

    #[test]
    fn loads_a_versioned_bank_with_package_local_assets() {
        let root = tempdir().expect("create temp bank");
        std::fs::create_dir_all(root.path().join("tasks")).expect("create tasks directory");
        std::fs::create_dir_all(root.path().join("assets")).expect("create assets directory");
        std::fs::write(
            root.path().join(MANIFEST_FILE),
            r#"{
  "protocol": "wunderbench.question_bank",
  "schema_version": 1,
  "id": "sample-bank",
  "version": "1.0",
  "name": "Sample bank"
}"#,
        )
        .expect("write manifest");
        std::fs::write(root.path().join("assets/input.txt"), "sample input")
            .expect("write asset");
        std::fs::write(
            root.path().join("tasks/task_sample.md"),
            r#"---
id: task_sample
name: Sample task
suite: sample-suite
grading_type: automated
workspace_files:
  - source: input.txt
    dest: input.txt
---

## Prompt

Create the requested output.

## Automated Checks

```python
def grade(transcript, workspace_path):
    return {"ok": 1.0}
```
"#,
        )
        .expect("write task");

        let bank = load_imported_bank(root.path().to_path_buf()).expect("load bank");
        assert_eq!(bank.summary.id, "sample-bank");
        assert_eq!(bank.summary.task_count, 1);
        assert_eq!(bank.tasks[0].asset_root, Some(root.path().join("assets")));
    }
}
