//! Interlink tunnel for the resident 舵机 session (互通方案 §7.3, I9).
//!
//! The tunnel engine lives in the runtime (`wunder_server::interlink::client`);
//! this module is only the CLI's own surface: whether a 舵机 may open a link,
//! how its state is worded in the TUI, and what happens to a remote approval
//! when nothing can answer it. It starts no chain of its own and never keeps a
//! second client — the engine's process-wide singleton is the only one.

use crossterm::event::KeyCode;
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wunder_server::cloud::shared as cloud_shared;
use wunder_server::interlink::client::{
    self as tunnel, Decision, InterlinkLocalOptions, PendingApproval, TunnelState,
};

use crate::runtime::CliRuntime;

/// The status window the CLI already keeps for the cloud badge; the tunnel poll
/// rides it instead of adding a second timer.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Stable reason codes (docs §7.3); the server audits the same words.
pub const REASON_NO_SURFACE: &str = "NO_APPROVAL_SURFACE";
pub const REASON_DENY_ALL: &str = "DENY_ALL_POLICY";
pub const REASON_READONLY: &str = "READONLY_POLICY";

/// The tunnel as the TUI sees it. A CLI-local mirror of [`TunnelState`] so the
/// projection stays testable and carries a sensible default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TunnelPhase {
    Connecting,
    Connected,
    Reconnecting,
    #[default]
    Disabled,
}

impl TunnelPhase {
    pub fn from_engine(state: TunnelState) -> Self {
        match state {
            TunnelState::Connecting => Self::Connecting,
            TunnelState::Connected => Self::Connected,
            TunnelState::Reconnecting => Self::Reconnecting,
            TunnelState::Disabled => Self::Disabled,
        }
    }

    /// Machine-readable word, kept in one place so the four states never drift
    /// across the log, the thread center and the audit line.
    pub fn word(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Disabled => "disabled",
        }
    }

    pub fn label(self, is_zh: bool) -> &'static str {
        match self {
            Self::Connecting => {
                if is_zh {
                    "接入中"
                } else {
                    "connecting"
                }
            }
            Self::Connected => {
                if is_zh {
                    "已连接"
                } else {
                    "connected"
                }
            }
            Self::Reconnecting => {
                if is_zh {
                    "重连中"
                } else {
                    "reconnecting"
                }
            }
            Self::Disabled => {
                if is_zh {
                    "未启用"
                } else {
                    "disabled"
                }
            }
        }
    }
}

/// Process-level tunnel state for the thread center. `remote_pending_approvals`
/// counts prompts a remote node opened on this machine; it is deliberately not
/// the same field as a thread's own tool-approval count.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TunnelSummary {
    pub phase: TunnelPhase,
    pub remote_pending_approvals: usize,
    /// §10.2: the only sign a saturated tunnel degraded rather than blocked.
    pub dropped_frames: u64,
}

impl TunnelSummary {
    pub fn line(&self, is_zh: bool) -> String {
        tunnel_summary_line(
            self.phase,
            self.remote_pending_approvals,
            self.dropped_frames,
            is_zh,
        )
    }
}

/// One remote approval waiting for a keystroke here.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteApproval {
    pub approval_id: String,
    pub command_id: String,
    pub kind: String,
    pub level: String,
    pub risk: String,
    pub from_node: String,
    pub prompt: String,
    pub expires_at: f64,
}

impl RemoteApproval {
    fn from_engine(value: &PendingApproval) -> Self {
        Self {
            approval_id: value.approval_id.clone(),
            command_id: value.command_id.clone(),
            kind: value.kind.clone(),
            level: value.level.clone(),
            risk: value.risk.clone(),
            from_node: value.from_node.clone(),
            prompt: value.prompt.clone(),
            expires_at: value.expires_at,
        }
    }
}

/// Whether this process can put a human in front of a remote command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSurface {
    Interactive,
    None,
}

/// What to do with the prompt queue right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteApprovalAction {
    /// Keep the row on screen and wait for `y` / `n`.
    Prompt,
    /// Refuse with a stable reason code instead of burning the window.
    Deny(&'static str),
}

/// Nothing can start a link without a cloud session, and the local kill switch
/// outranks everything else (docs §3.3).
pub fn start_allowed(has_session: bool, config_enabled: bool) -> bool {
    has_session && config_enabled
}

