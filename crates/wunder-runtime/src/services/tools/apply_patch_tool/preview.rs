use super::{FileDiffBlock, FileDiffLine};

const MAX_PREVIEW_LINES: usize = 320;
const MAX_FILE_PREVIEW_LINES: usize = 80;
const MAX_PREVIEW_TEXT_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Default)]
pub(super) struct DiffPreview {
    pub(super) blocks: Vec<FileDiffBlock>,
    pub(super) added_lines: usize,
    pub(super) deleted_lines: usize,
    pub(super) omitted_lines: usize,
    visible_lines: usize,
}

#[derive(Default)]
pub(super) struct DiffPreviewBudget {
    lines: usize,
    text_bytes: usize,
}

impl DiffPreviewBudget {
    // Share one budget across the whole patch, including files deleted by a
    // tiny input. Count every change, but allocate text only for visible rows.
    fn push_line(
        &mut self,
        preview: &mut DiffPreview,
        output: &mut Vec<FileDiffLine>,
        kind: &'static str,
        old_line: Option<usize>,
        new_line: Option<usize>,
        text: &str,
    ) {
        preview.added_lines += usize::from(kind == "add");
        preview.deleted_lines += usize::from(kind == "delete");
        if self.lines >= MAX_PREVIEW_LINES
            || preview.visible_lines >= MAX_FILE_PREVIEW_LINES
            || text.len() > MAX_PREVIEW_TEXT_BYTES.saturating_sub(self.text_bytes)
        {
            preview.omitted_lines += 1;
            return;
        }
        self.lines += 1;
        self.text_bytes += text.len();
        preview.visible_lines += 1;
        output.push(FileDiffLine {
            kind,
            old_line,
            new_line,
            text: text.to_string(),
        });
    }

    pub(super) fn update(&mut self, blocks: Vec<FileDiffBlock>) -> DiffPreview {
        let mut preview = DiffPreview::default();
        for mut block in blocks {
            let source = std::mem::take(&mut block.lines);
            for line in source {
                self.push_line(
                    &mut preview,
                    &mut block.lines,
                    line.kind,
                    line.old_line,
                    line.new_line,
                    &line.text,
                );
            }
            if !block.lines.is_empty() {
                preview.blocks.push(block);
            }
        }
        preview
    }

    pub(super) fn add(&mut self, lines: &[String]) -> DiffPreview {
        self.whole_file(lines.iter().map(String::as_str), false)
    }

    pub(super) fn delete(&mut self, source: &str) -> DiffPreview {
        // Match patch line normalization without allocating a String per line.
        let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
        self.whole_file(normalized.split_terminator('\n'), true)
    }

    fn whole_file<'a>(
        &mut self,
        lines: impl Iterator<Item = &'a str>,
        deleted: bool,
    ) -> DiffPreview {
        let mut preview = DiffPreview::default();
        let mut output = Vec::new();
        let mut total = 0;
        for (index, text) in lines.enumerate() {
            total = index + 1;
            self.push_line(
                &mut preview,
                &mut output,
                if deleted { "delete" } else { "add" },
                deleted.then_some(total),
                (!deleted).then_some(total),
                text,
            );
        }
        preview.blocks.push(FileDiffBlock {
            header: if deleted { "deleted file" } else { "new file" }.to_string(),
            start_line_before: usize::from(deleted),
            end_line_before: if deleted { total } else { 0 },
            start_line_after: usize::from(!deleted),
            end_line_after: if deleted { 0 } else { total },
            lines: output,
        });
        preview
    }
}
