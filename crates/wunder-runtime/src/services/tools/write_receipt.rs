//! 写入回执：把「写成功了」变成一份可验证的指纹，而不是一串计数。
//!
//! 问题来源：写入/编辑只回 `added_lines`/`bytes`，历史一压缩就只剩数字，模型
//! 既无法确认写进去的是什么，也不敢相信自己的记忆，只能整篇 `read_file` 回读。
//!
//! 回执只给信息量最高的少量事实：行数、内容 sha256 前缀、符号索引（带行区间）、
//! 语法校验结论、首行特征。约 300 字符就能让模型判断「这是不是我刚写的那个
//! 文件」，并且能直接拿符号行区间去做 `replace_range`。
//!
//! 设计约束：
//! - 纯函数、无 IO（语法校验拆在 [`super::write_syntax`]，唯一会起子进程的地方）；
//! - 任何一步失败只降级为 `skipped`/`unknown`，绝不阻断写入；
//! - 输出有硬上限：符号最多 [`MAX_SYMBOLS`] 个，回执整体不超过约 700 字符。

use super::write_syntax::{self, Verdict};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// 回执里最多列出的符号数；其余折叠成 `+N more`。
const MAX_SYMBOLS: usize = 16;
/// 单个符号名的字符上限，防止压缩后的长行撑爆回执。
const MAX_SYMBOL_NAME: usize = 48;
/// 首行预览的字符上限。
const MAX_HEAD_CHARS: usize = 96;
/// 参与符号扫描/语法校验的内容上限；超过即跳过，不影响写入。
const MAX_SCAN_BYTES: usize = 2 * 1024 * 1024;
/// `sha256` 前缀长度：足够区分不同版本，又不占上下文。
const SHA_PREFIX_CHARS: usize = 12;

/// 一份写入回执：结构化字段 + 面向模型的多行文本。
#[derive(Debug, Clone, Default)]
pub(crate) struct Receipt {
    pub lines: usize,
    pub bytes: usize,
    pub sha256: String,
    /// 已折行的符号索引，例如 `f(1-9), g(11-20)`；无符号时为空串。
    pub symbols: String,
    /// 语法校验结论，例如 `OK (py_compile)`、`ERROR line 3: ...`、`skipped`。
    pub syntax: String,
    /// 同一结论的结构化形态：`tool, status, line, message}`，便于程序化判断。
    pub check: Value,
    /// 首行特征（跳过 shebang / 空行）。
    pub head: Option<String>,
    /// 完整的多行回执文本，直接作为 `data.receipt`。
    pub text: String,
}

/// 构造回执。`verify=false` 时跳过语法校验（调用方显式要求时）。
pub(crate) fn build(path: &str, content: &str, verify: bool) -> Receipt {
    let lines = content.lines().count();
    let sha256 = sha256_prefix(content);
    let symbols = if content.len() <= MAX_SCAN_BYTES {
        scan_symbols(path, content)
    } else {
        String::new()
    };
    let verdict = if verify {
        check_syntax(path, content)
    } else {
        Verdict::skipped("none", "verify=false")
    };
    let syntax = verdict.render();
    let check = verdict.to_json();
    let head = head_line(content);
    let mut receipt = Receipt {
        lines,
        bytes: content.len(),
        sha256,
        symbols,
        syntax,
        check,
        head,
        text: String::new(),
    };
    receipt.text = render(path, &receipt);
    receipt
}

/// 把回执写进工具结果的 `data`：`receipt` 是人类/模型都可直接读的多行文本，
/// 其余是同一事实的结构化版本（前端与统计可以单独取用）。
pub(crate) fn insert_fields(map: &mut Map<String, Value>, receipt: &Receipt) {
    map.insert("receipt".to_string(), json!(receipt.text));
    map.insert("lines".to_string(), json!(receipt.lines));
    map.insert("sha256".to_string(), json!(receipt.sha256));
    map.insert("syntax".to_string(), json!(receipt.syntax));
    map.insert("check".to_string(), receipt.check.clone());
    if !receipt.symbols.is_empty() {
        map.insert("symbols".to_string(), json!(receipt.symbols));
    }
}

