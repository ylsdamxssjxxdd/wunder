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
const SAMPLE_WORDS: &[&str] = &["配置", "结构", "说明", "结果", "摘要", "示例"];

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
) -> VirtualReplayTurn {
    let mut rng = mix(session_seed, ((round as u64) << 32) | model_round as u64).max(1);
    if model_round < total_rounds && !tools.is_empty() {
        build_tool_turn(round, model_round, total_rounds, tools, &mut rng)
    } else {
        build_final_turn(round, model_round, total_rounds, &mut rng)
    }
}

fn build_tool_turn(
    round: usize,
    model_round: usize,
    total_rounds: usize,
    candidates: &[(String, Value)],
    rng: &mut u64,
) -> VirtualReplayTurn {
    let call_count = 1 + (next_u64(rng) as usize) % MAX_CALLS_PER_ROUND;
    let mut calls = Vec::with_capacity(call_count);
    for index in 0..call_count {
        let (name, schema) = &candidates[(next_u64(rng) as usize) % candidates.len()];
        let args = sample_arguments(schema, rng);
        calls.push(json!({
            "id": format!("sim_{round}_{model_round}_{index}"),
            "type": "function",
            "function": {"name": name, "arguments": args.to_string()},
        }));
    }
    let reasoning = format!(
        "{}预计还需 {} 轮完成本组工作。",
        pick(
            &[
                "先收集必要的上下文，再决定下一步操作。",
                "需要确认当前状态，避免基于过时信息行动。",
                "把当前发现整理出来，作为后续输入。",
                "继续补充信息，确保结论有依据。",
            ],
            rng
        ),
        total_rounds.saturating_sub(model_round)
    );
    let content = if next_u64(rng) % 2 == 0 {
        pick(
            &[
                "我先检查一下当前的情况。",
                "正在获取相关数据。",
                "继续收集信息。",
            ],
            rng,
        )
        .to_string()
    } else {
        String::new()
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
fn mix(session_seed: &str, nonce: u64) -> u64 {
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

fn next_u64(state: &mut u64) -> u64 {
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
            let turn = build_turn("session-a", 1, model_round, 40, &candidates);
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
            let turn = build_turn("session-a", 1, model_round, 10, &[]);
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
        let turn = build_turn("session-a", 3, 20, 20, &candidates);
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
}
