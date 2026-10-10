//! Storage-agnostic sampling model for synthetic stress threads.
//!
//! Everything here is pure computation: deterministic RNG, per-turn plans and
//! the pass-1 timeline estimate. The SQLite and PostgreSQL backends both write
//! from these plans so the two backends produce equally realistic threads
//! without duplicating the sampling logic.

use anyhow::{ensure, Result};
use serde_json::{json, Value};
use uuid::Uuid;

/// Hard cap protecting the live database from an unreasonable generation job.
pub const MAX_USER_ROUNDS: i64 = 2000;
pub const MAX_MODEL_ROUNDS: i64 = 2000;
/// 1000×1000 (the default stress profile) samples at most ~5M items.
pub const MAX_TOTAL_ITEMS: i64 = 6_000_000;
/// Change rows are a bounded recovery index; keep the same tail window the
/// runtime prunes to.
pub const CHANGE_TAIL_KEEP: i64 = 4096;

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

/// Validate the spec against the shared caps. Backends call this before
/// touching any connection.
pub fn validate_spec(spec: &StressThreadSpec) -> Result<()> {
    ensure!(
        !spec.session_id.trim().is_empty(),
        "missing session identity"
    );
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
    Ok(())
}

/// Deterministic seed from wall-clock nanos (shared by both backends).
pub fn base_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED_1234)
}

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
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

    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }

    pub(crate) fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1).max(1) as u64) as i64
    }

    pub(crate) fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.below(1_000_000) as f64 / 1e6) * (hi - lo)
    }

    pub(crate) fn chance(&mut self, percent: f64) -> bool {
        self.float(0.0, 100.0) < percent
    }

    pub(crate) fn pick_str(&mut self, values: &[&'static str]) -> &'static str {
        values[self.below(values.len() as u64) as usize]
    }

    pub(crate) fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[self.below(values.len() as u64) as usize]
    }
}

/// One builtin tool profile: canonical/display name plus its runtime function
/// name, matching how execute.rs decorates tool_result payloads.
pub(crate) struct ToolProfile {
    pub name: &'static str,
    pub function_name: &'static str,
    /// Typical execution cost window in milliseconds.
    pub cost_ms: (u64, u64),
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

pub(crate) fn fill_template(template: &str, rng: &mut Rng, index: i64) -> String {
    template
        .replace("{n}", &(rng.int(3, 480).max(2)).to_string())
        .replace("{path}", &format!("src/services/module_{}.rs", index % 97))
}

/// Prompt templates shared by both backends; `{n}`/`{path}` get filled per turn.
pub(crate) fn user_prompts() -> &'static [&'static str] {
    USER_PROMPTS
}

pub(crate) struct RoundPlan {
    pub tool_count: usize,
    pub tool_profiles: Vec<&'static ToolProfile>,
    pub tool_duration_s: Vec<f64>,
    pub answer_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub input_tokens: u64,
    pub prefill_duration_s: f64,
    pub decode_duration_s: f64,
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

pub(crate) fn build_tool_args(rng: &mut Rng, profile: &ToolProfile, turn_index: i64) -> Value {
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

pub(crate) fn build_tool_result(
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

pub(crate) fn build_answer_text(rng: &mut Rng, prompt: &str, turn_index: i64) -> String {
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

pub(crate) fn build_reasoning_text(rng: &mut Rng, budget_tokens: u64) -> String {
    let target_chars = (budget_tokens.saturating_mul(3)).max(60) as usize;
    let mut text = String::new();
    while text.chars().count() < target_chars {
        text.push_str(rng.pick_str(REASONING_LINES));
        text.push(' ');
    }
    text
}

pub(crate) struct TurnCost {
    pub turn_seed: u64,
    pub round_costs: Vec<f64>,
    pub gap_before_s: f64,
    pub total_s: f64,
    pub items_in_turn: i64,
    pub change_rows_in_turn: i64,
}

/// Plans are derived from a per-turn seed so pass 1 (cost sampling) and pass 2
/// (writing) see exactly the same tool counts and durations.
pub(crate) fn sample_turn_plans(
    turn_seed: u64,
    turn_index: i64,
    model_rounds: i64,
) -> Vec<RoundPlan> {
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

/// Pass-1 result shared by both backends: per-turn costs plus the simulated
/// clock anchor so the thread reads as recent history.
pub(crate) struct StressTimelinePlan {
    pub costs: Vec<TurnCost>,
    pub start_time: f64,
    pub total_change_rows: i64,
    pub change_tail_cutoff: i64,
}

pub(crate) fn plan_timeline(
    base_seed: u64,
    now: f64,
    spec: &StressThreadSpec,
) -> Result<StressTimelinePlan> {
    validate_spec(spec)?;
    let mut rng = Rng::new(base_seed);
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
    Ok(StressTimelinePlan {
        costs,
        start_time,
        total_change_rows,
        change_tail_cutoff,
    })
}

/// Committed-item change payload shape (matches committed_item_payload).
pub(crate) fn committed_item_json(
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

pub(crate) fn profile_hint(profiles: &[(&ToolProfile, f64)]) -> String {
    match profiles.first() {
        Some((profile, _)) => format!("已调用 {}", profile.name),
        None => "继续推理".to_string(),
    }
}
