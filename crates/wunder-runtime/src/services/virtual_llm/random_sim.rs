//! 无回放日志时的随机模拟：把一次用户轮次展开为随机 1-100 个模型轮次。
//! 中间轮次发出无害只读工具调用，最终轮次输出总结文本。所有行为由
//! (session, user_round, model_round) 确定性推导，取消恢复与重放保持一致。

use super::{VirtualReplayTurn, RANDOM_REPLAY_FORMAT, RANDOM_REPLAY_LOG_ID};
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 每个用户轮次的计划轮次上限（1-100 均匀分布）。
const MAX_PLAN_ROUNDS: u64 = 100;
/// 命中任一只读关键词的工具才可能被随机调用。
const READ_ONLY_HINTS: &[&str] = &[
    "read", "list", "ls", "glob", "grep", "search", "find", "get", "query", "view", "show",
    "inspect", "describe", "stat", "status", "time", "recall", "读取", "列出", "搜索", "查找",
    "检索", "查看", "浏览", "获取", "查询", "状态", "时间", "召回",
];
/// 命中任一排除关键词的工具一律跳过，即使它同时命中只读关键词。
const BLOCKED_HINTS: &[&str] = &[
    "write", "edit", "delete", "remove", "exec", "command", "run", "apply", "patch", "create",
    "update", "insert", "upload", "send", "post", "kill", "写入", "编辑", "删除", "执行", "运行",
    "命令", "创建", "更新", "上传", "发送", "子智能体", "定时", "计划", "问询", "记忆管理", "技能",
];
/// 单轮最多同时发出的工具调用数。
const MAX_CALLS_PER_ROUND: usize = 2;
const SAMPLE_WORDS: &[&str] = &[
    "配置", "结构", "说明", "结果", "摘要", "示例", "入口", "依赖", "日志", "错误", "步骤",
    "参数", "目录", "缓存", "接口", "状态",
];
/// 人类发起的轮次中，平均每多少轮登记一次长程目标（1/N 概率）。
const GOAL_ENTRY_ONE_IN: u64 = 3;
/// 目标轮次上限上限：足够大以便被目标持续驱动，直到人工停止。
const GOAL_MAX_ROUNDS: i64 = 10_000;
/// 合成的长程目标描述，贴近真实的可持续推进任务。
const GOAL_OBJECTIVES: &[&str] = &[
    "持续跟踪当前仓库的关键变更，并逐轮汇总进展，直到我要求停止。",
    "以一个较长的调研目标为线索，循环执行只读检查并输出阶段性结论。",
    "围绕当前主题持续推进，边查证边产出结论，直到我手动中断。",
    "把它当成一个需要长期推进的任务：每轮都往前推进一步并记录状态。",
];
/// 子智能体控制工具的规范名（在册校验以此为准）。
const SUBAGENT_TOOL_NAME: &str = "子智能体控制";
/// 首轮中平均每多少轮发出一条子智能体调用（1/N 概率）。
const SUBAGENT_ACTION_ONE_IN: u64 = 4;
/// 用户消息可覆盖的本次计划模型轮次上限（避免一次测试跑出过多轮次）。
const MAX_ROUND_OVERRIDE: u64 = 2000;
/// 合成子任务描述：真实派生子智能体时作为 `task` 参数。
const SUBAGENT_TASKS: &[&str] = &[
    "在只读范围内梳理当前目录结构并汇报。",
    "核对指定主题的相关资料并给出要点摘要。",
    "排查一处疑似问题并给出结论与证据。",
    "汇总最近变更并列出需要关注的条目。",
];

