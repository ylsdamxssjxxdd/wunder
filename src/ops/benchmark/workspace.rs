use super::loader::default_assets_dir;
use super::spec::{BenchmarkTaskSpec, WorkspaceFileSpec};
use crate::workspace::WorkspaceManager;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn prepare_attempt_workspace(
    workspace: &WorkspaceManager,
    user_id: &str,
    run_id: &str,
    task: &BenchmarkTaskSpec,
    attempt_no: u32,
) -> Result<(PathBuf, String)> {
    let relative_root = build_attempt_root(run_id, task.id(), attempt_no);
    let target = workspace.resolve_path(user_id, &relative_root)?;
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("清理 benchmark 工作区失败: {target:?}"))?;
    }
    std::fs::create_dir_all(&target)
        .with_context(|| format!("创建 benchmark 工作区失败: {target:?}"))?;

    for file in &task.frontmatter.workspace_files {
        write_workspace_file(&target, run_id, task.id(), attempt_no, &relative_root, file)?;
    }
    workspace.bump_version(user_id);
    Ok((target, relative_root))
}

pub fn build_attempt_root(run_id: &str, task_id: &str, attempt_no: u32) -> String {
    format!("benchmark/{run_id}/{task_id}/attempt_{attempt_no}")
}

pub fn apply_task_placeholders(
    text: &str,
    run_id: &str,
    task_id: &str,
    attempt_no: u32,
    attempt_root: &str,
) -> String {
    text.replace("{run_id}", run_id)
        .replace("{task_id}", task_id)
        .replace("{attempt_no}", &attempt_no.to_string())
        .replace("{attempt_root}", attempt_root)
}

pub fn build_artifact_manifest(root: &Path) -> Result<Vec<Value>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(path).with_context(|| format!("读取产物失败: {path:?}"))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let preview = std::str::from_utf8(&bytes)
            .map(|text| text.chars().take(500).collect::<String>())
            .unwrap_or_default();
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        output.push(json!({
            "path": relative_path,
            "size": bytes.len(),
            "sha256": sha256,
            "preview": preview,
            "text": !preview.is_empty(),
        }));
    }
    output.sort_by(|left, right| {
        left.get("path")
            .and_then(Value::as_str)
            .cmp(&right.get("path").and_then(Value::as_str))
    });
    Ok(output)
}

fn write_workspace_file(
    root: &Path,
    run_id: &str,
    task_id: &str,
    attempt_no: u32,
    attempt_root: &str,
    spec: &WorkspaceFileSpec,
) -> Result<()> {
    match spec {
        WorkspaceFileSpec::Asset { source, dest } => {
            let source_path = default_assets_dir().join(source);
            let target_path = root.join(apply_task_placeholders(
                dest,
                run_id,
                task_id,
                attempt_no,
                attempt_root,
            ));
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source_path, &target_path).with_context(|| {
                format!("复制 benchmark 素材失败: {source_path:?} -> {target_path:?}")
            })?;
        }
        WorkspaceFileSpec::Inline { path, content } => {
            let target_path = root.join(apply_task_placeholders(
                path,
                run_id,
                task_id,
                attempt_no,
                attempt_root,
            ));
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let rendered =
                apply_task_placeholders(content, run_id, task_id, attempt_no, attempt_root);
            std::fs::write(&target_path, rendered)
                .with_context(|| format!("写入 benchmark 素材失败: {target_path:?}"))?;
        }
    }
    Ok(())
}
