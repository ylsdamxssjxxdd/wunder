pub(crate) const SKILL_ROOT_PLACEHOLDER: &str = "{{SKILL_ROOT}}";

pub(crate) fn render_skill_markdown_for_model(raw: &str, skill_root: &str) -> String {
    if raw.contains(SKILL_ROOT_PLACEHOLDER) {
        raw.replace(SKILL_ROOT_PLACEHOLDER, skill_root)
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{render_skill_markdown_for_model, SKILL_ROOT_PLACEHOLDER};

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
}