fn render(path: &str, receipt: &Receipt) -> String {
    let mut out = String::with_capacity(512);
    out.push_str(&format!(
        "file: {} ({} lines, {}, sha256: {})",
        path,
        receipt.lines,
        human_bytes(receipt.bytes),
        receipt.sha256
    ));
    if !receipt.symbols.is_empty() {
        out.push('\n');
        out.push_str("symbols: ");
        out.push_str(&receipt.symbols);
    }
    out.push('\n');
    out.push_str("syntax: ");
    out.push_str(&receipt.syntax);
    if let Some(head) = receipt.head.as_deref() {
        out.push('\n');
        out.push_str("head: ");
        out.push_str(head);
    }
    out
}

fn human_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub(crate) fn sha256_prefix(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let hex = format!("{:x}", digest);
    hex.chars().take(SHA_PREFIX_CHARS).collect()
}

/// 首行特征：跳过空行、shebang 与 HTML 声明，取第一行有意义的内容。
fn head_line(content: &str) -> Option<String> {
    for raw in content.lines().take(12) {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with("#!") || line.starts_with("<?xml") {
            continue;
        }
        let mut head: String = line.chars().take(MAX_HEAD_CHARS).collect();
        if line.chars().count() > MAX_HEAD_CHARS {
            head.push('…');
        }
        return Some(head);
    }
    None
}

// ---------------------------------------------------------------------------
// 符号索引
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Symbol {
    name: String,
    start: usize,
    end: usize,
}

/// 按扩展名选择扫描器：Python 用缩进定界，C 系语言用花括号配对，其余跳过。
fn scan_symbols(path: &str, content: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let symbols = match ext.as_str() {
        "py" | "pyi" => python_symbols(content),
        "rs" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h"
        | "cc" | "cpp" | "hpp" | "cs" | "kt" | "swift" | "scss" | "less" => {
            brace_symbols(content, &ext)
        }
        "sh" | "bash" => shell_symbols(content),
        "css" => css_symbols(content),
        _ => Vec::new(),
    };
    render_symbols(&symbols)
}

fn render_symbols(symbols: &[Symbol]) -> String {
    if symbols.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = symbols
        .iter()
        .take(MAX_SYMBOLS)
        .map(|symbol| {
            let name: String = symbol.name.chars().take(MAX_SYMBOL_NAME).collect();
            if symbol.end > symbol.start {
                format!("{name}({}-{})", symbol.start, symbol.end)
            } else {
                format!("{name}({})", symbol.start)
            }
        })
        .collect();
    if symbols.len() > MAX_SYMBOLS {
        parts.push(format!("+{} more", symbols.len() - MAX_SYMBOLS));
    }
    wrap_symbol_parts(&parts)
}

/// 每行约 84 字符折行，续行缩进对齐 `symbols: `。
fn wrap_symbol_parts(parts: &[String]) -> String {
    const LINE_BUDGET: usize = 84;
    let mut out = String::new();
    let mut current = 0usize;
    for (index, part) in parts.iter().enumerate() {
        let piece_len = part.chars().count() + if index == 0 { 0 } else { 2 };
        if index > 0 && current + piece_len > LINE_BUDGET {
            out.push_str(",\n         ");
            current = 9;
        } else if index > 0 {
            out.push_str(", ");
            current += 2;
        }
        out.push_str(part);
        current += part.chars().count();
    }
    out
}

fn indent_width(line: &str) -> Option<usize> {
    let mut width = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 4,
            _ => return Some(width),
        }
    }
    None
}

/// Python：`def`/`async def`/`class` 起一条记录，遇到缩进回退（或文件结束）封口。
fn python_symbols(content: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = content.lines().collect();
    python_symbols_from(&lines)
}

