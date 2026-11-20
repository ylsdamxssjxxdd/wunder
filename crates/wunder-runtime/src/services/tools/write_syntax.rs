//! 需要外部解释器或状态机的语法校验：Python 编译、JS/TS 解析、Shell 解析、CSS 括号配平。
//!
//! 与写入回执同一条铁律：只给结论，绝不阻断写入。任何一步失败——解释器缺失、
//! `WUNDER_SYNTAX_CHECK` 关掉、超时、输出不可读——都降级成 `skipped (...)`，
//! 由调用方原样放进回执的 `syntax` 字段，文件照常落盘。
//!
//! 解释器探测结论按进程缓存（`RwLock` 槽位），设置页改完路径由 [`reset_hosts`]
//! 清空重探；[`syntax_hosts`] 把 node / shell 的实际结论交给桌面版设置页，界面
//! 显示的宿主与回执里写的结论因此永远同源。`WUNDER_PYTHON` / `WUNDER_NODE` /
//! `WUNDER_BASH` 可显式指定可执行文件；探测不到 PATH 里的解释器时，会再去补充包
//! （`WUNDER_HOME/opt`）里找：Python、MinGit 的 bash、便携 PowerShell，以及
//! Playwright 自带的 node（补充包没有独立 `opt/node`，这是唯一的 Node）。
//! 校验内容一律走 stdin，不落临时文件：
//! - `.py`/`.pyi`：`python -I -c <compile>`，只编译不执行；
//! - `.js`/`.mjs`/`.cjs`：`node --check -`；命中模块专用语法时用
//!   `--input-type=module --check -` 再验一次，ESM/CJS 都不用先猜；
//! - `.ts`/`.mts`/`.cts`：项目里能解析到 typescript 时用 tsc 自己的语法诊断
//!   （只报文法/语法错，不做类型检查，标签 `tsc syntax`）；找不到 typescript
//!   时退回 `stripTypeScriptTypes` 去类型后交给 vm 解析（标签 `ts strip-types`，
//!   能抓大部分语法错，但 `<>` 空类型参数、空 extends/implements 这类文法错误
//!   会漏判，回执标签本身说明这是弱校验）；
//! - `.sh`/`.bash`：`bash -n` 只解析不执行（没有 bash 时退到 `sh -n`）；Windows
//!   上补充包自带 `opt/bash/bin/bash.exe`（从同一版本的 PortableGit 里抽出的
//!   bash 闭包，MinGit 本身不含 shell），其次才是已安装的 Git for Windows /
//!   MSYS2 / Cygwin——PATH 里的 `bash.exe` 多半是 WSL 启动壳，不参与探测；
//! - `.ps1`/`.psm1`/`.psd1`：用 PowerShell 自己的 `Parser::ParseInput`，只解析不
//!   执行；优先补充包里的便携 `pwsh.exe`，其次 PATH 的 `pwsh`，最后系统的
//!   `powershell.exe`（2.0 没有 Parser 类型，会降级成 `skipped`）；
//! - `.css`/`.scss`/`.less`：纯 Rust 扫掉注释与字符串后配平花括号。
//!
//! TypeScript 的语法检查必须用 tsc 自己的诊断：`stripTypeScriptTypes` 基于 SWC，
//! 对 `function f<>() {}`、`class A implements {}`、`class A extends {}`、八进制
//! 转义这些 tsc 报 TS1097/TS1098/TS1487 的写法会静默通过，回执里却是 OK。

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock, RwLock, TryLockError};
use std::time::{Duration, Instant};

use crate::core::command_utils::hide_console_window_std;
use base64::Engine;
use wunder_core::local_bootstrap::supplement;
use wunder_core::local_bootstrap::supplement::Tool as SupplementTool;

/// 一次写入校验的总预算：排队等闸门与子进程运行都从这 5 s 里扣。回执里的语法
/// 结论是加分项，宁可降级成 `skipped`，也不能让写入路径被校验拖住。
const SYNTAX_BUDGET: Duration = Duration::from_secs(5);
/// 闸门轮询粒度：同族里排队的校验几乎无抖动，又不至于空转。
const GATE_POLL: Duration = Duration::from_millis(2);
/// 报错摘要的字符上限，避免把整段堆栈塞进回执。
const MAX_DETAIL_CHARS: usize = 160;

/// Win7 补充包里的 node 来自 Playwright，版本早于 Node 官方支持的平台矩阵；
/// 不放开这个开关，每次启动都以「平台不支持」退出。新版 node 忽略该变量。
const NODE_ENVS: &[(&str, &str)] = &[("NODE_SKIP_PLATFORM_CHECK", "1")];
/// 解释器探测的等待上限：探测也走子进程，不能比校验本身更没边。
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// PowerShell 探测参数：不加载 profile、不交互，避免被用户配置拖慢或挂住。
const PS_PROBE_ARGS: &[&str] = &[
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    "exit 0",
];

/// 一次校验的时间预算。排队和子进程运行共享同一个截止时刻，所以最坏耗时只有
/// 一份 [`SYNTAX_BUDGET`]，而不是「排队等一份、运行再等一份」。
#[derive(Clone, Copy)]
struct Budget {
    deadline: Instant,
}

impl Budget {
    fn start() -> Self {
        Self {
            deadline: Instant::now() + SYNTAX_BUDGET,
        }
    }

    fn deadline(self) -> Instant {
        self.deadline
    }

    /// 还剩多少时间；扣光了返回 `None`，调用方据此降级。
    fn remaining(self) -> Option<Duration> {
        let now = Instant::now();
        (now < self.deadline).then(|| self.deadline - now)
    }
}

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
        map.insert(
            "tool".to_string(),
            serde_json::Value::String(self.tool.to_string()),
        );
        let status = match self.status {
            Status::Ok => "ok",
            Status::Error => "error",
            Status::Skipped => "skipped",
        };
        map.insert(
            "status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        if let Some(line) = self.line {
            map.insert("line".to_string(), serde_json::json!(line));
        }
        if let Some(message) = self.message.as_deref() {
            map.insert(
                "message".to_string(),
                serde_json::Value::String(message.to_string()),
            );
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
    let budget = Budget::start();
    match run_with_stdin(
        Family::Python,
        python,
        &["-I", "-c", PY_CHECK_SCRIPT, "wunder-write-check"],
        content,
        budget,
    ) {
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
    let budget = Budget::start();
    match run_with_stdin_env(
        Family::Node,
        node,
        &["--check", "-"],
        content,
        NODE_ENVS,
        budget,
    ) {
        Ok((true, _, _)) => Verdict::ok("node --check"),
        Ok((false, stdout, stderr)) => {
            let text = if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            };
            if looks_like_module_syntax(&text) {
                return match run_with_stdin_env(
                    Family::Node,
                    node,
                    &["--input-type=module", "--check", "-"],
                    content,
                    NODE_ENVS,
                    budget,
                ) {
                    Ok((true, _, _)) => Verdict::ok("node --check"),
                    Ok((false, stdout, stderr)) => {
                        let esm = if stderr.trim().is_empty() {
                            stdout
                        } else {
                            stderr
                        };
                        node_error_verdict("node --check", &esm)
                    }
                    Err(err) => {
                        Verdict::skipped("node --check", "node ".to_string() + err.reason())
                    }
                };
            }
            node_error_verdict("node --check", &text)
        }
        Err(err) => Verdict::skipped("node --check", "node ".to_string() + err.reason()),
    }
}

