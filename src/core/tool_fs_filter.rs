use std::path::{Component, Path};
use walkdir::DirEntry;

const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
];

fn is_ignored_dir_name(name: &str) -> bool {
    DEFAULT_IGNORED_DIRS
        .iter()
        .any(|item| name.eq_ignore_ascii_case(item))
}

pub fn should_skip_walk_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    is_ignored_dir_name(name.as_ref())
}

pub fn should_skip_path(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        is_ignored_dir_name(name.to_string_lossy().as_ref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;
    use walkdir::WalkDir;

    #[test]
    fn should_skip_common_noise_directories() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".git").join("objects")).expect("create .git");
        fs::create_dir_all(dir.path().join("src")).expect("create src");

        let entries = WalkDir::new(dir.path())
            .into_iter()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();

        let git_entry = entries
            .iter()
            .find(|entry| entry.file_name().to_string_lossy() == ".git")
            .expect("git entry");
        let src_entry = entries
            .iter()
            .find(|entry| entry.file_name().to_string_lossy() == "src")
            .expect("src entry");

        assert!(should_skip_walk_entry(git_entry));
        assert!(!should_skip_walk_entry(src_entry));
    }

    #[test]
    fn should_skip_paths_inside_ignored_directories() {
        assert!(should_skip_path(Path::new("project/.git/config")));
        assert!(should_skip_path(Path::new(
            "workspace/node_modules/pkg/index.js"
        )));
        assert!(!should_skip_path(Path::new("workspace/src/main.rs")));
    }
}