fn python_symbols_from(lines: &[&str]) -> Vec<Symbol> {
    struct Open {
        symbol: Symbol,
        indent: usize,
    }
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let total = lines.len();
    // 每个位置之前最后一条代码行：符号区间收口到真实最后一行，而不是空白行。
    let mut last_code: Vec<usize> = vec![0; total + 1];
    let mut code_cursor = 0usize;
    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            code_cursor = index + 1;
        }
        last_code[index + 1] = code_cursor;
    }

    let close_to = |open: &mut Vec<Open>, symbols: &mut Vec<Symbol>, end: usize| {
        while let Some(item) = open.pop() {
            let mut symbol = item.symbol;
            symbol.end = symbol.end.max(end);
            symbols.push(symbol);
        }
    };

    for (index, raw) in lines.iter().enumerate() {
        let line_number = index + 1;
        let trimmed = raw.trim_start();
        let indent = indent_width(raw).unwrap_or(0);
        let is_code = !trimmed.is_empty() && !trimmed.starts_with('#');
        if is_code {
            while let Some(last) = open.last() {
                if indent <= last.indent {
                    let mut item = open.pop().expect("checked above");
                    item.symbol.end = last_code[line_number.saturating_sub(1)].max(item.symbol.start);
                    symbols.push(item.symbol);
                } else {
                    break;
                }
            }
        }
        let Some(name) = python_symbol_name(trimmed) else {
            continue;
        };
        open.push(Open {
            symbol: Symbol {
                name,
                start: line_number,
                end: line_number,
            },
            indent,
        });
    }
    close_to(&mut open, &mut symbols, last_code[total]);
    symbols.sort_by_key(|symbol| symbol.start);
    symbols
}

fn python_symbol_name(trimmed: &str) -> Option<String> {
    let rest = if let Some(rest) = trimmed.strip_prefix("async def ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("def ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("class ") {
        rest
    } else {
        return None;
    };
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// C 系语言：从关键字行找第一个 ``，按花括号配对（跳过字符串与注释）。
fn brace_symbols(content: &str, ext: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = content.lines().collect();
    brace_symbols_from(&lines, content, ext)
}

fn brace_symbols_from(lines: &[&str], _content: &str, ext: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        let Some(name) = brace_symbol_name(trimmed, ext) else {
            continue;
        };
        let Some(open_line) = find_block_open(&lines[index..]) else {
            continue;
        };
        let end = match match_block_end(&lines[index + open_line..]) {
            Some(offset) => index + open_line + offset + 1,
            None => lines.len(),
        };
        symbols.push(Symbol {
            name,
            start: index + 1,
            end,
        });
    }
    symbols
}

fn brace_symbol_name(trimmed: &str, ext: &str) -> Option<String> {
    let rest = match ext {
        "rs" => trimmed
            .strip_prefix("pub fn ")
            .or_else(|| trimmed.strip_prefix("pub(crate) fn "))
            .or_else(|| trimmed.strip_prefix("async fn "))
            .or_else(|| trimmed.strip_prefix("pub async fn "))
            .or_else(|| trimmed.strip_prefix("fn "))?,
        "go" => trimmed.strip_prefix("func ")?,
        "kt" => trimmed.strip_prefix("fun ")?,
        "swift" => trimmed.strip_prefix("func ")?,
        _ => trimmed
            .strip_prefix("export function ")
            .or_else(|| trimmed.strip_prefix("function "))
            .or_else(|| trimmed.strip_prefix("export class "))
            .or_else(|| trimmed.strip_prefix("class "))
            .or_else(|| trimmed.strip_prefix("export default function "))
            .or_else(|| trimmed.strip_prefix("async function "))?,
    };
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '$')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// CSS：规则头就是符号——选择器或 `@media`/`@keyframes` 一类的 at-rule，
/// 行区间覆盖整块，模型可以直接拿区间去替换某条规则。
///
/// 关键点：只认「代码区」的花括号。CSS 里字符串与注释都可能带着 ``/``
/// （`content: "{"`、data URI、被注释掉的旧规则），按行扫原文会把属性值当成
/// 选择器，产生 `font-family: "A` 这种假符号。所以先扫一遍得到真实的花括号
/// 事件流（跳过注释与字符串），再按事件配对算区间。
fn css_symbols(content: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = content.lines().collect();
    let events = css_brace_events(content);
    let pairs = css_brace_pairs(&events);
    let mut symbols = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if !event.open {
            continue;
        }
        let Some(line_text) = lines.get(event.line - 1) else {
            continue;
        };
        let col = event.col.min(line_text.len());
        let Some(name) = css_rule_name(&line_text[..col]) else {
            continue;
        };
        let end = pairs
            .get(&index)
            .map(|close| events[*close].line)
            .unwrap_or(lines.len());
        symbols.push(Symbol {
            name,
            start: event.line,
            end,
        });
    }
    symbols
}

struct CssBrace {
    line: usize,
    /// 行内字节列，用来切出 `` 之前的选择器文本。
    col: usize,
    open: bool,
}