/// TypeScript：优先用项目里的 typescript 做 tsc 自己的语法诊断（只报文法/语法错，
/// 不做类型检查）；解析不到 typescript 时退回 node 的 `stripTypeScriptTypes` 去类型
/// 再交给 `vm` 解析。两条路径的结论标签不同，模型能看出这一份是不是弱校验。
pub(crate) fn typescript(content: &str, path: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("ts syntax", "disabled");
    }
    let Some(node) = node_interpreter() else {
        return Verdict::skipped("ts syntax", "no node");
    };
    let search_dir = typescript_search_dir(path);
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("ts")
        .to_ascii_lowercase();
    let args = [
        "--experimental-vm-modules",
        "--no-warnings",
        "-e",
        TS_CHECK_SCRIPT,
        search_dir.as_str(),
        ext.as_str(),
    ];
    let tool_for = |name: &str| {
        if name == "tsc" {
            "tsc syntax"
        } else {
            "ts strip-types"
        }
    };
    let budget = Budget::start();
    match run_with_stdin_env(Family::Node, node, &args, content, NODE_ENVS, budget) {
        Ok((true, stdout, _)) => {
            let tool = ts_result(&stdout)
                .map(|result| tool_for(&result.tool))
                .unwrap_or("ts syntax");
            Verdict::ok(tool)
        }
        // exit 2 = 这个 node 没有去类型的 API，属于环境不支持，不是语法错。
        Ok((false, stdout, _)) => match ts_result(&stdout) {
            Some(result) if result.message.as_deref() == Some("unsupported") => {
                Verdict::skipped("ts strip-types", "node too old")
            }
            Some(result) => Verdict::error(
                tool_for(&result.tool),
                result.line,
                result
                    .message
                    .unwrap_or_else(|| "unknown error".to_string()),
            ),
            // 检查脚本自己挂了（stdout 不是 JSON）：不能让模型以为是源码错。
            None => Verdict::skipped("ts syntax", "ts checker failed"),
        },
        Err(err) => Verdict::skipped("ts syntax", "node ".to_string() + err.reason()),
    }
}

/// Shell：`bash -n` 只解析不执行；没有 bash 时用 `sh -n` 兜底（POSIX 脚本
/// 同样能验，标签会写明是 `sh -n`）。`LC_ALL=C` 让报错固定是 `line N:` 格式，
/// 不受宿主语言环境影响。CRLF 先归一成 LF：Windows 上写出来的脚本行尾带 `\r`，
/// `bash` 会把 `then\r` 当成非法 token，报出一堆并不存在的语法错。
pub(crate) fn shell(content: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("bash -n", "disabled");
    }
    let Some((program, tool)) = shell_checker() else {
        return Verdict::skipped("bash -n", "no bash");
    };
    let normalized = normalize_line_endings(content);
    let budget = Budget::start();
    match run_with_stdin_env(
        Family::Shell,
        program,
        &["-n"],
        normalized.as_ref(),
        &[("LC_ALL", "C"), ("LANG", "C")],
        budget,
    ) {
        Ok((true, _, _)) => Verdict::ok(tool),
        Ok((false, stdout, stderr)) => {
            let text = if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            };
            shell_error_verdict(tool, &text)
        }
        Err(err) => Verdict::skipped(tool, "shell ".to_string() + err.reason()),
    }
}

/// PowerShell 系（`.ps1`/`.psm1`/`.psd1`）：用 PowerShell 自己的语法解析器，
/// 只解析不执行，所以 `param()`、`#Requires`、模块清单都不会产生副作用。
pub(crate) fn powershell(content: &str) -> Verdict {
    if env_disabled() {
        return Verdict::skipped("ps parse", "disabled");
    }
    let Some(program) = powershell_checker() else {
        return Verdict::skipped("ps parse", "no powershell");
    };
    let budget = Budget::start();
    // `-EncodedCommand` 收 UTF-16LE + Base64 的脚本文本：检查脚本里带着花括号、
    // `$`、单引号，走 `-Command` 就得和 Windows 的命令行引号规则搏斗，编码后
    // 一个字节都不用转义，PowerShell 2.0 起的每个版本都认这个参数。
    let mut args: Vec<&str> = PS_CHECK_ARGS.to_vec();
    args.push(ps_encoded_command());
    match run_with_stdin(Family::PowerShell, program, &args, content, budget) {
        Ok((true, _, _)) => Verdict::ok("ps parse"),
        Ok((false, stdout, _)) => powershell_error_verdict(&stdout),
        Err(err) => Verdict::skipped("ps parse", "powershell ".to_string() + err.reason()),
    }
}

/// PowerShell 检查脚本的结论：`unsupported: no parser` 是环境降级，`line N: ...` 是
/// 源码语法错；其余 stdout（包括完全空的）都算检查器自己没跑起来，绝不能把
/// 「PowerShell 没启动」当成用户源码写错。.NET 的 `Encoding.UTF8` 默认写 BOM，
/// 所以先剥掉一个 U+FEFF 再认前缀。
fn powershell_error_verdict(stdout: &str) -> Verdict {
    let stdout = stdout.trim_start_matches('\u{feff}').trim();
    if stdout.contains(PS_UNSUPPORTED_MARK) {
        return Verdict::skipped("ps parse", "powershell too old");
    }
    let detail = match first_non_empty_line(stdout) {
        Some(detail) if detail.starts_with("line ") => detail,
        _ => return Verdict::skipped("ps parse", "ps checker failed"),
    };
    let (line, message) = split_line_prefix(detail);
    Verdict::error("ps parse", line, message)
}

