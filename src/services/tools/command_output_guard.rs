use serde_json::{json, Value};
use std::collections::VecDeque;

pub(crate) const STDOUT_CAPTURE_POLICY: CommandOutputPolicy = CommandOutputPolicy {
    head_bytes: 24 * 1024,
    tail_bytes: 40 * 1024,
};
pub(crate) const STDERR_CAPTURE_POLICY: CommandOutputPolicy = CommandOutputPolicy {
    head_bytes: 8 * 1024,
    tail_bytes: 56 * 1024,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandOutputPolicy {
    pub(crate) head_bytes: usize,
    pub(crate) tail_bytes: usize,
}

impl CommandOutputPolicy {
    pub(crate) const fn max_bytes(self) -> usize {
        self.head_bytes + self.tail_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandOutputCaptureMeta {
    pub(crate) truncated: bool,
    pub(crate) total_bytes: usize,
    pub(crate) kept_bytes: usize,
    pub(crate) omitted_bytes: usize,
    pub(crate) head_bytes: usize,
    pub(crate) tail_bytes: usize,
}

impl CommandOutputCaptureMeta {
    pub(crate) const fn empty() -> Self {
        Self {
            truncated: false,
            total_bytes: 0,
            kept_bytes: 0,
            omitted_bytes: 0,
            head_bytes: 0,
            tail_bytes: 0,
        }
    }

    pub(crate) fn to_json(self) -> Value {
        json!({
            "truncated": self.truncated,
            "total_bytes": self.total_bytes,
            "kept_bytes": self.kept_bytes,
            "omitted_bytes": self.omitted_bytes,
            "head_bytes": self.head_bytes,
            "tail_bytes": self.tail_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutputCapture {
    pub(crate) bytes: Vec<u8>,
    pub(crate) split_at: usize,
    pub(crate) meta: CommandOutputCaptureMeta,
}

impl CommandOutputCapture {
    pub(crate) fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            split_at: 0,
            meta: CommandOutputCaptureMeta::empty(),
        }
    }
}

pub(crate) struct CommandOutputCollector {
    policy: CommandOutputPolicy,
    total_bytes: usize,
    truncated: bool,
    full: Vec<u8>,
    head: Vec<u8>,
    tail: VecDeque<u8>,
}

impl CommandOutputCollector {
    pub(crate) fn new(policy: CommandOutputPolicy) -> Self {
        Self {
            policy,
            total_bytes: 0,
            truncated: false,
            full: Vec::new(),
            head: Vec::new(),
            tail: VecDeque::new(),
        }
    }

    pub(crate) fn push_chunk(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        self.total_bytes += chunk.len();

        if !self.truncated {
            let max_bytes = self.policy.max_bytes();
            if self.full.len() + chunk.len() <= max_bytes {
                self.full.extend_from_slice(chunk);
                return;
            }

            // Once the configured cap is exceeded, switch to head+tail mode and
            // stop retaining the full stream to keep memory usage bounded.
            self.truncated = true;
            let keep_head = self.policy.head_bytes.min(self.full.len());
            self.head.extend_from_slice(&self.full[..keep_head]);
            let carry_tail = self.full[keep_head..].to_vec();
            self.full.clear();
            self.push_tail_bytes(&carry_tail);
        }

        let mut remaining = chunk;
        if self.head.len() < self.policy.head_bytes {
            let missing = self.policy.head_bytes - self.head.len();
            let take = missing.min(remaining.len());
            self.head.extend_from_slice(&remaining[..take]);
            remaining = &remaining[take..];
        }
        self.push_tail_bytes(remaining);
    }

    pub(crate) fn finish(self) -> CommandOutputCapture {
        if !self.truncated {
            let kept_bytes = self.full.len();
            return CommandOutputCapture {
                split_at: kept_bytes,
                bytes: self.full,
                meta: CommandOutputCaptureMeta {
                    truncated: false,
                    total_bytes: self.total_bytes,
                    kept_bytes,
                    omitted_bytes: self.total_bytes.saturating_sub(kept_bytes),
                    head_bytes: kept_bytes,
                    tail_bytes: 0,
                },
            };
        }

        let tail = self.tail.into_iter().collect::<Vec<_>>();
        let split_at = self.head.len();
        let kept_bytes = split_at + tail.len();
        let mut bytes = self.head;
        bytes.extend_from_slice(&tail);

        CommandOutputCapture {
            split_at,
            bytes,
            meta: CommandOutputCaptureMeta {
                truncated: true,
                total_bytes: self.total_bytes,
                kept_bytes,
                omitted_bytes: self.total_bytes.saturating_sub(kept_bytes),
                head_bytes: split_at,
                tail_bytes: tail.len(),
            },
        }
    }

    fn push_tail_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || self.policy.tail_bytes == 0 {
            return;
        }
        let limit = self.policy.tail_bytes;
        if bytes.len() >= limit {
            self.tail.clear();
            self.tail
                .extend(bytes[bytes.len().saturating_sub(limit)..].iter().copied());
            return;
        }

        let overflow = self.tail.len() + bytes.len();
        if overflow > limit {
            // Evict oldest bytes so the tail always reflects the latest output.
            for _ in 0..(overflow - limit) {
                let _ = self.tail.pop_front();
            }
        }
        self.tail.extend(bytes.iter().copied());
    }
}

pub(crate) fn render_command_output<F>(capture: &CommandOutputCapture, decode: F) -> String
where
    F: Fn(&[u8]) -> String,
{
    if !capture.meta.truncated {
        return decode(&capture.bytes);
    }

    let split_at = capture.split_at.min(capture.bytes.len());
    let head = decode(&capture.bytes[..split_at]);
    let tail = decode(&capture.bytes[split_at..]);
    let marker = format!(
        "...(truncated command output, omitted {} bytes)...",
        capture.meta.omitted_bytes
    );

    match (head.is_empty(), tail.is_empty()) {
        (true, true) => marker,
        (true, false) => format!("{marker}\n{tail}"),
        (false, true) => format!("{head}\n{marker}"),
        (false, false) => format!("{head}\n{marker}\n{tail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_utf8_lossy(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).to_string()
    }

    #[test]
    fn collector_keeps_full_output_when_within_limit() {
        let mut collector = CommandOutputCollector::new(CommandOutputPolicy {
            head_bytes: 4,
            tail_bytes: 6,
        });
        collector.push_chunk(b"abc");
        collector.push_chunk(b"def");
        let capture = collector.finish();

        assert_eq!(capture.meta.truncated, false);
        assert_eq!(capture.meta.total_bytes, 6);
        assert_eq!(capture.meta.kept_bytes, 6);
        assert_eq!(capture.meta.omitted_bytes, 0);
        assert_eq!(capture.bytes, b"abcdef");
        assert_eq!(render_command_output(&capture, decode_utf8_lossy), "abcdef");
    }

    #[test]
    fn collector_keeps_head_and_tail_after_overflow() {
        let mut collector = CommandOutputCollector::new(CommandOutputPolicy {
            head_bytes: 4,
            tail_bytes: 6,
        });
        collector.push_chunk(b"0123456789");
        collector.push_chunk(b"ABCDEFGHIJ");
        let capture = collector.finish();

        assert_eq!(capture.meta.truncated, true);
        assert_eq!(capture.meta.total_bytes, 20);
        assert_eq!(capture.meta.kept_bytes, 10);
        assert_eq!(capture.meta.omitted_bytes, 10);
        assert_eq!(capture.meta.head_bytes, 4);
        assert_eq!(capture.meta.tail_bytes, 6);
        assert_eq!(capture.bytes, b"0123EFGHIJ");
    }

    #[test]
    fn render_command_output_includes_marker_for_truncated_capture() {
        let mut collector = CommandOutputCollector::new(CommandOutputPolicy {
            head_bytes: 3,
            tail_bytes: 3,
        });
        collector.push_chunk(b"abcdef");
        collector.push_chunk(b"ghijkl");
        let capture = collector.finish();
        let rendered = render_command_output(&capture, decode_utf8_lossy);

        assert!(rendered.starts_with("abc"));
        assert!(rendered.contains("truncated command output"));
        assert!(rendered.ends_with("jkl"));
    }
}