/// 从请求提供的工具中筛选可安全随机调用的只读工具摘要 (名称, 参数 schema)。
/// 必须在进入 'static 闭包前完成提取，结果为 owned 数据。
pub(super) fn tool_summaries(tools: Option<&[Value]>) -> Vec<(String, Value)> {
    let Some(items) = tools else {
        return Vec::new();
    };
    let mut summaries = Vec::new();
    for tool in items {
        let Some(name) = tool
            .pointer("/function/name")
            .or_else(|| tool.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let canonical = crate::tools::resolve_tool_name(name).to_lowercase();
        let raw = name.to_lowercase();
        if BLOCKED_HINTS
            .iter()
            .any(|hint| canonical.contains(hint) || raw.contains(hint))
        {
            continue;
        }
        if !READ_ONLY_HINTS
            .iter()
            .any(|hint| canonical.contains(hint) || raw.contains(hint))
        {
            continue;
        }
        let schema = tool
            .pointer("/function/parameters")
            .or_else(|| tool.get("input_schema"))
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        summaries.push((name.to_string(), schema));
    }
    summaries
}

/// 请求中提供的 `create_goal` 工具（若运行时暴露了它）。
///
/// `create_goal` 会被 [`tool_summaries`] 的写操作过滤掉（名字里含 create），
/// 但目标编排是运行时级能力，这里单独取出来供目标入口使用，并沿用模型实际
/// 看到的工具名（可能是本地化别名），确保 tool_call 能通过在册校验。
pub(super) fn goal_entry_candidate(tools: Option<&[Value]>) -> Option<(String, Value)> {
    let items = tools?;
    items.iter().find_map(|tool| {
        let name = tool
            .pointer("/function/name")
            .or_else(|| tool.get("name"))
            .and_then(Value::as_str)?;
        let canonical = crate::tools::resolve_tool_name(name);
        let is_create_goal = canonical.trim() == crate::services::goal::TOOL_CREATE_GOAL
            || name.trim() == crate::services::goal::TOOL_CREATE_GOAL;
        if !is_create_goal {
            return None;
        }
        let schema = tool
            .pointer("/function/parameters")
            .or_else(|| tool.get("input_schema"))
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        Some((name.to_string(), schema))
    })
}

/// 请求中提供的子智能体控制工具（若运行时暴露了它）。
///
/// 该工具会被 [`tool_summaries`] 的“子智能体”排除词过滤掉（避免它被当作普通
/// 只读工具随机调用），这里单独取出来，用于发出真实的子智能体调用（见
/// [`subagent_arguments`]），让模拟轨迹更贴近真实工具面。递归由调用方用
/// `allow_subagent` 兜底（子会话不再派生）。
pub(super) fn subagent_entry_candidate(tools: Option<&[Value]>) -> Option<(String, Value)> {
    let items = tools?;
    items.iter().find_map(|tool| {
        let name = tool
            .pointer("/function/name")
            .or_else(|| tool.get("name"))
            .and_then(Value::as_str)?;
        let canonical = crate::tools::resolve_tool_name(name);
        if canonical.trim() != SUBAGENT_TOOL_NAME && name.trim() != SUBAGENT_TOOL_NAME {
            return None;
        }
        let schema = tool
            .pointer("/function/parameters")
            .or_else(|| tool.get("input_schema"))
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        Some((name.to_string(), schema))
    })
}

/// 构造子智能体调用参数：真实派生或巡检子任务。
///
/// 默认派生一个子任务（`spawn`），并周期性派发一批（`batch_spawn`），偶尔用
/// `list` 做一次无副作用巡检。这些动作在编排器里都会**真实执行**；递归由调用方
/// 兜底——被派生的子会话自身不再被允许派生子智能体（见 `load_turn_for_round`
/// 的 `allow_subagent`）。
fn subagent_arguments(rng: &mut u64) -> Value {
    match next_u64(rng) % 5 {
        0 => json!({ "action": "list", "limit": 20 }),
        1 => json!({
            "action": "batch_spawn",
            "tasks": [
                { "task": pick(SUBAGENT_TASKS, rng), "label": "sim-child-1" },
                { "task": pick(SUBAGENT_TASKS, rng), "label": "sim-child-2" },
            ],
        }),
        _ => json!({
            "action": "spawn",
            "task": pick(SUBAGENT_TASKS, rng),
            "label": "sim-child",
        }),
    }
}

/// 从用户消息中解析“本次计划模型轮次数”覆盖值：取消息里第一个纯数字 token。
///
/// 例如 `100` 或 `100 subagent` -> 100；`跑 12 轮` -> 12。结果被夹到
/// `1..=MAX_ROUND_OVERRIDE`，因而“用户输入一个数字就决定本次模型轮次”这一
/// 测试约定被严格遵守。`abc100` 这类以字母开头的 token 不会被误读。
pub(super) fn parse_round_override(message: &str) -> Option<usize> {
    message.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']'
                    | '，' | '。' | '；' | '：' | '！' | '？' | '、' | '（' | '）'
            )
        });
        if token.is_empty() || !token.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        token
            .parse::<u64>()
            .ok()
            .map(|value| value.clamp(1, MAX_ROUND_OVERRIDE) as usize)
    })
}

