//! Bulk synthetic thread generation for message-rendering stress tests.
//!
//! Writes the same SQLite schema the runtime reads, but bypasses the per-event
//! commit path on purpose: one bulk connection with relaxed journal settings
//! and one transaction per user turn. Timestamps are simulated backwards from
//! "now" so the generated thread looks like it happened in the recent past.

use super::SqliteStorage;
use anyhow::{ensure, Result};
use chrono::{Local, TimeZone};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;
use wunder_core::storage_backend::StorageLifecycle;

/// Hard cap protecting the live database from an unreasonable generation job.
pub const MAX_USER_ROUNDS: i64 = 2000;
pub const MAX_MODEL_ROUNDS: i64 = 2000;
/// 1000×1000 (the default stress profile) samples at most ~5M items.
pub const MAX_TOTAL_ITEMS: i64 = 6_000_000;
/// Change rows are a bounded recovery index; keep the same tail window the
/// runtime prunes to.
const CHANGE_TAIL_KEEP: i64 = 4096;

pub struct StressThreadSpec {
    pub session_id: String,
    pub title: String,
    pub user_rounds: i64,
    pub model_rounds_per_turn: i64,
}

pub struct StressThreadStats {
    pub session_id: String,
    pub user_turns: i64,
    pub items: i64,
    pub tool_calls: i64,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }

    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1).max(1) as u64) as i64
    }

    fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.below(1_000_000) as f64 / 1e6) * (hi - lo)
    }

    fn chance(&mut self, percent: f64) -> bool {
        self.float(0.0, 100.0) < percent
    }

    fn pick_str(&mut self, values: &[&'static str]) -> &'static str {
        values[self.below(values.len() as u64) as usize]
    }

    fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[self.below(values.len() as u64) as usize]
    }
}

/// One builtin tool profile: canonical/display name plus its runtime function
/// name, matching how execute.rs decorates tool_result payloads.
struct ToolProfile {
    name: &'static str,
    function_name: &'static str,
    /// Typical execution cost window in milliseconds.
    cost_ms: (u64, u64),
}

const TOOL_PROFILES: &[ToolProfile] = &[
    ToolProfile { name: "执行命令", function_name: "execute_command", cost_ms: (600, 15_000) },
    ToolProfile { name: "命令会话", function_name: "command_session", cost_ms: (80, 900) },
    ToolProfile { name: "ptc", function_name: "execute_ptc", cost_ms: (200, 6_000) },
    ToolProfile { name: "列出文件", function_name: "list_files", cost_ms: (10, 300) },
    ToolProfile { name: "glob", function_name: "glob_files", cost_ms: (15, 400) },
    ToolProfile { name: "搜索内容", function_name: "search_content", cost_ms: (40, 1_200) },
    ToolProfile { name: "读取文件", function_name: "read_files", cost_ms: (10, 250) },
    ToolProfile { name: "技能调用", function_name: "execute_skill_call", cost_ms: (300, 5_000) },
    ToolProfile { name: "写入文件", function_name: "write_file", cost_ms: (5, 120) },
    ToolProfile { name: "文本编辑", function_name: "text_edit", cost_ms: (5, 120) },
    ToolProfile { name: "子智能体控制", function_name: "subagent_control", cost_ms: (400, 4_000) },
    ToolProfile { name: "web_search", function_name: "tool_web_search", cost_ms: (1_200, 8_000) },
    ToolProfile { name: "web_fetch", function_name: "tool_web_fetch", cost_ms: (800, 6_000) },
    ToolProfile { name: "记忆管理", function_name: "execute_memory_manager_tool", cost_ms: (20, 400) },
    ToolProfile { name: "计划面板", function_name: "execute_plan_tool", cost_ms: (2, 30) },
    ToolProfile { name: "定时任务", function_name: "execute_schedule_task_tool", cost_ms: (10, 200) },
];

const USER_PROMPTS: &[&str] = &[
    "帮我看一下这个模块最近为什么性能下降，给出排查步骤",
    "把 {n} 号日志里的报错整理成一份摘要，标出可疑的调用链",
    "阅读 {path} 的实现，画一下关键函数之间的调用关系",
    "为当前工程补一组针对存储层的回归测试，重点覆盖并发写入",
    "把 {path} 里的过时字段清理掉，保持对外接口不变",
    "统计一下最近 {n} 次构建的失败原因分布，输出一个表格",
    "给这个会话写一份周报草稿，语气简洁一些",
    "检查 {path} 的异常处理是否有吞错的情况",
    "帮我把 {n} 个接口的返回结构统一成 snake_case",
    "对比两份配置文件的差异，指出会影响运行的项",
    "给 {path} 里的长函数做拆分重构，注意不要改变行为",
    "梳理 {n} 个遗留任务，按优先级排一下执行顺序",
];

const ANSWER_OPENINGS: &[&str] = &[
    "已完成排查，结论如下：",
    "整理好了，先说重点：",
    "这一轮检查完成，情况如下：",
    "按照你的要求处理完了：",
    "先给结论，再给依据：",
];

