use std::cmp;

const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone)]
pub(crate) struct IndentationReadOptions {
    pub(crate) anchor_line: Option<usize>,
    pub(crate) max_levels: usize,
    pub(crate) include_siblings: bool,
    pub(crate) include_header: bool,
    pub(crate) max_lines: Option<usize>,
}

impl Default for IndentationReadOptions {
    fn default() -> Self {
        Self {
            anchor_line: None,
            max_levels: 0,
            include_siblings: false,
            include_header: true,
            max_lines: None,
        }
    }
}

fn indentation_of(line: &str) -> usize {
    let mut width = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += TAB_WIDTH,
            _ => break,
        }
    }
    width
}

fn is_comment_or_attribute(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#')
        || trimmed.starts_with("//")
        || trimmed.starts_with("--")
        || trimmed.starts_with('@')
}

fn first_non_empty_line(lines: &[&str], index: usize) -> usize {
    if !lines
        .get(index)
        .map(|line| line.trim().is_empty())
        .unwrap_or(true)
    {
        return index;
    }
    for offset in 1..lines.len() {
        let down = index.saturating_add(offset);
        if let Some(line) = lines.get(down) {
            if !line.trim().is_empty() {
                return down;
            }
        }
        if let Some(up) = index.checked_sub(offset) {
            if !lines[up].trim().is_empty() {
                return up;
            }
        }
    }
    index
}

/// Read an indentation-aware code block around `anchor_line`.
///
/// The function keeps line numbers stable and returns `(line_no, content)` pairs.
pub(crate) fn read_block(content: &str, options: &IndentationReadOptions) -> Vec<(usize, String)> {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }

    let requested_anchor = options.anchor_line.unwrap_or(1).max(1);
    let anchor_index = requested_anchor.saturating_sub(1).min(lines.len() - 1);
    let anchor_index = first_non_empty_line(&lines, anchor_index);
    let anchor_indent = indentation_of(lines[anchor_index]);
    let min_indent = if options.max_levels == 0 {
        0
    } else {
        anchor_indent.saturating_sub(options.max_levels.saturating_mul(TAB_WIDTH))
    };

    let mut start = anchor_index;
    let mut end = anchor_index;
    let mut met_floor_up = false;

    // Expand upward while preserving the anchor indentation tree.
    while let Some(prev) = start.checked_sub(1) {
        let line = lines[prev];
        if line.trim().is_empty() {
            start = prev;
            continue;
        }
        let indent = indentation_of(line);
        if indent < min_indent {
            break;
        }
        if !options.include_siblings && indent == min_indent {
            if met_floor_up {
                break;
            }
            met_floor_up = true;
        }
        start = prev;
    }

    let mut floor_seen_down = !options.include_siblings
        && (start..=end).any(|index| {
            let line = lines[index];
            !line.trim().is_empty() && indentation_of(line) == min_indent
        });

    // Expand downward with the same floor rule.
    while end + 1 < lines.len() {
        let next = end + 1;
        let line = lines[next];
        if line.trim().is_empty() {
            end = next;
            continue;
        }
        let indent = indentation_of(line);
        if indent < min_indent {
            break;
        }
        if !options.include_siblings && indent == min_indent {
            if floor_seen_down {
                break;
            }
            floor_seen_down = true;
        }
        end = next;
    }

    if options.include_header {
        while let Some(prev) = start.checked_sub(1) {
            let line = lines[prev];
            if line.trim().is_empty() {
                start = prev;
                continue;
            }
            let indent = indentation_of(line);
            if indent > anchor_indent || !is_comment_or_attribute(line) {
                break;
            }
            start = prev;
        }
    }

    let line_limit = options
        .max_lines
        .unwrap_or(lines.len())
        .max(1)
        .min(lines.len());
    let selected_len = end.saturating_sub(start) + 1;
    let final_end = if selected_len > line_limit {
        start + line_limit - 1
    } else {
        end
    };

    let mut output = Vec::with_capacity(cmp::min(selected_len, line_limit));
    for (idx, line) in lines.iter().enumerate().take(final_end + 1).skip(start) {
        output.push((idx + 1, (*line).to_string()));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_block_keeps_anchor_tree_without_siblings() {
        let content = r#"def outer():
    x = 1
    if x:
        run()

def sibling():
    pass
"#;
        let options = IndentationReadOptions {
            anchor_line: Some(3),
            max_levels: 1,
            include_siblings: false,
            include_header: true,
            max_lines: None,
        };
        let block = read_block(content, &options);
        let lines = block.into_iter().map(|item| item.0).collect::<Vec<_>>();
        assert_eq!(lines, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn read_block_respects_max_lines() {
        let content = "a\nb\nc\nd\ne\n";
        let options = IndentationReadOptions {
            anchor_line: Some(1),
            max_levels: 0,
            include_siblings: true,
            include_header: true,
            max_lines: Some(3),
        };
        let block = read_block(content, &options);
        assert_eq!(block.len(), 3);
        assert_eq!(block[0].0, 1);
        assert_eq!(block[2].0, 3);
    }

    #[test]
    fn read_block_can_include_siblings_when_enabled() {
        let content = r#"def outer():
    x = 1
    if x:
        run()

def sibling():
    pass
"#;
        let options = IndentationReadOptions {
            anchor_line: Some(3),
            max_levels: 1,
            include_siblings: true,
            include_header: true,
            max_lines: None,
        };
        let block = read_block(content, &options);
        let lines = block.into_iter().map(|item| item.0).collect::<Vec<_>>();
        assert_eq!(lines, vec![1, 2, 3, 4, 5, 6, 7]);
    }
}
