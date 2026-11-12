use super::{build_model_tool_success_with_hint, context::ToolContext};
use crate::i18n;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

/// 计划条目里可以当“步骤文字”用的键。模型在中文或英文场景下常换键名，
/// 接住这些别名比让整次调用失败划算得多。
const PLAN_ITEM_TEXT_KEYS: &[&str] = &["step", "title", "content", "text", "task", "description"];
/// 计划列表本身的键名别名。
const PLAN_LIST_KEYS: &[&str] = &["plan", "steps", "todos", "items"];
/// 整份参数被写成字符串时的外层键名：文本/XML 协议会把 `<input>` 落成 `input`，
/// 宽松恢复路径会落成 `raw`。
/// 计划参数的顶层字段白名单：超出这个集合的字段一律报错，不再静默丢弃。
/// `current` 只作为旧形态兼容保留（不再出现在 schema 与工具面文案里），
/// 有显式 status 时一律以 status 为准，不会因为两者并存而失败。
const PLAN_TOP_LEVEL_KEYS: &[&str] = &[
    "plan",
    "steps",
    "todos",
    "items",
    "explanation",
    "note",
    "current",
];
/// 合法状态值：schema 里也只列这三个，别名在解析时归一。
const PLAN_STATUS_VALUES: &[&str] = &["pending", "in_progress", "completed"];
/// 报错里回显给模型的示范形态，省得它再猜一遍：只有一种形态，
/// 每步必带 status，没有第二种写法可选（对齐 codex 的 update_plan）。
const PLAN_SHAPE_EXAMPLE: &str =
    r#"{"plan":[{"step":"甲","status":"in_progress"},{"step":"乙","status":"pending"}]}"#;
/// 步骤条目内部允许出现的键，同样不静默跳过未知键。
const PLAN_ITEM_KEYS: &[&str] = &[
    "step",
    "title",
    "content",
    "text",
    "task",
    "description",
    "status",
    "state",
];
const PLAN_WRAPPER_KEYS: &[&str] = &[
    "raw",
    "input",
    "value",
    "arguments",
    "args",
    "payload",
    "text",
];

pub(crate) async fn execute_plan_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    // 先走全局的宽松恢复（转义引号、换行等），再补计划工具自己的包装解包。
    let recovery = recover_plan_args(&super::recover_tool_args_value(args));
    if let Some(broken) = recovery.broken.as_ref() {
        return Ok(unparsable_wrapper(broken).into_tool_result());
    }
    let args = &recovery.value;
    let request = match parse_plan_request(args) {
        Ok(request) => request,
        Err(error) => return Ok(error.into_tool_result()),
    };
    if request.steps.is_empty() {
        return Ok(
            PlanArgError::shape(i18n::t("tool.plan.plan_required"), json!({ "keys": [] }))
                .with_code("TOOL_PLAN_EMPTY")
                .into_tool_result(),
        );
    }
    let normalized_plan = request.normalized_plan();
    let progress = plan_progress(&normalized_plan);
    let hint = progress.hint();
    // hint 同时放进 data：模型侧的通用投影会剥掉外层 summary/next_step_hint，
    // 但 data 里的字段一定能读回来，否则「继续维护面板」的提醒等于没发。
    let mut data = json!({
        "status": "ok",
        "completed": progress.done,
        "total": progress.total,
        "current_step": progress.current,
        "next_step": progress.next,
    });
    if let Some(text) = hint.clone() {
        data["hint"] = Value::String(text);
    }
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "plan_update",
            json!({
                "explanation": request.explanation,
                "plan": normalized_plan
            }),
        );
    }
    // 回执是给模型的状态卡：进度、当前步、下一步都在这里，模型不必回读历史
    // 就知道还剩什么，也才有依据在收尾时把最后一步标成已完成。
    Ok(build_model_tool_success_with_hint(
        "plan_update",
        "completed",
        progress.summary(),
        data,
        hint,
    ))
}

/// 计划参数的形状容错。模型（尤其是文本/XML 协议下）常把整份参数写成一个
/// JSON 字符串、把 plan 列表写成字符串，或直接传一个数组，这些都应被接住；
/// 接不住时要落到带示例与键名的可执行报错上，而不是 serde 的
/// “missing field `plan`”。
/// 参数恢复结果：能用的参数，外加“包装键存在但里面不是合法 JSON”的诊断。
/// 后者必须带上 serde 的原文位置和原文片段：模型写坏一个引号时，
/// 含糊的“别传字符串”会把它引到错误的排查方向（重试几次都改不到点上）。
struct PlanRecovery {
    value: Value,
    broken: Option<PlanJsonBroken>,
}

/// 一次解不开的包装 JSON：wrapper 是外层键名，error 是 serde 给的
/// “expected `:` at line 1 column 15” 之类定位，snippet 是原文开头。
struct PlanJsonBroken {
    wrapper: String,
    error: String,
    snippet: String,
}

impl PlanRecovery {
    fn ok(value: Value) -> Self {
        Self {
            value,
            broken: None,
        }
    }

    fn broken(raw: &str, wrapper: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            value: Value::Null,
            broken: Some(PlanJsonBroken {
                wrapper: wrapper.into(),
                error: error.into(),
                snippet: snippet(raw),
            }),
        }
    }
}

