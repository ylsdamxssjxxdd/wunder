//! 同一线程内的「读文件回执」：重复读同一段内容时不再第二次返回正文。
//!
//! 判定不看请求参数，而看**这次要发给模型的字节**的摘要：范围写法、输出预算、
//! 文件改动、编码解码都会改变摘要，因此摘要相同就等价于「这段话模型已经收到
//! 过」。这样不需要猜测模型读了什么，也不会因为 mtime 精度或写盘时间戳没变而
//! 误判「文件没改」。
//!
//! 回执的失效条件：
//! - 会话发生上下文压缩（旧工具结果被摘要替换）→ [`invalidate_session`]；
//! - 这次读取被 token 预算截断或被转存成 tool-result 句柄（正文没完整进上下文）
//!   → 同样 [`invalidate_session`]；
//! - 跨了两个及以上用户轮次（更早的历史不保证还在请求里）；
//! - 容量淘汰（全局 512 条、每会话 32 条），内存占用有界。

use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// 全局回执上限：一条回执只有几十字节，512 条不足以影响内存，却足以覆盖
/// 一个长任务里同时被反复读取的文件。
const MAX_RECEIPTS: usize = 512;
/// 单会话上限：一个线程里反复读的文件通常只有几个，超出说明在读不同的文件，
/// 此时保留最早的几条已无意义。
const MAX_RECEIPTS_PER_SESSION: usize = 32;

/// 读取发生在哪个逻辑时刻。「模型轮次」在一次用户轮次内从 1 递增，
/// 用户轮次在每个线程内单调递增，因此这对值足以判断「读得还算不算近」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadClock {
    pub(crate) user_round: i64,
    pub(crate) model_round: i64,
}

impl ReadClock {
    /// 没有轮次身份时（沙箱执行、基准脚本）返回 `None`：这类调用既无法判断
    /// “读得近不近”，也收不到压缩失效通知，宁可不复用。
    pub(crate) fn from_rounds(user_round: Option<i64>, model_round: Option<i64>) -> Option<Self> {
        Some(Self {
            user_round: user_round?,
            model_round: model_round?,
        })
    }

    /// 同一用户轮次内的历史一定还在这次请求里；紧邻的下一轮也还在（压缩会
    /// 单独作废回执）。再往前的轮次不再复用，避免回执活过它指向的正文。
    fn still_nearby(self, earlier: Self) -> bool {
        self.user_round.saturating_sub(earlier.user_round) <= 1
    }
}

/// 命中的回执：只回给调用方用于拼提示和统计，不包含正文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReceiptHit {
    pub(crate) user_round: i64,
    pub(crate) model_round: i64,
    pub(crate) body_bytes: usize,
}

#[derive(Clone)]
struct Receipt {
    session_id: String,
    path: String,
    digest: [u8; 16],
    body_bytes: usize,
    clock: ReadClock,
}

static RECEIPTS: OnceLock<Mutex<VecDeque<Receipt>>> = OnceLock::new();

fn store() -> &'static Mutex<VecDeque<Receipt>> {
    RECEIPTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 正文摘要。截断到 128 位足够区分同一文件的不同窗口，碰撞概率远低于
