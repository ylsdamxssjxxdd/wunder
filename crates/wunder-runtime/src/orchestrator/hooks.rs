//! 工具生命周期钩子：`HookRegistry` + dsh 风格的事件名。
//!
//! 对齐 dsh `hooks/hook-protocol` 的最小实现：由 `config.hooks` 声明命令钩子，
//! 按事件名 + 工具名匹配；管线在工具执行前后触发，并发出
//! `hook/invoked` / `hook/result` 事件（见 `orchestrator::execute_tools`）。
//!
//! 默认关闭：`config.hooks.enabled = false` 或没有任何匹配钩子时，
//! 注册表为空、不产生任何事件与额外开销。

use crate::config::{Config, HookDefinition};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;

/// 管线已接入的钩子事件，命名与 dsh 的 CC/Codex 兼容钩子点对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// 工具执行前（dsh `PreToolUse`）。
    PreToolUse,
    /// 工具执行后（dsh `PostToolUse`）。
    PostToolUse,
}

impl HookEvent {
    /// 规范事件名（事件载荷与配置匹配都用它）。
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
        }
    }

    /// 解析配置里的 `event` 字段（大小写 / `_` / `-` / 空格不敏感）。
    fn parse(name: &str) -> Option<Self> {
        let key: String = name
            .trim()
            .chars()
            .filter(|c| !matches!(c, '_' | '-' | ' '))
            .flat_map(|c| c.to_lowercase())
            .collect();
        match key.as_str() {
            "pretooluse" => Some(HookEvent::PreToolUse),
            "posttooluse" => Some(HookEvent::PostToolUse),
            _ => None,
        }
    }
}

/// 一条已解析的命令钩子。
#[derive(Debug, Clone)]
pub struct ResolvedHook {
    event: HookEvent,
    /// 工具名匹配模式（支持 `*` 通配）；为空表示匹配所有工具。
    matchers: Vec<String>,
    command: String,
    timeout_ms: u64,
}

impl ResolvedHook {
    fn from_definition(def: &HookDefinition) -> Option<Self> {
        let event = HookEvent::parse(&def.event)?;
        let command = def.command.trim().to_string();
        if command.is_empty() {
            return None;
        }
        let timeout_ms = def.timeout_ms.filter(|v| *v > 0).unwrap_or(60_000);
        Some(Self {
            event,
            matchers: def.matcher.clone(),
            command,
            timeout_ms,
        })
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    fn matches(&self, tool_name: &str) -> bool {
        if self.matchers.is_empty() {
            return true;
        }
        self.matchers
            .iter()
            .any(|pattern| glob_match(pattern, tool_name))
    }

    /// 运行命令钩子：把工具上下文以 JSON 经 stdin 传入，捕获 stdout / stderr，
    /// 超时则终止进程。
    pub async fn run(&self, input: &Value) -> HookRunOutcome {
        let started = Instant::now();
        let mut command = shell_command(&self.command);
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                return HookRunOutcome {
                    exit_code: None,
                    timed_out: false,
                    spawn_error: Some(err.to_string()),
                    stdout: String::new(),
                    stderr: String::new(),
                    duration_ms: started.elapsed().as_millis() as u64,
                };
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let payload = serde_json::to_vec(input).unwrap_or_default();
            tokio::spawn(async move {
                let _ = stdin.write_all(&payload).await;
                let _ = stdin.shutdown().await;
            });
        }

        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buf).await;
            }
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buf).await;
            }
            buf
        });

        let wait = tokio::time::timeout(Duration::from_millis(self.timeout_ms), child.wait()).await;
        let (exit_code, timed_out, spawn_error) = match wait {
            Ok(Ok(status)) => (status.code(), false, None),
            Ok(Err(err)) => (None, false, Some(err.to_string())),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                (None, true, None)
            }
        };

        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();

        HookRunOutcome {
            exit_code,
            timed_out,
            spawn_error,
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }
}

/// 命令钩子的一次运行结果。
#[derive(Debug, Clone)]
pub struct HookRunOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// 声明式命令钩子注册表。
#[derive(Debug, Clone, Default)]
pub struct HookRegistry {
    enabled: bool,
    hooks: Vec<ResolvedHook>,
}

