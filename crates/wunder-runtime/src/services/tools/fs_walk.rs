//! 共享文件系统遍历辅助：默认忽略目录、`.gitignore` 语义与 glob 排除。
//!
//! `文件名匹配`（glob）与 `列出文件`（list_files）共用这里的规则，避免两套工具
//! 对「哪些目录不该出现在结果里」给出不同答案：默认剪掉依赖与构建缓存目录，并
//! 尊重 `.gitignore`；调用方需要看全部内容时传 `include_ignored=true`。
//!
//! 隐藏文件仍然可见（保持 dsh 行为），只有版本控制元数据目录始终剪枝。

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde_json::Value;
use std::path::Path;

/// 版本控制元数据目录：始终剪枝，`include_ignored=true` 也不返回。
pub(crate) const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];

/// 依赖与构建缓存目录：默认剪枝，`include_ignored=true` 时放开。
pub(crate) const DEFAULT_IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".cargo",
    ".rustup",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".next",
    ".nuxt",
    ".turbo",
    ".gradle",
    ".pnpm-store",
    "coverage",
];

/// 该目录是否需要剪枝（剪枝即整棵子树都不进入遍历）。
pub(crate) fn prunes_dir(name: &str, include_ignored: bool) -> bool {
    VCS_DIRS.contains(&name) || (!include_ignored && DEFAULT_IGNORED_DIRS.contains(&name))
}

/// 构造遍历器：`include_ignored` 为 false 时读取 `.gitignore` / `.ignore` 规则。
///
/// `require_git(false)` 让 `.gitignore` 在工作区本身不是 git 仓库时也生效，
/// 这对挂载进来的工作区与导出的离线目录同样重要。
pub(crate) fn walk_builder(root: &Path, include_ignored: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .parents(!include_ignored)
        .ignore(!include_ignored)
        .git_ignore(!include_ignored)
        .git_global(!include_ignored)
        .git_exclude(!include_ignored);
    builder
}