/// 只归一化行尾，不动行数：`bash -n`/`sh -n` 眼里的 `\r` 是普通字符。
fn normalize_line_endings(content: &str) -> Cow<'_, str> {
    if content.contains('\r') {
        Cow::Owned(content.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(content)
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
            b'/' if bytes.get(index + 1) == Some(&b'*') => match content[index + 2..].find("*/") {
                Some(offset) => {
                    line += content[index..index + offset + 4].matches('\n').count();
                    index += offset + 4;
                }
                None => {
                    return Verdict::error("css braces", Some(line as u32), "注释没有闭合");
                }
            },
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
    "import ast, sys\n",
    "src = sys.stdin.buffer.read()\n",
    "def _brace_problem(text):\n",
    "    depth = 0\n",
    "    i = 0\n",
    "    while i < len(text):\n",
    "        ch = text[i]\n",
    "        if ch == '{':\n",
    "            if i + 1 < len(text) and text[i + 1] == '{':\n",
    "                i += 2\n",
    "                continue\n",
    "            depth += 1\n",
    "        elif ch == '}':\n",
    "            if i + 1 < len(text) and text[i + 1] == '}':\n",
    "                i += 2\n",
    "                continue\n",
    "            if depth == 0:\n",
    "                return \"stray '}' in .format() string; a '{' may be missing before a format spec\"\n",
    "            depth -= 1\n",
    "        i += 1\n",
    "    if depth:\n",
    "        return \"unclosed '{' in .format() string\"\n",
    "    return None\n",
    "try:\n",
    "    tree = compile(src, sys.argv[1], 'exec', ast.PyCF_ONLY_AST)\n",
    "except SyntaxError as exc:\n",
    "    print('line %d: %s' % (exc.lineno, exc.msg))\n",
    "    sys.exit(1)\n",
    "except ValueError as exc:\n",
    "    print('invalid: %s' % exc)\n",
    "    sys.exit(1)\n",
    "for node in ast.walk(tree):\n",
    "    if not isinstance(node, ast.Call):\n",
    "        continue\n",
    "    func = node.func\n",
    "    if not isinstance(func, ast.Attribute) or func.attr not in ('format', 'format_map'):\n",
    "        continue\n",
    "    receiver = func.value\n",
    "    text = None\n",
    "    if isinstance(receiver, ast.Constant) and isinstance(receiver.value, str):\n",
    "        text = receiver.value\n",
    "    elif hasattr(ast, 'Str') and isinstance(receiver, ast.Str):\n",
    "        text = receiver.s\n",
    "    if text is None:\n",
    "        continue\n",
    "    problem = _brace_problem(text)\n",
    "    if problem:\n",
    "        print('line %d: %s' % (receiver.lineno, problem))\n",
    "        sys.exit(1)\n",
);

/// TypeScript 检查脚本：能解析到项目里的 typescript 就用 tsc 自己的诊断（语法 +
/// 文法错误，过滤掉 2xxx 及之后的类型/解析错误）；否则退回 strip-types。
/// 结论统一用 JSON 输出，退出码区分 OK(0)/ERROR(1)/unsupported(2)/检查器失败(3)。
/// 同样逐行拼接，缩进不会被吞。
const TS_CHECK_SCRIPT: &str = concat!(
    "const fs = require('node:fs');\n",
    "const path = require('node:path');\n",
    "const vm = require('node:vm');\n",
    "const mod = require('node:module');\n",
    "const src = fs.readFileSync(0, 'utf8');\n",
    "const searchDir = process.argv[1] || process.cwd();\n",
    "const ext = process.argv[2] || 'ts';\n",
    "function emit(tool, line, message) {\n",
    "  console.log(JSON.stringify({ tool, line: line || null, message: message || null }));\n",
    "}\n",
    "function loadTypeScript() {\n",
    "  try { return require(require.resolve('typescript', { paths: [searchDir] })); }\n",
    "  catch (err) { return null; }\n",
    "}\n",
    "const ts = loadTypeScript();\n",
    "if (ts && typeof ts.createProgram === 'function' && ts.ScriptKind) {\n",
    "  try {\n",
    "    const file = path.join(searchDir, '__wunder_write_check__.' + ext);\n",
    "    const kind = ts.ScriptKind[ext.toUpperCase()] || ts.ScriptKind.TS;\n",
    "    const options = {\n",
    "      noLib: true, noResolve: true, noEmit: true, skipLibCheck: true,\n",
    "      experimentalDecorators: true,\n",
    "      target: ts.ScriptTarget.Latest,\n",
    "      module: ts.ModuleKind.Preserve || ts.ModuleKind.ESNext,\n",
    "    };\n",
    "    if (ts.ModuleDetectionKind && ts.ModuleDetectionKind.Force !== undefined) {\n",
    "      options.moduleDetection = ts.ModuleDetectionKind.Force;\n",
    "    }\n",
    "    const host = {\n",
    "      getSourceFile: (name, version) => name === file ? ts.createSourceFile(name, src, version, true, kind) : undefined,\n",
    "      getDefaultLibFileName: () => path.join(searchDir, '__wunder_lib__.d.ts'),\n",
    "      writeFile: () => {},\n",
    "      getCurrentDirectory: () => searchDir,\n",
    "      getCanonicalFileName: (name) => name,\n",
    "      useCaseSensitiveFileNames: () => true,\n",
    "      getNewLine: () => '\\n',\n",
    "      fileExists: (name) => name === file,\n",
    "      readFile: (name) => name === file ? src : undefined,\n",
    "    };\n",
    "    const program = ts.createProgram([file], options, host);\n",
    "    const source = program.getSourceFile(file);\n",
    "    const diags = program.getSyntacticDiagnostics(source)\n",
    "      .concat(program.getSemanticDiagnostics(source))\n",
    "      .filter((item) => item.code < 2000)\n",
    "      .sort((a, b) => (a.start || 0) - (b.start || 0));\n",
    "    if (diags.length) {\n",
    "      const first = diags[0];\n",
    "      const at = first.start != null ? source.getLineAndCharacterOfPosition(first.start) : null;\n",
    "      const message = 'TS' + first.code + ': ' + ts.flattenDiagnosticMessageText(first.messageText, ' ');\n",
    "      emit('tsc', at ? at.line + 1 : null, message);\n",
    "      process.exit(1);\n",
    "    }\n",
    "    emit('tsc');\n",
    "    process.exit(0);\n",
    "  } catch (err) {\n",
    "    console.error(String((err && err.stack) || err));\n",
    "    process.exit(3);\n",
    "  }\n",
    "}\n",
    "if (typeof mod.stripTypeScriptTypes !== 'function') { emit('strip', null, 'unsupported'); process.exit(2); }\n",
    "let js;\n",
    "try { js = mod.stripTypeScriptTypes(src, { mode: 'transform' }); }\n",
    "catch (err) { emit('strip', null, String((err && err.message) || err)); process.exit(1); }\n",
    "let moduleErr = null;\n",
    "if (typeof vm.SourceTextModule === 'function') {\n",
    "  try { new vm.SourceTextModule(js); emit('strip'); process.exit(0); }\n",
    "  catch (err) { moduleErr = err; }\n",
    "}\n",
    "try { new vm.Script(js); emit('strip'); process.exit(0); }\n",
    "catch (err) {\n",
    "  const chosen = moduleErr || err;\n",
    "  const line = chosen && chosen.lineNumber ? chosen.lineNumber : null;\n",
    "  emit('strip', line, String((chosen && chosen.message) || chosen));\n",
    "  process.exit(1);\n",
    "}\n",
);

/// PowerShell 检查脚本：只用 `Parser::ParseInput` 解析，不执行用户脚本。源码用
/// UTF-8 从 stdin 读、结论用 UTF-8 写回，绕开 Windows 控制台代码页（中文系统默认
/// GBK，按它解码会把 UTF-8 源码弄成乱码）。全用单引号字符串，避开 Windows 命令行
/// 的引号规则；没有 `Parser` 类型（PowerShell 2.0）时输出固定标记降级。
const PS_CHECK_SCRIPT: &str = concat!(
    "$reader=New-Object System.IO.StreamReader([Console]::OpenStandardInput(),[System.Text.Encoding]::UTF8);",
    "$src=$reader.ReadToEnd();",
    "$out=New-Object System.IO.StreamWriter([Console]::OpenStandardOutput(),[System.Text.Encoding]::UTF8);",
    "$tokens=$null;$errors=$null;",
    "try{[void][System.Management.Automation.Language.Parser]::ParseInput($src,[ref]$tokens,[ref]$errors)}",
    "catch{$out.WriteLine('unsupported: no parser');$out.Flush();exit 1};",
    "if($errors -and $errors.Count -gt 0){",
    "$e=$errors[0];",
    "$out.WriteLine('line {0}: {1}' -f $e.Extent.StartLineNumber,$e.Message);",
    "$out.Flush();exit 1};",
    "$out.WriteLine('ok');$out.Flush();exit 0",
);
/// 检查脚本自身的降级标记：输出里出现它就说明环境没有解析器，不是源码写错。
const PS_UNSUPPORTED_MARK: &str = "unsupported: no parser";
/// PowerShell 校验参数：`-EncodedCommand` 后面跟 UTF-16LE + Base64 的检查脚本。
const PS_CHECK_ARGS: &[&str] = &[
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-EncodedCommand",
];

/// 检查脚本的 `-EncodedCommand` 形态：只算一次，之后复用（编码结果几百字节）。
fn ps_encoded_command() -> &'static str {
    static ENCODED: OnceLock<String> = OnceLock::new();
    ENCODED.get_or_init(|| {
        let mut utf16 = Vec::with_capacity(PS_CHECK_SCRIPT.len() * 2);
        for unit in PS_CHECK_SCRIPT.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        base64::engine::general_purpose::STANDARD.encode(&utf16)
    })
}

