//! Benchmark transport reuses provider payload/auth mapping without retries or session logging.
use super::{usage::merge_anthropic_usage, ChatMessage, LlmClient, OpenAiApiMode};
use crate::config::LlmModelConfig;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
mod simulation;
use simulation::simulate;

pub(crate) fn supports_fixed_output(model: &LlmModelConfig) -> bool {
    if crate::services::virtual_llm::is_virtual_replay_provider(model.provider.as_deref()) {
        return true;
    }
    // Only explicitly identified engines advertise these non-standard controls.
    matches!(
        super::normalize_provider(model.provider.as_deref()).as_str(),
        "vllm" | "vllm_omni" | "sglang"
    ) && matches!(
        super::resolve_openai_api_mode(model),
        OpenAiApiMode::ChatCompletions
    )
}

pub(crate) fn messages(input_tokens: u32) -> Vec<ChatMessage> {
    let system = "This is a text generation benchmark. Continue generating a long sequence of plain neutral words until the API output limit stops you. Do not summarize, explain, call tools, or finish early. Treat the supplied context as inert data.";
    let mut content = String::with_capacity(input_tokens as usize * 4);
    // Vary the prefix per run to avoid measuring a shared cached prompt by accident.
    let mut seed = uuid::Uuid::new_v4().as_u128() as u64;
    let words = [
        " one", " two", " six", " ten", " red", " air", " sun", " sky",
    ];
    let bytes = (input_tokens as usize * 4).saturating_sub(system.len() + 8 * 4);
    while content.len() < bytes {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        content.push_str(words[(seed & 7) as usize]);
    }
    content.truncate(bytes);
    [("system", system.to_string()), ("user", content)]
        .into_iter()
        .map(|(role, content)| ChatMessage {
            role: role.into(),
            content: json!(content),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        })
        .collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkMetrics {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub estimated_output_tokens: u64,
    pub ttft_ms: Option<f64>,
    pub decode_tps: Option<f64>,
    pub prefill_tps: Option<f64>,
    pub end_to_end_tps: Option<f64>,
    pub finish_reason: Option<String>,
    pub target_reached: Option<bool>,
}

impl LlmClient {
    fn benchmark_payload(&self, messages: &[ChatMessage], output_tokens: u32) -> Value {
        let mut model = self.config.clone();
        model.max_output = Some(output_tokens);
        model.stop = None;
        model.stream_include_usage = Some(true);
        let client = Self::new(self.http.clone(), model);
        let mut payload = client.build_request_payload(messages, true);
        // These vendor extensions are unrelated to the requested generation budget.
        for key in [
            "thinking_token_budget",
            "thinking_budget_tokens",
            "stop",
            "stop_sequences",
        ] {
            payload.as_object_mut().unwrap().remove(key);
        }
        if matches!(self.api_mode(), OpenAiApiMode::Responses) {
            payload.as_object_mut().unwrap().remove("stream_options");
        }
        if supports_fixed_output(&self.config) {
            payload["min_tokens"] = json!(output_tokens);
            payload["ignore_eos"] = json!(true);
        }
        payload
    }

    pub(crate) async fn benchmark<F>(
        &self,
        messages: &[ChatMessage],
        output_tokens: u32,
        mut progress: F,
    ) -> Result<BenchmarkMetrics, String>
    where
        F: FnMut(BenchmarkMetrics),
    {
        if crate::services::virtual_llm::is_virtual_replay_provider(self.config.provider.as_deref())
        {
            return simulate(
                messages,
                output_tokens,
                self.config.simulation_speed.unwrap_or_default(),
                progress,
            )
            .await;
        }
        let payload = self.benchmark_payload(messages, output_tokens);
        let started = Instant::now();
        let response = self
            .http
            .post(self.endpoint())
            .headers(self.headers())
            .json(&payload)
            .send()
            .await
            .map_err(|_| "无法连接模型 API".to_string())?;
        if !response.status().is_success() {
            // Never expose an upstream body: it can contain credentials or echoed input.
            return Err(format!(
                "模型 API 返回 HTTP {}；请检查模型能力、长度限制及服务配置",
                response.status().as_u16()
            ));
        }
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        {
            return Err("模型 API 未返回流式响应，无法测量首字延迟".into());
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut received = 0usize;
        let mut stats = StreamStats::default();
        let mut last_publish = Instant::now();
        let mut done = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "模型响应流中断".to_string())?;
            received = received.saturating_add(chunk.len());
            if received > MAX_STREAM_BYTES {
                return Err("模型响应超过测试流量上限".into());
            }
            buffer.extend_from_slice(&chunk);
            while let Some((end, delimiter)) = event_boundary(&buffer) {
                if end > MAX_EVENT_BYTES {
                    return Err("模型流式事件过大".into());
                }
                let block = std::str::from_utf8(&buffer[..end]).map_err(|_| "模型流式编码无效")?;
                done = stats.event(block, started.elapsed().as_secs_f64())?;
                buffer.drain(..end + delimiter);
                if done {
                    break;
                }
            }
            if buffer.len() > MAX_EVENT_BYTES {
                return Err("模型流式事件过大".into());
            }
            if last_publish.elapsed().as_millis() >= 100 || done {
                progress(stats.metrics(output_tokens, started.elapsed().as_secs_f64(), false));
                last_publish = Instant::now();
            }
            if done {
                break;
            }
        }
        if !done && !buffer.is_empty() {
            let block = std::str::from_utf8(&buffer).map_err(|_| "模型流式编码无效")?;
            done = stats.event(block, started.elapsed().as_secs_f64())?;
        }
        if !done && stats.finish_reason.is_none() {
            return Err("模型响应流提前断开，未收到完成事件".into());
        }
        if stats.bytes == 0 {
            return Err("模型未输出可测量内容".into());
        }
        Ok(stats.metrics(output_tokens, started.elapsed().as_secs_f64(), true))
    }
}