/// 用户消息是否要求本次必定调用子智能体工具（大小写不敏感的 `subagent`）。
pub(super) fn wants_subagent(message: &str) -> bool {
    message.to_ascii_lowercase().contains("subagent")
}

/// 构造 `create_goal` 的合法参数：必需 objective + 一个把轮次上限顶到天花板的
/// `max_goal_rounds`，从而实现“被目标持续驱动直到人工停止”。
fn goal_create_arguments(schema: &Value, rng: &mut u64) -> Value {
    let mut arguments = Map::new();
    arguments.insert("objective".to_string(), json!(pick(GOAL_OBJECTIVES, rng)));
    let declares_limit = schema
        .get("properties")
        .and_then(|properties| properties.get("max_goal_rounds"))
        .is_some();
    if declares_limit {
        arguments.insert(
            "max_goal_rounds".to_string(),
            json!(GOAL_MAX_ROUNDS),
        );
    }
    Value::Object(arguments)
}

/// 同一 (session, user_round) 的计划轮次数：1-100 均匀分布，跨模型轮次稳定。
pub(super) fn plan_rounds(session_seed: &str, user_round: usize) -> usize {
    let seed = mix(session_seed, user_round as u64);
    (1 + seed % MAX_PLAN_ROUNDS) as usize
}

pub(super) fn build_turn(
    session_seed: &str,
    round: usize,
    model_round: usize,
    total_rounds: usize,
    tools: &[(String, Value)],
    goal_entry: Option<&(String, Value)>,
    // 子智能体工具入口，附带“本轮是否强制发出”的开关。用元组承载强制位，这样
    // 只有 `Some(..)` 的调用点需要关心它，`None` 的调用点无需改动。
    subagent_entry: Option<(&(String, Value), bool)>,
) -> VirtualReplayTurn {
    let mut rng = mix(session_seed, ((round as u64) << 32) | model_round as u64).max(1);
    if model_round < total_rounds && !tools.is_empty() {
        build_tool_turn(
            round,
            model_round,
            total_rounds,
            tools,
            goal_entry,
            subagent_entry,
            &mut rng,
        )
    } else {
        build_final_turn(round, model_round, total_rounds, &mut rng)
    }
}