/// 扫出代码区的花括号：注释（含未闭合）与字符串一律跳过。
/// CSS 字符串不跨行，到行尾即视为结束，避免一个落单引号吞掉整个文件。
fn css_brace_events(content: &str) -> Vec<CssBrace> {
    let bytes = content.as_bytes();
    let mut events: Vec<CssBrace> = Vec::new();
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                line += 1;
                index += 1;
                line_start = index;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                match content[index + 2..].find("*/") {
                    Some(offset) => {
                        line += content[index + 2..index + 2 + offset].matches('\n').count();
                        index += offset + 4;
                    }
                    None => break,
                }
            }
            b'\'' | b'"' => {
                let quote = bytes[index];
                let mut cursor = index + 1;
                while cursor < bytes.len() && bytes[cursor] != quote && bytes[cursor] != b'\n' {
                    cursor += if bytes[cursor] == b'\\' { 2 } else { 1 };
                }
                index = (cursor + 1).min(bytes.len());
            }
            b'{' => {
                events.push(CssBrace {
                    line,
                    col: index - line_start,
                    open: true,
                });
                index += 1;
            }
            b'}' => {
                events.push(CssBrace {
                    line,
                    col: index - line_start,
                    open: false,
                });
                index += 1;
            }
            _ => index += 1,
        }
    }
    events
}

/// 一次遍历算出每个 `` 对应的闭合事件下标（栈配平，未闭合的缺席）。
fn css_brace_pairs(events: &[CssBrace]) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    let mut stack: Vec<usize> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if event.open {
            stack.push(index);
        } else if let Some(open) = stack.pop() {
            pairs.insert(open, index);
        }
    }
    pairs
}

/// 从 `` 之前的那段文本取出规则名：同一行 `` 之后的部分算下一条规则，
/// 空白折叠成一个空格。keyframes 的 `0%`/`from`/`to` 不是可复用选择器，跳过。
fn css_rule_name(head: &str) -> Option<String> {
    let head = match head.rfind('}') {
        Some(pos) => &head[pos + 1..],
        None => head,
    };
    let head = head.trim();
    if head.is_empty() || head.starts_with("//") || head.starts_with("/*") {
        return None;
    }
    if head == "from" || head == "to" {
        return None;
    }
    if let Some(stripped) = head.strip_suffix('%') {
        if !stripped.is_empty() && stripped.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
            return None;
        }
    }
    Some(
        head.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(MAX_SYMBOL_NAME)
            .collect(),
    )
}

/// 返回块起始 `` 相对当前窗口的偏移（以行计）。
fn find_block_open(lines: &[&str]) -> Option<usize> {
    for (offset, raw) in lines.iter().take(4).enumerate() {
        if raw.contains('{') {
            return Some(offset);
        }
    }
    None
}