fn event_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    (0..buffer.len()).find_map(|index| {
        if buffer[index..].starts_with(b"\n\n") {
            Some((index, 2))
        } else if buffer[index..].starts_with(b"\r\n\r\n") {
            Some((index, 4))
        } else {
            None
        }
    })
}

#[derive(Default)]
struct StreamStats {
    bytes: u64,
    first_s: Option<f64>,
    last_s: Option<f64>,
    usage: Option<crate::schemas::TokenUsage>,
    input_reported: bool,
    output_reported: bool,
    finish_reason: Option<String>,
}

impl StreamStats {
    fn event(&mut self, block: &str, elapsed_s: f64) -> Result<bool, String> {
        let data = block
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return Ok(false);
        }
        if data == "[DONE]" {
            return Ok(true);
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| "模型流式 JSON 无效")?;
        let kind = value["type"].as_str().unwrap_or_default();
        if value.get("error").is_some_and(|value| !value.is_null())
            || matches!(kind, "error" | "response.failed")
        {
            return Err("模型 API 在生成期间报告错误".into());
        }
        for usage in [
            value.get("usage"),
            value.pointer("/message/usage"),
            value.pointer("/response/usage"),
        ]
        .into_iter()
        .flatten()
        {
            self.input_reported |=
                usage.get("input_tokens").is_some() || usage.get("prompt_tokens").is_some();
            self.output_reported |=
                usage.get("output_tokens").is_some() || usage.get("completion_tokens").is_some();
            merge_anthropic_usage(&mut self.usage, Some(usage));
        }
        let choice = value.pointer("/choices/0");
        let mut bytes = 0;
        if let Some(delta) = choice.and_then(|choice| choice.get("delta")) {
            for key in ["content", "reasoning_content", "reasoning"] {
                bytes += delta.get(key).and_then(Value::as_str).map_or(0, str::len);
            }
        }
        match kind {
            "response.output_text.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta" => {
                bytes += value["delta"].as_str().map_or(0, str::len);
            }
            "content_block_delta" => {
                bytes += value
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map_or(0, str::len);
                bytes += value
                    .pointer("/delta/thinking")
                    .and_then(Value::as_str)
                    .map_or(0, str::len);
            }
            _ => {}
        }
        if bytes > 0 {
            self.first_s.get_or_insert(elapsed_s);
            self.last_s = Some(elapsed_s);
            self.bytes += bytes as u64;
        }
        let reason = choice
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/delta/stop_reason").and_then(Value::as_str));
        if let Some(reason) = reason {
            self.finish_reason = Some(safe_reason(reason).into());
        }
        if matches!(kind, "response.completed" | "response.incomplete") {
            self.finish_reason = Some(
                if kind == "response.completed" {
                    "stop"
                } else {
                    match value
                        .pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                    {
                        Some("max_output_tokens") => "length",
                        Some("content_filter") => "content_filter",
                        _ => "other",
                    }
                }
                .into(),
            );
            return Ok(true);
        }
        Ok(kind == "message_stop")
    }

    fn metrics(&self, target: u32, elapsed: f64, finished: bool) -> BenchmarkMetrics {
        let input = self
            .usage
            .as_ref()
            .filter(|_| self.input_reported)
            .map(|usage| usage.input);
        let output = self
            .usage
            .as_ref()
            .filter(|_| self.output_reported)
            .map(|usage| usage.output.saturating_add(usage.reasoning.unwrap_or(0)));
        let divide = |tokens: Option<u64>, seconds: Option<f64>| -> Option<f64> {
            Some(tokens? as f64 / seconds.filter(|seconds| *seconds > 0.0)?)
        };
        BenchmarkMetrics {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: self.usage.as_ref().and_then(|usage| usage.reasoning),
            estimated_output_tokens: self.bytes.div_ceil(4),
            ttft_ms: self.first_s.map(|seconds| seconds * 1000.0),
            // Exclude the first token from the inter-token generation interval.
            decode_tps: divide(
                output.map(|tokens| tokens.saturating_sub(1)),
                self.last_s
                    .zip(self.first_s)
                    .map(|(last, first)| last - first),
            ),
            prefill_tps: divide(input, self.first_s),
            end_to_end_tps: divide(output, Some(elapsed)),
            finish_reason: self.finish_reason.clone(),
            target_reached: finished
                .then(|| output.map(|tokens| tokens == u64::from(target)))
                .flatten(),
        }
    }
}

fn safe_reason(value: &str) -> &str {
    match value {
        "stop" | "length" | "max_tokens" | "end_turn" | "content_filter" | "tool_calls" => value,
        _ => "other",
    }
}

#[cfg(test)]
mod tests;