pub fn remote_approval_action(surface: ApprovalSurface, approval_default: &str) -> RemoteApprovalAction {
    if surface == ApprovalSurface::Interactive {
        return RemoteApprovalAction::Prompt;
    }
    match approval_default.trim().to_ascii_lowercase().as_str() {
        "deny_all" => RemoteApprovalAction::Deny(REASON_DENY_ALL),
        "allow_readonly" => RemoteApprovalAction::Deny(REASON_READONLY),
        // `prompt` with nobody to prompt is still a refusal: fail closed (docs §14).
        _ => RemoteApprovalAction::Deny(REASON_NO_SURFACE),
    }
}

/// `y` approves, `n` denies. Enter and Esc keep their composer meaning so a
/// confirm row can never send a message or drop a round by accident.
pub fn remote_approval_choice(key: KeyCode) -> Option<Decision> {
    match key {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Approve),
        KeyCode::Char('n') | KeyCode::Char('N') => Some(Decision::Deny),
        _ => None,
    }
}

pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

/// Whole seconds left of the approval window; an expired window reads as zero.
pub fn seconds_left(expires_at: f64, now: f64) -> i64 {
    ((expires_at - now).ceil() as i64).max(0)
}

pub fn tunnel_summary_line(
    phase: TunnelPhase,
    remote_pending_approvals: usize,
    dropped_frames: u64,
    is_zh: bool,
) -> String {
    let mut parts = if is_zh {
        vec![format!("互通 {}", phase.label(true))]
    } else {
        vec![format!("tunnel {}", phase.label(false))]
    };
    if remote_pending_approvals > 0 {
        parts.push(if is_zh {
            format!("远程待审批 {remote_pending_approvals}")
        } else {
            format!("remote approvals {remote_pending_approvals}")
        });
    }
    // Congestion is silent unless the counter is surfaced (§10.2).
    if dropped_frames > 0 {
        parts.push(if is_zh {
            format!("丢帧 {dropped_frames}")
        } else {
            format!("dropped {dropped_frames}")
        });
    }
    parts.join(" · ")
}

/// The confirm row: source, tier, risk, parameter summary, window.
pub fn remote_approval_lines(approval: &RemoteApproval, now: f64, is_zh: bool) -> Vec<String> {
    let left = seconds_left(approval.expires_at, now).max(0);
    if is_zh {
        vec![
            format!(
                "[互通审批] {} · {} · 风险 {} · 来源 {}",
                approval.kind, approval.level, approval.risk, approval.from_node
            ),
            approval.prompt.clone(),
            format!("剩余 {left}s · y 批准 · n 拒绝（不记住本次决定）"),
        ]
    } else {
        vec![
            format!(
                "[interlink approval] {} · {} · risk {} · from {}",
                approval.kind, approval.level, approval.risk, approval.from_node
            ),
            approval.prompt.clone(),
            format!("{left}s left · y approve · n deny (never remembered)"),
        ]
    }
}

/// A refusal that has to reach the operator, not just the audit trail. The
/// command id is quoted so the line can be matched against the server ledger.
pub fn denied_notice(approval: &RemoteApproval, reason: &'static str, is_zh: bool) -> String {
    if is_zh {
        format!(
            "已拒绝远程命令 {}（{}），原因 {reason}",
            approval.kind, approval.command_id
        )
    } else {
        format!(
            "denied remote command {} ({}), reason {reason}",
            approval.kind, approval.command_id
        )
    }
}

/// What the operator's own keystroke did, so the transcript states the outcome
/// instead of implying it.
pub fn decision_notice(approval: &RemoteApproval, approved: bool, is_zh: bool) -> String {
    match (approved, is_zh) {
        (true, true) => format!("已批准远程命令 {}（{}，仅本次）", approval.kind, approval.from_node),
        (true, false) => {
            format!("approved remote command {} ({}, this one only)", approval.kind, approval.from_node)
        }
        (false, true) => format!("已拒绝远程命令 {}（{}）", approval.kind, approval.from_node),
        (false, false) => {
            format!("denied remote command {} ({})", approval.kind, approval.from_node)
        }
    }
}

/// The tunnel changed state: one line, in the operator's language.
pub fn phase_notice(previous: TunnelPhase, current: TunnelPhase, is_zh: bool) -> String {
    if is_zh {
        format!("互通隧道 {} → {}", previous.label(true), current.label(true))
    } else {
        format!("interlink tunnel {} → {}", previous.word(), current.word())
    }
}