/// 从 `` 所在行开始配对，返回闭合行相对偏移；不配对时返回 `None`。
fn match_block_end(lines: &[&str]) -> Option<usize> {
    let mut depth = 0i64;
    let mut started = false;
    for (offset, raw) in lines.iter().enumerate() {
        let mut chars = raw.chars().peekable();
        let mut in_line_comment = false;
        while let Some(ch) = chars.next() {
            if in_line_comment {
                break;
            }
            match ch {
                '/' if chars.peek() == Some(&'/') => in_line_comment = true,
                '"' | '\'' | '`' => {
                    // 粗略跳过字符串：本函数只用于行号提示，不追求词法精确。
                    for inner in chars.by_ref() {
                        if inner == ch {
                            break;
                        }
                    }
                }
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth <= 0 {
                        return Some(offset);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn shell_symbols(content: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = content.lines().collect();
    let mut symbols = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        let name: String = trimmed
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .collect();
        let has_definition = trimmed.starts_with(&format!("{name}()"))
            || trimmed.starts_with(&format!("function {name}"));
        if name.is_empty() || !has_definition {
            continue;
        }
        let end = lines[index..]
            .iter()
            .skip(1)
            .position(|line| line.trim() == "}")
            .map(|offset| index + offset + 2)
            .unwrap_or(lines.len());
        symbols.push(Symbol {
            name,
            start: index + 1,
            end,
        });
    }
    symbols
}

// ---------------------------------------------------------------------------
// 语法校验
// ---------------------------------------------------------------------------

/// 语法校验：只覆盖能低成本、确定性判定的格式；其余明确写 `skipped`。
pub(crate) fn check_syntax(path: &str, content: &str) -> Verdict {
    if content.len() > MAX_SCAN_BYTES {
        return Verdict::skipped("none", "file too large");
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "py" | "pyi" => write_syntax::python(content),
        "js" | "mjs" | "cjs" => write_syntax::javascript(content),
        "ts" | "mts" | "cts" => write_syntax::typescript(content),
        "css" | "scss" | "less" => write_syntax::css(content),
        // JSX/TSX 需要 esbuild/tsc 才能解析，缺了就明说，绝不用 node 硬解析。
        "jsx" | "tsx" => Verdict::skipped("none", "no jsx checker"),
        "json" => json_syntax(content),
        "svg" => tag_syntax(content, MarkupKind::Xml),
        "xml" | "xhtml" => tag_syntax(content, MarkupKind::Xml),
        "html" | "htm" => tag_syntax(content, MarkupKind::Html),
        _ => Verdict::skipped("none", "no checker"),
    }
}

fn json_syntax(content: &str) -> Verdict {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_) => Verdict::ok("json"),
        Err(err) => {
            let detail = truncate_chars(
                err.to_string().split(" at line").next().unwrap_or(""),
                120,
            );
            let message = "column ".to_string() + &err.column().to_string() + ": " + &detail;
            Verdict::error("json", Some(err.line() as u32), message)
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum MarkupKind {
    Xml,
    Html,
}

/// 标签闭合检查：只用栈扫一遍，报告第一个不匹配的位置。
fn tag_syntax(content: &str, kind: MarkupKind) -> Verdict {
    let tool = match kind {
        MarkupKind::Xml => "xml tags",
        MarkupKind::Html => "html tags",
    };
    let mut stack: Vec<(String, usize)> = Vec::new();
    let bytes = content.as_bytes();
    let mut index = 0usize;
    let mut line = 1usize;
    let mut raw_text_tag: Option<String> = None;

    while index < bytes.len() {
        if let Some(tag) = raw_text_tag.clone() {
            // script/style 内部允许出现裸 `<`：直接跳到对应的结束标签。
            let needle = String::from("</") + &tag;
            match find_ignore_case(&content[index..], &needle) {
                Some(offset) => {
                    line += content[index..index + offset].matches('\n').count();
                    index += offset;
                    raw_text_tag = None;
                }
                None => {
                    // 结束标签都找不到：跳出循环，交给下面的「未闭合」回报。
                    break;
                }
            }
            continue;
        }
        if bytes[index] != b'<' {
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
            continue;
        }
        // 注释 / CDATA / 声明 / 结束标签 / 开始标签
        let rest = &content[index..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|offset| offset + 3).unwrap_or(rest.len());
            line += rest[..end].matches('\n').count();
            index += end;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest
                .find("]]>")
                .map(|offset| offset + 3)
                .unwrap_or(rest.len());
            line += rest[..end].matches('\n').count();
            index += end;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').map(|offset| offset + 1).unwrap_or(rest.len());
            line += rest[..end].matches('\n').count();
            index += end;
            continue;
        }
        let Some(close) = rest.find('>') else {
            return Verdict::error(tool, Some(line as u32), "未结束的标签，开始于一个 `<`");
        };
        let tag_text = &rest[1..close];
        line += tag_text.matches('\n').count();
        let is_closing = tag_text.starts_with('/');
        let self_closing = tag_text.ends_with('/');
        let name: String = tag_text
            .trim_start_matches('/')
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == ':' || *ch == '_')
            .collect::<String>()
            .to_ascii_lowercase();
        index += close + 1;
        if name.is_empty() {
            continue;
        }
        if is_closing {
            if raw_text_tag.as_deref() == Some(name.as_str()) {
                raw_text_tag = None;
                continue;
            }
            match stack.pop() {
                Some((open, _)) if open == name => {}
                Some((open, open_line)) => {
                    stack.push((open.clone(), open_line));
                    let message = "闭合标签 </".to_string()
                        + &name
                        + "> 与第 "
                        + &open_line.to_string()
                        + " 行的 <"
                        + &open
                        + "> 不匹配";
                    return Verdict::error(tool, Some(line as u32), message);
                }
                None => {
                    return Verdict::error(
                        tool,
                        Some(line as u32),
                        "闭合标签 </".to_string() + &name + "> 没有对应的开始标签",
                    );
                }
            }
            continue;
        }
        if self_closing || (kind == MarkupKind::Html && is_void_element(&name)) {
            continue;
        }
        if kind == MarkupKind::Html && (name == "script" || name == "style") {
            raw_text_tag = Some(name.clone());
        }
        stack.push((name, line));
    }

    if let Some((open, open_line)) = stack.pop() {
        let message = "文件结束仍有未闭合标签 <".to_string()
            + &open
            + ">（第 "
            + &open_line.to_string()
            + " 行），共 "
            + &(stack.len() + 1).to_string()
            + " 个";
        return Verdict::error(tool, Some(open_line as u32), message);
    }
    Verdict::ok(tool)
}

/// ASCII 忽略大小写查找；只用于找 `</script>` 这类固定结束标记。
fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let hay = haystack.as_bytes();
    let pattern = needle.as_bytes();
    if pattern.is_empty() || pattern.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - pattern.len())
        .find(|start| hay[*start..*start + pattern.len()].eq_ignore_ascii_case(pattern))
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// 文本相似度（编辑失败恢复用；放在这里复用同一套行切分）
// ---------------------------------------------------------------------------

pub(crate) fn split_lines(content: &str) -> Vec<&str> {
    content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// 字符二元组多重集，用于 Dice 相似度。
pub(crate) fn char_bigrams(text: &str) -> HashMap<(char, char), u32> {
    let mut map: HashMap<(char, char), u32> = HashMap::new();
    let mut previous: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            *map.entry((prev, ch)).or_insert(0) += 1;
        }
        previous = Some(ch);
    }
    map
}

pub(crate) fn dice_similarity(
    left: &HashMap<(char, char), u32>,
    right: &HashMap<(char, char), u32>,
) -> f32 {
    let left_total: u32 = left.values().sum();
    let right_total: u32 = right.values().sum();
    if left_total == 0 || right_total == 0 {
        return if left_total == right_total { 1.0 } else { 0.0 };
    }
    let shared: u32 = left
        .iter()
        .map(|(key, count)| right.get(key).copied().unwrap_or(0).min(*count))
        .sum();
    (2.0 * shared as f32) / (left_total + right_total) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_reports_lines_hash_symbols_and_syntax() {
        let content = "\"\"\"标题\"\"\"\n\n\ndef make_bg(w, h):\n    return w * h\n\n\ndef draw_bike(t):\n    return t\n";
        let receipt = build("temp/pelican.py", content, false);
        assert_eq!(receipt.lines, 9);
        assert_eq!(receipt.sha256.len(), SHA_PREFIX_CHARS);
        assert!(receipt.symbols.contains("make_bg(4-5)"), "{}", receipt.symbols);
        assert!(receipt.symbols.contains("draw_bike(8-9)"), "{}", receipt.symbols);
        assert_eq!(receipt.head.as_deref(), Some("\"\"\"标题\"\"\""));
        assert!(receipt.text.starts_with("file: temp/pelican.py (9 lines"));
        assert!(receipt.text.contains("syntax: skipped (verify=false)"));
    }

    #[test]
    fn python_symbols_close_on_dedent_and_cover_methods() {
        let content = "class A:\n    def run(self):\n        return 1\n\n    def stop(self):\n        pass\n\n\ndef top():\n    pass\n";
        let symbols = python_symbol_name_all(content);
        assert_eq!(
            symbols,
            vec!["A(1-6)", "run(2-3)", "stop(5-6)", "top(9-10)"]
        );
    }

    fn python_symbol_name_all(content: &str) -> Vec<String> {
        python_symbols(content)
            .into_iter()
            .map(|symbol| format!("{}({}-{})", symbol.name, symbol.start, symbol.end))
            .collect()
    }

    #[test]
    fn brace_symbols_span_the_whole_body() {
        let content = "function draw(t) {\n  if (t) {\n    return 1;\n  }\n  return 2;\n}\n\nconst x = 1;\n";
        let symbols = brace_symbols(content, "js");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "draw");
        assert_eq!(symbols[0].start, 1);
        assert_eq!(symbols[0].end, 6);
    }

    #[test]
    fn python_receipt_reports_a_real_verdict() {
        if write_syntax::env_disabled() {
            return;
        }
        let receipt = build("temp/a.py", "class A:\n    def f(self):\n        return 1\n", true);
        // 没有 python 时只能是 skipped；有的话必须是真编译结论，绝不能是
        // 「File "<string>", line 4」这种检查器自身的报错。
        if receipt.syntax.starts_with("skipped") {
            assert!(receipt.syntax.contains("python") || receipt.syntax.contains("disabled"));
            return;
        }
        assert_eq!(receipt.syntax, "OK (py_compile)");
        assert!(receipt.text.contains("syntax: OK (py_compile)"));
        let broken = build("temp/b.py", "class A\n    pass\n", true);
        assert!(broken.syntax.starts_with("ERROR line 1"), "{}", broken.syntax);
        assert!(broken.text.contains("syntax: ERROR line 1"));
    }

    #[test]
    fn check_syntax_dispatches_script_and_style_extensions() {
        // 脚本类交给解释器：有 node 就是真解析，没 node 也必须只是 skipped。
        let js = check_syntax("a.js", "const a = 1;\n").render();
        assert!(
            js == "OK (node --check)" || js.starts_with("skipped"),
            "{js}"
        );
        let css = check_syntax("a.css", ".a { color: red; }").render();
        assert_eq!(css, "OK (css braces)");
        // JSX/TSX 不硬猜：没有专用解析器就明说跳过。
        assert_eq!(
            check_syntax("a.jsx", "const a = <div/>;").render(),
            "skipped (no jsx checker)"
        );
        assert_eq!(
            check_syntax("a.unknownext", "x").render(),
            "skipped (no checker)"
        );
    }

    #[test]
    fn css_symbols_span_rule_blocks() {
        let content = ".card {\n  color: red;\n}\n\n@media (max-width: 640px) {\n  .card {\n    color: blue;\n  }\n}\n";
        let symbols = css_symbols(content);
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        // 属性声明（color: red;）没有「{」，不算符号；嵌套规则会被单独列出。
        assert_eq!(names, vec![".card", "@media (max-width: 640px)", ".card"]);
        assert_eq!((symbols[0].start, symbols[0].end), (1, 3));
        assert_eq!((symbols[2].start, symbols[2].end), (6, 8));
    }

    #[test]
    fn json_syntax_points_at_the_offending_line() {
        let ok = check_syntax("a.json", "{\"a\": 1}").render();
        assert_eq!(ok, "OK (json)");
        let bad = check_syntax("a.json", "{\n  \"a\": 1,\n}").render();
        assert!(bad.starts_with("ERROR line 3"), "{bad}");
    }

    #[test]
    fn tag_syntax_reports_unclosed_and_mismatched_tags() {
        assert_eq!(
            check_syntax("a.svg", "<svg><rect/></svg>").render(),
            "OK (xml tags)"
        );
        let unclosed = check_syntax("a.svg", "<svg>\n<rect/>").render();
        assert!(unclosed.contains("<svg>"), "{unclosed}");
        let mismatched = check_syntax("a.html", "<div>\n<p>hi</div>").render();
        assert!(mismatched.contains("不匹配"), "{mismatched}");
        assert_eq!(
            check_syntax("a.html", "<div><br><img src=\"x\"></div>").render(),
            "OK (html tags)"
        );
    }

    #[test]
    fn html_raw_text_blocks_ignore_inner_angle_brackets() {
        let html = "<html><body><script>if (a < b) { x(); }</script></body></html>";
        assert_eq!(check_syntax("a.html", html).render(), "OK (html tags)");
    }

    #[test]
    fn symbol_list_is_bounded_and_folded() {
        let mut content = String::new();
        for index in 0..40 {
            content.push_str(&format!("def f{index}():\n    pass\n\n"));
        }
        let symbols = scan_symbols("a.py", &content);
        assert!(symbols.contains("+24 more"), "{symbols}");
        assert!(symbols.lines().count() > 1);
        assert!(symbols.lines().all(|line| line.chars().count() <= 96));
    }

    #[test]
    fn dice_similarity_prefers_near_matches() {
        let needle = char_bigrams("pouch = [(385, 114)");
        let near = char_bigrams("pouch  = [(385, 114)");
        let far = char_bigrams("def draw_bike(t):");
        assert!(dice_similarity(&needle, &near) > dice_similarity(&needle, &far));
    }
}
