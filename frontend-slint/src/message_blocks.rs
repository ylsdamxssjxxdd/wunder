//! Lightweight text blocks. A stream only reshapes its bounded, unfinished tail.
use crate::TextBlock;
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;

const BLOCK_BYTES: usize = 2048;
// The software renderer uses signed 16-bit drawing coordinates. Bound each
// bubble's visual height even when a reply contains many tiny newline deltas.
const DISPLAY_BYTES: usize = 16 * 1024;
const DISPLAY_LINES: usize = 160;
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

pub struct Blocks {
    pub raw: String,
    pub model: Rc<VecModel<TextBlock>>,
    committed: usize,
    count: usize,
    code: bool,
}
impl Blocks {
    pub fn new() -> Self {
        Self {
            raw: String::new(),
            model: Rc::new(VecModel::default()),
            committed: 0,
            count: 0,
            code: false,
        }
    }
    pub fn append(&mut self, delta: &str) -> Result<(), String> {
        if self.raw.len() + delta.len() > MAX_TEXT_BYTES {
            return Err("回复超过显示缓冲限制，请在历史中查看完整结果".into());
        }
        self.raw.push_str(delta);
        Ok(())
    }
    pub fn replace(&mut self, text: &str) -> Result<(), String> {
        if self.raw == text {
            return Ok(());
        }
        if text.starts_with(&self.raw) {
            return self.append(&text[self.raw.len()..]);
        }
        if text.len() > MAX_TEXT_BYTES {
            return Err("回复超过显示缓冲限制".into());
        }
        self.raw = text.into();
        self.committed = 0;
        self.count = 0;
        self.code = false;
        self.model.set_vec(Vec::new());
        Ok(())
    }
    pub fn flush(&mut self) {
        // Only process newly completed chunks; older rows never change on a delta.
        let byte_end = boundary(&self.raw, self.raw.len().min(DISPLAY_BYTES));
        let display_end = self.raw[..byte_end]
            .match_indices('\n')
            .nth(DISPLAY_LINES - 1)
            .map_or(byte_end, |(offset, _)| offset + 1);
        let capped = self.raw.len() > display_end;
        while self.committed < display_end {
            let remainder = &self.raw[self.committed..display_end];
            let cut = if remainder.len() > BLOCK_BYTES {
                let maximum = boundary(remainder, BLOCK_BYTES);
                remainder[..maximum].rfind('\n').map_or(maximum, |i| i + 1)
            } else {
                remainder.len()
            };
            let stable = capped || cut < remainder.len() || remainder.ends_with('\n');
            let mut code = self.code;
            let block = format_block(&remainder[..cut], &mut code);
            if self.model.row_count() > self.count {
                self.model.set_row_data(self.count, block);
            } else {
                self.model.push(block);
            }
            if !stable {
                break;
            }
            self.code = code;
            self.committed += cut;
            self.count += 1;
        }
        if capped && self.model.row_count() == self.count {
            self.model.push(TextBlock {
                text: "长回复已限制展示，可复制完整原文。".into(),
                kind: 2,
            });
        }
    }
}
pub fn from_text(text: &str) -> ModelRc<TextBlock> {
    let mut blocks = Blocks::new();
    blocks.raw = text.to_string();
    blocks.flush();
    ModelRc::from(blocks.model)
}
fn boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
fn format_block(raw: &str, code: &mut bool) -> TextBlock {
    let mut display = String::with_capacity(raw.len());
    let started_code = *code;
    let mut heading = false;
    for line in raw.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            *code = !*code;
            continue;
        }
        if !*code {
            let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
            if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                display.push_str(&trimmed[hashes + 1..]);
                heading = true;
                continue;
            }
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                display.push_str("• ");
                display.push_str(item);
                continue;
            }
        }
        display.push_str(line);
    }
    TextBlock {
        text: display.into(),
        kind: if started_code || *code {
            1
        } else if heading {
            2
        } else {
            0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_stream_keeps_raw_but_bounds_visual_height() {
        let text = "内容\n".repeat(20_000);
        let mut blocks = Blocks::new();
        for chunk in text.as_bytes().chunks(700) {
            blocks.append(std::str::from_utf8(chunk).unwrap()).unwrap();
            blocks.flush();
        }
        assert_eq!(blocks.raw, text);
        let lines: usize = blocks
            .model
            .iter()
            .map(|row| row.text.matches('\n').count())
            .sum();
        assert!(lines <= DISPLAY_LINES);
        assert!(blocks
            .model
            .iter()
            .last()
            .unwrap()
            .text
            .contains("完整原文"));
    }
    #[test]
    fn chunked_unicode_matches_raw_and_has_bounded_tail() {
        let text = "# 标题\n- 内容\n```\n  code\n```\n".repeat(400);
        let mut blocks = Blocks::new();
        for c in text.chars() {
            blocks.append(&c.to_string()).unwrap();
            blocks.flush();
        }
        assert_eq!(blocks.raw, text);
        assert!(blocks
            .model
            .iter()
            .all(|block| block.text.len() <= BLOCK_BYTES + 8));
        let rows = blocks.model.row_count();
        blocks.replace(&text).unwrap();
        blocks.flush();
        assert_eq!(blocks.model.row_count(), rows);
    }
}