/// Open the tunnel when this form may. A no-session start would only add an
/// idle poller, so the CLI refuses it here rather than in the engine.
pub async fn start(runtime: &CliRuntime) -> bool {
    let has_session = cloud_shared().session().is_some();
    let config_enabled = runtime.state.config_store.get().await.interlink.enabled;
    if !start_allowed(has_session, config_enabled) {
        return false;
    }
    let state = Arc::clone(&runtime.state);
    let options = InterlinkLocalOptions {
        local_user_id: runtime.user_id.clone(),
        workspace_id: None,
        session_base_dir: runtime.wunder_home.clone(),
    };
    // `start` takes a blocking lock, so it stays off the async worker threads.
    tokio::task::spawn_blocking(move || tunnel::start_with_options(state, options))
        .await
        .is_ok()
}

pub fn stop() {
    tunnel::stop();
}

pub fn snapshot() -> TunnelSummary {
    let status = tunnel::status();
    TunnelSummary {
        phase: TunnelPhase::from_engine(status.state),
        remote_pending_approvals: status.pending_approvals,
        dropped_frames: status.dropped_frames,
    }
}

pub fn pending_approvals() -> Vec<RemoteApproval> {
    tunnel::pending_approvals()
        .iter()
        .map(RemoteApproval::from_engine)
        .collect()
}

/// `remember` is pinned to false: the CLI must not open a slot the engine would
/// refuse anyway, and no remote grant should outlive this keystroke (§7.3 3).
pub fn decide(approval_id: &str, decision: Decision) -> bool {
    tunnel::decide_approval(approval_id, decision, false)
}

pub fn approval_surface() -> ApprovalSurface {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        ApprovalSurface::Interactive
    } else {
        ApprovalSurface::None
    }
}