fn recover_plan_args(args: &Value) -> PlanRecovery {
    match args {
        Value::String(text) => match embedded_json(text).map(as_plan_object) {
            Ok(Some(value)) => PlanRecovery::ok(value),
            Ok(None) => PlanRecovery::broken(text, "[arguments]", "顶层不是 JSON 对象或数组"),
            Err(error) => PlanRecovery::broken(text, "[arguments]", error),
        },
        Value::Array(_) => PlanRecovery::ok(json!({ "plan": args.clone() })),
        Value::Object(map) => {
            // 整份参数被包了一层字符串：wrapper 之外只允许再带说明字段，
            // 这样 explanation + input 并存时也能救回来。
            if !PLAN_LIST_KEYS.iter().any(|key| map.contains_key(*key)) {
                if let Some((wrapper, raw)) = map
                    .iter()
                    .find(|(key, _)| PLAN_WRAPPER_KEYS.contains(&key.as_str()))
                    .and_then(|(key, value)| value.as_str().map(|text| (key.clone(), text)))
                {
                    return match embedded_json(raw).map(as_plan_object) {
                        Ok(Some(Value::Object(mut merged))) => {
                            for key in ["explanation", "note"] {
                                if let Some(text) = map.get(key) {
                                    merged
                                        .entry(key.to_string())
                                        .or_insert_with(|| text.clone());
                                }
                            }
                            PlanRecovery::ok(Value::Object(merged))
                        }
                        Ok(_) => PlanRecovery::broken(raw, wrapper, "里面不是 JSON 对象或数组"),
                        Err(error) => PlanRecovery::broken(raw, wrapper, error),
                    };
                }
            }
            // 只有列表本身被写成字符串。
            for key in PLAN_LIST_KEYS {
                let Some(text) = map.get(*key).and_then(Value::as_str) else {
                    continue;
                };
                match embedded_json(text) {
                    // 列表字符串里其实包着整份参数：当成外层参数用，别让模型再发一次。
                    Ok(Value::Object(mut inner))
                        if PLAN_LIST_KEYS
                            .iter()
                            .any(|key| inner.contains_key(*key)) =>
                    {
                        for key in ["explanation", "note"] {
                            if let Some(value) = map.get(key) {
                                inner.entry(key.to_string()).or_insert_with(|| value.clone());
                            }
                        }
                        return PlanRecovery::ok(Value::Object(inner));
                    }
                    Ok(parsed) if parsed.is_array() => {
                        let mut next = map.clone();
                        next.insert((*key).to_string(), parsed);
                        return PlanRecovery::ok(Value::Object(next));
                    }
                    Ok(_) => return PlanRecovery::broken(text, *key, "里面不是 JSON 数组"),
                    Err(error) => return PlanRecovery::broken(text, *key, error),
                }
            }
            PlanRecovery::ok(args.clone())
        }
        _ => PlanRecovery::ok(args.clone()),
    }
}

fn embedded_json(text: &str) -> std::result::Result<Value, String> {
    let trimmed = text.trim();
    let body = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.split_once('\n').map(|(_, body)| body).unwrap_or(rest);
        rest.trim_end()
            .strip_suffix("```")
            .unwrap_or(rest.trim_end())
            .trim()
    } else {
        trimmed
    };
    if !(body.starts_with('{') || body.starts_with('[')) {
        return Err("not a JSON object or array".to_string());
    }
    // 严格解析失败后走与参数管线同一套宽松修复（补回漏掉的 {"、未闭合字符串），
    // 模型的一次编码抖动不该换来一轮重试；真解不开才把解析器位置回给模型。
    if let Some(value) = crate::core::tool_args::parse_json_with_repairs(body) {
        return Ok(value);
    }
    serde_json::from_str::<Value>(body).map_err(|error| error.to_string())
}

/// Raw text echoed back in a parse error, so the model sees its own brackets
/// and quotes instead of guessing which layer was wrong.
fn snippet(text: &str) -> String {
    const LIMIT: usize = 160;
    let trimmed = text.trim();
    let mut out: String = trimmed.chars().take(LIMIT).collect();
    if trimmed.chars().count() > LIMIT {
        out.push('…');
    }
    out
}

fn as_plan_object(value: Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value),
        Value::Array(_) => Some(json!({ "plan": value })),
        _ => None,
    }
}

/// 参数错误的模型侧形态：既要人能读（message），也要结构化（field/received/
/// allowed），这样模型下一轮能直接按字段改，而不是照原样重试。
#[derive(Debug)]
struct PlanArgError {
    code: &'static str,
    message: String,
    data: Value,
    hint: Option<String>,
}

impl PlanArgError {
    fn new(code: &'static str, message: String, data: Value, hint: Option<String>) -> Self {
        Self {
            code,
            message,
            data,
            hint,
        }
    }

    fn shape(message: String, data: Value) -> Self {
        Self::new("TOOL_PLAN_INVALID_ARGS", message, data, None)
    }

    fn with_code(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }

    fn with_hint(mut self, hint: String) -> Self {
        self.hint = Some(hint);
        self
    }

    /// 统一失败外壳：`ok:false` + 错误码 + hint，重试由模型自己决定（参数错误
    /// 天然可重试，所以这里标记 retryable=true）。
    fn into_tool_result(self) -> Value {
        super::tool_error::build_failed_tool_result(
            self.message,
            self.data,
            super::tool_error::ToolErrorMeta::new(self.code, self.hint, true, None),
            false,
        )
    }
}

impl std::fmt::Display for PlanArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Debug)]
struct PlanRequest {
    explanation: Option<String>,
    /// 步骤文字与显式状态；没有显式状态时由 `current` 或默认 pending 决定。
    steps: Vec<(String, Option<String>)>,
    current: Option<usize>,
}

impl PlanRequest {
    /// 归一成步骤状态对（`step` + `status`）。显式状态优先；用最省形态（字符串清单 + `current`）
    /// 时按线性清单解释：`current` 之前的步骤算已完成，当前步进行中，其余待办。
    /// 否则最省形态永远累计不到 completed，面板进度条推到最后也停在 0。
    fn normalized_plan(&self) -> Vec<Value> {
        self.steps
            .iter()
            .enumerate()
            .map(|(index, (step, status))| {
                let status = status.clone().unwrap_or_else(|| match self.current {
                    Some(current) if index < current => "completed".to_string(),
                    Some(current) if index == current => "in_progress".to_string(),
                    _ => "pending".to_string(),
                });
                json!({ "step": step, "status": status })
            })
            .collect()
    }
}