impl HookRegistry {
    /// 未启用 / 无钩子的空注册表。
    pub fn disabled() -> Self {
        Self::default()
    }

    /// 从配置构建；`hooks.enabled = false` 时返回空注册表。
    pub fn from_config(config: &Config) -> Self {
        if !config.hooks.enabled {
            return Self::default();
        }
        let hooks = config
            .hooks
            .hooks
            .iter()
            .filter_map(ResolvedHook::from_definition)
            .collect();
        Self {
            enabled: true,
            hooks,
        }
    }

    /// 是否有生效的钩子（启用且至少一条）。
    pub fn is_active(&self) -> bool {
        self.enabled && !self.hooks.is_empty()
    }

    /// 返回匹配该事件与工具名的钩子（保持配置顺序）。
    pub fn matching(&self, event: HookEvent, tool_name: &str) -> Vec<&ResolvedHook> {
        if !self.is_active() {
            return Vec::new();
        }
        self.hooks
            .iter()
            .filter(|hook| hook.event == event && hook.matches(tool_name))
            .collect()
    }
}

/// 极简 glob 匹配：支持前缀 `*`、后缀 `*`、双侧 `*`（子串）与精确匹配。
fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    let starts = pattern.starts_with('*');
    let ends = pattern.ends_with('*');
    let core = pattern.trim_matches('*');
    match (starts, ends) {
        (true, true) => value.contains(core),
        (true, false) => value.ends_with(core),
        (false, true) => value.starts_with(core),
        (false, false) => value == pattern,
    }
}

fn shell_command(command: &str) -> TokioCommand {
    #[cfg(windows)]
    {
        let mut cmd = TokioCommand::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = TokioCommand::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_case_and_separator_insensitive() {
        assert_eq!(HookEvent::parse("PreToolUse"), Some(HookEvent::PreToolUse));
        assert_eq!(
            HookEvent::parse("pre_tool_use"),
            Some(HookEvent::PreToolUse)
        );
        assert_eq!(
            HookEvent::parse("POST-TOOL-USE"),
            Some(HookEvent::PostToolUse)
        );
        assert_eq!(HookEvent::parse("nope"), None);
    }

    #[test]
    fn disabled_by_default() {
        let registry = HookRegistry::from_config(&Config::default());
        assert!(!registry.is_active());
        assert!(registry
            .matching(HookEvent::PreToolUse, "读取文件")
            .is_empty());
    }

    #[test]
    fn matching_honours_matchers_and_default_all() {
        let config: Config = serde_yaml::from_str(
            r#"
hooks:
  enabled: true
  hooks:
    - event: PreToolUse
      matcher: ["write*", "文本编辑"]
      command: echo hi
    - event: PreToolUse
      command: echo all
    - event: post_tool_use
      matcher: ["*"]
      command: echo post
"#,
        )
        .expect("config");
        let registry = HookRegistry::from_config(&config);
        assert!(registry.is_active());
        // "write*" 不匹配中文名；无 matcher 的一条一律匹配
        let pre_write = registry.matching(HookEvent::PreToolUse, "写入文件");
        assert_eq!(pre_write.len(), 1);
        assert_eq!(pre_write[0].command(), "echo all");
        // "文本编辑" 命中 matcher，叠加无 matcher 的一条
        let pre_edit = registry.matching(HookEvent::PreToolUse, "文本编辑");
        assert_eq!(pre_edit.len(), 2);
        // 事件名大小写不敏感
        let post = registry.matching(HookEvent::PostToolUse, "文本编辑");
        assert_eq!(post.len(), 1);
    }

    #[test]
    fn glob_match_variants() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("write*", "write_file"));
        assert!(glob_match("*file", "read_file"));
        assert!(glob_match("*tool*", "my_tool_x"));
        assert!(glob_match("glob", "glob"));
        assert!(!glob_match("glob", "glog"));
        assert!(!glob_match("  ", "glob"));
    }
}