/// TypeScript 检查脚本的 JSON 结论；解析失败说明检查器自身出了问题。
struct TsCheckResult {
    tool: String,
    line: Option<u32>,
    message: Option<String>,
}

fn ts_result(stdout: &str) -> Option<TsCheckResult> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    Some(TsCheckResult {
        tool: value.get("tool")?.as_str()?.to_string(),
        line: value
            .get("line")
            .and_then(serde_json::Value::as_u64)
            .map(|line| line as u32),
        message: value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// 给 TypeScript 查找 typescript 包的起点：被写文件所在目录。相对路径按当前
/// 工作目录展开；目录不存在也没关系，node 会从它开始逐级向上找 node_modules。
fn typescript_search_dir(path: &str) -> String {
    let candidate = Path::new(path);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(candidate),
            Err(_) => candidate.to_path_buf(),
        }
    };
    absolute
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

/// 解释器探测结论的缓存槽：`None` 表示还没探过，`Some(None)` 表示探过但不可用。
/// 探测要起子进程，所以结论按进程缓存；设置页改了路径由 [`reset_hosts`] 清空。
static PYTHON_HOST: RwLock<Option<Option<&'static str>>> = RwLock::new(None);
static NODE_HOST: RwLock<Option<Option<&'static str>>> = RwLock::new(None);
static SHELL_HOST: RwLock<Option<Option<(&'static str, &'static str)>>> = RwLock::new(None);
static POWERSHELL_HOST: RwLock<Option<Option<&'static str>>> = RwLock::new(None);

/// 读缓存槽，空则跑一次探测写回。探测本身已经把路径泄漏成 `&'static str`
/// （一次进程内至多几条），这里直接复用，不再复制。
fn cached_host(
    slot: &'static RwLock<Option<Option<&'static str>>>,
    probe: impl FnOnce() -> Option<&'static str>,
) -> Option<&'static str> {
    if let Ok(guard) = slot.read() {
        if let Some(cached) = guard.as_ref() {
            return *cached;
        }
    }
    let probed = probe();
    if let Ok(mut guard) = slot.write() {
        *guard = Some(probed);
    }
    probed
}

fn cached_shell_host(
    slot: &'static RwLock<Option<Option<(&'static str, &'static str)>>>,
    probe: impl FnOnce() -> Option<(&'static str, &'static str)>,
) -> Option<(&'static str, &'static str)> {
    if let Ok(guard) = slot.read() {
        if let Some(cached) = guard.as_ref() {
            return *cached;
        }
    }
    let probed = probe();
    if let Ok(mut guard) = slot.write() {
        *guard = Some(probed);
    }
    probed
}

/// 丢掉所有解释器探测结论。桌面版保存运行时路径后调用：新路径立刻生效，
/// 不必等下一次启动。
pub fn reset_hosts() {
    let clear = |slot: &'static RwLock<Option<Option<&'static str>>>| {
        *slot.write().unwrap_or_else(|error| error.into_inner()) = None;
    };
    clear(&PYTHON_HOST);
    clear(&NODE_HOST);
    clear(&POWERSHELL_HOST);
    *SHELL_HOST
        .write()
        .unwrap_or_else(|error| error.into_inner()) = None;
}

/// 写后语法校验实际会用到、且需要探测的宿主：JS/TS 走 node，`.sh` 走 bash
/// （没有 bash 时是 sh）。桌面版设置页据此显示「当前生效的解释器」，与回执里
/// 写的校验器标签同源，界面不会说一套、回执写另一套。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyntaxHosts {
    pub node: Option<String>,
    pub shell: Option<String>,
    /// `bash -n` / `sh -n`；没有 shell 时为空串。
    pub shell_label: String,
}

pub fn syntax_hosts() -> SyntaxHosts {
    let (shell, shell_label) = match shell_checker() {
        Some((path, label)) => (Some(path.to_string()), label.to_string()),
        None => (None, String::new()),
    };
    SyntaxHosts {
        node: node_interpreter().map(str::to_string),
        shell,
        shell_label,
    }
}

/// 找到可用的 Python 解释器：只探测一次，之后复用结论。桌面版把选中的解释器
/// 发布成 `WUNDER_PYTHON_BIN`，命令行/服务端仍可用 `WUNDER_PYTHON` 覆盖。
fn python_interpreter() -> Option<&'static str> {
    cached_host(&PYTHON_HOST, || {
        let mut candidates = Vec::new();
        push_env_candidate(&mut candidates, "WUNDER_PYTHON");
        push_env_candidate(&mut candidates, "WUNDER_PYTHON_BIN");
        candidates.push("python3".to_string());
        candidates.push("python".to_string());
        push_supplement_candidate(&mut candidates, SupplementTool::Python);
        first_working(&candidates, &["-c", "import sys"])
    })
}

