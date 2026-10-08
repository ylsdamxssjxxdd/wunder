use std::path::{Path, PathBuf};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) fn format_directory_display(
    directory: &Path,
    repo_root: Option<&Path>,
    max_width: Option<usize>,
) -> String {
    let formatted = repo_root
        .and_then(|root| format_repo_relative_path(directory, root))
        .unwrap_or_else(|| format_path_for_display(directory));

    match max_width {
        Some(limit) => truncate_path_display(formatted.as_str(), limit),
        None => formatted,
    }
}

fn format_repo_relative_path(directory: &Path, repo_root: &Path) -> Option<String> {
    let relative = directory.strip_prefix(repo_root).ok()?;
    let root_name = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())?;

    if relative.as_os_str().is_empty() {
        return Some(root_name.to_string());
    }

    Some(format!(
        "{root_name}/{}",
        normalize_path_separators(relative)
    ))
}

fn format_path_for_display(path: &Path) -> String {
    if let Some(home) = resolve_home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            if relative.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", normalize_path_separators(relative));
        }
    }
    normalize_path_separators(path)
}

fn resolve_home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            let mut home = PathBuf::from(drive);
            home.push(path);
            Some(home)
        })
}

fn normalize_path_separators(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
fn truncate_path_display(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }

    if let Some(compact) = compact_path_segments(text, max_width) {
        if UnicodeWidthStr::width(compact.as_str()) <= max_width {
            return compact;
        }
    }

    center_truncate_text(text, max_width)
}

fn compact_path_segments(text: &str, max_width: usize) -> Option<String> {
    let leading = if text.starts_with("~/") {
        "~/"
    } else if text.starts_with('/') {
        "/"
    } else {
        ""
    };
    let trimmed = text.trim_start_matches("~/").trim_start_matches('/');
    let segments = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }

    let first = segments[0];
    let mut tail_segments = vec![segments[segments.len() - 1]];
    let mut best = format!("{leading}{first}/…/{}", tail_segments.join("/"));

    for segment in segments[1..segments.len() - 1].iter().rev() {
        let mut candidate_tail = Vec::with_capacity(tail_segments.len() + 1);
        candidate_tail.push(*segment);
        candidate_tail.extend(tail_segments.iter().copied());
        let candidate = format!("{leading}{first}/…/{}", candidate_tail.join("/"));
        if UnicodeWidthStr::width(candidate.as_str()) > max_width {
            break;
        }
        tail_segments = candidate_tail;
        best = candidate;
    }

    Some(best)
}

fn center_truncate_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let suffix_budget = max_width / 2;
    let prefix_budget = max_width.saturating_sub(suffix_budget + 1);
    let prefix = take_prefix_width(text, prefix_budget);
    let remaining_suffix_budget =
        max_width.saturating_sub(UnicodeWidthStr::width(prefix.as_str()) + 1);
    let suffix = take_suffix_width(text, remaining_suffix_budget);

    format!("{prefix}…{suffix}")
}

fn take_prefix_width(text: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut output = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        output.push(ch);
    }
    output
}

fn take_suffix_width(text: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut chars = Vec::new();
    for ch in text.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        chars.push(ch);
    }
    chars.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::format_directory_display;
    use std::path::PathBuf;

    #[test]
    fn prefers_repo_relative_directory_display() {
        let repo_root = PathBuf::from("workspace").join("wunder");
        let directory = repo_root.join("frontend").join("src").join("views");

        let display =
            format_directory_display(directory.as_path(), Some(repo_root.as_path()), None);

        assert_eq!(display, "wunder/frontend/src/views");
    }

    #[test]
    fn truncates_middle_segments_before_falling_back() {
        let repo_root = PathBuf::from("workspace").join("wunder");
        let directory = repo_root.join("frontend").join("src").join("views");

        let display =
            format_directory_display(directory.as_path(), Some(repo_root.as_path()), Some(18));

        assert_eq!(display, "wunder/…/src/views");
    }
}