/// 「模型恰好把两段不同内容当成同一段」的代价。
pub(crate) fn body_digest(body: &str) -> [u8; 16] {
    let digest = Sha256::digest(body.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

/// 查这次正文是否已经原样发给过模型。
pub(crate) fn lookup(
    session_id: &str,
    path: &str,
    digest: &[u8; 16],
    now: ReadClock,
) -> Option<ReceiptHit> {
    if session_id.is_empty() || path.is_empty() {
        return None;
    }
    let store = store().lock().ok()?;
    store
        .iter()
        .rev()
        .find(|receipt| {
            receipt.session_id == session_id
                && receipt.path == path
                && &receipt.digest == digest
                && now.still_nearby(receipt.clock)
        })
        .map(|receipt| ReceiptHit {
            user_round: receipt.clock.user_round,
            model_round: receipt.clock.model_round,
            body_bytes: receipt.body_bytes,
        })
}

/// 记录一次确实完整送到了模型上下文里的正文。
pub(crate) fn record(
    session_id: &str,
    path: &str,
    digest: [u8; 16],
    body_bytes: usize,
    clock: ReadClock,
) {
    if session_id.is_empty() || path.is_empty() {
        return;
    }
    let Ok(mut store) = store().lock() else {
        return;
    };
    store.push_back(Receipt {
        session_id: session_id.to_string(),
        path: path.to_string(),
        digest,
        body_bytes,
        clock,
    });
    // 同一会话读同一个文件的新窗口时，旧窗口仍然有效（模型可能只回看其中一段），
    // 所以这里只做容量裁剪，不按路径去重。
    let mut per_session = store
        .iter()
        .filter(|receipt| receipt.session_id == session_id)
        .count();
    while store.len() > MAX_RECEIPTS || per_session > MAX_RECEIPTS_PER_SESSION {
        let Some(oldest) = store.pop_front() else {
            break;
        };
        if oldest.session_id == session_id {
            per_session = per_session.saturating_sub(1);
        }
    }
}

/// 上下文丢掉旧工具结果（压缩、结果转存、观测被截断）后，整条线程的回执一起
/// 作废：此时再声称「正文还在上下文里」只会让模型去引用一段已经不存在的文字。
pub(crate) fn invalidate_session(session_id: &str) -> usize {
    if session_id.is_empty() {
        return 0;
    }
    let Ok(mut store) = store().lock() else {
        return 0;
    };
    let before = store.len();
    store.retain(|receipt| receipt.session_id != session_id);
    before - store.len()
}

/// 线程结束/删除时清空，避免会话 id 复用时把上一轮的正文当成这一轮的。
pub(crate) fn forget_session(session_id: &str) {
    let _ = invalidate_session(session_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(user_round: i64, model_round: i64) -> ReadClock {
        ReadClock {
            user_round,
            model_round,
        }
    }

    /// 测试共享同一个进程级存储，因此每个用例都用独立 session id，
    /// 不做全局清空（并行执行时清空会互相打断）。
    #[test]
    fn same_body_hits_within_the_same_turn() {
        let digest = body_digest("alpha\nbeta\n");
        assert!(lookup("s-same", "a.txt", &digest, clock(3, 1)).is_none());
        record("s-same", "a.txt", digest, 11, clock(3, 1));
        let hit = lookup("s-same", "a.txt", &digest, clock(3, 7)).expect("same turn");
        assert_eq!(hit.model_round, 1);
        assert_eq!(hit.body_bytes, 11);
    }

    #[test]
    fn different_body_or_path_misses() {
        record("s-diff", "a.txt", body_digest("alpha\n"), 6, clock(1, 1));
        assert!(lookup("s-diff", "a.txt", &body_digest("beta\n"), clock(1, 2)).is_none());
        assert!(lookup("s-diff", "b.txt", &body_digest("alpha\n"), clock(1, 2)).is_none());
        assert!(lookup("s-other", "a.txt", &body_digest("alpha\n"), clock(1, 2)).is_none());
    }

    #[test]
    fn receipt_survives_one_turn_but_not_two() {
        let digest = body_digest("gamma\n");
        record("s-turn", "a.txt", digest, 6, clock(2, 5));
        assert!(lookup("s-turn", "a.txt", &digest, clock(3, 1)).is_some());
        assert!(lookup("s-turn", "a.txt", &digest, clock(4, 1)).is_none());
    }

    #[test]
    fn compaction_invalidates_only_that_session() {
        let digest = body_digest("delta\n");
        record("s-keep", "a.txt", digest, 6, clock(1, 1));
        record("s-drop", "a.txt", digest, 6, clock(1, 1));
        assert_eq!(invalidate_session("s-drop"), 1);
        assert!(lookup("s-drop", "a.txt", &digest, clock(1, 2)).is_none());
        assert!(lookup("s-keep", "a.txt", &digest, clock(1, 2)).is_some());
    }

    #[test]
    fn per_session_capacity_is_bounded() {
        let session = "s-capacity";
        for index in 0..(MAX_RECEIPTS_PER_SESSION + 8) {
            record(
                session,
                &format!("file-{index}.txt"),
                body_digest(&index.to_string()),
                index,
                clock(1, 1),
            );
        }
        let kept = store()
            .lock()
            .expect("store")
            .iter()
            .filter(|receipt| receipt.session_id == session)
            .count();
        assert_eq!(kept, MAX_RECEIPTS_PER_SESSION);
    }
}
