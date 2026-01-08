// 路径工具：统一处理跨平台路径规范化与边界检查。
use std::fs;
use std::path::{Path, PathBuf};

pub fn normalize_existing_path(path: &Path) -> PathBuf {
    if path.exists() {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

pub fn normalize_target_path(path: &Path) -> PathBuf {
    if path.exists() {
        return fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    }
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    if !parent.exists() {
        return path.to_path_buf();
    }
    let parent_canonical = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    match path.file_name() {
        Some(name) => parent_canonical.join(name),
        None => parent_canonical,
    }
}

pub fn normalize_path_for_compare(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let text = path.to_string_lossy().replace('/', "\\");
        let trimmed = text.strip_prefix(r"\\?\").unwrap_or(&text);
        PathBuf::from(trimmed.to_lowercase())
    } else {
        path.to_path_buf()
    }
}

pub fn is_within_root(root: &Path, target: &Path) -> bool {
    let normalized_root = normalize_existing_path(root);
    let normalized_target = normalize_target_path(target);
    let root_compare = normalize_path_for_compare(&normalized_root);
    let target_compare = normalize_path_for_compare(&normalized_target);
    target_compare.starts_with(&root_compare)
}