const ANSWER_PARAGRAPHS: &[&str] = &[
    "整体链路没有明显阻塞，主要耗时集中在批量写入阶段，占比约 {n}%。可以从两个方向优化：减小单次事务的体积，或者把索引维护延后到批量结束后统一执行。",
    "我把相关调用点都翻了一遍，目前有三处路径会把大对象整体复制一份，其中两处完全可以改成引用传递，另一处需要引入分页来避免一次性加载。",
    "日志里的告警是同一种模式反复触发：连接在空闲时被提前回收，后续请求又重新建立。把保活时间调长之后，这类告警基本不会再出现。",
    "按优先级列了三个改动点：第一，先补上缺失的取消传播；第二，把轮询间隔改成指数退避；第三，给失败路径加统一的错误出口。",
    "对比结果显示，两个版本的行为差异只在边界条件上：当输入为空时旧实现会静默返回，新实现会显式报错，这个语义变化需要同步到文档。",
    "重构后的结构更清晰了：入口只负责装配，执行细节下沉到独立模块，状态更新统一走一个出口。后续加功能不会再往入口文件堆积代码。",
    "测试覆盖里有一个盲区：并发提交时主键冲突的回滚路径没有断言。我补了两个用例，一个验证冲突被拒绝，一个验证已有数据不受影响。",
    "从耗时分布看，前 10% 的请求占了大约一半的等待时间，基本都是冷启动路径。加一层有界缓存之后，尾部延迟明显收敛。",
];

const REASONING_LINES: &[&str] = &[
    "先确认输入的边界条件，再决定走哪条分支。",
    "这个假设和上一轮观察到的现象矛盾，换个角度验证。",
    "把问题拆成两步：先定位差异来源，再评估改动影响面。",
    "直接改表层治标不治本，应该从数据流向的上游解决。",
    "这里的时间戳只在本地有意义，跨节点比较需要统一到 UTC。",
    "容量估算保守一点，宁可先少配，留出观察窗口。",
];

fn fill_template(template: &str, rng: &mut Rng, index: i64) -> String {
    template
        .replace("{n}", &(rng.int(3, 480).max(2)).to_string())
        .replace("{path}", &format!("src/services/module_{}.rs", index % 97))
}

struct RoundPlan {
    tool_count: usize,
    tool_profiles: Vec<&'static ToolProfile>,
    tool_duration_s: Vec<f64>,
    answer_tokens: u64,
    reasoning_tokens: Option<u64>,
    input_tokens: u64,
    prefill_duration_s: f64,
    decode_duration_s: f64,
}

fn sample_round(rng: &mut Rng, is_final: bool, turn_index: i64, round_index: i64) -> RoundPlan {
    // Context grows with every turn and every round inside the turn.
    let base_input: u64 = 1_400
        + (turn_index as u64 % 60) * 90
        + (round_index as u64).saturating_mul(420)
        + rng.int(0, 350) as u64;
    let input_tokens = base_input.min(190_000);
    let prefill_speed = rng.float(24_000.0, 46_000.0);
    let prefill_duration_s = input_tokens as f64 / prefill_speed;

    let tool_count = if is_final { 0 } else { rng.below(5) as usize };
    let mut tool_profiles = Vec::with_capacity(tool_count);
    let mut tool_duration_s = Vec::with_capacity(tool_count);
    for _ in 0..tool_count {
        let profile = rng.pick(TOOL_PROFILES);
        let ms = profile.cost_ms.0
            + rng.below((profile.cost_ms.1 - profile.cost_ms.0).max(1));
        tool_profiles.push(profile);
        tool_duration_s.push(ms as f64 / 1000.0);
    }

    let answer_tokens: u64 = if is_final {
        rng.int(320, 2_400) as u64
    } else {
        rng.int(60, 260) as u64
    };
    let reasoning_tokens = if !is_final || rng.chance(45.0) {
        Some(rng.int(120, 1_500) as u64)
    } else {
        None
    };
    let decode_speed = rng.float(55.0, 95.0);
    let decode_duration_s =
        (answer_tokens as f64 + reasoning_tokens.unwrap_or(0) as f64) / decode_speed;

    RoundPlan {
        tool_count,
        tool_profiles,
        tool_duration_s,
        answer_tokens,
        reasoning_tokens,
        input_tokens,
        prefill_duration_s,
        decode_duration_s,
    }
}

fn build_tool_args(rng: &mut Rng, profile: &ToolProfile, turn_index: i64) -> Value {
    let path = format!("src/module_{}.rs", turn_index % 40);
    match profile.name {
        "执行命令" => json!({
            "command": pick_command(rng),
            "timeout_ms": rng.int(5_000, 30_000),
        }),
        "命令会话" => json!({ "action": rng.pick_str(&["create", "exec", "close"]), "command": pick_command(rng) }),
        "ptc" => json!({ "code": "return items.filter(i => i.ok).length;" }),
        "列出文件" => json!({ "path": ".", "depth": rng.int(1, 3) }),
        "glob" => json!({ "pattern": "**/*.rs", "path": "src" }),
        "搜索内容" => json!({ "query": rng.pick_str(&["latest_change_seq", "journal_mode", "user_round", "tool_call_id"]), "path": "src" }),
        "读取文件" => json!({ "path": path, "start_line": 1 }),
        "技能调用" => json!({ "skill": "code-review", "args": { "scope": "storage" } }),
        "写入文件" => json!({ "path": format!("docs/notes_{}.md", turn_index), "content": "# 笔记\n生成的批量写入说明。\n" }),
        "文本编辑" => json!({ "path": path, "old_string": "fn legacy_path(", "new_string": "fn current_path(" }),
        "子智能体控制" => json!({ "action": "spawn", "prompt": "梳理存储层并发写入点", "name": "investigator" }),
        "web_search" => json!({ "query": rng.pick(&["sqlite wal checkpoint strategy", "tokio blocking pool sizing", "rust json serialization perf"]) }),
        "web_fetch" => json!({ "url": "https://example.com/docs/batch-insert" }),
        "记忆管理" => json!({ "action": "remember", "content": "批量写入使用独立连接并关闭日志以提速。" }),
        "计划面板" => json!({
            "plan": [
                { "step": "定位慢路径", "status": "completed" },
                { "step": "给出修复方案", "status": rng.pick_str(&["in_progress", "pending"]) },
            ]
        }),
        "定时任务" => json!({ "action": "add", "name": format!("daily-report-{}", turn_index), "schedule": { "kind": "every", "every_ms": 3_600_000 } }),
        _ => json!({}),
    }
}

