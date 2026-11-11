//! One failure, one card.
//!
//! The runtime already classifies every failed turn: its error code says what
//! broke and its message carries the provider's own sentence. The timeline
//! used to drop both and only settle the footer on "执行失败", which reads the
//! same whether a model key is wrong, a provider is down or the context is too
//! long. This module turns the same payload the live stream and restored
//! history both carry into the four lines the assistant's place renders:
//! what failed, what it means, the runtime's detail, and what to do next.
use serde_json::Value;

/// A failure as the assistant's own reply. All strings are user-facing.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorCard {
    /// Short classification, e.g. "模型鉴权失败".
    pub title: String,
    /// One sentence saying what went wrong in plain language.
    pub message: String,
    /// The runtime's raw sentence when it adds detail the summary omits.
    pub detail: String,
    /// The concrete next step the user can take.
    pub hint: String,
}

/// Provider bodies can be long; a card keeps a readable excerpt only.
const DETAIL_LIMIT: usize = 600;

impl ErrorCard {
    fn new(title: &str, message: &str, detail: &str, hint: &str) -> Self {
        let detail = bound(detail);
        Self {
            title: title.into(),
            message: message.into(),
            // Repeating the summary as "detail" would render the same sentence
            // twice, so only a distinct raw sentence earns the extra line.
            detail: if detail.is_empty() || detail == message {
                String::new()
            } else {
                detail
            },
            hint: hint.into(),
        }
    }
}

fn bound(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(DETAIL_LIMIT).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// The classifier both call sites share. `code` is the runtime's
/// `OrchestratorError` code; `message` is the sentence the runtime produced.
/// `None` means the turn did not fail in a way the user needs a card for
/// (a user cancellation already reads as "已停止").
pub fn from_parts(code: &str, message: &str) -> Option<ErrorCard> {
    let code = code.trim().to_ascii_uppercase();
    let message = message.trim();
    if message.is_empty() || code == "CANCELLED" {
        return None;
    }
    let text = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    let card = match code.as_str() {
        "LLM_UNAVAILABLE"
            if has(&[
                "至少需要一套模型配置",
                "llm_config_required",
                "模型配置",
                "未配置",
                "not configured",
            ]) =>
        {
            ErrorCard::new(
                "未配置可用模型",
                "当前没有可用的模型配置，请求还没有发出去。",
                message,
                "打开 设置 → 模型，添加并启用一套可用的模型配置后重试。",
            )
        }
        "LLM_UNAVAILABLE"
            if has(&[
                "http 5", "500", "502", "503", "504", "550", "bad gateway",
                "gateway timeout", "internal server error", "服务端",
            ]) =>
        {
            ErrorCard::new(
                "模型服务暂时不可用",
                "模型服务端持续返回错误，运行时已自动重试仍未成功，本轮已中断。",
                message,
                "稍后重试，或在设置中切换到其他可用模型。",
            )
        }
        "LLM_UNAVAILABLE" => ErrorCard::new(
            "模型服务不可用",
            "模型服务没有返回可用结果，本轮已中断。",
            message,
            "请检查网络、模型服务地址与密钥，然后重试。",
        ),
        "LLM_REQUEST_REJECTED"
            if has(&[
                "401",
                "403",
                "authentication",
                "unauthorized",
                "invalid_api_key",
                "invalid api key",
                "鉴权",
                "密钥",
                "api key",
            ]) =>
        {
            ErrorCard::new(
                "模型鉴权失败",
                "模型服务拒绝了这次请求，通常是 API Key 无效、过期或没有该模型的权限。",
                message,
                "核对模型的 API Key 与账户权限后重试。",
            )
        }
        "LLM_REQUEST_REJECTED"
            if has(&[
                "model_not_found",
                "not found",
                "404",
                "模型不存在",
                "无此模型",
            ]) =>
        {
            ErrorCard::new(
                "模型不存在",
                "模型服务找不到配置里的模型名称。",
                message,
                "核对模型名称与服务端可用列表，保持一致后重试。",
            )
        }
        "LLM_REQUEST_REJECTED"
            if has(&["insufficient_quota", "quota", "billing", "余额", "额度"]) =>
        {
            ErrorCard::new(
                "模型额度不足",
                "模型账户的额度或计费状态拒绝了本次调用。",
                message,
                "充值、换用其他模型账户后再试。",
            )
        }
        "LLM_REQUEST_REJECTED" if has(&["rate_limit", "429", "too many requests", "频繁"]) => {
            ErrorCard::new(
                "请求过于频繁",
                "模型服务正在限流，本次请求没有完成。",
                message,
                "稍等片刻后重试，或降低同时运行的任务数。",
            )
        }
        "LLM_REQUEST_REJECTED" => ErrorCard::new(
            "模型请求被拒绝",
            "模型服务拒绝了本次请求。",
            message,
            "检查模型名称、请求参数与账户状态，调整后重试。",
        ),
        "CONTEXT_WINDOW_EXCEEDED" => ErrorCard::new(
            "上下文超出模型上限",
            "这次对话的内容超过了模型可接受的长度。",
            message,
            "压缩上下文或缩短本轮输入后重试。",
        ),
        "LLM_REQUEST_TOO_LARGE" => ErrorCard::new(
            "请求体积超出上限",
            "本轮请求包含的历史图片过多，超过了模型接口可接收的体积，请求没有发出。",
            message,
            "运行时已自动保留最近图片；继续时只需重读必要的画面，或新开线程再试。",
        ),
        "USER_QUOTA_EXCEEDED" | "USER_QUOTA_INSUFFICIENT" => ErrorCard::new(
            "额度不足",
            "当前账户额度不足，本轮没有执行。",
            message,
            "额度恢复或补充后再试。",
        ),
        "USER_BUSY" => ErrorCard::new(
            "线程正在处理其他任务",
            "当前线程还在执行上一轮任务，这次输入没有受理。",
            message,
            "等上一轮结束，或先停止它再发送。",
        ),
        "LLM_OUTPUT_LOOP" => ErrorCard::new(
            "模型输出异常",
            "模型连续输出循环内容，运行时已中断本轮。",
            message,
            "直接重试，或换一个模型后再试。",
        ),
        "INVALID_REQUEST" => ErrorCard::new(
            "请求无效",
            "运行时拒绝了这次请求。",
            message,
            "调整输入或附件后重试。",
        ),
        _ => ErrorCard::new(
            "执行失败",
            "运行时在本轮完成前出错。",
            message,
            "可直接重试；若持续失败，请查看运行时日志。",
        ),
    };
    Some(card)
}

/// The `error` event, a terminal frame's `error` object and a queue failure all
/// carry the same payload shape; some wrap it as `{"error": {...}}`, some put
/// the sentence at the top level. Accept either without inventing a code.
pub fn from_payload(payload: &Value) -> Option<ErrorCard> {
    let nested = payload.get("error").filter(|error| error.is_object());
    let source = nested.unwrap_or(payload);
    let code = source
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| payload.get("code").and_then(Value::as_str))
        .unwrap_or_default();
    let message = source
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .or_else(|| payload.get("error").and_then(Value::as_str))
        .unwrap_or_default();
    from_parts(code, message)
}