/// 解析并**严格校验**计划参数。未知字段、未知状态、越界下标一律报错并回显
/// 位置：静默丢弃语义比失败更危险，模型会把错误状态一路带下去。
fn parse_plan_request(args: &Value) -> Result<PlanRequest, PlanArgError> {
    let Some(map) = args.as_object() else {
        return Err(plan_shape_error(args));
    };
    if let Some(field) = map
        .keys()
        .find(|key| !PLAN_TOP_LEVEL_KEYS.contains(&key.as_str()))
    {
        return Err(unknown_field(field, PLAN_TOP_LEVEL_KEYS));
    }
    let list_keys = PLAN_LIST_KEYS
        .iter()
        .copied()
        .filter(|key| map.contains_key(*key))
        .collect::<Vec<_>>();
    if list_keys.len() > 1 {
        return Err(list_conflict(&list_keys));
    }
    let list_key = *list_keys.first().ok_or_else(|| plan_shape_error(args))?;
    let Some(array) = map.get(list_key).and_then(Value::as_array) else {
        return Err(plan_shape_error(args));
    };
    let current = map.get("current").and_then(|value| plan_index_hint(value, array.len()));
    let mut steps = Vec::with_capacity(array.len());
    let mut in_progress_fields = Vec::new();
    for (index, item) in array.iter().enumerate() {
        let field = format!("{list_key}[{index}]");
        match item {
            Value::String(text) => steps.push((text.trim().to_string(), None)),
            Value::Object(entry) => {
                if let Some(key) = entry
                    .keys()
                    .find(|key| !PLAN_ITEM_KEYS.contains(&key.as_str()))
                {
                    return Err(unknown_field(&format!("{field}.{key}"), PLAN_ITEM_KEYS));
                }
                let text = PLAN_ITEM_TEXT_KEYS
                    .iter()
                    .find_map(|key| entry.get(*key).and_then(Value::as_str))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return Err(item_text_required(&field));
                }
                let status = entry.get("status").or_else(|| entry.get("state"));
                let status = match status {
                    None | Some(Value::Null) => None,
                    Some(Value::String(raw)) => {
                        let parsed = parse_plan_status(raw, &format!("{field}.status"))?;
                        if parsed == "in_progress" {
                            in_progress_fields.push(format!("{field}.status"));
                        }
                        Some(parsed)
                    }
                    Some(other) => {
                        return Err(unknown_status(
                            &format!("{field}.status"),
                            &other.to_string(),
                        ))
                    }
                };
                steps.push((text, status));
            }
            _ => return Err(item_text_required(&field)),
        }
    }
    if in_progress_fields.len() > 1 {
        return Err(multiple_in_progress(&in_progress_fields));
    }
    // 显式 status 优先：旧形态的 `current` 是“没有 status 时的退化提示”，
    // 两者同时出现按 status 解释，不报错——多一种写法就多一次重试。
    let current = if steps.iter().any(|(_, status)| status.is_some()) {
        None
    } else {
        current
    };
    let explanation = map
        .get("explanation")
        .or_else(|| map.get("note"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    Ok(PlanRequest {
        explanation,
        steps,
        current,
    })
}

/// 旧形态（字符串清单 + `current`）的下标提示。越界或非数字一律当作没写，
/// 让这次调用照常落地，而不是让模型为一个人人都不会再写的可选字段重试。
fn plan_index_hint(value: &Value, len: usize) -> Option<usize> {
    let parsed = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    match parsed {
        Some(index) if index >= 0 && (index as usize) < len => Some(index as usize),
        _ => None,
    }
}

fn parse_plan_status(raw: &str, field: &str) -> Result<String, PlanArgError> {
    let normalized = raw.trim().to_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "pending" | "todo" | "待办" | "未开始" => Ok("pending".to_string()),
        "in_progress" | "inprogress" | "doing" | "active" | "进行中" | "正在做" => {
            Ok("in_progress".to_string())
        }
        "completed" | "complete" | "done" | "finished" | "已完成" | "完成" | "已结束" => {
            Ok("completed".to_string())
        }
        _ => Err(unknown_status(field, raw)),
    }
}

fn plan_shape_error(args: &Value) -> PlanArgError {
    let keys = match args {
        Value::Object(map) => map.keys().cloned().collect::<Vec<_>>(),
        Value::Array(_) => vec!["[array]".to_string()],
        Value::String(_) => vec!["[string]".to_string()],
        Value::Null => vec!["null".to_string()],
        _ => vec!["?".to_string()],
    };
    let mut params = HashMap::new();
    params.insert(
        "keys".to_string(),
        if keys.is_empty() {
            "—".to_string()
        } else {
            keys.join(", ")
        },
    );
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.invalid_shape", &params),
        json!({ "keys": keys, "expected": PLAN_SHAPE_EXAMPLE }),
    )
}

fn unparsable_wrapper(broken: &PlanJsonBroken) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("field".to_string(), broken.wrapper.clone());
    params.insert("error".to_string(), broken.error.clone());
    params.insert("snippet".to_string(), broken.snippet.clone());
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.unparsable_wrapper", &params),
        json!({
            "field": broken.wrapper,
            "parse_error": broken.error,
            "received": broken.snippet,
            "expected": PLAN_SHAPE_EXAMPLE,
        }),
    )
    .with_code("TOOL_PLAN_INVALID_JSON")
    .with_hint(i18n::t("tool.plan.example_hint"))
}

fn unknown_field(field: &str, allowed: &[&str]) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("field".to_string(), field.to_string());
    params.insert("allowed".to_string(), allowed.join(", "));
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.unknown_field", &params),
        json!({
            "field": field,
            "allowed": allowed,
            "example": PLAN_SHAPE_EXAMPLE,
        }),
    )
    .with_code("TOOL_PLAN_UNKNOWN_FIELD")
    .with_hint(i18n::t("tool.plan.example_hint"))
}

