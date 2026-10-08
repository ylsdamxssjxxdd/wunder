//! 工具执行管线的显式阶段模型（对齐 dsh `core/tools` 的瀑布流）。
//!
//! dsh 把一次工具调用建模为四阶段瀑布流：
//! `pre-execute` → `execute` → `post-execute` → `result`。
//! wunder 此前把这四段全部内联在 `execute_tools.rs` 的单个闭包里，缺少可复用的
//! 阶段边界与决策类型。本模块提供：
//!
//! * [`ToolPipelineStage`]：四阶段的规范化命名与顺序（用于事件载荷与文档）。
//! * [`PreToolDecision`]：pre-execute 阶段的 `allow` / `deny` 决策（对齐 dsh
//!   `PreToolDecision` 的核心子集）。
//! * [`PostToolDecision`]：post-execute 阶段的 `accept` / `block` 决策（对齐 dsh
//!   `PostToolDecision` 的核心子集）。
//!
//! 决策来源是命令钩子的运行结果：任一钩子失败（非零退出 / 超时 / 无法启动）即
//! fail-closed 地拒绝或阻断。钩子默认关闭（`config.hooks.enabled = false`），
//! 因此默认配置下恒为 `Allow` / `Accept`，不改变既有行为。

use super::hooks::{HookEvent, HookRunOutcome};

/// dsh 风格的工具执行管线阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPipelineStage {
    /// 执行前：策略判定、审批、pre 钩子。
    PreExecute,
    /// 执行：真正调用工具（含并行守卫与超时）。
    Execute,
    /// 执行后：工作区版本比对、post 钩子。
    PostExecute,
    /// 收口：结果规范化、截断、事件发射。
    Result,
}

impl ToolPipelineStage {
    /// 完整阶段顺序（对齐 dsh 瀑布流）。
    pub const ORDER: [ToolPipelineStage; 4] = [
        ToolPipelineStage::PreExecute,
        ToolPipelineStage::Execute,
        ToolPipelineStage::PostExecute,
        ToolPipelineStage::Result,
    ];

    /// 规范阶段名（事件载荷与文档都用它）。
    pub fn as_str(self) -> &'static str {
        match self {
            ToolPipelineStage::PreExecute => "pre-execute",
            ToolPipelineStage::Execute => "execute",
            ToolPipelineStage::PostExecute => "post-execute",
            ToolPipelineStage::Result => "result",
        }
    }

    /// 阶段序号（0-based），等于其在 [`ToolPipelineStage::ORDER`] 中的位置。
    pub fn index(self) -> usize {
        match self {
            ToolPipelineStage::PreExecute => 0,
            ToolPipelineStage::Execute => 1,
            ToolPipelineStage::PostExecute => 2,
            ToolPipelineStage::Result => 3,
        }
    }

    /// 钩子事件所归属的阶段。
    pub fn for_hook_event(event: HookEvent) -> Self {
        match event {
            HookEvent::PreToolUse => ToolPipelineStage::PreExecute,
            HookEvent::PostToolUse => ToolPipelineStage::PostExecute,
        }
    }
}

/// pre-execute 阶段决策（dsh `PreToolDecision` 的 `allow` / `deny` 子集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreToolDecision {
    /// 放行，继续后续阶段。
    Allow,
    /// 拒绝：跳过执行，直接产出错误结果。
    Deny { reason: String },
}

impl PreToolDecision {
    pub fn allow() -> Self {
        Self::Allow
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    /// 由钩子运行结果推导：任一钩子失败即 fail-closed 拒绝。
    pub fn from_hook_outcomes(outcomes: &[HookRunOutcome]) -> Self {
        for outcome in outcomes {
            if let Some(reason) = hook_failure_reason(outcome) {
                return Self::deny(reason);
            }
        }
        Self::Allow
    }
}

/// post-execute 阶段决策（dsh `PostToolDecision` 的 `accept` / `block` 子集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostToolDecision {
    /// 接受工具结果。
    Accept,
    /// 阻断：用反馈替换工具结果。
    Block { reason: String },
}

