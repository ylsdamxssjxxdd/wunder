//! 需要外部解释器或状态机的语法校验：Python 编译、JS/TS 解析、CSS 括号配平。
//!
//! 与写入回执同一条铁律：只给结论，绝不阻断写入。任何一步失败——解释器缺失、
//! `WUNDER_SYNTAX_CHECK` 关掉、超时、输出不可读——都降级成 `skipped (...)`，
//! 由调用方原样放进回执的 `syntax` 字段，文件照常落盘。
//!
//! 解释器只探测一次（`OnceLock`），之后复用结论；`WUNDER_PYTHON` / `WUNDER_NODE`
//! 可显式指定可执行文件。校验内容一律走 stdin，不落临时文件：
//! - `.py`/`.pyi`：`python -I -c <compile>`，只编译不执行；
//! - `.js`/`.mjs`/`.cjs`：`node --check -`；命中模块专用语法时用
//!   `--input-type=module --check -` 再验一次，ESM/CJS 都不用先猜；
//! - `.ts`/`.mts`/`.cts`：node 里 `stripTypeScriptTypes` 去掉类型后再交给 vm 解析；
//! - `.css`/`.scss`/`.less`：纯 Rust 扫掉注释与字符串后配平花括号。

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

/// 子进程校验的等待上限；超时即降级，不让一次写入被校验卡住。
const SYNTAX_TIMEOUT: Duration = Duration::from_secs(5);
/// 同时只跑一个校验子进程：node 单次常驻约 42 MiB，并发写入叠加会把进程树内存
/// 推到数百 MiB。一次校验通常 20 ms 上下，所以排队等 1 s 足够排掉几十个请求而不
/// 影响结论；只有解释器卡死这种异常才会等不到，届时降级 `skipped (checker busy)`。
const GATE_POLL: Duration = Duration::from_millis(2);
const GATE_WAIT: Duration = Duration::from_millis(1000);
/// 报错摘要的字符上限，避免把整段堆栈塞进回执。
const MAX_DETAIL_CHARS: usize = 160;

/// 子进程路失败的原因，拼成 `skipped (<tool> <reason>)`。
#[derive(Debug)]
enum RunError {
    Unavailable,
    StdinClosed,
    Timeout,
    WaitFailed,
    OutputUnreadable,
    /// 闸门被别的校验占着，等不到空闲；写入照常进行。
    Busy,
}

impl RunError {
    fn reason(&self) -> &'static str {
        match self {
            RunError::Unavailable => "unavailable",
            RunError::StdinClosed => "stdin closed",
            RunError::Timeout => "timeout",
            RunError::WaitFailed => "wait failed",
            RunError::OutputUnreadable => "output unreadable",
            RunError::Busy => "checker busy",
        }
    }
}

/// `WUNDER_SYNTAX_CHECK=0/false/off/no` 时整体关闭语法校验。
pub(crate) fn env_disabled() -> bool {
    disabled_value(std::env::var("WUNDER_SYNTAX_CHECK").ok().as_deref())
}

