pub const WORKER_CARD_FILE_SUFFIX: &str = ".worker-card.json";
const WORKER_CARD_ID_SEPARATOR: &str = "--";

pub fn sanitize_worker_card_filename_part(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        let mapped = match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            '\u{0000}'..='\u{001f}' => '_',
            _ => ch,
        };
        output.push(mapped);
    }
    output = output
        .trim_matches(|ch: char| ch.is_whitespace() || ch == '.' || ch == '_')
        .to_string();
    if output.is_empty() {
        String::new()
    } else {
        output
    }
}

pub fn normalize_worker_card_identity(raw: Option<&str>, default_value: &str) -> String {
    let cleaned = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_value);
    if cleaned.eq_ignore_ascii_case("default") {
        default_value.to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn worker_card_file_name(display_name: Option<&str>, stable_id: Option<&str>) -> String {
    let name_part = display_name
        .map(sanitize_worker_card_filename_part)
        .filter(|value| !value.is_empty());
    let id_part = stable_id
        .map(sanitize_worker_card_filename_part)
        .filter(|value| !value.is_empty());
    let stem = match (name_part, id_part) {
        (Some(name), Some(id)) if name != id => format!("{name}{WORKER_CARD_ID_SEPARATOR}{id}"),
        (Some(name), Some(_)) => name,
        (Some(name), None) => name,
        (None, Some(id)) => id,
        (None, None) => "worker-card".to_string(),
    };
    format!("{stem}{WORKER_CARD_FILE_SUFFIX}")
}

pub fn stable_id_from_worker_card_file_name(file_name: &str) -> Option<String> {
    let trimmed = file_name.trim();
    if !trimmed.ends_with(WORKER_CARD_FILE_SUFFIX) {
        return None;
    }
    let stem = trimmed
        .trim_end_matches(WORKER_CARD_FILE_SUFFIX)
        .trim()
        .to_string();
    if stem.is_empty() {
        return None;
    }
    if let Some((_, id)) = stem.rsplit_once(WORKER_CARD_ID_SEPARATOR) {
        let cleaned = id.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    Some(stem)
}

#[cfg(test)]
mod tests {
    use super::{
        sanitize_worker_card_filename_part, stable_id_from_worker_card_file_name,
        worker_card_file_name,
    };

    #[test]
    fn worker_card_file_name_prefers_name_prefix_and_stable_id_suffix() {
        assert_eq!(
            worker_card_file_name(Some("Demo Agent"), Some("agent_demo")),
            "Demo Agent--agent_demo.worker-card.json"
        );
        assert_eq!(
            worker_card_file_name(Some("agent_demo"), Some("agent_demo")),
            "agent_demo.worker-card.json"
        );
    }

    #[test]
    fn stable_id_from_worker_card_file_name_reads_suffix_when_present() {
        assert_eq!(
            stable_id_from_worker_card_file_name("Demo Agent--agent_demo.worker-card.json"),
            Some("agent_demo".to_string())
        );
        assert_eq!(
            stable_id_from_worker_card_file_name("agent_demo.worker-card.json"),
            Some("agent_demo".to_string())
        );
    }

    #[test]
    fn sanitize_worker_card_filename_part_replaces_windows_invalid_chars() {
        assert_eq!(
            sanitize_worker_card_filename_part(" Demo:Agent? "),
            "Demo_Agent".to_string()
        );
    }
}