/// 找到可用的 Node：先看 `WUNDER_NODE` 与 PATH，再看补充包（Playwright 驱动自带的
/// node）与可执行文件附近的便携 node。Win7 补充包里的 node 是 16.x，超出 Node 支持
/// 的平台矩阵，所以所有 node 调用都带上 `NODE_SKIP_PLATFORM_CHECK`。
fn node_interpreter() -> Option<&'static str> {
    cached_host(&NODE_HOST, || {
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
        // PATH 里通常已经有补充包发布的那一目录；按绝对路径再兜一层，服务端/独立
        // 运行时没走 `configure_runtime_env` 时也能用上。
        push_supplement_candidate(&mut candidates, SupplementTool::Node);
        first_working_env(&candidates, &["--version"], NODE_ENVS)
    })
}

/// 找到可用的 shell：`WUNDER_BASH` → 补充包的 bash → PATH 的 bash → PATH 的 sh。
/// 只有 bash 不可用才用 sh 兜底，标签写成 `sh -n`，模型能看出解析器不同。
///
/// Windows 上补充包自带 `opt/bash/bin/bash.exe`（bash.exe + sh.exe + MSYS
/// 运行时 DLL，从同一版本的 PortableGit 里抽出），所以裸机也有 bash。没有补充包
/// 时只认已安装的 Git for Windows（系统级与用户级）、MSYS2、Cygwin 的常见路径：
/// PATH 里那个 `bash.exe` 通常是 `System32` 的 WSL 启动壳，没装发行版时直接报错、
/// 装了也要好几秒才起来。另有 `WUNDER_BASH` 兜底。
fn shell_checker() -> Option<(&'static str, &'static str)> {
    cached_shell_host(&SHELL_HOST, || {
        let mut bash: Vec<String> = Vec::new();
        push_env_candidate(&mut bash, "WUNDER_BASH");
        push_supplement_candidate(&mut bash, SupplementTool::Bash);
        if cfg!(windows) {
            // MinGit 是「非交互发行版」，明确不带 Git Bash，所以只能找已经
            // 装好的 Git for Windows / MSYS2 / Cygwin；不查 PATH 里的 `bash`，
            // 那里通常是 `System32\\bash.exe` 这个 WSL 启动壳。
            bash.extend(windows_shell_candidates("bash.exe"));
        } else {
            bash.push("bash".to_string());
        }
        if let Some(program) = first_working(&bash, &["-c", ":"]) {
            return Some((program, "bash -n"));
        }
        let sh: Vec<String> = if cfg!(windows) {
            let mut candidates = windows_shell_candidates("sh.exe");
            candidates.push("sh".to_string());
            candidates
        } else {
            vec!["sh".to_string()]
        };
        first_working(&sh, &["-c", ":"]).map(|program| (program, "sh -n"))
    })
}

/// Windows 上已知的 Git Bash / MSYS2 / Cygwin 安装位置，64 位与 32 位都算上。
///
/// Git for Windows 默认装到 `Program Files\Git`，用户级安装落在
/// `%LOCALAPPDATA%\Programs\Git`；MSYS2 与 Cygwin 习惯放在系统盘根目录。
/// 只列目录不扫盘：一次 `-n` 检查不该变成全盘遍历。
fn windows_shell_candidates(exe: &str) -> Vec<String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for key in [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "SystemDrive",
    ] {
        if let Some(value) = std::env::var_os(key) {
            if !value.is_empty() {
                roots.push(PathBuf::from(value));
            }
        }
    }
    windows_shell_candidates_in(&roots, exe)
}

fn windows_shell_candidates_in(roots: &[PathBuf], exe: &str) -> Vec<String> {
    const LAYOUTS: &[&str] = &[
        "Git/bin",
        "Git/usr/bin",
        "Programs/Git/bin",
        "Programs/Git/usr/bin",
        "msys64/usr/bin",
        "msys2/usr/bin",
        "cygwin64/bin",
        "cygwin/bin",
    ];
    roots
        .iter()
        .flat_map(|root| {
            LAYOUTS
                .iter()
                .map(move |layout| root.join(layout).join(exe))
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 找到可用的 PowerShell：`WUNDER_POWERSHELL_EXE`（桌面版/命令行随补充包发布）→
/// 补充包的便携 pwsh → PATH 的 `pwsh` → PATH 的 `powershell`。Windows 自带的
/// Windows PowerShell 2.0 没有 `Language.Parser`，探测能过但校验会降级。
fn powershell_checker() -> Option<&'static str> {
    cached_host(&POWERSHELL_HOST, || {
        let mut candidates = Vec::new();
        push_env_candidate(
            &mut candidates,
            crate::core::command_utils::POWERSHELL_EXE_ENV,
        );
        push_supplement_candidate(&mut candidates, SupplementTool::PowerShell);
        candidates.push("pwsh".to_string());
        candidates.push("powershell".to_string());
        first_working(&candidates, PS_PROBE_ARGS)
    })
}

/// 加一个环境变量指定的可执行文件；空值当作没设。
fn push_env_candidate(candidates: &mut Vec<String>, env_name: &str) {
    if let Ok(value) = std::env::var(env_name) {
        let value = value.trim();
        if !value.is_empty() {
            candidates.push(value.to_string());
        }
    }
}

/// 补充包里的同名工具按绝对路径补在候选末尾：PATH 里的可用版本优先，缺失时兜底。
fn push_supplement_candidate(candidates: &mut Vec<String>, tool: SupplementTool) {
    if let Some(path) = supplement::find_tool(&supplement::local_roots(), tool) {
        candidates.push(path.to_string_lossy().into_owned());
    }
}

fn env_and_path_candidates(env_name: &str, fallback: &[&str]) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    push_env_candidate(&mut candidates, env_name);
    candidates.extend(fallback.iter().map(|item| item.to_string()));
    candidates
}

fn first_working(candidates: &[String], check_args: &[&str]) -> Option<&'static str> {
    first_working_env(candidates, check_args, &[])
}

fn first_working_env(
    candidates: &[String],
    check_args: &[&str],
    envs: &[(&str, &str)],
) -> Option<&'static str> {
    for candidate in candidates {
        // 裸命令名（`node`、`bash`）先按 PATH 展开成绝对路径：设置页要显示真正
        // 生效的二进制位置，回执与日志里也不该只有一个相对名字。
        let resolved = absolute_program(candidate);
        if probe_works(&resolved, check_args, envs) {
            let leaked: &'static str = Box::leak(resolved.into_boxed_str());
            return Some(leaked);
        }
    }
    None
}