/// 开关判定本身是纯函数：测试只验它，不碰进程级环境变量。
fn disabled_value(raw: Option<&str>) -> bool {
    raw.map(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
    .unwrap_or(false)
}

/// 校验结论的结构化形态：程序化判断读 `status`，人/模型读 [`Verdict::render`]。
pub(crate) struct Verdict {
    pub tool: &'static str,
    pub status: Status,
    pub line: Option<u32>,
    pub message: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Ok,
    Error,
    Skipped,
}

impl Verdict {
    pub(crate) fn ok(tool: &'static str) -> Self {
        Self {
            tool,
            status: Status::Ok,
            line: None,
            message: None,
        }
    }

    pub(crate) fn error(tool: &'static str, line: Option<u32>, message: impl Into<String>) -> Self {
        Self {
            tool,
            status: Status::Error,
            line,
            message: Some(message.into()),
        }
    }

    pub(crate) fn skipped(tool: &'static str, reason: impl Into<String>) -> Self {
        Self {
            tool,
            status: Status::Skipped,
            line: None,
            message: Some(reason.into()),
        }
    }

    /// 面向模型/日志的一行结论，保持既有格式不变。
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        match self.status {
            Status::Ok => {
                out.push_str("OK ");
                out.push('(');
                out.push_str(self.tool);
                out.push(')');
            }
            Status::Error => {
                out.push_str("ERROR ");
                if let Some(line) = self.line {
                    out.push_str("line ");
                    out.push_str(&line.to_string());
                    out.push_str(": ");
                }
                if let Some(message) = self.message.as_deref() {
                    out.push_str(message);
                    out.push(' ');
                }
                out.push('(');
                out.push_str(self.tool);
                out.push(')');
            }
            Status::Skipped => {
                out.push_str("skipped ");
                out.push('(');
                out.push_str(self.message.as_deref().unwrap_or("unknown"));
                out.push(')');
            }
        }
        out
    }

    /// 结构化字段：`{`tool, status, line, message}`，空值不出现。
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("tool".to_string(), serde_json::Value::String(self.tool.to_string()));
        let status = match self.status {
            Status::Ok => "ok",
            Status::Error => "error",
            Status::Skipped => "skipped",
        };
        map.insert("status".to_string(), serde_json::Value::String(status.to_string()));
        if let Some(line) = self.line {
            map.insert("line".to_string(), serde_json::json!(line));
        }
        if let Some(message) = self.message.as_deref() {
            map.insert("message".to_string(), serde_json::Value::String(message.to_string()));
        }
        serde_json::Value::Object(map)
    }
}

/// Python：把源码喂给 `compile()`，只编译不执行，所以不会有 import 副作用。
pub(crate) fn python(content: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("py_compile", "disabled");
    }
    let Some(python) = python_interpreter() else {
        return Verdict::skipped("py_compile", "no python3");
    };
    match run_with_stdin(python, &["-I", "-c", PY_CHECK_SCRIPT, "wunder-write-check"], content) {
        Ok((true, _, _)) => Verdict::ok("py_compile"),
        // 检查脚本自己挂了（stdout 空）不能算用户代码错，否则模型会被一句
        // File "<string>", line 4 误导。
        Ok((false, stdout, _stderr)) => match first_non_empty_line(&stdout) {
            Some(detail) => {
                let (line, message) = split_line_prefix(detail);
                Verdict::error("py_compile", line, message)
            }
            None => Verdict::skipped("py_compile", "py checker failed"),
        },
        Err(err) => Verdict::skipped("py_compile", "python ".to_string() + err.reason()),
    }
}

/// JavaScript：`node --check -` 只解析不执行。走 stdin 时 node 不按路径探测
/// 模块类型，所以遇到 `import`/`export` 这类只能在模块里出现的语法，会用
/// `--input-type=module` 再验一次——先按脚本过一遍，避免把 ESM 误判成语法错。
pub(crate) fn javascript(content: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("node --check", "disabled");
    }
    let Some(node) = node_interpreter() else {
        return Verdict::skipped("node --check", "no node");
    };
    match run_with_stdin(node, &["--check", "-"], content) {
        Ok((true, _, _)) => Verdict::ok("node --check"),
        Ok((false, stdout, stderr)) => {
            let text = if stderr.trim().is_empty() { stdout } else { stderr };
            if looks_like_module_syntax(&text) {
                return match run_with_stdin(node, &["--input-type=module", "--check", "-"], content) {
                    Ok((true, _, _)) => Verdict::ok("node --check"),
                    Ok((false, stdout, stderr)) => {
                        let esm = if stderr.trim().is_empty() { stdout } else { stderr };
                        node_error_verdict("node --check", &esm)
                    }
                    Err(err) => Verdict::skipped("node --check", "node ".to_string() + err.reason()),
                };
            }
            node_error_verdict("node --check", &text)
        }
        Err(err) => Verdict::skipped("node --check", "node ".to_string() + err.reason()),
    }
}

