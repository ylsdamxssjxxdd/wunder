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

pub fn should_skip_walk_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    DEFAULT_IGNORED_DIRS
        .iter()
        .any(|item| name.eq_ignore_ascii_case(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
}