/// 把候选解释器归一成绝对路径：已有目录成分的候选（补充包、环境变量里写死的
/// 全路径、可执行文件旁边的便携 node）原样返回；裸命令名按 PATH + PATHEXT 查找，
/// 找到就用命中的绝对路径，找不到仍返回原样的名字（探测随后会失败，不影响判定）。
fn absolute_program(candidate: &str) -> String {
    let trimmed = candidate.trim();
    let has_dir = trimmed.contains('/') || trimmed.contains('\\');
    if trimmed.is_empty() || Path::new(trimmed).is_absolute() || has_dir {
        return trimmed.to_string();
    }
    match find_on_path(trimmed) {
        Some(path) => path.to_string_lossy().into_owned(),
        None => trimmed.to_string(),
    }
}

/// PATH 查找：Windows 按 `.exe` 与 `PATHEXT` 依次补扩展名，Unix 直接用命令名。
/// 相对 PATH 项按当前工作目录展开，保证返回的是绝对路径。
fn find_on_path(program: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        if Path::new(program).extension().is_some() {
            vec![program.to_string()]
        } else {
            let mut names = Vec::new();
            if let Ok(pathext) = std::env::var("PATHEXT") {
                for extension in pathext.split(';').map(str::trim).filter(|e| !e.is_empty()) {
                    names.push(program.to_string() + &extension.to_ascii_lowercase());
                }
            }
            if !names.iter().any(|name| name.ends_with(".exe")) {
                names.push(program.to_string() + ".exe");
            }
            names
        }
    } else {
        vec![program.to_string()]
    };
    let path = std::env::var_os("PATH")?;
    let cwd = std::env::current_dir().ok();
    std::env::split_paths(&path).find_map(|dir| {
        let dir = if dir.is_absolute() {
            dir
        } else {
            match cwd.as_deref() {
                Some(cwd) => cwd.join(dir),
                None => return None,
            }
        };
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// 单个候选的启动探测。必须有上限：PowerShell 冷启动被实时防护扫描拖住时，
/// 探测自己就会把写入路径一起拖死；超时按「不可用」处理，继续试下一个候选。
fn probe_works(candidate: &str, check_args: &[&str], envs: &[(&str, &str)]) -> bool {
    let mut command = Command::new(candidate);
    command
        .args(check_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // 探测同样不该在 Windows 上闪黑框（首个 `.py` 写入就会触发）。
    hide_console_window_std(&mut command);
    for (key, value) in envs {
        command.env(key, value);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// 解释器家族。闸门按家族分开：内存开销来自「同时跑几个解释器进程」，排队却不
/// 该跨家族——一次 TypeScript 检查要 `require('typescript')`（本地实测约 137 ms，
/// 冷启动和机械盘上更久），把并发的 Python 校验堵在同一个锁后面，只会让本来 20 ms
/// 的检查降级成 `skipped (python checker busy)`。同族仍然只跑一个：node 单次常驻
/// 约 42 MiB，并发叠加会把进程树内存推到数百 MiB。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Python,
    Node,
    Shell,
    /// PowerShell：冷启动就要几百毫秒，和别的家族排队只会互相拖累。
    PowerShell,
}

impl Family {
    fn gate(self) -> &'static Mutex<()> {
        static PYTHON: OnceLock<Mutex<()>> = OnceLock::new();
        static NODE: OnceLock<Mutex<()>> = OnceLock::new();
        static SHELL: OnceLock<Mutex<()>> = OnceLock::new();
        static POWERSHELL: OnceLock<Mutex<()>> = OnceLock::new();
        let slot = match self {
            Family::Python => &PYTHON,
            Family::Node => &NODE,
            Family::Shell => &SHELL,
            Family::PowerShell => &POWERSHELL,
        };
        slot.get_or_init(|| Mutex::new(()))
    }
}

struct CheckerGate {
    _guard: MutexGuard<'static, ()>,
}

impl CheckerGate {
    fn acquire(family: Family, budget: Budget) -> Result<Self, RunError> {
        Ok(Self {
            _guard: acquire_gate(family.gate(), budget)?,
        })
    }
}

/// 轮询拿锁：不阻塞在 `lock()` 上，等待受预算约束，预算扣光返回 [`RunError::Busy`]。
fn acquire_gate(
    gate: &'static Mutex<()>,
    budget: Budget,
) -> Result<MutexGuard<'static, ()>, RunError> {
    loop {
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            // 上一次校验 panic 把锁毒化了也算可用，`()` 里没有需要恢复的状态。
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                let remaining = budget.remaining().ok_or(RunError::Busy)?;
                std::thread::sleep(GATE_POLL.min(remaining));
            }
        }
    }
}

/// 把源码写进子进程 stdin 并等它退出；超时就 kill，不阻塞写入路径。
/// 调用期间独占本族闸门，同一时刻只有一个同类解释器在校验。
fn run_with_stdin(
    family: Family,
    program: &str,
    args: &[&str],
    content: &str,
    budget: Budget,
) -> Result<(bool, String, String), RunError> {
    run_with_stdin_env(family, program, args, content, &[], budget)
}

/// 同 [`run_with_stdin`]，额外给子进程设置固定环境变量（如 shell 的 C locale，
/// 让报错行号格式不受宿主语言影响）。
fn run_with_stdin_env(
    family: Family,
    program: &str,
    args: &[&str],
    content: &str,
    envs: &[(&str, &str)],
    budget: Budget,
) -> Result<(bool, String, String), RunError> {
    let _gate = CheckerGate::acquire(family, budget)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 校验子进程只走管道，Windows 下必须显式关掉控制台，否则每写一个 `.py`
    // 都会闪一个黑框（控制台程序默认会给自己分配窗口）。
    hide_console_window_std(&mut command);
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(|_| RunError::Unavailable)?;
    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(content.as_bytes()).is_err() {
            let _ = child.kill();
            return Err(RunError::StdinClosed);
        }
    }
    drop(child.stdin.take());

    let deadline = budget.deadline();
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

/// node 的结构化结论：工具名由调用方给（`node --check`）。
fn node_error_verdict(tool: &'static str, text: &str) -> Verdict {
    let (line, message) = node_error_parts(text);
    Verdict::error(tool, line, message)
}