/// TypeScript：node 的 `stripTypeScriptTypes` 去类型（`transform` 模式支持
/// `enum`/命名空间），再交给 `vm` 解析。没有该 API 的老 node 直接降级。
pub(crate) fn typescript(content: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("ts check", "disabled");
    }
    let Some(node) = node_interpreter() else {
        return Verdict::skipped("ts check", "no node");
    };
    let args = [
        "--experimental-vm-modules",
        "--no-warnings",
        "-e",
        TS_CHECK_SCRIPT,
    ];
    match run_with_stdin(node, &args, content) {
        Ok((true, _, _)) => Verdict::ok("ts check"),
        // exit 2 = 这个 node 没有去类型的 API，属于环境不支持，不是语法错。
        Ok((false, stdout, _)) if stdout.trim() == "unsupported" => {
            Verdict::skipped("ts check", "node too old")
        }
        Ok((false, stdout, stderr)) => {
            let text = if stdout.trim().is_empty() { stderr } else { stdout };
            node_error_verdict("ts check", &text)
        }
        Err(err) => Verdict::skipped("ts check", "node ".to_string() + err.reason()),
    }
}

/// CSS 系：扫掉注释与字符串后配平花括号。抓的是最常见的写坏方式——
/// 少一个 `}`、多一个 `}`、注释没闭合，而不是 CSS 语义。
pub(crate) fn css(content: &str) -> Verdict {
    let bytes = content.as_bytes();
    let mut open_lines: Vec<usize> = Vec::new();
    let mut line = 1usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                line += 1;
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                match content[index + 2..].find("*/") {
                    Some(offset) => {
                        line += content[index..index + offset + 4].matches('\n').count();
                        index += offset + 4;
                    }
                    None => {
                        return Verdict::error("css braces", Some(line as u32), "注释没有闭合");
                    }
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
                open_lines.push(line);
                index += 1;
            }
            b'}' => {
                if open_lines.pop().is_none() {
                    return Verdict::error("css braces", Some(line as u32), "多余的右花括号");
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    if let Some(open) = open_lines.last() {
        return Verdict::error(
            "css braces",
            Some(line as u32),
            "未闭合的左花括号 始于第 ".to_string() + &open.to_string() + " 行",
        );
    }
    Verdict::ok("css braces")
}

/// 检查脚本用 `concat!` 逐行拼接：Rust 的行尾续写符会连缩进一起吃掉，
/// 早先那种写法把 `compile(...)` 顶了格，导致每次都变成检查脚本自身的
/// IndentationError（回执里就是一句 `File "<string>", line 4`）。
const PY_CHECK_SCRIPT: &str = concat!(
    "import sys\n",
    "src = sys.stdin.buffer.read()\n",
    "try:\n",
    "    compile(src, sys.argv[1], 'exec')\n",
    "except SyntaxError as exc:\n",
    "    print('line %d: %s' % (exc.lineno, exc.msg))\n",
    "    sys.exit(1)\n",
    "except ValueError as exc:\n",
    "    print('invalid: %s' % exc)\n",
    "    sys.exit(1)\n",
);

/// TypeScript 的去类型 + 解析脚本；同样逐行拼接，缩进不会被吞。
const TS_CHECK_SCRIPT: &str = concat!(
    "const fs = require('node:fs');\n",
    "const vm = require('node:vm');\n",
    "const mod = require('node:module');\n",
    "const src = fs.readFileSync(0, 'utf8');\n",
    "if (typeof mod.stripTypeScriptTypes !== 'function') { console.log('unsupported'); process.exit(2); }\n",
    "let js;\n",
    "try { js = mod.stripTypeScriptTypes(src, { mode: 'transform' }); }\n",
    "catch (err) { console.log(String((err && err.message) || err)); process.exit(1); }\n",
    "let moduleErr = null;\n",
    "if (typeof vm.SourceTextModule === 'function') {\n",
    "  try { new vm.SourceTextModule(js); process.exit(0); }\n",
    "  catch (err) { moduleErr = err; }\n",
    "}\n",
    "try { new vm.Script(js); process.exit(0); }\n",
    "catch (err) {\n",
    "  const chosen = moduleErr || err;\n",
    "  const at = chosen && chosen.lineNumber ? chosen.lineNumber + ': ' : '';\n",
    "  console.log(at + String((chosen && chosen.message) || chosen));\n",
    "  process.exit(1);\n",
    "}\n",
);

/// 找到可用的 Python 解释器：只探测一次，之后复用结论。
fn python_interpreter() -> Option<&'static str> {
    static PYTHON: OnceLock<Option<&'static str>> = OnceLock::new();
    PYTHON
        .get_or_init(|| {
            let candidates = env_and_path_candidates("WUNDER_PYTHON", &["python3", "python"]);
            first_working(&candidates, &["-c", "import sys"])
        })
        .as_ref()
        .copied()
}

/// 找到可用的 Node：先看 `WUNDER_NODE` 与 PATH，再看便携包把 node 放在
/// 可执行文件附近的情况（本地版本自带 node 时不在 PATH 里）。
fn node_interpreter() -> Option<&'static str> {
    static NODE: OnceLock<Option<&'static str>> = OnceLock::new();
    NODE.get_or_init(|| {
        let mut candidates = env_and_path_candidates("WUNDER_NODE", &["node", "nodejs"]);
        if let Ok(exe) = std::env::current_exe() {
            for base in exe.ancestors().skip(1).take(3) {
                for relative in [
                    "node",
                    "nodejs",
                    "bin/node",
                    "node/bin/node",
                    "resources/node/bin/node",
                    "runtime/node/bin/node",
                ] {
                    candidates.push(base.join(relative).to_string_lossy().to_string());
                }
            }
        }
        first_working(&candidates, &["--version"])
    })
    .as_ref()
    .copied()
}