/// 读取 `exclude` / `excludes` 参数：单个字符串（逗号、分号或换行分隔）或字符串数组。
pub(crate) fn parse_exclude_patterns(args: &Value) -> std::result::Result<Vec<String>, String> {
    let Some(raw) = args.get("exclude").or_else(|| args.get("excludes")) else {
        return Ok(Vec::new());
    };
    let mut patterns = Vec::new();
    match raw {
        Value::Null => {}
        Value::String(text) => {
            // 调用方可能把数组写成字符串化的 JSON（`["*.log","dist"]`），
            // 按逗号硬拆会得到 `["*.log"` 这种片段，排除规则就整体失效。
            let trimmed = text.trim();
            let parsed = if trimmed.starts_with('[') {
                serde_json::from_str::<Value>(trimmed).ok()
            } else {
                None
            };
            if let Some(Value::Array(items)) = parsed {
                for item in items {
                    let Some(text) = item.as_str() else { continue };
                    let pattern = text.trim();
                    if !pattern.is_empty() {
                        patterns.push(pattern.to_string());
                    }
                }
            } else {
                for part in trimmed.split([',', ';', '\n']) {
                    let pattern = part.trim();
                    if !pattern.is_empty() {
                        patterns.push(pattern.to_string());
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                let Value::String(text) = item else {
                    return Err(format!(
                        "exclude 只接受字符串或字符串数组，其中一项是 {item}"
                    ));
                };
                let pattern = text.trim();
                if !pattern.is_empty() {
                    patterns.push(pattern.to_string());
                }
            }
        }
        other => {
            return Err(format!("exclude 只接受字符串或字符串数组，收到 {other}"));
        }
    }
    Ok(patterns)
}

/// `include_ignored`：默认 false，即剪掉默认忽略目录并尊重 `.gitignore`。
pub(crate) fn parse_include_ignored(args: &Value) -> bool {
    args.get("include_ignored")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 排除过滤器：不含分隔符的模式匹配文件/目录名，含分隔符的模式匹配相对路径。
#[derive(Debug, Default)]
pub(crate) struct ExcludeFilter {
    name_set: Option<GlobSet>,
    path_set: Option<GlobSet>,
}

impl ExcludeFilter {
    pub(crate) fn new(patterns: &[String]) -> std::result::Result<Option<Self>, String> {
        if patterns.is_empty() {
            return Ok(None);
        }
        let mut name_builder = GlobSetBuilder::new();
        let mut path_builder = GlobSetBuilder::new();
        let mut has_name = false;
        let mut has_path = false;
        for pattern in patterns {
            let normalized = pattern.replace('\\', "/");
            let glob =
                Glob::new(&normalized).map_err(|err| format!("无效的 exclude 模式：{err}"))?;
            if normalized.contains('/') {
                path_builder.add(glob);
                has_path = true;
            } else {
                name_builder.add(glob);
                has_name = true;
            }
        }
        let name_set = if has_name {
            Some(
                name_builder
                    .build()
                    .map_err(|err| format!("无效的 exclude 模式：{err}"))?,
            )
        } else {
            None
        };
        let path_set = if has_path {
            Some(
                path_builder
                    .build()
                    .map_err(|err| format!("无效的 exclude 模式：{err}"))?,
            )
        } else {
            None
        };
        Ok(Some(Self { name_set, path_set }))
    }

    /// 目录也参与排除（整棵子树剪掉）；文件按同一规则过滤。
    pub(crate) fn is_match(&self, rel_path: &str, name: &str) -> bool {
        self.name_set.as_ref().is_some_and(|set| set.is_match(name))
            || self
                .path_set
                .as_ref()
                .is_some_and(|set| set.is_match(rel_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prunes_dependency_dirs_by_default_but_keeps_vcs_always_pruned() {
        assert!(prunes_dir("node_modules", false));
        assert!(prunes_dir("target", false));
        assert!(!prunes_dir("node_modules", true));
        assert!(prunes_dir(".git", true));
        assert!(!prunes_dir("src", false));
    }

    #[test]
    fn exclude_accepts_string_and_array_forms() {
        assert_eq!(
            parse_exclude_patterns(&json!({ "exclude": "a,b;c\nd" })).expect("string form"),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(
            parse_exclude_patterns(&json!({ "exclude": ["a", " b "] })).expect("array form"),
            vec!["a", "b"]
        );
        assert!(parse_exclude_patterns(&json!({ "exclude": [1] })).is_err());
        assert!(parse_exclude_patterns(&json!({}))
            .expect("missing")
            .is_empty());
    }

    #[test]
    fn exclude_filter_matches_names_and_relative_paths() {
        let filter = ExcludeFilter::new(&["*.md".to_string(), "docs/legacy/*".to_string()])
            .expect("filter")
            .expect("set");
        assert!(filter.is_match("README.md", "README.md"));
        assert!(filter.is_match("docs/legacy/a.txt", "a.txt"));
        assert!(!filter.is_match("src/lib.rs", "lib.rs"));
    }

    #[test]
    fn walk_skips_default_ignored_dirs_but_include_ignored_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).expect("mkdir");
        std::fs::write(dir.path().join("node_modules/pkg/a.js"), "x").expect("write");
        std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
        std::fs::write(dir.path().join("src/a.rs"), "x").expect("write");

        let mut builder = walk_builder(dir.path(), false);
        builder.filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            !(is_dir && prunes_dir(&entry.file_name().to_string_lossy(), false))
        });
        let pruned = builder
            .build()
            .filter_map(|item| item.ok())
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .map(|entry| entry.path().to_path_buf())
            .collect::<Vec<_>>();
        assert!(pruned.iter().any(|path| path.ends_with("src/a.rs")));
        assert!(!pruned
            .iter()
            .any(|path| path.to_string_lossy().contains("node_modules")));

        let raw = walk_builder(dir.path(), true)
            .build()
            .filter_map(|item| item.ok())
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .count();
        assert_eq!(raw, 2);
    }

    #[test]
    fn exclude_filter_requires_valid_glob() {
        assert!(ExcludeFilter::new(&["[".to_string()]).is_err());
        assert!(ExcludeFilter::new(&[]).expect("empty ok").is_none());
    }
}