fn build_tool_turn(
    round: usize,
    model_round: usize,
    total_rounds: usize,
    candidates: &[(String, Value)],
    goal_entry: Option<&(String, Value)>,
    subagent_entry: Option<(&(String, Value), bool)>,
    rng: &mut u64,
) -> VirtualReplayTurn {
    let mut calls = Vec::with_capacity(MAX_CALLS_PER_ROUND);
    // 确定性抽样的部分轮次会登记一个长程目标，让目标驱动器持续排轮次，直到
    // 人工停止。目标管理需要直接的人类授权，因此只在人类发起的轮次触发
    // （驱动器派发的目标轮里 goal_entry 为 None）。
    let opens_goal = model_round == 1
        && goal_entry.is_some()
        && next_u64(rng) % GOAL_ENTRY_ONE_IN == 0;
    let mut special_calls = 0usize;
    if opens_goal {
        if let Some((name, schema)) = goal_entry {
            let args = goal_create_arguments(schema, rng);
            calls.push(json!({
                "id": format!("sim_{round}_{model_round}_goal"),
                "type": "function",
                "function": {"name": name, "arguments": args.to_string()},
            }));
            special_calls += 1;
        }
    }
    // 子智能体工具：与目标入口同为“首轮、确定性抽样”的特殊动作。这里发出的是
    // **真实**的派生/巡检调用（见 subagent_arguments）；递归由调用方通过
    // `allow_subagent` 兜底——子会话不会再被派生子智能体。用户消息含 `subagent`
    // 时强制位为真，本轮必定发出；否则平均每 SUBAGENT_ACTION_ONE_IN 轮抽样一次。
    // 仅在提供该工具时才消耗随机数，保持与历史轨迹一致的确定性。
    let opens_subagent = match subagent_entry {
        Some((_, force)) if model_round == 1 => {
            let roll = next_u64(rng);
            force || roll % SUBAGENT_ACTION_ONE_IN == 0
        }
        _ => false,
    };
    if opens_subagent {
        if let Some(((name, _schema), _)) = subagent_entry {
            let args = subagent_arguments(rng);
            calls.push(json!({
                "id": format!("sim_{round}_{model_round}_subagent"),
                "type": "function",
                "function": {"name": name, "arguments": args.to_string()},
            }));
            special_calls += 1;
        }
    }
    // 特殊动作占据名额后，只读调用相应减少，整体不超过 MAX_CALLS_PER_ROUND。
    let read_only_calls = if special_calls >= MAX_CALLS_PER_ROUND {
        0
    } else if special_calls > 0 {
        1
    } else {
        1 + (next_u64(rng) as usize) % MAX_CALLS_PER_ROUND
    };
    for index in 0..read_only_calls {
        let (name, schema) = &candidates[(next_u64(rng) as usize) % candidates.len()];
        let args = sample_arguments(schema, rng);
        calls.push(json!({
            "id": format!("sim_{round}_{model_round}_{index}"),
            "type": "function",
            "function": {"name": name, "arguments": args.to_string()},
        }));
    }
    let mut reasoning = format!(
        "{}预计还需 {} 轮完成本组工作。",
        pick(
            &[
                "先收集必要的上下文，再决定下一步操作。",
                "需要确认当前状态，避免基于过时信息行动。",
                "把当前发现整理出来，作为后续输入。",
                "继续补充信息，确保结论有依据。",
                "这一步只做只读探查，不会改动任何数据。",
                "先把范围缩小，避免无关结果干扰判断。",
                "上一次的结果指向下一处需要核实的位置。",
                "确认没有遗漏关键上下文后再继续。",
            ],
            rng
        ),
        total_rounds.saturating_sub(model_round)
    );
    if opens_goal {
        reasoning = "这是一项需要持续推进的工作，先登记目标，再逐轮展开。".to_string();
    } else if opens_subagent {
        reasoning = "先委派子智能体处理其中一部分工作，再继续推进。".to_string();
    }
    let content = if next_u64(rng) % 3 != 0 {
        pick(
            &[
                "我先检查一下当前的情况。",
                "正在获取相关数据。",
                "继续收集信息。",
                "接下来核对相关的几处细节。",
                "先读取必要的信息再作判断。",
                "补充一些上下文，方便后续处理。",
            ],
            rng,
        )
        .to_string()
    } else {
        String::new()
    };
    let content = if opens_goal {
        "我会把它作为一个持续任务来推进，先建立目标。".to_string()
    } else if opens_subagent {
        "先派一个子智能体去处理这部分工作。".to_string()
    } else {
        content
    };
    VirtualReplayTurn {
        finish_reason: None,
        content,
        reasoning,
        usage: None,
        tool_calls: Some(Value::Array(calls)),
        source_log_id: RANDOM_REPLAY_LOG_ID.to_string(),
        source_log_name: crate::i18n::t("virtual_llm.random.log_name"),
        source_round: round,
        source_model_round: Some(model_round),
        format: RANDOM_REPLAY_FORMAT.to_string(),
    }
}