pub fn approval_default() -> String {
    cloud_shared()
        .session()
        .map(|session| session.interlink.approval_policy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval() -> RemoteApproval {
        RemoteApproval {
            approval_id: "ap_1".to_string(),
            command_id: "cmd_1".to_string(),
            kind: "workspace.write",
            level: "L2".to_string(),
            risk: "high".to_string(),
            from_node: "node-b".to_string(),
            prompt: "write notes.txt".to_string(),
            expires_at: 1_000.0,
        }
    }

    #[test]
    fn a_session_and_the_local_switch_both_gate_the_link() {
        assert!(start_allowed(true, true));
        assert!(!start_allowed(false, true));
        assert!(!start_allowed(true, false));
        assert!(!start_allowed(false, false));
    }

    #[test]
    fn the_four_tunnel_states_have_distinct_words_and_labels() {
        let phases = [
            TunnelPhase::Connecting,
            TunnelPhase::Connected,
            TunnelPhase::Reconnecting,
            TunnelPhase::Disabled,
        ];
        let words: Vec<&str> = phases.iter().map(|phase| phase.word()).collect();
        let zh: Vec<&str> = phases.iter().map(|phase| phase.label(true)).collect();
        let en: Vec<&str> = phases.iter().map(|phase| phase.label(false)).collect();
        assert_eq!(words, vec!["connecting", "connected", "reconnecting", "disabled"]);
        assert_eq!(zh.len(), zh.iter().collect::<std::collections::HashSet<_>>().len());
        assert_eq!(en.len(), en.iter().collect::<std::collections::HashSet<_>>().len());
        for phase in phases {
            assert_eq!(
                TunnelPhase::from_engine(match phase {
                    TunnelPhase::Connecting => TunnelState::Connecting,
                    TunnelPhase::Connected => TunnelState::Connected,
                    TunnelPhase::Reconnecting => TunnelState::Reconnecting,
                    TunnelPhase::Disabled => TunnelState::Disabled,
                }),
                phase
            );
        }
    }

    #[test]
    fn an_unanswered_surface_denies_and_keeps_a_stable_reason() {
        assert_eq!(
            remote_approval_action(ApprovalSurface::Interactive, "prompt"),
            RemoteApprovalAction::Prompt
        );
        assert_eq!(
            remote_approval_action(ApprovalSurface::None, "prompt"),
            RemoteApprovalAction::Deny(REASON_NO_SURFACE)
        );
        assert_eq!(
            remote_approval_action(ApprovalSurface::None, "deny_all"),
            RemoteApprovalAction::Deny(REASON_DENY_ALL)
        );
        assert_eq!(
            remote_approval_action(ApprovalSurface::None, "allow_readonly"),
            RemoteApprovalAction::Deny(REASON_READONLY)
        );
        // An unknown word is not permission: it lands on the fail-closed arm.
        assert_eq!(
            remote_approval_action(ApprovalSurface::None, "always_yes"),
            RemoteApprovalAction::Deny(REASON_NO_SURFACE)
        );
    }

    #[test]
    fn only_y_and_n_answer_the_row() {
        assert_eq!(remote_approval_choice(KeyCode::Char('y')), Some(Decision::Approve));
        assert_eq!(remote_approval_choice(KeyCode::Char('Y')), Some(Decision::Approve));
        assert_eq!(remote_approval_choice(KeyCode::Char('n')), Some(Decision::Deny));
        assert_eq!(remote_approval_choice(KeyCode::Char('N')), Some(Decision::Deny));
        assert_eq!(remote_approval_choice(KeyCode::Enter), None);
        assert_eq!(remote_approval_choice(KeyCode::Esc), None);
        assert_eq!(remote_approval_choice(KeyCode::Char('a')), None);
    }

    #[test]
    fn the_countdown_never_runs_below_zero() {
        assert_eq!(seconds_left(1_000.0, 940.0), 60);
        assert_eq!(seconds_left(1_000.0, 1_000.0), 0);
        assert_eq!(seconds_left(1_000.0, 1_200.0), 0);
        let lines = remote_approval_lines(&approval(), 1_200.0, true);
        assert!(lines[2].contains("剩余 0s"), "{}", lines[2]);
    }

    #[test]
    fn the_row_carries_source_tier_risk_prompt_and_window() {
        let zh = remote_approval_lines(&approval(), 940.0, true);
        assert_eq!(zh.len(), 3);
        for needle in ["workspace.write", "L2", "high", "node-b", "write notes.txt", "剩余 60s"] {
            assert!(zh.iter().any(|line| line.contains(needle)), "{needle} missing");
        }
        let en = remote_approval_lines(&approval(), 940.0, false);
        assert!(en.iter().any(|line| line.contains("60s left")));
        assert!(en.iter().any(|line| line.contains("node-b")));
    }

    #[test]
    fn the_summary_shows_congestion_and_pending_only_when_nonzero() {
        let quiet = tunnel_summary_line(TunnelPhase::Connected, 0, 0, true);
        assert_eq!(quiet, "互通 已连接");
        let noisy = tunnel_summary_line(TunnelPhase::Reconnecting, 2, 7, true);
        assert_eq!(noisy, "互通 重连中 · 远程待审批 2 · 丢帧 7");
        let english = tunnel_summary_line(TunnelPhase::Connected, 1, 3, false);
        assert_eq!(english, "tunnel connected · remote approvals 1 · dropped 3");
    }

    #[test]
    fn a_refusal_names_the_command_and_the_reason() {
        let notice = denied_notice(&approval(), REASON_NO_SURFACE, true);
        assert!(notice.contains("workspace.write"));
        assert!(notice.contains(REASON_NO_SURFACE));
        let english = denied_notice(&approval(), REASON_NO_SURFACE, false);
        assert!(english.contains("denied remote command"));
        assert!(english.contains(REASON_NO_SURFACE));
    }

    #[test]
    fn the_default_projection_is_a_quiet_disabled_tunnel() {
        let summary = TunnelSummary::default();
        assert_eq!(summary.phase, TunnelPhase::Disabled);
        assert_eq!(summary.remote_pending_approvals, 0);
        assert_eq!(summary.dropped_frames, 0);
        assert_eq!(summary.line(true), "互通 未启用");
    }

    #[test]
    fn a_state_change_and_a_keypress_are_both_reported() {
        let notice = phase_notice(TunnelPhase::Connecting, TunnelPhase::Connected, true);
        assert_eq!(notice, "互通隧道 接入中 → 已连接");
        assert_eq!(
            phase_notice(TunnelPhase::Connected, TunnelPhase::Reconnecting, false),
            "interlink tunnel connected → reconnecting"
        );
        let approved = decision_notice(&approval(), true, true);
        assert!(approved.contains("workspace.write") && approved.contains("仅本次"), "{approved}");
        assert!(decision_notice(&approval(), false, false).starts_with("denied remote command"));
    }
}