fn pick_command(rng: &mut Rng) -> String {
    rng.pick_str(&[
        "cargo build -p wunder-runtime",
        "cargo test --quiet storage",
        "git log --oneline -5",
        "ls -la config/data",
        "python scripts/update_feature_log.py --help",
    ])
    .to_string()
}

fn build_tool_result(
    rng: &mut Rng,
    profile: &ToolProfile,
    args: &Value,
    duration_ms: i64,
) -> (bool, Value, String, Value) {
    // A small share of calls fail, matching real-world tool error rates.
    let failed = rng.chance(6.0);
    let data = if failed {
        json!({ "detail": rng.pick_str(&[
            "路径不存在或不可读",
            "连接超时，稍后可重试",
            "参数校验未通过",
        ]) })
    } else {
        match profile.name {
            "执行命令" | "命令会话" | "ptc" => json!({
                "exit_code": 0,
                "stdout": rng.pick_str(&[
                    "Finished dev [unoptimized + debuginfo] target(s)",
                    "test result: ok. 12 passed; 0 failed",
                    "4 files changed, 86 insertions(+)",
                ]),
                "stderr": "",
            }),
            "列出文件" | "glob" => {
                let mut entries = Vec::new();
                for i in 0..rng.int(3, 12) {
                    entries.push(json!({
                        "name": format!("file_{}.rs", i),
                        "kind": "file",
                        "size": rng.int(240, 48_000),
                    }));
                }
                json!({ "entries": entries, "total": entries.len() })
            }
            "搜索内容" => {
                let mut matches = Vec::new();
                for i in 0..rng.int(2, 9) {
                    matches.push(json!({
                        "file": format!("src/module_{}.rs", i),
                        "line": rng.int(12, 900),
                        "text": "        let seq = tx.query_row(\"SELECT latest_change_seq+1 ...",
                    }));
                }
                json!({ "matches": matches, "total": matches.len() })
            }
            "读取文件" => json!({
                "path": args.get("path").cloned().unwrap_or_else(|| json!("src/main.rs")),
                "content": "// 读取到的源码片段（截断示意）\nfn run() { /* ... */ }\n",
                "total_lines": rng.int(60, 2_400),
            }),
            "写入文件" => json!({ "path": args.get("path").cloned().unwrap_or(json!("notes.md")), "bytes": rng.int(40, 9_000) }),
            "文本编辑" => json!({ "replacements": 1 }),
            "子智能体控制" => json!({ "session_id": Uuid::new_v4().to_string(), "status": "spawned" }),
            "web_search" => {
                let mut results = Vec::new();
                for i in 0..rng.int(3, 6) {
                    results.push(json!({
                        "title": format!("参考资料 {}", i),
                        "url": "https://example.com/docs/article",
                        "snippet": "与批量写入和索引维护相关的要点摘录。",
                    }));
                }
                json!({ "results": results, "query": args.get("query").cloned().unwrap_or(json!("")) })
            }
            "web_fetch" => json!({ "title": "批量写入指南", "content": "相关章节的正文摘要……", "truncated": true }),
            "记忆管理" => json!({ "action": "remember", "memory_id": Uuid::new_v4().to_string() }),
            "计划面板" => json!({ "accepted": true }),
            "定时任务" => json!({ "job_id": Uuid::new_v4().to_string(), "enabled": true }),
            _ => json!({ "ok": true }),
        }
    };
    let meta = json!({
        "duration_ms": duration_ms,
        "output_chars": data.to_string().chars().count(),
    });
    let error = if failed {
        rng.pick_str(&[
            "工具执行失败：目标不可达",
            "工具执行失败：参数超出允许范围",
        ])
        .to_string()
    } else {
        String::new()
    };
    (failed, data, error, meta)
}

fn build_answer_text(rng: &mut Rng, prompt: &str, turn_index: i64) -> String {
    let mut text = String::new();
    text.push_str(rng.pick_str(ANSWER_OPENINGS));
    text.push('\n');
    text.push_str(&format!("### 关于「{}」\n\n", prompt.chars().take(24).collect::<String>()));
    let paragraph_count = rng.int(2, 5);
    for i in 0..paragraph_count {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(&fill_template(rng.pick_str(ANSWER_PARAGRAPHS), rng, turn_index));
    }
    text.push_str("\n\n下一步：\n");
    for step in 0..rng.int(2, 4) {
        text.push_str(&format!("{}. 继续跟踪第 {} 项改动\n", step + 1, rng.int(1, 40)));
    }
    text
}