fn build_final_turn(
    round: usize,
    model_round: usize,
    total_rounds: usize,
    rng: &mut u64,
) -> VirtualReplayTurn {
    let mut sections: Vec<String> = Vec::with_capacity(5);
    sections.push(format!(
        "本轮处理已完成，共执行 {} 个模型轮次。以下是本次工作的整理结果。",
        model_round.min(total_rounds).max(1)
    ));
    sections.push("执行过程".to_string());
    let steps = [
        "确认输入要求，明确处理范围。",
        "收集相关上下文信息并逐一核对。",
        "对收集到的信息进行整理与比对。",
        "汇总各步骤结果，形成初步结论。",
        "复核输出内容，确认没有遗漏项。",
    ];
    let step_count = 2 + (next_u64(rng) as usize % 4);
    let start = (next_u64(rng) as usize) % (steps.len() - step_count + 1);
    let mut list = String::from("\n");
    for (index, step) in steps[start..start + step_count].iter().enumerate() {
        list.push_str(&format!("{}. {}\n", index + 1, step));
    }
    sections.push(list);
    if next_u64(rng) % 5 < 2 {
        sections.push(format!(
            "关键数据摘要：\n\n```text\nround: {round}\nmodel_rounds: {model_round}\nsamples: {}\n```",
            (next_u64(rng) % 900) + 100
        ));
    }
    sections.push(
        pick(
            &[
                "整体流程正常完成，结果已就绪。如需进一步处理，可以直接继续对话说明要求。",
                "以上为本轮工作的汇总。若需要调整方向或补充细节，请直接说明。",
            ],
            rng,
        )
        .to_string(),
    );
    let reasoning = pick(
        &[
            "信息已经足够，整理要点并给出最终答复。",
            "各步骤结果一致，可以收尾汇总。",
            "没有发现阻碍结论的异常，输出最终整理结果。",
        ],
        rng,
    );
    VirtualReplayTurn {
        finish_reason: None,
        content: sections.join("\n\n"),
        reasoning: reasoning.to_string(),
        usage: None,
        tool_calls: None,
        source_log_id: RANDOM_REPLAY_LOG_ID.to_string(),
        source_log_name: crate::i18n::t("virtual_llm.random.log_name"),
        source_round: round,
        source_model_round: Some(model_round),
        format: RANDOM_REPLAY_FORMAT.to_string(),
    }
}

/// 依据 input_schema 生成必填参数；启发式取值让只读工具大概率执行成功。
fn sample_arguments(schema: &Value, rng: &mut u64) -> Value {
    sample_object(schema, rng, 0)
}

fn sample_object(schema: &Value, rng: &mut u64, depth: u32) -> Value {
    let mut map = Map::new();
    if depth > 2 {
        return Value::Object(map);
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    for key in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = key.as_str() else { continue };
        let prop_schema = properties
            .and_then(|props| props.get(name))
            .cloned()
            .unwrap_or_else(|| json!({}));
        map.insert(name.to_string(), sample_value(name, &prop_schema, rng, depth + 1));
    }
    Value::Object(map)
}

fn sample_value(name: &str, schema: &Value, rng: &mut u64, depth: u32) -> Value {
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        return first.clone();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("integer" | "number") => json!(numeric_argument(name, rng)),
        Some("boolean") => json!(false),
        Some("array") => {
            let item_schema = schema.get("items").cloned().unwrap_or_else(|| json!({}));
            json!([sample_value(name, &item_schema, rng, depth + 1)])
        }
        Some("object") => sample_object(schema, rng, depth),
        _ => json!(string_argument(name, rng)),
    }
}

fn string_argument(name: &str, rng: &mut u64) -> String {
    let lowered = name.to_lowercase();
    if lowered.contains("path") || lowered.contains("dir") || lowered.contains("folder") {
        return ".".into();
    }
    if lowered.contains("pattern") || lowered.contains("glob") {
        return "*".into();
    }
    if lowered.contains("url") || lowered.contains("link") {
        return "https://example.com".into();
    }
    if lowered.contains("query")
        || lowered.contains("keyword")
        || lowered.contains("search")
        || lowered.contains("text")
        || lowered.contains("content")
    {
        return pick(SAMPLE_WORDS, rng).to_string();
    }
    format!("sample-{}", (next_u64(rng) % 900) + 100)
}

