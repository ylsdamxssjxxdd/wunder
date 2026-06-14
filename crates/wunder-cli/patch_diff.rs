#[derive(Debug, Clone, Default)]
pub(crate) struct PatchDiffPreview {
    pub(crate) blocks: Vec<PatchDiffBlock>,
    pub(crate) omitted_text: Option<String>,
}

impl PatchDiffPreview {
    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.omitted_text.is_none()
    }

    pub(crate) fn item_count(&self) -> usize {
        self.blocks.len() + usize::from(self.omitted_text.is_some())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PatchDiffBlock {
    pub(crate) kind: PatchDiffBlockKind,
    pub(crate) header: String,
    pub(crate) lines: Vec<PatchDiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchDiffBlockKind {
    Add,
    Delete,
    Update,
    Rename,
}

#[derive(Debug, Clone)]
pub(crate) struct PatchDiffLine {
    pub(crate) kind: PatchDiffLineKind,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchDiffLineKind {
    Hunk,
    Context,
    Add,
    Delete,
    Note,
}

struct PatchDiffBlockBuilder {
    kind: PatchDiffBlockKind,
    path: String,
    to_path: Option<String>,
    lines: Vec<PatchDiffLine>,
    hidden_lines: usize,
    max_lines: usize,
    is_zh: bool,
}

impl PatchDiffBlockBuilder {
    fn new(kind: PatchDiffBlockKind, path: &str, max_lines: usize, is_zh: bool) -> Self {
        Self {
            kind,
            path: path.trim().to_string(),
            to_path: None,
            lines: Vec::new(),
            hidden_lines: 0,
            max_lines,
            is_zh,
        }
    }

    fn push_line(&mut self, kind: PatchDiffLineKind, text: &str) {
        if self.lines.len() >= self.max_lines {
            self.hidden_lines = self.hidden_lines.saturating_add(1);
            return;
        }
        self.lines.push(PatchDiffLine {
            kind,
            text: text.to_string(),
        });
    }

    fn finish(mut self) -> PatchDiffBlock {
        if self.kind == PatchDiffBlockKind::Delete && self.lines.is_empty() {
            self.lines.push(PatchDiffLine {
                kind: PatchDiffLineKind::Delete,
                text: if self.is_zh {
                    "整个文件已删除".to_string()
                } else {
                    "entire file deleted".to_string()
                },
            });
        }

        if self.hidden_lines > 0 {
            self.lines.push(PatchDiffLine {
                kind: PatchDiffLineKind::Note,
                text: if self.is_zh {
                    format!("还有 {} 行变更", self.hidden_lines)
                } else {
                    format!("+{} more lines", self.hidden_lines)
                },
            });
        }

        let header = match (self.kind, self.to_path.as_deref()) {
            (PatchDiffBlockKind::Add, _) => {
                if self.is_zh {
                    format!("diff {}（新文件）", self.path)
                } else {
                    format!("diff {} (new file)", self.path)
                }
            }
            (PatchDiffBlockKind::Delete, _) => {
                if self.is_zh {
                    format!("diff {}（已删除）", self.path)
                } else {
                    format!("diff {} (deleted)", self.path)
                }
            }
            (PatchDiffBlockKind::Rename, Some(to_path)) => {
                format!("diff {} → {}", self.path, to_path)
            }
            _ => format!("diff {}", self.path),
        };

        PatchDiffBlock {
            kind: self.kind,
            header,
            lines: self.lines,
        }
    }
}

pub(crate) fn build_patch_diff_preview(
    patch: &str,
    max_files: usize,
    max_lines_per_file: usize,
    is_zh: bool,
) -> PatchDiffPreview {
    let mut preview = PatchDiffPreview::default();
    let mut active: Option<PatchDiffBlockBuilder> = None;
    let mut hidden_files = 0usize;

    fn push_active(preview: &mut PatchDiffPreview, active: &mut Option<PatchDiffBlockBuilder>) {
        if let Some(block) = active.take() {
            preview.blocks.push(block.finish());
        }
    }

    for raw_line in patch.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed == "*** Begin Patch" || trimmed == "*** End Patch" {
            continue;
        }

        let mut start_block = |kind: PatchDiffBlockKind, path: &str| {
            push_active(&mut preview, &mut active);
            if preview.blocks.len() >= max_files {
                hidden_files = hidden_files.saturating_add(1);
                active = None;
                return;
            }
            active = Some(PatchDiffBlockBuilder::new(
                kind,
                path,
                max_lines_per_file,
                is_zh,
            ));
        };

        if let Some(path) = trimmed.strip_prefix("*** Add File:") {
            start_block(PatchDiffBlockKind::Add, path);
            continue;
        }
        if let Some(path) = trimmed.strip_prefix("*** Delete File:") {
            start_block(PatchDiffBlockKind::Delete, path);
            continue;
        }
        if let Some(path) = trimmed.strip_prefix("*** Update File:") {
            start_block(PatchDiffBlockKind::Update, path);
            continue;
        }
        if let Some(path) = trimmed.strip_prefix("*** Move to:") {
            if let Some(block) = active.as_mut() {
                block.kind = PatchDiffBlockKind::Rename;
                block.to_path = Some(path.trim().to_string());
            }
            continue;
        }

        let Some(block) = active.as_mut() else {
            continue;
        };

        if trimmed == "*** End of File" {
            block.push_line(
                PatchDiffLineKind::Note,
                if is_zh { "文件结尾" } else { "end of file" },
            );
            continue;
        }

        if raw_line.starts_with("@@") {
            let text = raw_line.trim_start_matches("@@").trim_start();
            block.push_line(PatchDiffLineKind::Hunk, text);
        } else if let Some(text) = raw_line.strip_prefix('+') {
            block.push_line(PatchDiffLineKind::Add, text);
        } else if let Some(text) = raw_line.strip_prefix('-') {
            block.push_line(PatchDiffLineKind::Delete, text);
        } else if let Some(text) = raw_line.strip_prefix(' ') {
            block.push_line(PatchDiffLineKind::Context, text);
        }
    }

    push_active(&mut preview, &mut active);
    if hidden_files > 0 {
        preview.omitted_text = Some(if is_zh {
            format!("… 还有 {hidden_files} 个文件")
        } else {
            format!("… +{hidden_files} more files")
        });
    }
    preview
}

pub(crate) fn format_patch_diff_preview_lines(preview: &PatchDiffPreview) -> Vec<String> {
    let mut lines = Vec::new();
    for block in &preview.blocks {
        lines.push(block.header.clone());
        for line in &block.lines {
            let marker = match line.kind {
                PatchDiffLineKind::Hunk => "@@",
                PatchDiffLineKind::Context => " ",
                PatchDiffLineKind::Add => "+",
                PatchDiffLineKind::Delete => "-",
                PatchDiffLineKind::Note => "…",
            };
            if line.text.is_empty() && line.kind == PatchDiffLineKind::Hunk {
                lines.push(marker.to_string());
            } else {
                lines.push(format!("{marker} {}", line.text));
            }
        }
    }
    if let Some(text) = preview.omitted_text.as_ref() {
        lines.push(text.clone());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_diff_preview_parses_update_and_add_lines() {
        let preview = build_patch_diff_preview(
            "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n unchanged\n+new\n*** End Patch",
            4,
            16,
            false,
        );
        assert_eq!(preview.blocks.len(), 1);
        assert_eq!(preview.blocks[0].header, "diff src/main.rs");
        assert_eq!(preview.blocks[0].lines.len(), 4);
        assert_eq!(preview.blocks[0].lines[0].kind, PatchDiffLineKind::Hunk);
        assert_eq!(preview.blocks[0].lines[1].kind, PatchDiffLineKind::Delete);
        assert_eq!(preview.blocks[0].lines[2].kind, PatchDiffLineKind::Context);
        assert_eq!(preview.blocks[0].lines[3].kind, PatchDiffLineKind::Add);
    }

    #[test]
    fn patch_diff_preview_truncates_extra_files() {
        let preview = build_patch_diff_preview(
            "*** Begin Patch\n*** Add File: a.txt\n+1\n*** Add File: b.txt\n+2\n*** Add File: c.txt\n+3\n*** End Patch",
            2,
            8,
            false,
        );
        assert_eq!(preview.blocks.len(), 2);
        assert_eq!(preview.omitted_text.as_deref(), Some("… +1 more files"));
    }
}
