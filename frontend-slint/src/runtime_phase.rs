//! One decision point for "what does this thread's status mean".
//!
//! Every screen used to read the status text with its own prefix table, so a
//! phrase that one table knew (`请求中`, `模型响应中`, `上下文压缩中…`,
//! `模型请求正在排队`) fell through to "idle" in another. A running thread then
//! showed an idle window title while its work process was still streaming.
//!
//! Callers that own the transition now pass a phase explicitly, so the text may
//! stay free-form and a forgotten phrase cannot demote a live thread. Only
//! foreign text — the durable catalogue, a cloud row, a stored message state —
//! goes through [`Phase::of`], which is one table for the whole app.

/// What the thread is doing right now, independent of the phrase shown.
///
/// The discriminants are the Slint ABI: `ui/runtime_status.slint` maps the same
/// numbers to glyphs, so keep both sides in step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Nothing pending: the row shows the idle pause glyph.
    #[default]
    Idle = 0,
    /// Accepted but not executing yet (queue position, admission wait).
    Pending = 1,
    /// The engine owns the turn: request, model round, tool, compaction.
    Running = 2,
    /// The turn is parked on the user (question, approval).
    Inquiry = 3,
    /// Settled with an answer.
    Done = 4,
    /// Settled without an answer (failure, stop, rejection).
    Error = 5,
}

impl Phase {
    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn from_code(code: i32) -> Phase {
        match code {
            1 => Phase::Pending,
            2 => Phase::Running,
            3 => Phase::Inquiry,
            4 => Phase::Done,
            5 => Phase::Error,
            _ => Phase::Idle,
        }
    }

    /// True while the engine still owes the user work. A running or queued
    /// thread must never be presented as idle, and vice versa.
    pub fn live(self) -> bool {
        matches!(self, Phase::Pending | Phase::Running | Phase::Inquiry)
    }

    /// Classify a status phrase this process did not author: a durable
    /// catalogue value, a cloud row, or a message state.
    ///
    /// Keep every phrase the front end writes in the tables below; the tests
    /// enumerate them so a new phrase cannot be added without a phase.
    pub fn of(status: &str) -> Phase {
        let value = status.trim();
        if value.is_empty() {
            return Phase::Idle;
        }
        match value {
            "idle" | "ready" | "none" => return Phase::Idle,
            "pending" | "queued" | "waiting" | "waiting_approval" => return Phase::Pending,
            "waiting_input"
            | "waiting_user_input"
            | "pending_question"
            | "pending_confirm"
            | "awaiting_confirmation"
            | "awaiting_approval"
            | "approval_pending"
            | "question"
            | "asking" => return Phase::Inquiry,
            "running" | "streaming" | "executing" | "processing" | "in_progress" | "active" => {
                return Phase::Running
            }
            "done" | "finished" | "completed" | "success" | "completed_ok" => return Phase::Done,
            "error" | "failed" | "failure" | "system_error" | "rejected" | "timeout"
            | "aborted" | "terminated" | "cancelled" | "canceled" | "stopped" | "interrupted" => {
                return Phase::Error
            }
            _ => {}
        }
        // Phrase families the engine and this front end emit. Order matters:
        // a waiting phrase outranks the `正在` family it may contain.
        if value.starts_with("等待用户输入")
            || value.starts_with("等待回答")
            || value.starts_with("等待确认")
            || value.starts_with("等待审批")
        {
            return Phase::Inquiry;
        }
        if value.starts_with("等待") || value.contains("排队") {
            return Phase::Pending;
        }
        if value.starts_with("任务完成") || value.starts_with("输出已结束") || value == "已完成"
        {
            return Phase::Done;
        }
        if value.starts_with("执行失败")
            || value.contains("失败")
            || value.contains("错误")
            || value.contains("超时")
            || value.contains("中断")
            || value.contains("取消")
            || value.contains("已停止")
        {
            return Phase::Error;
        }
        // `请求中` is the admission phrase: the engine has the turn, so it is
        // running, not queued. This is the case the old tables missed.
        if value == "请求中"
            || value.starts_with("请求")
            || value.starts_with("正在")
            || value.starts_with("目标")
            || value.starts_with("模型响应")
            || value.contains("压缩中")
        {
            return Phase::Running;
        }
        Phase::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every phrase this front end writes as a thread status. A new phrase
    /// belongs in this table with its phase, so no view has to guess.
    #[test]
    fn every_phrase_this_front_end_writes_has_a_live_phase() {
        for phrase in [
            "请求中",
            "正在生成…",
            "正在思考…",
            "正在执行工具…",
            "模型响应中",
            "目标继续执行…",
            "上下文压缩中…",
            "正在排队",
            "任务已排队…",
            "模型请求正在排队",
            "模型请求正在排队 · 前方 2 个任务",
        ] {
            assert!(
                Phase::of(phrase).live(),
                "{phrase} must read as a live thread, not idle"
            );
        }
    }

    #[test]
    fn a_running_request_is_running_not_queued() {
        assert_eq!(Phase::of("请求中"), Phase::Running);
        assert_eq!(Phase::of("正在排队"), Phase::Pending);
        assert_eq!(
            Phase::of("模型请求正在排队 · 前方 3 个任务"),
            Phase::Pending
        );
    }

    #[test]
    fn terminal_phrases_are_terminal() {
        assert_eq!(Phase::of("任务完成"), Phase::Done);
        assert_eq!(Phase::of("输出已结束"), Phase::Done);
        assert_eq!(Phase::of("执行失败"), Phase::Error);
        assert_eq!(Phase::of("已停止"), Phase::Error);
        assert_eq!(Phase::of(""), Phase::Idle);
        assert_eq!(Phase::of("idle"), Phase::Idle);
    }

    #[test]
    fn waiting_phrases_park_the_turn_on_the_user() {
        for phrase in ["等待用户输入", "waiting_input", "等待确认", "等待审批"] {
            let phase = Phase::of(phrase);
            assert!(phase.live(), "{phrase} must not read as idle");
            assert_ne!(phase, Phase::Running, "{phrase} is a user wait");
        }
    }
}