fn env_and_path_candidates(env_name: &str, fallback: &[&str]) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(value) = std::env::var(env_name) {
        let value = value.trim();
        if !value.is_empty() {
            candidates.push(value.to_string());
        }
    }
    candidates.extend(fallback.iter().map(|item| item.to_string()));
    candidates
}

fn first_working(candidates: &[String], check_args: &[&str]) -> Option<&'static str> {
    for candidate in candidates {
        let ok = Command::new(candidate)
            .args(check_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            let leaked: &'static str = Box::leak(candidate.clone().into_boxed_str());
            return Some(leaked);
        }
    }
    None
}

/// 全局闸门：语法校验的单次子进程内存是解释器的固定开销，同时跑多个只是把
/// 峰值乘上去，结论却完全一样。用一个 `Mutex` 限流，等不到就降级。
static CHECKER_GATE: OnceLock<Mutex<()>> = OnceLock::new();

struct CheckerGate {
    _guard: MutexGuard<'static, ()>,
}

impl CheckerGate {
    fn acquire() -> Result<Self, RunError> {
        let gate = CHECKER_GATE.get_or_init(|| Mutex::new(()));
        Ok(Self {
            _guard: acquire_gate(gate, GATE_WAIT)?,
        })
    }
}

/// 轮询拿锁：不阻塞在 `lock()` 上，等待有上限，超时就返回 [`RunError::Busy`]。
fn acquire_gate(
    gate: &'static Mutex<()>,
    wait: Duration,
) -> Result<MutexGuard<'static, ()>, RunError> {
    let deadline = Instant::now() + wait;
    loop {
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            // 上一次校验 panic 把锁毒化了也算可用，`()` 里没有需要恢复的状态。
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Err(RunError::Busy);
                }
                std::thread::sleep(GATE_POLL);
            }
        }
    }
}