fn build_reasoning_text(rng: &mut Rng, budget_tokens: u64) -> String {
    let target_chars = (budget_tokens.saturating_mul(3)).max(60) as usize;
    let mut text = String::new();
    while text.chars().count() < target_chars {
        text.push_str(rng.pick_str(REASONING_LINES));
        text.push(' ');
    }
    text
}

struct TurnCost {
    turn_seed: u64,
    round_costs: Vec<f64>,
    gap_before_s: f64,
    total_s: f64,
    items_in_turn: i64,
    change_rows_in_turn: i64,
}

/// Plans are derived from a per-turn seed so pass 1 (cost sampling) and pass 2
/// (writing) see exactly the same tool counts and durations.
fn sample_turn_plans(turn_seed: u64, turn_index: i64, model_rounds: i64) -> Vec<RoundPlan> {
    let mut rng = Rng::new(
        turn_seed ^ (turn_index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    (0..model_rounds)
        .map(|m| sample_round(&mut rng, m + 1 == model_rounds, turn_index, m))
        .collect()
}

fn sample_turn_cost(turn_seed: u64, turn_index: i64, model_rounds: i64, rng: &mut Rng) -> TurnCost {
    let plans = sample_turn_plans(turn_seed, turn_index, model_rounds);
    let mut round_costs = Vec::with_capacity(model_rounds as usize);
    let mut items_in_turn = 1i64; // user bubble
    for plan in &plans {
        let mut cost = plan.prefill_duration_s + plan.decode_duration_s;
        cost += plan.tool_duration_s.iter().sum::<f64>();
        cost += rng.float(0.4, 2.2); // inter-round orchestration overhead
        round_costs.push(cost);
        items_in_turn += plan.tool_count as i64 + 1; // tools + assistant message
    }
    let gap_before_s = rng.float(4.0, 90.0);
    let total_s = gap_before_s + round_costs.iter().sum::<f64>();
    // Per turn: turn_upsert(queued) + user bubble upsert + one change per item
    // + final turn_upsert(completed).
    let change_rows_in_turn = items_in_turn + 3;
    TurnCost {
        turn_seed,
        round_costs,
        gap_before_s,
        total_s,
        items_in_turn,
        change_rows_in_turn,
    }
}

impl SqliteStorage {
    /// Generate one complete synthetic thread in bulk. Progress is reported
    /// once per finished user turn as (done_rounds, items_written).
    pub fn generate_stress_thread(
        &self,
        user_id: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        self.ensure_initialized()?;
        ensure!(!user_id.trim().is_empty(), "missing user identity");
        ensure!(!spec.session_id.trim().is_empty(), "missing session identity");
        ensure!(
            spec.user_rounds > 0 && spec.user_rounds <= MAX_USER_ROUNDS,
            "user_rounds out of range (1..={MAX_USER_ROUNDS})"
        );
        ensure!(
            spec.model_rounds_per_turn > 0 && spec.model_rounds_per_turn <= MAX_MODEL_ROUNDS,
            "model_rounds out of range (1..={MAX_MODEL_ROUNDS})"
        );
        ensure!(
            spec.user_rounds * (spec.model_rounds_per_turn * 5 + 1) <= MAX_TOTAL_ITEMS,
            "requested thread exceeds the total item budget"
        );

        let base_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED_1234);
        let mut rng = Rng::new(base_seed);

        // Pass 1: sample the whole timeline so the simulated clock can end at
        // "now"; the thread then reads as recent history.
        let now = Self::now_ts();
        let mut costs = Vec::with_capacity(spec.user_rounds as usize);
        let mut total_s = 0f64;
        let mut total_items = 1i64; // session row itself
        let mut total_change_rows = 0i64;
        for turn_index in 1..=spec.user_rounds {
            let turn_seed = base_seed ^ (turn_index as u64).wrapping_mul(0x5DEE_CE66);
            let cost = sample_turn_cost(turn_seed, turn_index, spec.model_rounds_per_turn, &mut rng);
            total_s += cost.total_s;
            total_items += cost.items_in_turn;
            total_change_rows += cost.change_rows_in_turn;
            costs.push(cost);
        }
        ensure!(
            total_items <= MAX_TOTAL_ITEMS,
            "requested thread exceeds the total item budget ({total_items})"
        );
        let change_tail_cutoff = (total_change_rows - CHANGE_TAIL_KEEP).max(0);
        let start_time = now - total_s.min(180.0 * 24.0 * 3600.0); // never older than ~180 days

        // Bulk connection: journal in memory, no fsync per commit. WAL is
        // restored afterwards so the live runtime keeps its normal settings.
        let mut conn = Connection::open(&self.db_path)?;
        conn.busy_timeout(Duration::from_secs(30)).ok();
        conn.pragma_update(None, "journal_mode", "MEMORY").ok();
        conn.pragma_update(None, "synchronous", "OFF").ok();
        conn.pragma_update(None, "cache_size", -262_144).ok();

        let write_result = self.write_stress_thread(
            &mut conn,
            user_id,
            spec,
            &costs,
            start_time,
            total_change_rows,
            change_tail_cutoff,
            &mut progress,
        );
        let stats = match write_result {
            Ok(stats) => stats,
            Err(err) => {
                // Remove the partial thread so no half-rendered session lingers.
                let _ = conn.execute_batch(&format!(
                    "BEGIN; \
                     DELETE FROM thread_items WHERE session_id='{sid}'; \
                     DELETE FROM thread_turns WHERE session_id='{sid}'; \
                     DELETE FROM thread_item_blocks WHERE session_id='{sid}'; \
                     DELETE FROM thread_log_changes WHERE session_id='{sid}'; \
                     DELETE FROM thread_log_metrics WHERE session_id='{sid}'; \
                     DELETE FROM tool_logs WHERE session_id='{sid}'; \
                     DELETE FROM thread_logs WHERE session_id='{sid}'; \
                     DELETE FROM chat_sessions WHERE session_id='{sid}'; \
                     COMMIT;",
                    sid = spec.session_id.replace('\'', "''")
                ));
                return Err(err);
            }
        };

        conn.pragma_update(None, "journal_mode", "WAL").ok();
        Ok(stats)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_stress_thread(
        &self,
        conn: &mut Connection,
        user_id: &str,
        spec: &StressThreadSpec,
        costs: &[TurnCost],
        start_time: f64,
        total_change_rows: i64,
        change_tail_cutoff: i64,
        progress: &mut impl FnMut(i64, i64),
    ) -> Result<StressThreadStats> {
        let session_id = spec.session_id.as_str();
        let mut seq: i64 = 0;
        let mut clock = start_time;
        let mut items_written: i64 = 0;
        let mut tool_calls_total: i64 = 0;
        let mut turn_rng = Rng::new(0xC0FF_EE00 ^ (total_change_rows as u64));

        for (offset, cost) in costs.iter().enumerate() {
            let turn_index = offset as i64 + 1;
            clock += cost.gap_before_s;
            let turn_start = clock;
            let turn_id = Uuid::new_v4().to_string();
            let client_message_id = format!("stress-{}", Uuid::new_v4().simple());
            let prompt_template = turn_rng.pick_str(USER_PROMPTS);
            let prompt = fill_template(prompt_template, &mut turn_rng, turn_index);

            // Same per-turn seed as pass 1: identical tool counts/durations.
            let plans = sample_turn_plans(cost.turn_seed, turn_index, spec.model_rounds_per_turn);

            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Turn row (terminal state, like a settled real turn).
            tx.execute(
                "INSERT INTO thread_turns(session_id,turn_id,root_turn_id,trigger_kind,client_message_id,user_id,user_turn_index,status,summary,payload,created_time,updated_time) \
                 VALUES(?,?,?,?,?,?,?,'completed',?,'{}',?,?)",
                params![
                    session_id,
                    turn_id,
                    turn_id,
                    "user",
                    client_message_id,
                    user_id,
                    turn_index,
                    prompt.chars().take(240).collect::<String>(),
                    turn_start,
                    turn_start,
                ],
            )?;
            // turn_upsert change row mirrors accept_thread_turn.
            seq += 1;
            if seq > change_tail_cutoff {
                tx.execute(
                    "INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'turn_upsert',?,NULL,1,?,?)",
                    params![
                        session_id,
                        seq,
                        user_id,
                        turn_id,
                        serde_json::to_string(&json!({
                            "turn_id": turn_id,
                            "root_turn_id": turn_id,
                            "trigger_kind": "user",
                            "status": "queued",
                            "user_round": turn_index,
                            "client_message_id": client_message_id,
                        }))?,
                        turn_start,
                    ],
                )?;
            }

            // User bubble item.
            let user_item_id = format!("{turn_id}:user");
            let user_payload = serde_json::to_string(&json!({
                "role": "user",
                "content": prompt,
                "client_message_id": client_message_id,
                "session_id": session_id,
                "turn_id": turn_id,
                "user_round": turn_index,
                "item_id": user_item_id,
                "status": "completed",
            }))?;
            tx.execute(
                "INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) \
                 VALUES(?,?,?,?,'user',?,0,'user_message','completed',?,?,?,?)",
                params![
                    session_id,
                    user_item_id,
                    turn_id,
                    turn_id,
                    user_id,
                    user_payload,
                    turn_start,
                    turn_start,
                    seq,
                ],
            )?;
            items_written += 1;
            seq += 1;
            if seq > change_tail_cutoff {
                tx.execute(
                    "INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'item_upsert',?,?,1,?,?)",
                    params![
                        session_id,
                        seq,
                        user_id,
                        turn_id,
                        user_item_id,
                        self.stress_committed_item_json(
                            &user_item_id,
                            0,
                            "user_message",
                            "completed",
                            &user_payload,
                            turn_start,
                            turn_start,
                            &turn_id,
                            seq,
                        )?,
                        turn_start,
                    ],
                )?;
            }

            let mut item_index: i64 = 0;
            let mut turn_clock = turn_start;

            for (m, plan) in plans.iter().enumerate() {
                let model_round = m as i64 + 1;
                // Per-round factor fits the sampled round cost exactly (plans
                // are identical to pass 1, so the factor is ~1.0 and only
                // absorbs floating-point drift).
                let raw_round = plan.prefill_duration_s
                    + plan.decode_duration_s
                    + plan.tool_duration_s.iter().sum::<f64>();
                let scale = cost.round_costs[m] / raw_round.max(1e-9);
                turn_clock += plan.prefill_duration_s * scale;
                let round_start = turn_clock;

                let mut call_profiles: Vec<(&ToolProfile, f64)> = Vec::new();
                for (i, profile) in plan.tool_profiles.iter().enumerate() {
                    call_profiles.push((*profile, plan.tool_duration_s[i] * scale));
                }
                let tool_done_at = round_start + call_profiles.iter().map(|(_, d)| *d).sum::<f64>();

                for (profile, duration_s) in &call_profiles {
                    let call_id = format!("call_{}", Uuid::new_v4().simple());
                    let args = build_tool_args(&mut turn_rng, profile, turn_index);
                    let duration_ms = (*duration_s * 1000.0).round() as i64;
                    let (failed, data, error, meta) =
                        build_tool_result(&mut turn_rng, profile, &args, duration_ms);
                    let tool_done = round_start + duration_s;
                    let item_id = format!("{turn_id}:tool-{call_id}");
                    let usage = json!({
                        "input_tokens": plan.input_tokens,
                        "output_tokens": plan.answer_tokens,
                        "total_tokens": plan.input_tokens + plan.answer_tokens,
                        "reasoning_tokens": plan.reasoning_tokens.unwrap_or(0),
                        "estimated": true,
                    });
                    let mut payload = json!({
                        "tool": profile.name,
                        "ok": !failed,
                        "data": data,
                        "args": args,
                        "request_context_tokens": plan.input_tokens,
                        "request_usage": usage,
                        "tool_runtime_name": profile.name,
                        "tool_display_name": profile.name,
                        "tool_function_name": profile.function_name,
                        "tool_call_id": call_id,
                        "model_observation": { "tool": profile.name, "ok": !failed },
                        "event_type": "tool_result",
                        "session_id": session_id,
                        "item_id": item_id,
                        "kind": "tool_call",
                        "status": if failed { "failed" } else { "completed" },
                        "user_round": turn_index,
                        "model_round": model_round,
                        "turn_id": turn_id,
                    });
                    if !error.is_empty() {
                        payload["error"] = json!(error);
                    }
                    payload["meta"] = meta;
                    let payload_text = serde_json::to_string(&payload)?;
                    item_index += 1;
                    seq += 1;
                    tx.execute(
                        "INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) \
                         VALUES(?,?,?,?,'user',?,?, 'tool_call',?,?,?,?,?)",
                        params![
                            session_id,
                            item_id,
                            turn_id,
                            turn_id,
                            user_id,
                            item_index,
                            if failed { "failed" } else { "completed" },
                            payload_text,
                            tool_done,
                            tool_done,
                            seq,
                        ],
                    )?;
                    items_written += 1;
                    tool_calls_total += 1;
                    if seq > change_tail_cutoff {
                        tx.execute(
                            "INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'item_upsert',?,?,1,?,?)",
                            params![
                                session_id,
                                seq,
                                user_id,
                                turn_id,
                                item_id,
                                self.stress_committed_item_json(
                                    &item_id,
                                    item_index,
                                    "tool_call",
                                    if failed { "failed" } else { "completed" },
                                    &payload_text,
                                    tool_done,
                                    tool_done,
                                    &turn_id,
                                    seq,
                                )?,
                                tool_done,
                            ],
                        )?;
                    }
                    // tool_logs mirrors append_tool_log's payload column.
                    tx.execute(
                        "INSERT INTO tool_logs(user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?,?)",
                        params![
                            user_id,
                            session_id,
                            profile.name,
                            if failed { 0 } else { 1 },
                            error,
                            serde_json::to_string(&args)?,
                            serde_json::to_string(&payload["data"])?,
                            Local.timestamp_millis_opt((tool_done * 1000.0) as i64).single().map(|t| t.to_rfc3339()).unwrap_or_default(),
                            serde_json::to_string(&payload)?,
                            tool_done,
                        ],
                    )?;
                }

                // Assistant message closes the round.
                turn_clock = tool_done_at + plan.decode_duration_s * scale;
                let done_at = turn_clock;
                let decode_output_tokens = plan.answer_tokens;
                let usage = json!({
                    "input_tokens": plan.input_tokens,
                    "output_tokens": plan.answer_tokens + plan.reasoning_tokens.unwrap_or(0),
                    "total_tokens": plan.input_tokens + plan.answer_tokens + plan.reasoning_tokens.unwrap_or(0),
                    "reasoning_tokens": plan.reasoning_tokens.unwrap_or(0),
                    "estimated": true,
                });
                let answer = if m as i64 + 1 == spec.model_rounds_per_turn {
                    build_answer_text(&mut turn_rng, &prompt, turn_index)
                } else {
                    format!("继续处理：{}（第 {} 轮）", profile_hint(&call_profiles), model_round)
                };
                let reasoning = plan
                    .reasoning_tokens
                    .map(|tokens| build_reasoning_text(&mut turn_rng, tokens))
                    .unwrap_or_default();
                let prefill_ms = (plan.prefill_duration_s * scale * 1000.0).round() as u64;
                let decode_ms = (plan.decode_duration_s * scale * 1000.0).round() as u64;
                let content_chars = answer.chars().count();
                let payload = serde_json::to_string(&json!({
                    "content": answer,
                    "reasoning": reasoning,
                    "usage": usage,
                    "decode_output_tokens": decode_output_tokens,
                    "tool_calls": [],
                    "prefill_duration_s": plan.prefill_duration_s * scale,
                    "decode_duration_s": plan.decode_duration_s * scale,
                    "stream_timing": {
                        "chunk_count": (decode_output_tokens / 3).max(8),
                        "content_delta_chars": content_chars,
                        "reasoning_delta_chars": reasoning.chars().count(),
                        "prefill_ms": prefill_ms,
                        "decode_ms": decode_ms,
                        "content_decode_ms": decode_ms,
                        "max_chunk_gap_ms": turn_rng.int(20, 600),
                    },
                    "ttft_ms": prefill_ms,
                    "prefill_tokens": plan.input_tokens,
                    "prefill_speed_tps": (plan.input_tokens as f64 / (plan.prefill_duration_s * scale).max(1e-6)),
                    "prefill_speed_lower_bound": 0,
                    "decode_tokens": decode_output_tokens + plan.reasoning_tokens.unwrap_or(0),
                    "decode_speed_tps": ((decode_output_tokens + plan.reasoning_tokens.unwrap_or(0)) as f64
                        / (plan.decode_duration_s * scale).max(1e-6)),
                    "decode_stream_chunk_tokens": turn_rng.int(2, 6),
                    "finish_reason": "stop",
                    "output_limit_reached": false,
                    "max_output": 32768,
                    "thinking_token_budget": Value::Null,
                    "thinking_disabled": false,
                    "event_type": "llm_output",
                    "session_id": session_id,
                    "item_id": format!("{turn_id}:text-{model_round}"),
                    "kind": "assistant_message",
                    "status": "completed",
                    "user_round": turn_index,
                    "model_round": model_round,
                    "turn_id": turn_id,
                }))?;
                item_index += 1;
                seq += 1;
                let item_id = format!("{turn_id}:text-{model_round}");
                tx.execute(
                    "INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) \
                     VALUES(?,?,?,?,'user',?,?,'assistant_message','completed',?,?,?,?)",
                    params![
                        session_id,
                        item_id,
                        turn_id,
                        turn_id,
                        user_id,
                        item_index,
                        payload,
                        done_at,
                        done_at,
                        seq,
                    ],
                )?;
                items_written += 1;
                if seq > change_tail_cutoff {
                    tx.execute(
                        "INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'item_upsert',?,?,1,?,?)",
                        params![
                            session_id,
                            seq,
                            user_id,
                            turn_id,
                            item_id,
                            self.stress_committed_item_json(
                                &item_id,
                                item_index,
                                "assistant_message",
                                "completed",
                                &payload,
                                done_at,
                                done_at,
                                &turn_id,
                                seq,
                            )?,
                            done_at,
                        ],
                    )?;
                }
            }

            // Terminal turn_upsert mirrors update_thread_turn's final change.
            let turn_end = turn_clock;
            seq += 1;
            if seq > change_tail_cutoff {
                tx.execute(
                    "INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'turn_upsert',?,NULL,?, ?,?)",
                    params![
                        session_id,
                        seq,
                        user_id,
                        turn_id,
                        seq,
                        serde_json::to_string(&json!({
                            "turn_id": turn_id,
                            "status": "completed",
                            "root_turn_id": turn_id,
                            "trigger_kind": "user",
                            "user_round": turn_index,
                        }))?,
                        turn_end,
                    ],
                )?;
            }
            tx.execute(
                "UPDATE thread_turns SET updated_time=? WHERE session_id=? AND turn_id=?",
                params![turn_end, session_id, turn_id],
            )?;
            tx.commit()?;
            progress(turn_index, items_written);
        }

        // Final envelope: session registration, log header, metrics. One short
        // transaction; chat_sessions appears last so the list only ever shows a
        // complete thread.
        let end_time = clock;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES(?,?,?,?,?,?)",
            params![session_id, user_id, spec.user_rounds, seq, start_time, end_time],
        )?;
        tx.execute(
            "INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) VALUES(?,?, 'user_turn_total', ?, ?) \
             ON CONFLICT(session_id,metric_key) DO UPDATE SET metric_value=excluded.metric_value,updated_time=excluded.updated_time",
            params![session_id, user_id, spec.user_rounds, end_time],
        )?;
        tx.execute(
            "INSERT INTO chat_sessions(session_id,user_id,title,status,created_at,updated_at,last_message_at) VALUES(?,?,?,'active',?,?,?)",
            params![session_id, user_id, spec.title, start_time, end_time, end_time],
        )?;
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
            params![session_id, change_tail_cutoff],
        )?;
        tx.commit()?;

        Ok(StressThreadStats {
            session_id: spec.session_id.clone(),
            user_turns: spec.user_rounds,
            items: items_written,
            tool_calls: tool_calls_total,
        })
    }

    /// Committed-item change payload shape (matches committed_item_payload).
    #[allow(clippy::too_many_arguments)]
    fn stress_committed_item_json(
        &self,
        item_id: &str,
        item_index: i64,
        kind: &str,
        status: &str,
        payload: &str,
        created_time: f64,
        updated_time: f64,
        turn_id: &str,
        created_seq: i64,
    ) -> Result<String> {
        Ok(serde_json::to_string(&json!({
            "item_id": item_id,
            "item_index": item_index,
            "kind": kind,
            "status": status,
            "revision": 1,
            "payload": serde_json::from_str::<Value>(payload).unwrap_or(Value::Null),
            "created_time": created_time,
            "updated_time": updated_time,
            "turn_id": turn_id,
            "visibility": "user",
            "root_turn_id": turn_id,
            "created_seq": created_seq,
        }))?)
    }
}