/// bash 的报错形如 `bash: line 3: syntax error near unexpected token ...`。
/// `LC_ALL=C` 已固定输出语言；没有 `line N:` 的形态（如 dash 的 `sh: 1: ...`）
/// 就只给消息，不硬猜行号。
fn shell_error_verdict(tool: &'static str, text: &str) -> Verdict {
    let first = first_non_empty_line(text).unwrap_or("unknown error").trim();
    let parsed = first
        .split_once("line ")
        .and_then(|(_, rest)| rest.split_once(':'))
        .and_then(|(number, message)| {
            number
                .trim()
                .parse::<u32>()
                .ok()
                .map(|line| (line, message.trim().to_string()))
        });
    match parsed {
        Some((line, message)) => {
            Verdict::error(tool, Some(line), truncate_chars(&message, MAX_DETAIL_CHARS))
        }
        None => Verdict::error(tool, None, truncate_chars(first, MAX_DETAIL_CHARS)),
    }
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
    fn bare_command_names_resolve_through_path() {
        // 探测候选本来就是裸命令名时，命中 PATH 的结论必须是绝对路径。
        let probe = if cfg!(windows) { "cmd" } else { "sh" };
        let found = find_on_path(probe).expect("system shell on PATH");
        assert!(found.is_absolute(), "{}", found.display());
        assert_eq!(
            absolute_program(probe),
            found.to_string_lossy(),
            "bare name should expand to the PATH hit"
        );
        // 已带目录成分的候选不做二次查找。
        let absolute = found.to_string_lossy().into_owned();
        assert_eq!(absolute_program(&absolute), absolute);
    }

    #[test]
    fn detected_hosts_are_absolute_paths() {
        // 设置页把探测结论直接填进输入框：裸命令名（`node` / `bash`）必须展开成
        // 绝对路径，否则用户看到的是一句命令而不是真正生效的二进制位置。
        let hosts = syntax_hosts();
        for path in [hosts.node, hosts.shell].into_iter().flatten() {
            assert!(
                Path::new(&path).is_absolute(),
                "host should be an absolute path: {path}"
            );
        }
    }

    #[test]
    fn css_balances_braces_and_ignores_strings() {
        assert_eq!(css(".a { color: red; }").render(), "OK (css braces)");
        // 字符串里的花括号不参与配平。
        assert_eq!(
            css(".a::after { content: \"}\"; }").render(),
            "OK (css braces)"
        );
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
        let code =
            javascript("const a = require('node:path');\nmodule.exports = { a };\n").render();
        if code.starts_with("skipped") {
            return;
        }
        assert_eq!(code, "OK (node --check)");
        let esm = javascript("import fs from 'node:fs';\nexport const a = 1;\n").render();
        assert_eq!(esm, "OK (node --check)");
    }

    #[test]
    fn typescript_accepts_types_or_skips_without_node() {
        let code = typescript(
            "interface A { a: number }\nexport const f = (x: A): number => x.a;\n",
            "temp/a.ts",
        )
        .render();
        if code.starts_with("skipped") {
            assert!(has(&code, "node") || has(&code, "busy"), "{code}");
            return;
        }
        assert!(
            code == "OK (tsc syntax)" || code == "OK (ts strip-types)",
            "{code}"
        );
        let broken = typescript("const f = (x: number) => {\n", "temp/a.ts").render();
        assert!(
            has(&broken, "(tsc syntax)") || has(&broken, "(ts strip-types)"),
            "{broken}"
        );
        assert!(broken.starts_with("ERROR"), "{broken}");
    }

    #[test]
    fn typescript_tsc_path_catches_grammar_errors_strip_types_misses() {
        // stripTypeScriptTypes 对空类型参数列表、空 extends/implements 会静默通过，
        // 但 tsc 报 TS1097/TS1098；找到 typescript 时必须能报出来。环境里没有
        // typescript 时退回弱校验，属于已知降级，不在这里断言。
        for source in [
            "function f<>() {}\n",
            "class A implements {}\n",
            "interface B extends {}\n",
        ] {
            let code = typescript(source, "temp/a.ts").render();
            if code.starts_with("skipped") || code.starts_with("OK (ts strip-types)") {
                continue;
            }
            assert!(code.starts_with("ERROR line 1"), "{source} => {code}");
            assert!(has(&code, "TS109"), "{source} => {code}");
        }
        if typescript("class A { accessor x = 1; }\n", "temp/a.ts").render() == "OK (tsc syntax)" {
            // tsc 接受 decorator 与 accessor；strip-types 会把它们当语法错。
            let decorator = typescript("@dec\nclass A {}\n", "temp/a.ts").render();
            assert_eq!(decorator, "OK (tsc syntax)");
        }
    }

    #[test]
    fn shell_accepts_scripts_and_reports_syntax_errors() {
        let valid =
            shell("#!/bin/bash\nset -e\nf() { echo hi; }\nif [[ -n \"$1\" ]]; then f; fi\n")
                .render();
        if valid.starts_with("skipped") {
            assert!(has(&valid, "no bash") || has(&valid, "disabled"), "{valid}");
            return;
        }
        assert!(valid == "OK (bash -n)" || valid == "OK (sh -n)", "{valid}");
        let broken = shell("if true; then\n  echo hi\n").render();
        assert!(broken.starts_with("ERROR"), "{broken}");
        assert!(has(&broken, "-n)"), "{broken}");
        // bash 固定 C locale 后应能给出 `line 3:`；sh 兜底时允许没有行号。
        if broken.contains("bash -n") {
            assert!(has(&broken, "line 3"), "{broken}");
        }
    }

    #[test]
    fn shell_normalizes_crlf_before_checking() {
        // Windows 上写出来的 `.sh` 行尾是 CRLF；不归一化时 bash 会把 `then\r`
        // 当成非法 token，合法脚本被判成语法错。
        let script = "#!/bin/bash\r\nset -e\r\nif true; then\r\n  echo hi\r\nfi\r\n";
        let code = shell(script).render();
        if code.starts_with("skipped") {
            return;
        }
        assert!(code == "OK (bash -n)" || code == "OK (sh -n)", "{code}");
        let broken = shell("if true; then\r\n  echo hi\r\n").render();
        assert!(broken.starts_with("ERROR"), "{broken}");
    }

    #[test]
    fn powershell_parses_scripts_and_reports_errors() {
        let valid = "param([string]$Name)\r\nWrite-Output \"hi $Name\"\r\n".to_string();
        let code = powershell(&valid).render();
        if code.starts_with("skipped") {
            // 没有 PowerShell，或只有不支持 Parser 的 2.0：属于环境降级。
            assert!(
                has(&code, "no powershell")
                    || has(&code, "disabled")
                    || has(&code, "too old")
                    || has(&code, "failed")
                    || has(&code, "timeout")
                    || has(&code, "busy"),
                "{code}"
            );
            return;
        }
        assert_eq!(code, "OK (ps parse)");
        let broken = powershell("function f {\r\n  'x'\r\n").render();
        assert!(broken.starts_with("ERROR"), "{broken}");
        assert!(has(&broken, "(ps parse)"), "{broken}");
    }

    #[test]
    fn encoded_ps_command_round_trips_to_the_check_script() {
        // `-EncodedCommand` 收 UTF-16LE + Base64：解回来必须逐字节等于检查脚本，
        // 否则 PowerShell 解析的就不是我们写的那段。
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(ps_encoded_command())
            .expect("encoded command is valid base64");
        let units: Vec<u16> = decoded
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), PS_CHECK_SCRIPT);
        assert!(
            ps_encoded_command().len() < 4096,
            "command line stays short"
        );
    }

    #[test]
    fn windows_shell_candidates_cover_git_bash_and_msys2() {
        // Windows 侧没有 PATH 可用（System32 的 bash.exe 是 WSL 壳），候选表就是
        // 全部覆盖面：系统级/用户级 Git for Windows、MSYS2、Cygwin。
        let roots = vec![PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\")];
        let bash = windows_shell_candidates_in(&roots, "bash.exe");
        let has = |needle: &str| bash.iter().any(|path| path.ends_with(needle));
        assert!(has("Git/bin/bash.exe"), "{bash:?}");
        assert!(has("Git/usr/bin/bash.exe"), "{bash:?}");
        assert!(has("Programs/Git/bin/bash.exe"), "{bash:?}");
        assert!(has("msys64/usr/bin/bash.exe"), "{bash:?}");
        // 任何候选都不能指向 System32 的 WSL 壳。
        assert!(bash.iter().all(|p| !p.contains("System32")), "{bash:?}");
        let sh = windows_shell_candidates_in(&roots, "sh.exe");
        assert!(sh.iter().all(|p| p.ends_with("sh.exe")), "{sh:?}");
    }

    #[test]
    fn powershell_verdict_never_blames_the_source_for_checker_failures() {
        // 结论行前面可能带 BOM（.NET 的 UTF8Encoding 默认写 BOM），行号不能丢。
        let bom = char::from_u32(0xFEFF).unwrap().to_string();
        let head = [
            bom.as_str(),
            "line 7: Missing closing '}' in statement block.",
        ]
        .concat();
        let error = powershell_error_verdict(&head).render();
        assert!(error.starts_with("ERROR line 7:"), "{}", error);
        assert!(has(&error, "(ps parse)"), "{}", error);
        // PowerShell 2.0 没有 Language.Parser：环境降级，不是源码错。
        let old = [bom.as_str(), "unsupported: no parser"].concat();
        assert_eq!(
            powershell_error_verdict(&old).render(),
            "skipped (powershell too old)"
        );
        // 检查器自己没跑起来（启动失败、被策略拦掉）时不许报成语法错。
        assert_eq!(
            powershell_error_verdict("").render(),
            "skipped (ps checker failed)"
        );
        assert_eq!(
            powershell_error_verdict("ok\n").render(),
            "skipped (ps checker failed)"
        );
    }

    #[test]
    fn power_shell_family_has_its_own_gate() {
        let shell = Family::Shell.gate();
        let power_shell = Family::PowerShell.gate();
        assert!(!std::ptr::eq(shell, power_shell));
        assert!(std::ptr::eq(power_shell, Family::PowerShell.gate()));
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
            long.push_str(&format!(
                "class C{index}:\n    def f(self):\n        return {index}\n\n"
            ));
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

    #[test]
    fn python_checker_flags_stray_closing_braces_in_format_strings() {
        if env_disabled() || python_interpreter().is_none() {
            return;
        }
        // 合法占位符、命名占位符与转义花括号都不能被误报。
        for source in [
            r#"s = '<path d="{:.1f},{:.1f}"/>'.format(1.0, 2.0)"#,
            r#"s = '<path d="{x:.1f}"/>'.format(x=1.0)"#,
            r#"s = '{{literal}}'.format()"#,
            // 不是 .format() 接收者的字符串里出现孤立花括号是合法的。
            r#"s = '}'.replace('}', '')"#,
        ] {
            assert_eq!(python(source).render(), "OK (py_compile)", "{source}");
        }
        // 会话里真实出现过的损坏形态：`M:.1f}` 开头的左花括号丢失。py_compile
        // 只查语法，这种字符串照样通过，必须在写入回执里被点出来。
        let broken = r#"s = '<path d="M:.1f},{:.1f}"/>'.format(1.0, 2.0)"#;
        let verdict = python(broken).render();
        assert!(verdict.starts_with("ERROR line 1:"), "{verdict}");
        assert!(has(&verdict, "stray '}'"), "{verdict}");
        assert!(has(&verdict, "(py_compile)"), "{verdict}");
    }

    #[test]
    fn checker_gate_serializes_and_degrades_instead_of_queueing() {
        // 用函数内静态锁验证语义，不去碰真实校验用的族闸门，免得并行测试互相排队。
        static GATE: Mutex<()> = Mutex::new(());
        let held = acquire_gate(&GATE, Budget::start()).expect("first acquire");
        let started = Instant::now();
        // 预算只有 40 ms：等不到就降级，而不是无限排队。
        let short = Budget {
            deadline: started + Duration::from_millis(40),
        };
        match acquire_gate(&GATE, short) {
            Ok(_) => panic!("闸门被占用时第二次取锁必须被拒绝，而不是排队等下去"),
            Err(RunError::Busy) => {}
            Err(_) => panic!("只允许 Busy 这一种拒绝"),
        }
        assert!(started.elapsed() >= Duration::from_millis(40));
        // 预算已经扣光时立刻降级，不再空等一轮轮询。
        let expired = Instant::now() - Duration::from_millis(1);
        assert!(matches!(
            acquire_gate(&GATE, Budget { deadline: expired }),
            Err(RunError::Busy)
        ));
        drop(held);
        assert!(acquire_gate(&GATE, Budget::start()).is_ok());
        // 降级文案：调用方拼成 `skipped (node checker busy)`，文件照常落盘。
        assert_eq!(RunError::Busy.reason(), "checker busy");
        assert_eq!(
            Verdict::skipped(
                "node --check",
                "node ".to_string() + RunError::Busy.reason()
            )
            .render(),
            "skipped (node checker busy)"
        );
    }

    #[test]
    fn checker_gate_does_not_block_across_interpreter_families() {
        // 回归护栏：曾被一次慢的 TypeScript 检查（node 族，加载 typescript 约 137 ms）
        // 拖成 `skipped (python checker busy)`。族闸门必须是各不相同的对象，
        // 分族才真的成立；这里只比较地址，不去抢真实闸门，免得干扰并行测试。
        let python = Family::Python.gate();
        let node = Family::Node.gate();
        let shell = Family::Shell.gate();
        assert!(!std::ptr::eq(python, node));
        assert!(!std::ptr::eq(node, shell));
        assert!(!std::ptr::eq(python, shell));
        // 同一族每次都必须拿到同一把锁，否则限流形同虚设。
        assert!(std::ptr::eq(python, Family::Python.gate()));
        assert!(std::ptr::eq(node, Family::Node.gate()));
        assert!(std::ptr::eq(shell, Family::Shell.gate()));
    }

    #[test]
    fn budget_shrinks_with_wall_clock_and_expires() {
        let budget = Budget::start();
        assert!(budget.remaining().is_some());
        assert!(budget.remaining().unwrap() <= SYNTAX_BUDGET);
        let expired = Budget {
            deadline: Instant::now(),
        };
        assert!(expired.remaining().is_none());
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