/// The stream itself failed before any structured error reached the reducer.
/// The sentence is all we have, and it is still the best thing to show.
pub fn generic(message: &str) -> ErrorCard {
    let message = bound(message);
    if message.is_empty() {
        return ErrorCard::new(
            "执行失败",
            "本轮执行没有完成。",
            "",
            "可直接重试；若持续失败，请查看运行时日志。",
        );
    }
    ErrorCard::new(
        "执行失败",
        &message,
        "",
        "可直接重试；若持续失败，请检查模型配置或查看运行时日志。",
    )
}

/// One restored turn's failure code and message to its card.
pub fn from_failure(code: &str, message: &str) -> ErrorCard {
    from_parts(code, message).unwrap_or_else(|| generic(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_config_names_the_settings_step() {
        let card = from_parts("LLM_UNAVAILABLE", "模型不可用: 至少需要一套模型配置").unwrap();
        assert_eq!(card.title, "未配置可用模型");
        assert!(card.hint.contains("设置"));
        assert_eq!(card.detail, "模型不可用: 至少需要一套模型配置");
    }

    #[test]
    fn provider_5xx_gets_its_own_recovery_hint() {
        let card = from_parts(
            "LLM_UNAVAILABLE",
            "模型不可用: LLM request failed: HTTP 550 (empty response body) · 共尝试 6 次、耗时 89.5 秒；模型服务持续失败。",
        )
        .unwrap();
        assert_eq!(card.title, "模型服务暂时不可用");
        assert!(card.detail.contains("550"));
        assert!(card.hint.contains("稍后重试"));
    }

    #[test]
    fn provider_auth_failure_is_its_own_title() {
        let card = from_parts(
            "LLM_REQUEST_REJECTED",
            "LLM request failed: 401 {\"error\":{\"code\":\"invalid_api_key\"}}",
        )
        .unwrap();
        assert_eq!(card.title, "模型鉴权失败");
        assert!(card.detail.contains("401"));
    }

    #[test]
    fn status_codes_select_their_recovery_hint() {
        let context = from_parts("CONTEXT_WINDOW_EXCEEDED", "上下文超长").unwrap();
        assert_eq!(context.title, "上下文超出模型上限");
        let busy = from_parts("USER_BUSY", "线程正在忙").unwrap();
        assert_eq!(busy.title, "线程正在处理其他任务");
    }

    #[test]
    fn oversized_image_request_gets_its_own_recovery_hint() {
        let card = from_parts(
            "LLM_REQUEST_TOO_LARGE",
            "模型请求约 33.2 MiB，超过 32.0 MiB 的请求上限。",
        )
        .unwrap();
        assert_eq!(card.title, "请求体积超出上限");
        assert!(card.hint.contains("重读"));
    }

    #[test]
    fn a_cancelled_turn_never_gets_a_failure_card() {
        assert!(from_parts("CANCELLED", "已取消").is_none());
        assert!(from_parts("LLM_UNAVAILABLE", "").is_none());
    }

    #[test]
    fn nested_and_flat_payloads_classify_identically() {
        let nested = from_payload(&serde_json::json!({
            "error": {"code": "LLM_UNAVAILABLE", "message": "模型不可用: 至少需要一套模型配置"}
        }));
        let flat = from_payload(&serde_json::json!({
            "code": "LLM_UNAVAILABLE",
            "message": "模型不可用: 至少需要一套模型配置"
        }));
        assert_eq!(nested, flat);
    }
}