fn profile_hint(profiles: &[(&ToolProfile, f64)]) -> String {
    match profiles.first() {
        Some((profile, _)) => format!("已调用 {}", profile.name),
        None => "继续推理".to_string(),
    }
}

#[cfg(all(test, feature = "sqlite-storage"))]
mod tests {
    use super::*;
    use wunder_core::storage_backend::{ChatSessionStore, ThreadLogStore};

    fn temp_db() -> (std::path::PathBuf, SqliteStorage) {
        let dir = std::env::temp_dir().join(format!("wunder-stress-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("wunder.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        (dir, storage)
    }

    #[test]
    fn generated_thread_is_readable_through_normal_paths() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-session-1".to_string(),
            title: "渲染压测 3×4".to_string(),
            user_rounds: 3,
            model_rounds_per_turn: 4,
        };
        let mut progress_rounds = Vec::new();
        let stats = storage
            .generate_stress_thread("tester", &spec, |done, _| progress_rounds.push(done))
            .unwrap();
        assert_eq!(stats.user_turns, 3);
        assert!(stats.tool_calls > 0);
        assert!(stats.items > 3 * 4);
        assert_eq!(progress_rounds.len(), 3);

        // Session appears in the catalog.
        let (sessions, _) = storage
            .list_chat_sessions("tester", None, None, 0, 50)
            .unwrap();
        assert!(sessions
            .iter()
            .any(|s| s.session_id == "stress-session-1"));

        // Visible messages: every model round contributes an assistant bubble,
        // so 3 turns × (1 user + 4 assistant) = 15, newest first.
        let visible = storage
            .list_thread_visible_messages("tester", "stress-session-1", None, 500)
            .unwrap();
        assert_eq!(visible.len(), 15);
        let roles: Vec<&str> = visible
            .iter()
            .map(|m| m["role"].as_str().unwrap_or_default())
            .collect();
        // Newest first: each turn ends with its 4th assistant bubble, so the
        // reversed timeline reads [assistant×4, user] per turn.
        let mut expected: Vec<&str> = Vec::new();
        for _ in 0..3 {
            for _ in 0..4 {
                expected.push("assistant");
            }
            expected.push("user");
        }
        assert_eq!(roles, expected);
        for message in &visible {
            assert_eq!(message["status"], json!("completed"));
        }

        // Turn listing and turn detail both resolve with terminal items.
        let turns = storage
            .list_thread_turns("tester", "stress-session-1", None, 50)
            .unwrap();
        assert_eq!(turns.len(), 3);
        let turn_id = turns[0]["turn_id"].as_str().unwrap().to_string();
        assert_eq!(turns[0]["status"], json!("completed"));
        let detail = storage
            .get_thread_turn("tester", "stress-session-1", &turn_id, -1, 200, false)
            .unwrap()
            .unwrap();
        let items = detail["items"].as_array().unwrap();
        // user bubble + at least one item per model round (tools + assistant)
        assert!(items.len() >= 5, "turn items: {}", items.len());
        let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap_or_default()).collect();
        assert_eq!(kinds.first().copied(), Some("user_message"));
        assert!(kinds.contains(&"tool_call"));
        assert!(kinds.contains(&"assistant_message"));
        let tool_item = items.iter().find(|i| i["kind"] == json!("tool_call")).unwrap();
        let payload = &tool_item["payload"];
        assert!(payload["tool_call_id"].as_str().is_some());
        assert!(payload["tool"].as_str().is_some());
        assert!(payload["args"].is_object());
        assert!(payload["request_usage"].is_object());
        let assistant = items
            .iter()
            .find(|i| i["kind"] == json!("assistant_message"))
            .unwrap();
        let payload = &assistant["payload"];
        assert!(payload["content"].as_str().is_some());
        assert!(payload["usage"]["input_tokens"].as_u64().unwrap() > 0);
        assert!(payload["ttft_ms"].as_u64().unwrap() > 0);
        assert!(payload["prefill_speed_tps"].as_f64().unwrap() > 1000.0);
        assert_eq!(payload["event_type"], json!("llm_output"));

