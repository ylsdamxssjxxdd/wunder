use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn project_root_name(repo_root: &Path) -> Option<String> {
    repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let display = crate::path_display::format_directory_display(repo_root, None, None);
            let trimmed = display.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

pub(crate) fn read_git_branch(repo_root: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(repo_root)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    parse_git_head(head.as_str())
}

pub(crate) fn format_branch_display(branch: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_width {
        return trimmed.to_string();
    }
    if max_width == 1 {
        return "...".to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let suffix_width = max_width.saturating_sub(3) / 2;
    let prefix_width = max_width.saturating_sub(suffix_width + 3);
    let prefix = trimmed.chars().take(prefix_width).collect::<String>();
    let suffix = trimmed
        .chars()
        .rev()
        .take(suffix_width)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn resolve_git_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    if !dot_git.is_file() {
        return None;
    }
    let pointer = fs::read_to_string(dot_git).ok()?;
    let gitdir = pointer.trim().strip_prefix("gitdir:")?.trim();
    let candidate = PathBuf::from(gitdir);
    if candidate.is_absolute() {
        Some(candidate)
    } else {
        Some(repo_root.join(candidate))
    }
}

fn parse_git_head(head: &str) -> Option<String> {
    let trimmed = head.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(reference) = trimmed.strip_prefix("ref:") {
        return Path::new(reference.trim())
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string);
    }
    let short = trimmed.chars().take(7).collect::<String>();
    (!short.is_empty()).then_some(short)
}

#[cfg(test)]
mod tests {
    use super::{format_branch_display, project_root_name, read_git_branch};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn project_root_name_prefers_basename() {
        let root = PathBuf::from("workspace").join("wunder");
        assert_eq!(project_root_name(root.as_path()).as_deref(), Some("wunder"));
    }

    #[test]
    fn read_git_branch_from_dot_git_directory() {
        let repo = unique_temp_dir("branch-dir");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();

        assert_eq!(read_git_branch(repo.as_path()).as_deref(), Some("main"));

        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn read_git_branch_from_dot_git_pointer_file() {
        let repo = unique_temp_dir("branch-worktree");
        let actual_git_dir = repo.join(".wunder-git");
        fs::create_dir_all(&actual_git_dir).unwrap();
        fs::write(repo.join(".git"), "gitdir: .wunder-git\n").unwrap();
        fs::write(
            actual_git_dir.join("HEAD"),
            "ref: refs/heads/feature/footer\n",
        )
        .unwrap();

        assert_eq!(read_git_branch(repo.as_path()).as_deref(), Some("footer"));

        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn format_branch_display_truncates_middle() {
        assert_eq!(
            format_branch_display("feature/really-long-branch-name", 12),
            "featu...name"
        );
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wunder-cli-{label}-{unique}"))
    }
}