fn unknown_status(field: &str, received: &str) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("field".to_string(), field.to_string());
    params.insert("received".to_string(), received.to_string());
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.unknown_status", &params),
        json!({
            "field": field,
            "received": received,
            "allowed": PLAN_STATUS_VALUES,
            "example": PLAN_SHAPE_EXAMPLE,
        }),
    )
    .with_code("TOOL_PLAN_UNKNOWN_STATUS")
}

fn multiple_in_progress(fields: &[String]) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("fields".to_string(), fields.join(", "));
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.multiple_in_progress", &params),
        json!({ "fields": fields, "allowed": "at most one in_progress" }),
    )
    .with_code("TOOL_PLAN_CONFLICT")
}

fn list_conflict(fields: &[&str]) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("fields".to_string(), fields.join(", "));
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.list_conflict", &params),
        json!({ "fields": fields, "allowed": ["plan"] }),
    )
    .with_code("TOOL_PLAN_CONFLICT")
}

fn item_text_required(field: &str) -> PlanArgError {
    let mut params = HashMap::new();
    params.insert("field".to_string(), field.to_string());
    PlanArgError::shape(
        i18n::t_with_params("tool.plan.item_text_required", &params),
        json!({ "field": field, "example": PLAN_SHAPE_EXAMPLE }),
    )
    .with_code("TOOL_PLAN_INVALID_ARGS")
}

struct PlanProgress {
    done: usize,
    total: usize,
    current: Option<String>,
    next: Option<String>,
}

impl PlanProgress {
    fn summary(&self) -> String {
        let mut params = HashMap::new();
        params.insert("total".to_string(), self.total.to_string());
        if self.done >= self.total {
            return i18n::t_with_params("tool.plan.updated_all", &params);
        }
        params.insert("done".to_string(), self.done.to_string());
        params.insert(
            "current".to_string(),
            self.current.clone().unwrap_or_else(|| "—".to_string()),
        );
        params.insert(
            "next".to_string(),
            self.next.clone().unwrap_or_else(|| "—".to_string()),
        );
        i18n::t_with_params("tool.plan.updated", &params)
    }

    /// 还没做完时的下一次提醒。放在回执里，模型下结论前一定看得到。
    fn hint(&self) -> Option<String> {
        if self.done >= self.total {
            return None;
        }
        if self.current.is_none() {
            return Some(i18n::t("tool.plan.missing_in_progress"));
        }
        Some(i18n::t("tool.plan.keep_updating"))
    }
}

fn plan_progress(plan: &[Value]) -> PlanProgress {
    let mut progress = PlanProgress {
        done: 0,
        total: plan.len(),
        current: None,
        next: None,
    };
    for item in plan {
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let step = item
            .get("step")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match status {
            "completed" => progress.done += 1,
            "in_progress" => {
                if progress.current.is_none() {
                    progress.current = Some(step);
                }
            }
            _ => {
                if progress.next.is_none() {
                    progress.next = Some(step);
                }
            }
        }
    }
    progress
}

#[derive(Debug)]
struct QuestionPanelOption {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug)]
struct QuestionPanelQuestion {
    question: String,
    options: Vec<QuestionPanelOption>,
    multiple: bool,
}

/// 面板一次最多分页展示的问题数，与工具 schema 的 maxItems 对齐。
const MAX_QUESTION_PANEL_QUESTIONS: usize = 4;
/// 单题最多展示的选项数，与工具 schema 的 options.maxItems 对齐。
const MAX_QUESTION_PANEL_OPTIONS: usize = 4;
/// 顶层写成单题（question/options 平铺）时的兼容字段。
const QUESTION_PANEL_FLAT_QUESTION_KEYS: &[&str] = &["question", "prompt", "title", "header"];
const QUESTION_PANEL_FLAT_OPTION_KEYS: &[&str] = &["options", "routes", "choices"];

pub(crate) async fn execute_question_panel_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let parsed = normalize_question_panel_questions(args)?;
    let questions = parsed.questions;
    let questions_json = questions
        .iter()
        .map(question_panel_question_json)
        .collect::<Vec<_>>();
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "question_panel",
            json!({
                "questions": questions_json.clone(),
                "keep_open": true
            }),
        );
    }
    // 被丢掉或截断的题不能静默消失：提示里说清是哪一题、缺了什么或多在哪，
    // 模型下一轮才能补上。
    let hint = if parsed.dropped.is_empty() {
        None
    } else {
        let mut hint = String::from("Question panel: these items were not shown: ");
        hint.push_str(&parsed.dropped.join("; "));
        Some(hint)
    };
    Ok(build_model_tool_success_with_hint(
        "question_panel",
        "awaiting_input",
        "Opened a question panel and is waiting for user input.",
        json!({
            "questions": questions_json,
            "dropped_questions": parsed.dropped,
        }),
        hint,
    ))
}

fn question_panel_question_json(question: &QuestionPanelQuestion) -> Value {
    json!({
        "question": question.question,
        "multiple": question.multiple,
        "options": question.options.iter().map(|option| json!({
            "label": option.label,
            "description": option.description,
            "recommended": option.recommended
        })).collect::<Vec<_>>()
    })
}

#[derive(Debug)]
struct QuestionPanelParse {
    questions: Vec<QuestionPanelQuestion>,
    dropped: Vec<String>,
}