/// 把源码写进子进程 stdin 并等它退出；超时就 kill，不阻塞写入路径。
/// 调用期间独占闸门，同一时刻只有一个解释器在校验。
fn run_with_stdin(
    program: &str,
    args: &[&str],
    content: &str,
) -> Result<(bool, String, String), RunError> {
    let _gate = CheckerGate::acquire()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| RunError::Unavailable)?;
    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(content.as_bytes()).is_err() {
            let _ = child.kill();
            return Err(RunError::StdinClosed);
        }
    }
    drop(child.stdin.take());

    let deadline = Instant::now() + SYNTAX_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Timeout);
            }
            Err(_) => return Err(RunError::WaitFailed),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|_| RunError::OutputUnreadable)?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

/// node 的报错形如：`[stdin]:3` + 源码行 + `SyntaxError: ...`。
/// 取行号与 `SyntaxError` 那一行，拆成结构化结果。
fn node_error_parts(text: &str) -> (Option<u32>, String) {
    let mut line_no: Option<u32> = None;
    let mut message: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line_no.is_none() {
            if let Some((_, tail)) = line.rsplit_once(':') {
                if let Ok(value) = tail.trim().parse::<u32>() {
                    line_no = Some(value);
                    continue;
                }
            }
        }
        if message.is_none() {
            let head = line.split(':').next().unwrap_or_default().trim();
            if head.ends_with("Error") || head == "Error" {
                message = Some(line.to_string());
            }
        }
    }
    let text = message
        .or_else(|| first_non_empty_line(text).map(str::to_string))
        .unwrap_or_else(|| "unknown error".to_string());
    (line_no, truncate_chars(&text, MAX_DETAIL_CHARS))
}

/// node 的结构化结论：工具名由调用方给（`node --check` / `ts check`）。
fn node_error_verdict(tool: &'static str, text: &str) -> Verdict {
    let (line, message) = node_error_parts(text);
    Verdict::error(tool, line, message)
}

