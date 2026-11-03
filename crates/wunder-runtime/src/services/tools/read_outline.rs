pub(crate) fn read_outline(content: &str, max_entries: usize) -> (Vec<(usize, String)>, usize) {
    let mut entries = Vec::new();
    let mut total = 0usize;
    for (index, line) in content.lines().enumerate() {
        if !is_outline_line(line) {
            continue;
        }
        total += 1;
        if entries.len() < max_entries.max(1) {
            entries.push((index + 1, line.to_string()));
        }
    }
    (entries, total)
}

fn is_outline_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("--")
    {
        return false;
    }
    if trimmed.starts_with('#') {
        return trimmed
            .trim_start_matches('#')
            .starts_with(char::is_whitespace);
    }
    let declaration = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub(super) "))
        .or_else(|| trimmed.strip_prefix("pub "))
        .or_else(|| trimmed.strip_prefix("export "))
        .or_else(|| trimmed.strip_prefix("default "))
        .unwrap_or(trimmed);
    let declaration = declaration.strip_prefix("async ").unwrap_or(declaration);
    [
        "fn ",
        "func ",
        "def ",
        "class ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "interface ",
        "type ",
        "mod ",
        "namespace ",
        "record ",
        "function ",
    ]
    .iter()
    .any(|prefix| declaration.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_collects_markdown_and_common_code_declarations() {
        let content = "# Title\nfn main() {\n    let value = 1;\n}\n\nclass Example:\n    def method(self):\n        pass\n";
        let (entries, total) = read_outline(content, 20);
        assert_eq!(total, 4);
        assert_eq!(
            entries.iter().map(|entry| entry.0).collect::<Vec<_>>(),
            vec![1, 2, 6, 7]
        );
    }

    #[test]
    fn outline_reports_entries_beyond_the_return_cap() {
        let (entries, total) = read_outline("fn a() {}\nfn b() {}\nfn c() {}\n", 2);
        assert_eq!(entries.len(), 2);
        assert_eq!(total, 3);
    }
}
