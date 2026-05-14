pub(crate) const SKILL_ROOT_PLACEHOLDER: &str = "{{SKILL_ROOT}}";

pub(crate) fn render_skill_markdown_for_model(raw: &str, skill_root: &str) -> String {
    if raw.contains(SKILL_ROOT_PLACEHOLDER) {
        raw.replace(SKILL_ROOT_PLACEHOLDER, skill_root)
    } else {
        raw.to_string()
    }
}

pub(crate) fn parse_skill_name_candidates(raw_name: &str) -> Vec<String> {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut names = vec![trimmed.to_string()];
    if let Some((_, suffix)) = trimmed.split_once('@') {
        let suffix = suffix.trim();
        if !suffix.is_empty() && !names.iter().any(|item| item == suffix) {
            names.push(suffix.to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{
        parse_skill_name_candidates, render_skill_markdown_for_model, SKILL_ROOT_PLACEHOLDER,
    };

    #[test]
    fn replaces_skill_root_placeholder() {
        let raw = format!(
            "run: {SKILL_ROOT_PLACEHOLDER}/scripts/tool.py --input {SKILL_ROOT_PLACEHOLDER}/data.json"
        );
        let rendered = render_skill_markdown_for_model(&raw, "C:/tmp/skills/demo");
        assert_eq!(
            rendered,
            "run: C:/tmp/skills/demo/scripts/tool.py --input C:/tmp/skills/demo/data.json"
        );
    }

    #[test]
    fn keeps_content_without_placeholder() {
        let raw = "no placeholder";
        let rendered = render_skill_markdown_for_model(raw, "C:/tmp/skills/demo");
        assert_eq!(rendered, raw);
    }

    #[test]
    fn parses_skill_name_candidates_with_owner_alias() {
        assert_eq!(
            parse_skill_name_candidates("alice@planner"),
            vec!["alice@planner".to_string(), "planner".to_string()]
        );
    }

    #[test]
    fn parses_skill_name_candidates_with_plain_name() {
        assert_eq!(
            parse_skill_name_candidates("planner"),
            vec!["planner".to_string()]
        );
    }
}