/// 这些错误只在「按脚本解析」时出现，说明该文件其实是模块语法。
fn looks_like_module_syntax(text: &str) -> bool {
    [
        "Cannot use import statement outside a module",
        "Unexpected token 'export'",
        "Cannot use 'import.meta' outside a module",
        "await is only valid in async functions",
        "Unexpected token 'await'",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// Python 检查脚本的输出形如 `line 3: invalid syntax`，拆成行号与消息。
fn split_line_prefix(text: &str) -> (Option<u32>, String) {
    if let Some(rest) = text.strip_prefix("line ") {
        if let Some((number, message)) = rest.split_once(": ") {
            if let Ok(line) = number.trim().parse::<u32>() {
                return (Some(line), message.trim().to_string());
            }
        }
    }
    (None, text.trim().to_string())
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().find(|line| !line.trim().is_empty())
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(code: &str, needle: &str) -> bool {
        code.contains(needle)
    }

    #[test]
    fn css_balances_braces_and_ignores_strings() {
        assert_eq!(css(".a { color: red; }").render(), "OK (css braces)");
        // 字符串里的花括号不参与配平。
        assert_eq!(css(".a::after { content: \"}\"; }").render(), "OK (css braces)");
        let unclosed = css(".a {\n  color: red;\n").render();
        assert!(has(&unclosed, "未闭合的左花括号 始于第 1 行"), "{unclosed}");
        let surplus = css(".a { }\n}\n").render();
        assert!(has(&surplus, "多余的右花括号"), "{surplus}");
        let comment = css("/* 没关掉\n.a { }\n").render();
        assert!(has(&comment, "注释没有闭合"), "{comment}");
    }

    #[test]
    fn css_ignores_comments_when_counting() {
        assert_eq!(css("/* { } */\n.a { }").render(), "OK (css braces)");
        let broken = css("/* {\n*/\n.a { ").render();
        assert!(has(&broken, "未闭合的左花括号"), "{broken}");
    }

    #[test]
    fn javascript_reports_a_syntax_error_or_skips_without_node() {
        let code = javascript("const a = ;\n").render();
        if code.starts_with("skipped") {
            assert!(has(&code, "no node") || has(&code, "disabled"), "{code}");
            return;
        }
        assert!(has(&code, "line 1:"), "{code}");
        assert!(has(&code, "(node --check)"), "{code}");
    }

    #[test]
    fn javascript_accepts_commonjs_and_esm_shapes() {
        let code = javascript("const a = require('node:path');\nmodule.exports = { a };\n").render();
        if code.starts_with("skipped") {
            return;
        }
        assert_eq!(code, "OK (node --check)");
        let esm = javascript("import fs from 'node:fs';\nexport const a = 1;\n").render();
        assert_eq!(esm, "OK (node --check)");
    }

    #[test]
    fn typescript_accepts_types_or_skips_without_node() {
        let code = typescript("interface A { a: number }\nexport const f = (x: A): number => x.a;\n").render();
        if code.starts_with("skipped") {
            assert!(has(&code, "node"), "{code}");
            return;
        }
        assert_eq!(code, "OK (ts check)");
        let broken = typescript("const f = (x: number) => {\n").render();
        assert!(has(&broken, "(ts check)"), "{broken}");
    }

    #[test]
    fn python_checker_compiles_the_body_not_a_wrapper() {
        if env_disabled() || python_interpreter().is_none() {
            return;
        }
        // 空文件、单行赋值、多行类定义都必须真的编译通过。
        assert_eq!(python("").render(), "OK (py_compile)");
        assert_eq!(python("x = 1\n").render(), "OK (py_compile)");
        assert_eq!(
            python("class A:\n    def f(self, x):\n        return x\n").render(),
            "OK (py_compile)"
        );
        // 长文件同样只看源码本身：拼一份 300+ 行的合法脚本再喂一次。
        let mut long = String::new();
        for index in 0..60 {
            long.push_str(&format!("class C{index}:\n    def f(self):\n        return {index}\n\n"));
        }
        assert_eq!(python(&long).render(), "OK (py_compile)");
        // 回归护栏：报错必须来自用户源码，而不是检查脚本自己那层壳。
        for broken_src in ["x = 1\ndef broken(x:\n    pass\n", "class A\n    pass\n"] {
            let broken = python(broken_src).render();
            assert!(broken.starts_with("ERROR line "), "{broken}");
            assert!(!has(&broken, "<string>"), "{broken}");
            assert!(!has(&broken, "IndentationError"), "{broken}");
        }
        let missing_colon = python("class A\n    pass\n").render();
        assert!(has(&missing_colon, "line 1"), "{missing_colon}");
    }

    fn checker_gate_serializes_and_degrades_instead_of_queueing() {
        // 用函数内静态锁验证语义，不去碰真实校验用的全局闸门，免得并行测试互相排队。
        static GATE: Mutex<()> = Mutex::new(());
        let held = acquire_gate(&GATE, Duration::from_millis(5)).expect("first acquire");
        let started = Instant::now();
        match acquire_gate(&GATE, Duration::from_millis(40)) {
            Ok(_) => panic!("闸门被占用时第二次取锁必须被拒绝，而不是排队等下去"),
            Err(RunError::Busy) => {}
            Err(_) => panic!("只允许 Busy 这一种拒绝"),
        }
        assert!(started.elapsed() >= Duration::from_millis(40));
        drop(held);
        assert!(acquire_gate(&GATE, Duration::from_millis(5)).is_ok());
        // 降级文案：调用方拼成 `skipped (node checker busy)`，文件照常落盘。
        assert_eq!(RunError::Busy.reason(), "checker busy");
        assert_eq!(
            Verdict::skipped("node --check", "node ".to_string() + RunError::Busy.reason()).render(),
            "skipped (node checker busy)"
        );
    }

    #[test]
    fn disabled_switch_reads_the_usual_falsy_words() {
        // 纯函数断言：并行测试不会被进程级环境变量互相污染。
        for raw in ["0", "false", "OFF", "no", " 0 "] {
            assert!(disabled_value(Some(raw)), "{raw} should disable");
        }
        for raw in ["1", "true", "on", "yes", ""] {
            assert!(!disabled_value(Some(raw)), "{raw} should keep checks on");
        }
        assert!(!disabled_value(None));
    }
}