fn normalize_question_panel_questions(args: &Value) -> Result<QuestionPanelParse> {
    let Some(obj) = args.as_object() else {
        return Err(question_panel_shape_error(format!(
            "没有收到参数对象，实际是 {}",
            value_kind(args)
        )));
    };
    let raw_items: Vec<Value> = match obj.get("questions") {
        // questions 存在但不是数组时不要退化成单题：那会把错误指向合成出来的
        // 字段，让调用方以为自己的 options 写错了。
        Some(value) if !value.is_array() => {
            return Err(question_panel_shape_error(format!(
                "questions 必须是数组，实际是 {}",
                value_kind(value)
            )));
        }
        Some(value) => value.as_array().cloned().unwrap_or_default(),
        None => {
            let has_flat = QUESTION_PANEL_FLAT_QUESTION_KEYS
                .iter()
                .chain(QUESTION_PANEL_FLAT_OPTION_KEYS.iter())
                .any(|key| obj.contains_key(*key));
            if !has_flat {
                let keys = obj.keys().cloned().collect::<Vec<_>>().join("、");
                let detail = if keys.is_empty() {
                    "缺少 questions 数组，实际参数是空对象".to_string()
                } else {
                    format!("缺少 questions 数组，实际顶层字段有 {keys}")
                };
                return Err(question_panel_shape_error(detail));
            }
            // 单题扁平写法：历史线程与仍在用旧契约的调用方要能继续渲染。
            vec![json!({
                "question": first_panel_value(obj, QUESTION_PANEL_FLAT_QUESTION_KEYS),
                "options": first_panel_value(obj, QUESTION_PANEL_FLAT_OPTION_KEYS),
                "multiple": first_panel_value(obj, &["multiple", "allow_multiple", "multi"]),
            })]
        }
    };
    if raw_items.is_empty() {
        return Err(question_panel_shape_error(
            "questions 是空数组，至少要给出 1 题".to_string(),
        ));
    }
    let checked = raw_items
        .iter()
        .take(MAX_QUESTION_PANEL_QUESTIONS)
        .collect::<Vec<_>>();
    // 没展示出来的内容一律点名：被丢掉的题或选项不能静默消失。
    let mut dropped = question_panel_problems(&checked);
    if raw_items.len() > MAX_QUESTION_PANEL_QUESTIONS {
        let from = MAX_QUESTION_PANEL_QUESTIONS + 1;
        let to = raw_items.len();
        let position = if from == to {
            format!("第 {from} 题")
        } else {
            format!("第 {}–{} 题", from, to)
        };
        dropped.push(format!(
            "{position}超出单次上限 {} 题，未展示",
            MAX_QUESTION_PANEL_QUESTIONS
        ));
    }
    let mut questions = Vec::new();
    for (index, item) in checked.iter().enumerate() {
        let Value::Object(record) = item else {
            continue;
        };
        let question = record
            .get("question")
            .or_else(|| record.get("title"))
            .or_else(|| record.get("header"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let question = if question.is_empty() {
            i18n::t("tool.question_panel.default_question")
        } else {
            question
        };
        let raw_options = record
            .get("options")
            .or_else(|| record.get("routes"))
            .or_else(|| record.get("choices"));
        let option_total = raw_options.map(option_item_count).unwrap_or(0);
        let options = normalize_question_panel_options(raw_options);
        if options.is_empty() {
            continue;
        }
        if option_total > MAX_QUESTION_PANEL_OPTIONS {
            dropped.push(format!(
                "第 {} 题有 {} 个选项，超过上限 {} 个，仅展示前 {} 个",
                index + 1,
                option_total,
                MAX_QUESTION_PANEL_OPTIONS,
                MAX_QUESTION_PANEL_OPTIONS
            ));
        }
        let multiple = record
            .get("multiple")
            .or_else(|| record.get("allow_multiple"))
            .or_else(|| record.get("multi"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        questions.push(QuestionPanelQuestion {
            question,
            options,
            multiple,
        });
    }
    if questions.is_empty() {
        return Err(question_panel_options_error(&checked, args));
    }
    Ok(QuestionPanelParse { questions, dropped })
}

/// 选项缺失时不要只回一句「每个问题至少要给出一个选项」：把解析到什么、第几题
/// 缺字段、正确形状一起回给模型，才能一次改对而不是反复猜。
fn question_panel_options_error(checked: &[&Value], args: &Value) -> anyhow::Error {
    let mut details = question_panel_problems(checked);
    if details.is_empty() {
        details.push(match args.as_object() {
            Some(obj) if obj.is_empty() => "没有可渲染的问题项，实际参数是空对象".to_string(),
            Some(obj) => format!(
                "没有可渲染的问题项，实际顶层字段有 {}",
                obj.keys().cloned().collect::<Vec<_>>().join("、")
            ),
            None => format!("没有可渲染的问题项，参数实际是 {}", value_kind(args)),
        });
    }
    question_panel_shape_error_with(details)
}

fn question_panel_shape_error(detail: String) -> anyhow::Error {
    // 结构性问题（没收到 questions）不该套「每个问题至少要给出一个选项」，
    // 那句话会把人往错误的方向带。
    anyhow!(
        "问询面板参数不可用：{}。正确形状：{}。",
        detail,
        QUESTION_PANEL_SHAPE_HINT
    )
}

const QUESTION_PANEL_SHAPE_HINT: &str = "questions 数组，每项含 question 与 options（options 至少 1 项，可写字符串数组，也可写用 | 或 ，分隔的一整串）";

fn question_panel_shape_error_with(details: Vec<String>) -> anyhow::Error {
    // 文案自带句号，先剥掉再补，避免出现「选项。。」这种双句号。
    let mut message = i18n::t("tool.question_panel.options_required")
        .trim()
        .trim_end_matches(|c| c == '。' || c == '.')
        .to_string();
    message.push('。');
    if !details.is_empty() {
        message.push_str(&details.join("；"));
        message.push('。');
    }
    message.push_str("正确形状：");
    message.push_str(QUESTION_PANEL_SHAPE_HINT);
    message.push('。');
    anyhow!(message)
}

/// 逐项说明「第几题、什么问题、实际有什么字段」，供错误与丢弃提示共用。
fn question_panel_problems(items: &[&Value]) -> Vec<String> {
    let mut details = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let position = index + 1;
        match item {
            Value::Object(record) => {
                let option_count = record
                    .get("options")
                    .or_else(|| record.get("routes"))
                    .or_else(|| record.get("choices"))
                    .map(option_item_count)
                    .unwrap_or(0);
                if option_count > 0 {
                    continue;
                }
                let question = record
                    .get("question")
                    .or_else(|| record.get("title"))
                    .or_else(|| record.get("header"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let mut line = String::from("第 ");
                line.push_str(&position.to_string());
                line.push_str(" 题缺少 options");
                if !question.is_empty() {
                    line.push_str("（题目：");
                    line.push_str(question);
                    line.push('）');
                }
                let keys = record.keys().cloned().collect::<Vec<_>>();
                if !keys.is_empty() {
                    line.push_str("，实际字段有 ");
                    line.push_str(&keys.join("、"));
                }
                details.push(line);
            }
            other => {
                let mut line = String::from("第 ");
                line.push_str(&position.to_string());
                line.push_str(" 项不是对象（实际是 ");
                line.push_str(value_kind(other));
                line.push('）');
                details.push(line);
            }
        }
    }
    details
}

/// options 既可能是数组，也可能是 `甲|乙` 这样的整串；两种都算数。
fn option_item_count(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.len(),
        Value::String(text) => text
            .split(['|', '，'])
            .filter(|part| !part.trim().is_empty())
            .count(),
        _ => 0,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

fn first_panel_value(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| obj.get(*key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn normalize_question_panel_options(value: Option<&Value>) -> Vec<QuestionPanelOption> {
    // options 既接受对象/字符串数组，也接受用 | 或，分隔的一整串，少写一层嵌套。
    let items = match value {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => text
            .split(['|', '，'])
            .map(|part| Value::String(part.trim().to_string()))
            .collect(),
        _ => Vec::new(),
    };
    let mut normalized = Vec::new();
    for item in items {
        let (label, description, recommended) = match item {
            Value::String(value) => (value, None, false),
            Value::Object(map) => {
                let label = map
                    .get("label")
                    .or_else(|| map.get("title"))
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let description = map
                    .get("description")
                    .or_else(|| map.get("detail"))
                    .or_else(|| map.get("desc"))
                    .or_else(|| map.get("summary"))
                    .and_then(Value::as_str)
                    .map(|value| value.to_string());
                let recommended = map
                    .get("recommended")
                    .or_else(|| map.get("preferred"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (label, description, recommended)
            }
            _ => (String::new(), None, false),
        };
        let label = label.trim().to_string();
        if label.is_empty() {
            continue;
        }
        let description = description.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let recommended = recommended || label.contains("推荐");
        normalized.push(QuestionPanelOption {
            label,
            description,
            recommended,
        });
    }
    // 超出上限的选项不进入面板：调用方已在 dropped 里被明确告知。
    normalized.truncate(MAX_QUESTION_PANEL_OPTIONS);
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_status_accepts_aliases_and_rejects_unknown() {
        assert_eq!(
            parse_plan_status("in-progress", "plan[0].status").expect("alias"),
            "in_progress"
        );
        assert_eq!(
            parse_plan_status("已完成", "plan[0].status").expect("zh"),
            "completed"
        );
        let error = parse_plan_status("blocked", "plan[1].status")
            .expect_err("unknown status")
            .to_string();
        // 报错要能定位到具体哪一项，并列出合法值。
        assert!(error.contains("plan[1].status"), "{error}");
        assert!(error.contains("blocked"), "{error}");
        assert!(error.contains("in_progress"), "{error}");
    }

    /// 实测会话里的原始失败形态：模型把整份参数写成字符串，`[` 后面漏了 `{"`。
    /// 这种一次性的编码抖动必须在工具边界修好，不能再让模型重发一遍。
    #[test]
    fn plan_args_survive_a_dropped_object_opening() {
        let broken = "{\"plan\": [step\": \"接收测试指令，初始化计划\", \"status\": \"in_progress\"}, {\"step\": \"逐步推进示例任务\", \"status\": \"pending\"}]}";
        let recovered = recover_plan_args(&json!({ "raw": broken }));
        assert!(recovered.broken.is_none(), "must be repaired, not reported");
        let request = parse_plan_request(&recovered.value).expect("repaired plan");
        assert_eq!(request.steps.len(), 2);
        assert_eq!(request.steps[0].0, "接收测试指令，初始化计划");
        assert_eq!(request.steps[0].1.as_deref(), Some("in_progress"));
        assert_eq!(request.steps[1].1.as_deref(), Some("pending"));

        // 列表整体被写成字符串时同样适用。
        let nested = "{\"plan\": [{\"step\": \"甲\", \"status\": \"in_progress\"}]}";
        let args = json!({ "plan": nested });
        let recovered = recover_plan_args(&args);
        let request = parse_plan_request(&recovered.value).expect("stringified list");
        assert_eq!(request.steps[0].1.as_deref(), Some("in_progress"));
    }

    #[test]
    fn plan_recovery_reports_unparsable_wrapper_instead_of_unknown_field() {
        // 包装键里不是合法 JSON：要报“解不开”，不能含糊地说“未知字段 raw”。
        let recovered = recover_plan_args(&json!({
            "raw": "{\"plan\": [{\"step\": 拼错了}]}"
        }));
        let broken = recovered.broken.as_ref().expect("broken wrapper");
        assert_eq!(broken.wrapper, "raw");
        // serde 的行列定位 + 原文片段都要回给模型，它才能看见自己漏的引号。
        assert!(broken.error.contains("line 1"), "{}", broken.error);
        assert!(broken.snippet.contains("拼错了"), "{}", broken.snippet);
        assert!(broken.error.contains("line 1"), "{}", broken.error);

        let result = unparsable_wrapper(broken).into_tool_result();
        assert_eq!(
            result["error_meta"]["code"],
            json!("TOOL_PLAN_INVALID_JSON")
        );
        assert_eq!(result["error_meta"]["retryable"], json!(true));
        assert_eq!(result["data"]["field"], json!("raw"));
        assert!(result["data"]["received"]
            .as_str()
            .expect("snippet")
            .contains("拼错了"));
        assert!(
            result["error_meta"]["hint"]
                .as_str()
                .unwrap_or_default()
                .contains("status"),
            "hint must point at the canonical shape"
        );
    }

    #[test]
    fn plan_arg_error_carries_structured_location() {
        let result = unknown_field("mode", PLAN_TOP_LEVEL_KEYS).into_tool_result();
        assert_eq!(result["ok"], json!(false));
        assert_eq!(result["data"]["field"], json!("mode"));
        assert_eq!(
            result["error_meta"]["code"],
            json!("TOOL_PLAN_UNKNOWN_FIELD")
        );
        // 参数错误天然可重试：模型改正形状后立刻能成功。
        assert_eq!(result["error_meta"]["retryable"], json!(true));
    }

    #[test]
    fn plan_request_accepts_minimal_string_form_with_current() {
        let request = parse_plan_request(&json!({
            "plan": ["甲", "乙", "丙"], "current": 1
        }))
        .expect("minimal form");
        let plan = request.normalized_plan();
        // 最省形态按线性清单解释：current 之前都是已完成。
        assert_eq!(plan[0]["status"], json!("completed"));
        assert_eq!(plan[1]["status"], json!("in_progress"));
        assert_eq!(plan[2]["status"], json!("pending"));
        // 进度必须跟着 current 走，否则面板推到末尾也停在 0。
        let progress = plan_progress(&plan);
        assert_eq!((progress.done, progress.total), (1, 3));
        assert_eq!(progress.current.as_deref(), Some("乙"));
        assert_eq!(progress.next.as_deref(), Some("丙"));
    }

    #[test]
    fn plan_progress_counts_steps_before_current_as_done() {
        let request = parse_plan_request(&json!({
            "plan": ["甲", "乙", "丙"], "current": 2
        }))
        .expect("minimal form");
        let progress = plan_progress(&request.normalized_plan());
        assert_eq!((progress.done, progress.total), (2, 3));
        assert_eq!(progress.current.as_deref(), Some("丙"));
        assert_eq!(progress.next, None);
        // 显式状态照旧以 status 为准，不被 current 覆盖。
        let explicit = parse_plan_request(&json!({
            "plan": [
                { "step": "甲", "status": "pending" },
                { "step": "乙", "status": "in_progress" }
            ]
        }))
        .expect("explicit form");
        let progress = plan_progress(&explicit.normalized_plan());
        assert_eq!((progress.done, progress.total), (0, 2));
    }

    #[test]
    fn plan_request_rejects_unknown_field_and_echoes_it() {
        let error = parse_plan_request(&json!({ "plan": ["甲"], "mode": "fast" }))
            .expect_err("unknown field")
            .to_string();
        assert!(error.contains("mode"), "{error}");
        // 可用字段按白名单回显，并给出唯一形态的示范。
        assert!(error.contains("plan"), "{error}");
    }

    #[test]
    fn plan_request_rejects_unknown_status_with_path() {
        let error = parse_plan_request(&json!({
            "plan": [{ "step": "甲" }, { "step": "乙", "status": "blocked" }]
        }))
        .expect_err("unknown status")
        .to_string();
        assert!(error.contains("plan[1].status"), "{error}");
    }

    #[test]
    fn plan_request_rejects_two_in_progress_steps() {
        let error = parse_plan_request(&json!({
            "plan": [
                { "step": "甲", "status": "in_progress" },
                { "step": "乙", "status": "in_progress" }
            ]
        }))
        .expect_err("two in_progress")
        .to_string();
        assert!(error.contains("in_progress"), "{error}");
        assert!(error.contains("plan[0].status"), "{error}");
    }

    #[test]
    fn plan_request_still_rejects_two_lists_but_tolerates_legacy_current() {
        let error = parse_plan_request(&json!({
            "plan": ["甲", "乙"],
            "steps": ["丙"],
            "current": 0,
        }))
        .expect_err("conflicting list keys")
        .to_string();
        assert!(error.contains("plan"), "{error}");

        // 旧形态的 current 不再报错：越界当没写，调用照常落地。
        let request = parse_plan_request(&json!({ "plan": ["甲"], "current": 3 }))
            .expect("out-of-range current is ignored");
        assert_eq!(request.current, None);

        // 与显式 status 并存时以 status 为准，current 被忽略。
        let request = parse_plan_request(&json!({
            "plan": [{ "step": "甲", "status": "completed" }], "current": 0
        }))
        .expect("status wins over current");
        assert_eq!(request.current, None);
        assert_eq!(request.normalized_plan()[0]["status"], json!("completed"));
    }

    #[test]
    fn plan_args_are_recovered_from_stringified_wrappers() {
        // 文本/XML 协议下 `<input>` 会落成 {"input": "<json>"}。
        let recovered = recover_plan_args(&json!({
            "input": "{\"plan\":[{\"step\":\"探测环境\",\"status\":\"in_progress\"}]}"
        }));
        assert!(recovered.broken.is_none());
        let request = parse_plan_request(&recovered.value).expect("input wrapper");
        assert_eq!(request.steps[0].0, "探测环境");
        assert_eq!(request.steps[0].1.as_deref(), Some("in_progress"));

        // 外层同时带 explanation 也要能救回来。
        let recovered = recover_plan_args(&json!({
            "explanation": "补两步",
            "raw": "```json\n{\"plan\":[\"跑测试\"]}\n```"
        }));
        let request = parse_plan_request(&recovered.value).expect("raw + explanation");
        assert_eq!(request.steps[0].0, "跑测试");
        assert_eq!(request.explanation.as_deref(), Some("补两步"));
    }

    #[test]
    fn plan_args_are_recovered_from_stringified_list_and_bare_array() {
        let recovered = recover_plan_args(&json!({
            "plan": "[{\"step\":\"写代码\",\"status\":\"进行中\"}]"
        }));
        let request = parse_plan_request(&recovered.value).expect("stringified list");
        assert_eq!(request.normalized_plan()[0]["status"], json!("in_progress"));

        let recovered = recover_plan_args(&json!([{ "step": "第一步" }, "第二步"]));
        let request = parse_plan_request(&recovered.value).expect("bare array");
        assert_eq!(request.steps.len(), 2);
        assert_eq!(request.steps[1].0, "第二步");
    }

    #[test]
    fn plan_args_accept_alias_keys_and_report_shape_errors() {
        let recovered = recover_plan_args(&json!({
            "steps": [{ "title": "改前端", "state": "done" }, { "content": "补测试" }]
        }));
        let request = parse_plan_request(&recovered.value).expect("alias keys");
        assert_eq!(request.steps[0].0, "改前端");
        assert_eq!(request.steps[0].1.as_deref(), Some("completed"));
        assert_eq!(request.steps[1].0, "补测试");

        let error = parse_plan_request(&json!({ "foo": 1 }))
            .expect_err("bad shape")
            .to_string();
        assert!(error.contains("foo"), "{error}");
    }
    #[test]
    fn question_panel_keeps_every_question() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [
                {
                    "question": "第一个问题",
                    "options": [{ "label": "甲" }, { "label": "乙", "recommended": true }]
                },
                {
                    "question": "第二个问题",
                    "multiple": true,
                    "options": ["丙", "丁"]
                }
            ]
        }))
        .expect("questions")
        .questions;
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].options.len(), 2);
        assert!(questions[0].options[1].recommended);
        assert!(!questions[0].multiple);
        assert!(questions[1].multiple);
        assert_eq!(questions[1].options[0].label, "丙");
    }

    #[test]
    fn question_panel_still_reads_legacy_flat_payload() {
        let questions = normalize_question_panel_questions(&json!({
            "question": "旧版单题",
            "routes": [{ "label": "继续（推荐）" }],
            "allow_multiple": false
        }))
        .expect("legacy questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "旧版单题");
        // 标题里写「推荐」也要被识别成推荐项。
        assert!(questions[0].options[0].recommended);
    }

    #[test]
    fn question_panel_drops_optionless_questions_and_fails_when_empty() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "没有选项的一题", "options": [] },
                { "question": "有选项的一题", "options": ["甲"] }
            ]
        }))
        .expect("kept questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "有选项的一题");

        let err = normalize_question_panel_questions(&json!({ "questions": [] }))
            .expect_err("no usable question");
        assert!(err.to_string().contains("questions"), "{}", err);
    }

    #[test]
    fn question_panel_accepts_pipe_separated_options() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [{ "question": "选哪个方向", "options": "甲|乙" }]
        }))
        .expect("questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].options.len(), 2);
        assert_eq!(questions[0].options[1].label, "乙");
    }

    #[test]
    fn question_panel_reports_dropped_questions_instead_of_hiding_them() {
        let parsed = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "有选项的一题", "options": ["甲"] },
                { "question": "没有选项的一题", "hint": "漏了 options" }
            ]
        }))
        .expect("one usable question");
        assert_eq!(parsed.questions.len(), 1);
        assert_eq!(parsed.dropped.len(), 1);
        assert!(
            parsed.dropped[0].contains("第 2 题缺少 options"),
            "{:?}",
            parsed.dropped
        );
        assert!(
            parsed.dropped[0].contains("没有选项的一题"),
            "{:?}",
            parsed.dropped
        );
    }

    #[test]
    fn question_panel_error_keeps_a_shape_hint() {
        let err = normalize_question_panel_questions(&json!({ "question": "无选项" }))
            .expect_err("no usable question");
        let text = err.to_string();
        assert!(text.contains("第 1 题缺少 options"), "{}", text);
        assert!(text.contains("正确形状"), "{}", text);
    }

    #[test]
    fn question_panel_error_never_doubles_the_sentence_period() {
        let err = normalize_question_panel_questions(&json!({ "questions": [] }))
            .expect_err("empty questions");
        let text = err.to_string();
        assert!(!text.contains("。。"), "{}", text);
        assert!(!text.contains(".."), "{}", text);
    }

    #[test]
    fn question_panel_error_points_at_the_missing_questions_array() {
        // 参数整体没解析出来时，报错必须指向真正的问题，而不是编造一题缺 options。
        let err = normalize_question_panel_questions(&json!({})).expect_err("empty args");
        let text = err.to_string();
        assert!(text.contains("缺少 questions 数组"), "{}", text);
        assert!(text.contains("空对象"), "{}", text);
        assert!(!text.contains("第 1 题"), "{}", text);

        let err = normalize_question_panel_questions(&json!({ "raw": "{oops" }))
            .expect_err("unparsed payload");
        let text = err.to_string();
        assert!(text.contains("缺少 questions 数组"), "{}", text);
        assert!(text.contains("raw"), "{}", text);

        let err = normalize_question_panel_questions(&json!("甲|乙")).expect_err("not an object");
        assert!(err.to_string().contains("字符串"), "{}", err);
    }

    #[test]
    fn question_panel_rejects_non_array_questions() {
        let err = normalize_question_panel_questions(&json!({ "questions": "要问什么？" }))
            .expect_err("questions is a string");
        let text = err.to_string();
        assert!(text.contains("必须是数组"), "{}", text);
    }

    #[test]
    fn question_panel_reports_overflow_instead_of_dropping_silently() {
        let parsed = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "一", "options": ["甲", "乙", "丙", "丁", "戊"] },
                { "question": "二", "options": ["甲"] },
                { "question": "三", "options": ["甲"] },
                { "question": "四", "options": ["甲"] },
                { "question": "五", "options": ["甲"] }
            ]
        }))
        .expect("kept questions");
        assert_eq!(parsed.questions.len(), MAX_QUESTION_PANEL_QUESTIONS);
        assert_eq!(
            parsed.questions[0].options.len(),
            MAX_QUESTION_PANEL_OPTIONS
        );
        assert!(
            parsed
                .dropped
                .iter()
                .any(|line| line.contains("第 5 题") && line.contains("超出单次上限")),
            "{:?}",
            parsed.dropped
        );
        assert!(
            parsed
                .dropped
                .iter()
                .any(|line| line.contains("第 1 题") && line.contains("5 个选项")),
            "{:?}",
            parsed.dropped
        );
    }
}
