use std::fs;
use std::path::{Path, PathBuf};

pub fn strip_windows_verbatim_prefix(text: &str) -> &str {
    text.strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix("//?/"))
        .unwrap_or(text)
}

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
    for ancestor in path.ancestors() {
        if !ancestor.exists() {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(ancestor) {
            let base = fs::canonicalize(ancestor).unwrap_or_else(|_| ancestor.to_path_buf());
            if relative.as_os_str().is_empty() {
                return base;
            }
            return base.join(relative);
        }
    }
    path.to_path_buf()
}

pub fn normalize_path_for_compare(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let text = path.to_string_lossy().replace('/', "\\");
        let trimmed = strip_windows_verbatim_prefix(&text);
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

#[cfg(test)]
mod tests {
    use super::{is_within_root, normalize_path_for_compare, strip_windows_verbatim_prefix};
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn strip_windows_verbatim_prefix_handles_double_slash_variants() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\C:\sample\file.txt"),
            r"C:\sample\file.txt"
        );
        assert_eq!(
            strip_windows_verbatim_prefix("//?/C:/sample/file.txt"),
            "C:/sample/file.txt"
        );
    }

    #[test]
    #[cfg(windows)]
    fn normalize_path_for_compare_strips_verbatim_prefix() {
        let path = normalize_path_for_compare(Path::new(r"\\?\C:\sample\file.txt"));
        assert_eq!(path, Path::new(r"c:\sample\file.txt"));
    }

    #[test]
    fn is_within_root_accepts_existing_descendant() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("root");
        let child_dir = root.join("child");
        fs::create_dir_all(&child_dir).expect("create child");
        let target = child_dir.join("file.txt");
        fs::write(&target, "ok").expect("write target");

        assert!(is_within_root(&root, &target));
    }

    #[test]
    fn is_within_root_accepts_missing_descendant_under_existing_parent() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).expect("create root");
        let target = root.join("child").join("future.txt");

        assert!(is_within_root(&root, &target));
    }

    #[test]
    fn is_within_root_rejects_sibling_with_same_prefix() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("root");
        let sibling = dir.path().join("root_sibling");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&sibling).expect("create sibling");
        let target = sibling.join("file.txt");

        assert!(!is_within_root(&root, &target));
    }
}