fn numeric_argument(name: &str, rng: &mut u64) -> i64 {
    let lowered = name.to_lowercase();
    if lowered.contains("limit") || lowered.contains("count") || lowered.contains("size") {
        return ((next_u64(rng) % 40) + 10) as i64;
    }
    1
}

fn pick<'a>(items: &[&'a str], rng: &mut u64) -> &'a str {
    items[(next_u64(rng) as usize) % items.len()]
}

/// FNV 思路的 64bit 混合，同一输入跨重启稳定。
pub(super) fn mix(session_seed: &str, nonce: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    session_seed.hash(&mut hasher);
    nonce.hash(&mut hasher);
    let seed = hasher.finish();
    if seed == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        seed
    }
}

pub(super) fn next_u64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(name: &str, schema: Value) -> (String, Value) {
        (name.to_string(), schema)
    }

    #[test]
    fn plan_rounds_is_deterministic_and_within_bounds() {
        for round in 1..=50 {
            let plan = plan_rounds("session-a", round);
            assert!((1..=100).contains(&plan));
            assert_eq!(plan, plan_rounds("session-a", round));
        }
        assert_ne!(
            plan_rounds("session-a", 1),
            plan_rounds("session-b", 1),
            "different sessions should diverge"
        );
    }

    #[test]
    fn tool_summaries_filter_offered_readonly_tools() {
        let tools = vec![
            json!({"type":"function","function":{"name":"读取文件","parameters":json!({"type":"object"})}}),
            json!({"type":"function","function":{"name":"写入文件","parameters":json!({"type":"object"})}}),
            json!({"type":"function","function":{"name":"执行命令","parameters":json!({"type":"object"})}}),
        ];
        let summaries = tool_summaries(Some(&tools));
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].0, "读取文件");
        assert!(tool_summaries(None).is_empty());
    }

    #[test]
    fn tool_rounds_emit_only_offered_readonly_tools() {
        let candidates = vec![summary(
            "读取文件",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        )];
        for model_round in 1..30 {
            let turn = build_turn("session-a", 1, model_round, 40, &candidates, None, None);
            if model_round < 40 {
                let calls = turn
                    .tool_calls
                    .as_ref()
                    .and_then(Value::as_array)
                    .expect("tool turn has calls");
                for call in calls {
                    let name = call.pointer("/function/name").and_then(Value::as_str).unwrap();
                    assert_eq!(name, "读取文件");
                    assert!(call.pointer("/function/arguments").is_some());
                }
            } else {
                assert!(turn.tool_calls.is_none());
                assert!(!turn.content.trim().is_empty());
            }
        }
    }

    #[test]
    fn no_tools_means_single_final_turn() {
        for model_round in 1..10 {
            let turn = build_turn("session-a", 1, model_round, 10, &[], None, None);
            assert!(turn.tool_calls.is_none());
            assert!(!turn.content.trim().is_empty());
        }
    }

    #[test]
    fn final_turn_completes_when_plan_exhausted() {
        let candidates = vec![summary(
            "列出文件",
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        )];
        let turn = build_turn("session-a", 3, 20, 20, &candidates, None, None);
        assert!(turn.tool_calls.is_none());
        assert!(turn.content.contains("模型轮次"));
    }

    #[test]
    fn required_arguments_follow_schema() {
        let mut rng = 42_u64.max(1);
        let schema = json!({"type":"object","required":["pattern","limit"],"properties":{
            "pattern":{"type":"string"},
            "limit":{"type":"integer"},
            "recursive":{"type":"boolean"}
        }});
        let args = sample_arguments(&schema, &mut rng);
        assert!(args.get("pattern").and_then(Value::as_str).is_some());
        assert!(args.get("limit").and_then(Value::as_i64).is_some());
        assert!(args.get("recursive").is_none(), "optional fields stay unset");
    }

    #[test]
    fn goal_candidate_matches_localized_create_goal() {
        let tools = vec![
            json!({"type":"function","function":{"name":"读取文件","parameters":json!({"type":"object"})}}),
            json!({"type":"function","function":{"name":"create_goal","parameters":json!({"type":"object","required":["objective"],"properties":{"objective":{"type":"string"}}})}}),
        ];
        let candidate = goal_entry_candidate(Some(&tools)).expect("create_goal is offered");
        assert_eq!(candidate.0, "create_goal");
        assert!(goal_entry_candidate(None).is_none());
        // create_goal is a goal tool, never treated as a plain read-only tool.
        assert!(tool_summaries(Some(&tools))
            .iter()
            .all(|(name, _)| name != "create_goal"));
    }

    #[test]
    fn goal_entry_is_deterministic_and_only_on_first_model_round() {
        let candidates = vec![summary(
            "读取文件",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        )];
        let goal = (
            "create_goal".to_string(),
            json!({"type":"object","required":["objective"],"properties":{
                "objective":{"type":"string"},
                "max_goal_rounds":{"type":"integer"}
            }}),
        );
        let mut opened = 0;
        for round in 1..=24 {
            // Later model rounds never re-open a goal.
            let later = build_turn("session-a", round, 2, 40, &candidates, Some(&goal), None);
            let later_calls = later.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            assert!(later_calls
                .iter()
                .all(|call| call.pointer("/function/name").and_then(Value::as_str) != Some("create_goal")));
            // The first model round may open a goal, and only deterministically.
            let first = build_turn("session-a", round, 1, 40, &candidates, Some(&goal), None);
            let again = build_turn("session-a", round, 1, 40, &candidates, Some(&goal), None);
            assert_eq!(first.tool_calls, again.tool_calls, "goal entry must be reproducible");
            let calls = first.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            assert!(calls.len() <= MAX_CALLS_PER_ROUND);
            if let Some(call) = calls
                .iter()
                .find(|call| call.pointer("/function/name").and_then(Value::as_str) == Some("create_goal"))
            {
                opened += 1;
                let args: Value = serde_json::from_str(
                    call.pointer("/function/arguments").and_then(Value::as_str).expect("args"),
                )
                .expect("goal arguments are json");
                assert!(args.get("objective").and_then(Value::as_str).is_some());
                assert_eq!(
                    args.get("max_goal_rounds").and_then(Value::as_i64),
                    Some(GOAL_MAX_ROUNDS)
                );
            }
        }
        assert!(opened > 0, "at least one human turn should arm a goal");
        assert!(opened < 24, "goal entry stays a deterministic subset, not every turn");
        // A driver-issued round never sees a goal candidate.
        let blocked = build_turn("session-a", 1, 1, 40, &candidates, None, None);
        let blocked_calls = blocked.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
        assert!(blocked_calls
            .iter()
            .all(|call| call.pointer("/function/name").and_then(Value::as_str) != Some("create_goal")));
    }

    #[test]
    fn subagent_candidate_matches_localized_control_tool() {
        let tools = vec![
            json!({"type":"function","function":{"name":"读取文件","parameters":json!({"type":"object"})}}),
            json!({"type":"function","function":{"name":"subagent_control","parameters":json!({"type":"object","required":["action"],"properties":{"action":{"type":"string"}}})}}),
        ];
        let candidate = subagent_entry_candidate(Some(&tools)).expect("subagent_control offered");
        assert_eq!(candidate.0, "subagent_control");
        assert!(subagent_entry_candidate(None).is_none());
        // 子智能体工具被排除词挡在只读候选之外，不会作为普通只读工具重复出现。
        assert!(tool_summaries(Some(&tools))
            .iter()
            .all(|(name, _)| name != "subagent_control"));
    }

    #[test]
    fn subagent_call_only_on_first_round_and_deterministic() {
        let candidates = vec![summary(
            "读取文件",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        )];
        let subagent = summary(
            "子智能体控制",
            json!({"type":"object","required":["action"],"properties":{"action":{"type":"string"}}}),
        );
        let mut emitted = 0;
        for round in 1..=40 {
            // 后续模型轮次永不发出子智能体调用。
            let later =
                build_turn("session-a", round, 2, 40, &candidates, None, Some((&subagent, false)));
            let later_calls = later.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            assert!(later_calls
                .iter()
                .all(|call| call.pointer("/function/name").and_then(Value::as_str)
                    != Some("子智能体控制")));
            // 首轮可确定性触发，且可复现。
            let first = build_turn("session-a", round, 1, 40, &candidates, None, Some((&subagent, false)));
            let again = build_turn("session-a", round, 1, 40, &candidates, None, Some((&subagent, false)));
            assert_eq!(first.tool_calls, again.tool_calls, "subagent entry reproducible");
            let calls = first.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            assert!(calls.len() <= MAX_CALLS_PER_ROUND);
            if let Some(call) = calls
                .iter()
                .find(|call| call.pointer("/function/name").and_then(Value::as_str)
                    == Some("子智能体控制"))
            {
                emitted += 1;
                let args: Value = serde_json::from_str(
                    call.pointer("/function/arguments").and_then(Value::as_str).expect("args"),
                )
                .expect("subagent arguments are json");
                // 真实动作：spawn / batch_spawn / list 三者之一。
                let action = args.get("action").and_then(Value::as_str).expect("action");
                assert!(matches!(action, "spawn" | "batch_spawn" | "list"));
            }
        }
        assert!(emitted > 0, "at least one human turn should inspect subagents");
        assert!(emitted < 40, "subagent entry stays a deterministic subset");
        // 未提供该工具时，永不发出。
        let absent = build_turn("session-a", 1, 1, 40, &candidates, None, None);
        let absent_calls = absent.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
        assert!(absent_calls
            .iter()
            .all(|call| call.pointer("/function/name").and_then(Value::as_str)
                != Some("子智能体控制")));
    }

    #[test]
    fn parse_round_override_reads_first_number_token() {
        assert_eq!(parse_round_override("100"), Some(100));
        assert_eq!(parse_round_override("100 subagent"), Some(100));
        assert_eq!(parse_round_override("跑 12 轮"), Some(12));
        assert_eq!(parse_round_override("no digits here"), None);
        assert_eq!(parse_round_override("abc100"), None);
        assert_eq!(parse_round_override("0"), Some(1));
        assert_eq!(parse_round_override("999999"), Some(MAX_ROUND_OVERRIDE as usize));
    }

    #[test]
    fn wants_subagent_is_case_insensitive() {
        assert!(wants_subagent("please SubAgent this"));
        assert!(wants_subagent("subagent"));
        assert!(!wants_subagent("100"));
    }

    #[test]
    fn forced_subagent_dispatches_on_first_round() {
        let candidates = vec![summary(
            "读取文件",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        )];
        let subagent = summary(
            "子智能体控制",
            json!({"type":"object","required":["action"],"properties":{"action":{"type":"string"}}}),
        );
        let mut dispatched = 0;
        for i in 0..8 {
            let seed = format!("session-forced-{i}");
            let turn = build_turn(&seed, 1, 1, 50, &candidates, None, Some((&subagent, true)));
            let calls = turn.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            let call = calls
                .iter()
                .find(|call| call.pointer("/function/name").and_then(Value::as_str)
                    == Some("子智能体控制"))
                .expect("forced subagent call present");
            let args: Value = serde_json::from_str(
                call.pointer("/function/arguments").and_then(Value::as_str).expect("args"),
            )
            .expect("subagent arguments are json");
            let action = args.get("action").and_then(Value::as_str).expect("action");
            assert!(matches!(action, "spawn" | "batch_spawn" | "list"));
            if matches!(action, "spawn" | "batch_spawn") {
                dispatched += 1;
            }
            // 强制位只在首轮生效；后续轮次不再发出子智能体调用。
            let later = build_turn(&seed, 1, 2, 50, &candidates, None, Some((&subagent, true)));
            let later_calls = later.tool_calls.as_ref().and_then(Value::as_array).expect("calls");
            assert!(later_calls
                .iter()
                .all(|call| call.pointer("/function/name").and_then(Value::as_str)
                    != Some("子智能体控制")));
        }
        assert!(dispatched > 0, "forced subagent should really dispatch");
    }
}