impl PostToolDecision {
    pub fn accept() -> Self {
        Self::Accept
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self::Block {
            reason: reason.into(),
        }
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Block { .. })
    }

    /// 由钩子运行结果推导：任一钩子失败即 fail-closed 阻断。
    pub fn from_hook_outcomes(outcomes: &[HookRunOutcome]) -> Self {
        for outcome in outcomes {
            if let Some(reason) = hook_failure_reason(outcome) {
                return Self::block(reason);
            }
        }
        Self::Accept
    }
}

/// 钩子失败原因；成功返回 `None`。优先回报 stderr，其次 stdout。
fn hook_failure_reason(outcome: &HookRunOutcome) -> Option<String> {
    if let Some(err) = outcome.spawn_error.as_ref() {
        return Some(format!("hook failed to start: {err}"));
    }
    if outcome.timed_out {
        return Some("hook timed out".to_string());
    }
    match outcome.exit_code {
        Some(0) => None,
        Some(code) => Some(if !outcome.stderr.is_empty() {
            outcome.stderr.clone()
        } else if !outcome.stdout.is_empty() {
            outcome.stdout.clone()
        } else {
            format!("hook exited with code {code}")
        }),
        None => Some("hook terminated without an exit code".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(
        exit_code: Option<i32>,
        timed_out: bool,
        spawn_error: Option<&str>,
    ) -> HookRunOutcome {
        HookRunOutcome {
            exit_code,
            timed_out,
            spawn_error: spawn_error.map(ToString::to_string),
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 1,
        }
    }

    #[test]
    fn stage_order_matches_dsh() {
        let names: Vec<&str> = ToolPipelineStage::ORDER
            .iter()
            .map(|stage| stage.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["pre-execute", "execute", "post-execute", "result"]
        );
        assert_eq!(ToolPipelineStage::PreExecute.index(), 0);
        assert_eq!(ToolPipelineStage::Result.index(), 3);
        assert_eq!(
            ToolPipelineStage::for_hook_event(HookEvent::PreToolUse),
            ToolPipelineStage::PreExecute
        );
        assert_eq!(
            ToolPipelineStage::for_hook_event(HookEvent::PostToolUse),
            ToolPipelineStage::PostExecute
        );
    }

    #[test]
    fn pre_decision_defaults_to_allow() {
        assert_eq!(
            PreToolDecision::from_hook_outcomes(&[]),
            PreToolDecision::Allow
        );
        assert_eq!(
            PreToolDecision::from_hook_outcomes(&[outcome(Some(0), false, None)]),
            PreToolDecision::Allow
        );
    }

    #[test]
    fn pre_decision_denies_on_any_hook_failure() {
        assert!(PreToolDecision::from_hook_outcomes(&[outcome(Some(2), false, None)]).is_denied());
        assert!(PreToolDecision::from_hook_outcomes(&[outcome(None, true, None)]).is_denied());
        assert!(
            PreToolDecision::from_hook_outcomes(&[outcome(None, false, Some("boom"))]).is_denied()
        );
    }

    #[test]
    fn post_decision_defaults_to_accept_and_blocks_on_failure() {
        assert_eq!(
            PostToolDecision::from_hook_outcomes(&[]),
            PostToolDecision::Accept
        );
        assert!(
            PostToolDecision::from_hook_outcomes(&[outcome(Some(1), false, None)]).is_blocked()
        );
    }

    #[test]
    fn denial_reason_prefers_stderr_then_stdout() {
        let mut stderr_outcome = outcome(Some(2), false, None);
        stderr_outcome.stderr = "blocked: forbidden".to_string();
        assert_eq!(
            PreToolDecision::from_hook_outcomes(&[stderr_outcome]),
            PreToolDecision::deny("blocked: forbidden")
        );

        let mut stdout_outcome = outcome(Some(2), false, None);
        stdout_outcome.stdout = "rationale text".to_string();
        assert_eq!(
            PostToolDecision::from_hook_outcomes(&[stdout_outcome]),
            PostToolDecision::block("rationale text")
        );
    }
}