        // Simulated timestamps are strictly increasing and end before now.
        let first_created = items[0]["created_time"].as_f64().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        assert!(first_created < now, "timestamps must be in the past");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_requests_are_rejected() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-too-big".to_string(),
            title: "超出上限".to_string(),
            user_rounds: MAX_USER_ROUNDS + 1,
            model_rounds_per_turn: 4,
        };
        assert!(storage.generate_stress_thread("tester", &spec, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore] // perf probe: run explicitly with -- --ignored
    fn mid_size_generation_perf() {
        let (dir, storage) = temp_db();
        let spec = StressThreadSpec {
            session_id: "stress-perf-50x20".to_string(),
            title: "渲染压测 50×20".to_string(),
            user_rounds: 50,
            model_rounds_per_turn: 20,
        };
        let started = std::time::Instant::now();
        let stats = storage
            .generate_stress_thread("tester", &spec, |done, items| {
                if done % 10 == 0 {
                    println!("progress rounds={done} items={items}");
                }
            })
            .unwrap();
        let elapsed = started.elapsed();
        println!(
            "generated 50x20: items={} tool_calls={} elapsed={elapsed:?} ({:.0} items/s)",
            stats.items,
            stats.tool_calls,
            stats.items as f64 / elapsed.as_secs_f64().max(1e-9)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
