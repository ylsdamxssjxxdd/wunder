// 线程轨迹的数据模型（对齐蜂巢 trajectoryModel.ts）：
// 把线程日志快照折叠成 轮次 → 步骤组 → 记录，并给出时间线投影、
// 请求编号、折叠与虚拟行布局。纯函数实现，不持有 DOM。

export const TRAJ_KINDS = ["system", "user", "context", "compacted", "message", "tool", "subtool"];

const REQUEST_SEPARATOR = "\u0000";
const TEXT_PREVIEW_SOURCE_LIMIT = 2048;
const TEXT_PREVIEW_LIMIT = 512;
const USER_TURN_KEYS = ["user_turn_index", "user_round", "turn_index", "turn"];
const STARTED_KEYS = ["started_at", "start_time", "started_time", "created_at"];
const COMPLETED_KEYS = ["completed_at", "end_time", "finished_at", "finished_time"];
const CONTEXT_RAW_KINDS = new Set([
  "queue",
  "plan",
  "approval",
  "terminal",
  "context",
  "note",
  "status",
  "context_message",
]);

export const createModelTranslate = (t) => (key, params) => t(`trajectory.${key}`, params);

/* ------------------------------------------------------------------ */
/* 格式化                                                              */
/* ------------------------------------------------------------------ */

/** 整数毫秒 + 千分位，如「11,049 毫秒」。 */
export function formatDurationMillis(milliseconds, t) {
  if (milliseconds === null || milliseconds === undefined || !Number.isFinite(milliseconds)) {
    return t("unavailable");
  }
  return t("durationMillis", { value: Math.round(milliseconds).toLocaleString("en-US") });
}

export function formatElapsedSeconds(seconds, t) {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) {
    return t("unavailable");
  }
  return formatDurationMillis(seconds * 1000, t);
}

const pad2 = (value) => String(value).padStart(2, "0");
const pad3 = (value) => String(value).padStart(3, "0");

/** YYYY-MM-DD HH:mm:ss.SSS（本地时区）。 */
export function formatStartedAt(ms) {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "";
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  return (
    `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ` +
    `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}.` +
    `${pad3(date.getMilliseconds())}`
  );
}

/** HH:mm:ss.SSS（本地时区）。 */
export function formatClock(ms) {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "";
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}.${pad3(date.getMilliseconds())}`;
}

/** 有界的纯文本预览：去掉 markdown 标记并截断。 */
export function markdownPreviewText(text) {
  const source = String(text || "");
  const bounded =
    source.length > TEXT_PREVIEW_SOURCE_LIMIT
      ? `${source.slice(0, TEXT_PREVIEW_SOURCE_LIMIT)}…`
      : source;
  const plain = bounded
    .replace(/```[^\n]*\n?([\s\S]*?)```/g, (match) => {
      const body = match.replace(/^```[^\n]*\n?/, "").replace(/```\s*$/, "");
      return body.trim();
    })
    .replace(/`([^`\n]+)`/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\(([^)]*)\)/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/^(\s*)[-*+]\s+(?:\[[ xX]\]\s+)?/gm, "$1")
    .replace(/^\s*\d+\.\s+/gm, "")
    .replace(/(\*\*\*|\*\*|__|~~)/g, "")
    .replace(/<[^>]+>/g, "");
  return plain.length > TEXT_PREVIEW_LIMIT ? `${plain.slice(0, TEXT_PREVIEW_LIMIT)}…` : plain;
}

/* ------------------------------------------------------------------ */
/* 快照字段读取                                                        */
/* ------------------------------------------------------------------ */

const isRecord = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

const payloadOf = (item) => {
  if (!isRecord(item)) return {};
  // Admin snapshots use both row-shaped items (`payload`) and flat projected
  // rows.  Preserve the row fields when no envelope exists so reasoning,
  // schema and compaction metadata do not disappear in the bridge.
  if (!isRecord(item.payload)) return item;
  return { ...item, ...item.payload };
};

const metaOf = (payload) => (isRecord(payload.meta) ? payload.meta : {});

function rawKindOf(item, payload) {
  if (typeof item.kind === "string" && item.kind) return item.kind;
  const eventType = payload.event_type;
  if (typeof eventType === "string" && eventType) return eventType;
  const metaType = metaOf(payload).type;
  return typeof metaType === "string" ? metaType : "";
}

/** 时间字段统一为毫秒时间戳；秒级数值按 1e11 阈值启发式放大。 */
function timestampMs(value) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.abs(value) < 1e11 ? value * 1000 : value;
  }
  if (typeof value === "string" && value) {
    const parsed = Date.parse(value);
    if (!Number.isNaN(parsed)) return parsed;
  }
  return null;
}

function firstTime(source, keys) {
  if (!isRecord(source)) return null;
  for (const key of keys) {
    const ms = timestampMs(source[key]);
    if (ms !== null) return ms;
  }
  return null;
}

function contentToText(value) {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    return value
      .map((block) => {
        if (typeof block === "string") return block;
        if (isRecord(block)) {
          const text = block.text ?? block.content ?? block.thinking;
          return typeof text === "string" ? text : "";
        }
        return "";
      })
      .filter((part) => part.length > 0)
      .join("\n");
  }
  if (isRecord(value)) {
    const text = value.text ?? value.content;
    if (typeof text === "string") return text;
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return "";
    }
  }
  return value === null || value === undefined ? "" : String(value);
}

function summarizeToolResult(payload) {
  const data = payload.data;
  if (typeof data === "string") return data;
  if (isRecord(data)) {
    const nested = data.result ?? data.output ?? data.content ?? data.text;
    if (typeof nested === "string") return nested;
    if (typeof data.summary === "string") return data.summary;
  }
  const fallback = payload.result ?? payload.output ?? payload.content;
  return contentToText(fallback);
}

function firstNonEmptyText(...values) {
  for (const value of values) {
    const text = contentToText(value).trim();
    if (text) return text;
  }
  return "";
}

function finiteNonNegative(value) {
  const number = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  return Number.isFinite(number) && number >= 0 ? number : null;
}

function assistantRecordedDurationSeconds(payload) {
  const meta = metaOf(payload);
  const stats = isRecord(payload.message_stats)
    ? payload.message_stats
    : isRecord(payload.messageStats)
      ? payload.messageStats
      : isRecord(meta.message_stats)
        ? meta.message_stats
        : isRecord(meta.messageStats) ? meta.messageStats : undefined;
  if (!stats) return null;
  return finiteNonNegative(
    stats.model_duration_s ?? stats.request_duration_s ?? stats.duration_s ?? stats.durationSeconds,
  );
}

// Stream snapshots may wrap the event body in `data`/`payload.data`.  Keep a
// single bounded merge so reasoning, compaction summaries and tool metadata
// are read identically in the bridge and hive projections.
function eventPayloadOf(payload) {
  if (!isRecord(payload)) return {};
  const nested = isRecord(payload.data) ? payload.data : null;
  return nested ? { ...payload, ...nested } : payload;
}

function isCompactionMarkerPayload(payload) {
  const meta = metaOf(payload);
  const type = String(meta.type ?? payload.type ?? '').trim().toLowerCase();
  return meta.manual_compaction === true ||
    payload.manual_compaction_marker === true ||
    payload.manualCompactionMarker === true ||
    type === 'manual_compaction_marker' ||
    (typeof meta.compaction_id === 'string' &&
      (type === 'system_message' || Array.isArray(meta.replacement_history)));
}

function extractUsage(payload) {
  const usage = {};
  const sources = [];
  // 历史快照和实时事件的用量字段并不完全一致：除了顶层 usage/stats，
  // 服务端还可能把它们包在 data、message_stats 或 round_usage 中。
  // 只展开有限的已知容器，避免对大 payload 做无界递归扫描。
  const addSource = (value) => {
    if (!isRecord(value)) return;
    sources.push(value);
    for (const key of ["usage", "round_usage", "roundUsage", "message_stats", "messageStats"]) {
      if (isRecord(value[key])) sources.push(value[key]);
    }
  };
  addSource(payload.usage);
  addSource(payload.round_usage);
  addSource(payload.roundUsage);
  addSource(payload.stats);
  addSource(payload.message_stats);
  addSource(payload.messageStats);
  addSource(payload.data);
  addSource(metaOf(payload).message_stats);
  addSource(metaOf(payload).messageStats);
  addSource(payload);
  if (sources.length === 0) return undefined;
  const numeric = (value) => {
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) return value;
    if (typeof value === "string" && value.trim()) {
      const parsed = Number(value);
      if (Number.isFinite(parsed) && parsed >= 0) return parsed;
    }
    return undefined;
  };
  const pick = (keys) => {
    for (const source of sources) {
      for (const key of keys) {
        const value = numeric(source[key]);
        if (value !== undefined) return value;
      }
    }
    return undefined;
  };
  usage.input = pick(["input_tokens", "prompt_tokens", "inputTokens", "promptTokens", "input", "prompt"]);
  usage.cacheRead = pick([
    "cached_input_tokens", "cache_read_input_tokens", "cached_tokens",
    "cacheReadTokens", "cacheReadInputTokens", "cache_read_tokens",
  ]);
  usage.cacheWrite = pick([
    "cache_creation_input_tokens", "cacheWriteTokens", "cacheCreationInputTokens",
  ]);
  usage.output = pick([
    "output_tokens", "completion_tokens", "outputTokens", "completionTokens", "output", "completion",
  ]);
  usage.think = pick(["reasoning_tokens", "reasoningTokens", "reasoning"]);
  return Object.values(usage).some((value) => value !== undefined) ? usage : undefined;
}

function resolveKind(rawKind, payload) {
  if (metaOf(payload).type === "system_prompt") return "system";
  if (rawKind === "assistant_message" || rawKind === "assistant") return "message";
  // Mailbox deliveries are context consumed by the model, not tool calls.
  // Keep this in the input lane so the bridge and hive use the same projection.
  if (rawKind === "subagent_message") return "context";
  if (rawKind === "subagent" || rawKind === "subagent_run") return "subtool";
  if (
    rawKind === "tool_message" ||
    rawKind === "tool_call" ||
    rawKind === "tool_result" ||
    rawKind === "tool"
  ) {
    return "tool";
  }
  if (rawKind === "compaction" || rawKind === "compacted" || rawKind === "compaction_summary") {
    return "compacted";
  }
  if (rawKind === "user_message" || rawKind === "user") return "user";
  if (rawKind === "system_message" || rawKind === "system") return "system";
  if (CONTEXT_RAW_KINDS.has(rawKind)) return "context";
  return "context";
}

function hasExplicitToolError(value) {
  if (value === true) return true;
  if (typeof value === "string") return value.trim().length > 0;
  return false;
}

function isToolFailure(payload, status = "") {
  return (
    hasExplicitToolError(payload.error ?? payload.is_error) ||
    payload.ok === false ||
    payload.success === false ||
    status === "failed" ||
    status === "error"
  );
}

/* ------------------------------------------------------------------ */
/* 单元格构建                                                          */
/* ------------------------------------------------------------------ */

function defineLazy(target, key, compute) {
    let ready = false;
    let cached;
    Object.defineProperty(target, key, {
        configurable: true,
        enumerable: true,
        get() {
            if (!ready) {
                cached = compute();
                ready = true;
            }
            return cached;
        },
    });
}

function contextFallbackText(payload) {
    // 故意不含 status/stop_reason：「completed」「model_response」这类生命周期
    // 值对时间线没有信息量，宁可显示无内容也不制造噪音。
    for (const key of [
        'summary', 'message', 'reason', 'stage', 'description', 'code',
    ]) {
        const value = payload[key];
        if (typeof value === 'string' && value.trim())
            return value;
    }
    return '';
}
function buildCell(item, payload, kind, index) {
    payload = eventPayloadOf(payload);
    const status = typeof item.status === 'string' ? item.status : '';
    const recordId = typeof item.item_id === 'string' ? item.item_id : String(index);
    const createdMs = firstTime(item, ['created_time']) ?? firstTime(payload, ['created_time']);
    const updatedMs = firstTime(item, ['updated_time']);
    let startedAt = firstTime(payload, STARTED_KEYS) ?? createdMs;
    const completedAt = ['running', 'queued', 'waiting_input', 'waiting_approval'].includes(status)
        ? null : firstTime(payload, COMPLETED_KEYS) ?? updatedMs;
    const recordedDuration = kind === 'message' ? assistantRecordedDurationSeconds(payload) : null;
    // Durable assistant rows are created when the model round is admitted, but
    // updated only after child/tool work and replay have settled.  That row
    // timestamp is bookkeeping, not the model span start.  When the runtime
    // recorded an explicit per-request duration, anchor the span at completion and
    // derive the start so delegated child work stays visibly separate.
    if (kind === 'message' && recordedDuration !== null
        && firstTime(payload, STARTED_KEYS) === null
        && firstTime(payload, COMPLETED_KEYS) === null
        && completedAt !== null) {
        startedAt = completedAt - recordedDuration * 1000;
    }
    const cell = {
        index,
        recordId,
        kind,
        text: '',
        timeSeconds: null,
        startedAt,
        status,
        failure: ['message', 'tool', 'subtool', 'compacted'].includes(kind) || ['terminal', 'turn_terminal'].includes(rawKindOf(item, payload)) ? trajectoryFailure(payload, status) : undefined,
        retry: trajectoryRetry(payload),
    };
    cell.overviewOnly = item.overview_only === true;
    const usage = extractUsage(payload);
    if (kind === 'message') {
        const contentText = contentToText(payload.content);
        // 流式壳写入 `reasoning`；会话写入器覆盖 payload 后字段名为
        // `reasoning_content`，两个来源都要读。
        const thinkingSources = [
            payload.reasoning, payload.reasoning_content, payload.thinking, payload.thought,
            payload.reasoningText, payload.thinking_content,
            isRecord(payload.meta) ? payload.meta.reasoning_content : undefined,
            isRecord(payload.meta) ? payload.meta.reasoning : undefined,
            isRecord(payload.meta) ? payload.meta.thinking : undefined,
        ];
        const thinkingText = thinkingSources.map(contentToText).find(value => value.trim()) ?? '';
        const toolCalls = payload.tool_calls;
        if (!contentText.trim() && Array.isArray(toolCalls) && toolCalls.length > 0) {
            cell.text = '';
            cell.toolCallOnly = true;
        }
        else {
            const source = contentText;
            defineLazy(cell, 'text', () => markdownPreviewText(source));
            cell.previewMarkdown = source.trim() ? source : undefined;
        }
        const prefill = typeof payload.prefill_duration_s === 'number'
            && Number.isFinite(payload.prefill_duration_s) ? payload.prefill_duration_s : null;
        const decode = typeof payload.decode_duration_s === 'number'
            && Number.isFinite(payload.decode_duration_s) ? payload.decode_duration_s : null;
        // Durable assistant rows often receive `updated_time` only after the
        // child/tool work and final replay are committed. Using that bookkeeping
        // timestamp makes the purple model span cover the whole child run. Prefer
        // an explicit per-request duration when assistant
        // start/end fields are absent; this is the same own-step duration DSH uses.
        cell.timeSeconds = recordedDuration !== null
            && firstTime(payload, STARTED_KEYS) === null
            && firstTime(payload, COMPLETED_KEYS) === null
            ? recordedDuration
            : startedAt !== null && completedAt !== null
                ? Math.max(0, (completedAt - startedAt) / 1000)
                : prefill !== null && decode !== null ? prefill + decode : null;
        // Assistant rows contain the model response.  The persisted row does not
        // carry the prompt that produced it, so exposing `content` as both input
        // and output makes the overview misleading.  Keep the response in the
        // output section and leave input absent until a real request payload is
        // available.
        cell.inputDetail = undefined;
        cell.outputDetail = contentText.trim() ? contentText : undefined;
        cell.thinkingDetail = thinkingText.trim() ? thinkingText : undefined;
        cell.usage = usage;
        const ttft = typeof payload.ttft_ms === 'number' && Number.isFinite(payload.ttft_ms)
            ? payload.ttft_ms
            : null;
        cell.metrics = {
            timingRecorded: startedAt !== null && completedAt !== null,
            stepStartTime: startedAt,
            firstTokenTime: ttft !== null && startedAt !== null ? startedAt + ttft : null,
            completedTime: completedAt,
            usageProvided: usage !== undefined,
            outputTokens: usage?.output ?? null,
        };
        cell.isError = cell.failure !== undefined || failedStatus(status);
    }
    else if (kind === 'tool' || kind === 'subtool') {
        const runtime = isRecord(payload.runtime) ? payload.runtime : undefined;
        const runtimeState = runtime && isRecord(runtime.agent_state) ? runtime.agent_state : undefined;
        const isSubagentRun = kind === 'subtool' && runtime !== undefined;
        const subagentLabel = isSubagentRun
            ? firstNonEmptyText(payload.spawn_label, payload.label, runtime?.spawn_label, runtime?.label, runtimeState?.label, 'subagent_run')
            : '';
        const subagentTask = isSubagentRun
            ? firstNonEmptyText(payload.task, payload.user_message, runtime?.user_message, runtime?.task, runtimeState?.user_message)
            : '';
        const subagentSummary = isSubagentRun
            ? firstNonEmptyText(runtime?.summary, runtime?.latest_message, runtimeState?.message, runtimeState?.assistant_message, payload.summary, payload.message)
            : '';
        if (isSubagentRun) {
            // DSH keeps delegated work as a distinct subtool span. Preserve the
            // child label, task and final report instead of rendering an empty
            // `subagent_run` placeholder beside the parent tool call.
            cell.text = subagentLabel;
            cell.toolName = subagentLabel;
            cell.inputDetail = subagentTask || undefined;
            cell.outputDetail = subagentSummary || undefined;
            cell.result = subagentSummary || undefined;
            if (subagentSummary)
                defineLazy(cell, 'resultPreviewMarkdown', () => markdownPreviewText(subagentSummary));
            const runtimeStart = firstTime(runtime ?? {}, STARTED_KEYS);
            const runtimeEnd = firstTime(runtime ?? {}, COMPLETED_KEYS);
            if (runtimeStart !== null)
                cell.startedAt = runtimeStart;
            if (runtimeStart !== null && runtimeEnd !== null) {
                cell.timeSeconds = Math.max(0, (runtimeEnd - runtimeStart) / 1000);
            }
            cell.isError = runtime?.failed === true || runtime?.status === 'failed' || status === 'failed';
            return cell;
        }
        const name = typeof payload.tool === 'string' && payload.tool.trim()
            ? payload.tool
            : typeof payload.name === 'string' && payload.name.trim() ? payload.name : '';
        const args = payload.args;
        const argsRaw = typeof args === 'string' ? args : isRecord(args) || Array.isArray(args)
            ? JSON.stringify(args, null, 2)
            : '';
        const resultRaw = summarizeToolResult(payload);
        cell.text = name;
        cell.toolName = name || undefined;
        cell.callId = typeof payload.tool_call_id === 'string' ? payload.tool_call_id : undefined;
        cell.inputDetail = argsRaw.trim() ? argsRaw : undefined;
        cell.outputDetail = resultRaw.trim() ? resultRaw : undefined;
        cell.result = resultRaw.trim() ? resultRaw : undefined;
        if (resultRaw.trim())
            defineLazy(cell, 'resultPreviewMarkdown', () => markdownPreviewText(resultRaw));
        else
            cell.resultPreviewMarkdown = undefined;
        const schema = payload.schema ?? payload.tool_schema ?? payload.schema_ref;
        if (typeof schema === 'string' && schema.trim())
            cell.schemaDetail = schema;
        else if (isRecord(schema))
            cell.schemaDetail = JSON.stringify(schema, null, 2);
        cell.isError = isToolFailure(payload, status);
        cell.timeSeconds = startedAt !== null && completedAt !== null
            ? Math.max(0, (completedAt - startedAt) / 1000)
            : null;
    }
    else if (kind === 'compacted') {
        const summaryText = contentToText(payload.summary_text ?? payload.summaryText ?? payload.summary_model_output ??
            payload.summaryModelOutput ?? payload.summary ??
            (payload.content === 'history' ? '' : payload.content));
        const text = summaryText.trim() ? summaryText : '';
        cell.text = text;
        cell.previewMarkdown = text.trim() ? text : undefined;
        cell.outputDetail = text.trim() ? text : undefined;
        cell.thinkingDetail = contentToText(payload.raw_output ?? payload.rawOutput ?? payload.summary_model_output ?? payload.summaryModelOutput).trim() || undefined;
        // A compaction row is a workflow result; its summary is output, not a
        // second copy of an input prompt.
        cell.inputDetail = undefined;
        cell.timeSeconds = startedAt !== null && completedAt !== null
            ? Math.max(0, (completedAt - startedAt) / 1000)
            : null;
    }
    else {
        let contentText = contentToText(payload.content);
        if (!contentText.trim())
            contentText = contextFallbackText(payload);
        const source = contentText;
        defineLazy(cell, 'text', () => markdownPreviewText(source));
        cell.previewMarkdown = source.trim() ? source : undefined;
        cell.inputDetail = source.trim() ? source : undefined;
        // 用户/上下文是瞬时事件（dsh 的输入通道即小刻度）：不用 updated_time
        // 兜底补时长，否则会被拉成覆盖整轮的长条，时间线输入通道因此总是塞满。
        cell.timeSeconds = startedAt !== null ? 0 : null;
    }
    if (cell.failure)
        cell.isError = true;
    return cell;
}


/* ------------------------------------------------------------------ */
/* 布局折叠                                                            */
/* ------------------------------------------------------------------ */

/** 把快照的轮次列表折叠成轨迹布局。 */
function diagnosticText(value, limit = 2048) {
    return typeof value === 'string' && value.trim() ? value.slice(0, limit) : undefined;
}
function diagnosticNumber(value) {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}
function failedStatus(status) {
    return ['failed', 'error', 'cancelled', 'interrupted', 'rejected'].includes(status);
}
export function trajectoryFailure(payload, status = '') {
    const diagnostics = isRecord(payload.diagnostics) ? payload.diagnostics : {};
    const direct = payload.trajectory_failure ?? diagnostics.failure;
    const error = isRecord(direct) ? direct : isRecord(payload.error) ? payload.error : {};
    const meta = isRecord(error.error_meta) ? error.error_meta : {};
    if (!isRecord(direct) && !failedStatus(status) && !hasExplicitToolError(payload.error ?? payload.is_error)
        && Object.keys(error).length === 0 && payload.ok !== false && payload.success !== false)
        return undefined;
    return {
        status: diagnosticText(error.status, 48) ?? (failedStatus(status) ? status : 'failed'),
        code: diagnosticText(error.code ?? payload.code ?? payload.stop_reason, 128),
        message: diagnosticText(error.message ?? payload.error ?? payload.message),
        retryable: typeof (error.retryable ?? payload.retryable ?? meta.retryable) === 'boolean'
            ? Boolean(error.retryable ?? payload.retryable ?? meta.retryable) : undefined,
        retryAfterMs: diagnosticNumber(error.retry_after_ms ?? payload.retry_after_ms ?? meta.retry_after_ms),
        recoveryAction: diagnosticText(error.recovery_action ?? payload.recovery_action ?? meta.recovery_action, 128),
        sourceStage: diagnosticText(error.source_stage ?? payload.source_stage ?? meta.source_stage, 128),
    };
}
export function trajectoryRetry(payload) {
    const diagnostics = isRecord(payload.diagnostics) ? payload.diagnostics : {};
    const retry = payload.trajectory_retry ?? diagnostics.retry;
    if (!isRecord(retry))
        return undefined;
    return {
        attempt: diagnosticNumber(retry.attempt),
        maxAttempts: diagnosticNumber(retry.max_attempts),
        delaySeconds: diagnosticNumber(retry.delay_s),
        reason: diagnosticText(retry.reason ?? retry.retry_reason, 128),
        message: diagnosticText(retry.message ?? retry.error),
        willRetry: retry.will_retry === true,
    };
}
/** Identical inspector fields for model failures, tool failures and recovery. */
export function trajectoryDiagnosticRows(cell, t) {
    const rows = [];
    const add = (key, value) => { if (value)
        rows.push({ label: t(`${key}`), value }); };
    const failure = cell.failure;
    if (failure) {
        add('errorCode', failure.code);
        add('errorMessage', failure.message);
        add('errorStage', failure.sourceStage);
        if (failure.retryable !== undefined)
            add('retryable', t(`${failure.retryable ? 'yes' : 'no'}`));
        if (failure.retryAfterMs !== undefined)
            add('retryAfter', formatDurationMillis(failure.retryAfterMs, t));
        add('recoveryAction', failure.recoveryAction ? t(`recovery.${failure.recoveryAction}`) : undefined);
    }
    const retry = cell.retry;
    if (retry) {
        add('retryAttempts', retry.attempt === undefined ? undefined : `${retry.attempt}${retry.maxAttempts === undefined ? '' : ` / ${retry.maxAttempts}`}`);
        if (retry.delaySeconds !== undefined)
            add('retryDelay', formatDurationMillis(retry.delaySeconds * 1000, t));
        add('lastAttemptError', retry.message);
        add('retryReason', retry.reason);
    }
    return rows;
}
function applyTrajectoryFailure(cell, failure, endedAt) {
    cell.failure = failure;
    cell.isError = true;
    cell.status = failure.status;
    if (cell.retry)
        cell.retry = { ...cell.retry, willRetry: false };
    if (cell.metrics && cell.metrics.completedTime === null && endedAt !== null) {
        cell.metrics = { ...cell.metrics, completedTime: endedAt, timingRecorded: cell.startedAt !== null };
        if (cell.startedAt !== null)
            cell.timeSeconds = Math.max(0, (endedAt - cell.startedAt) / 1000);
    }
}

export function buildTrajectoryLayout(rawTurns, t) {
    const turns = [];
    let index = 0;
    for (const rawTurn of Array.isArray(rawTurns) ? rawTurns : []) {
        if (!isRecord(rawTurn))
            continue;
        let turnNo = null;
        for (const key of USER_TURN_KEYS) {
            const value = rawTurn[key];
            if (typeof value === 'number' && Number.isFinite(value)) {
                turnNo = value;
                break;
            }
        }
        const items = Array.isArray(rawTurn.items) ? rawTurn.items : [];
        const model = { turn: turnNo, groups: [] };
        turns.push(model);
        const assistants = new Map();
        const lastAssistants = new Map();
        const pending = [];
        const unfinishedTools = [];
        const scopeOf = (item, payload) => String(item.turn_id ?? payload.turn_id ?? rawTurn.turn_id ?? '');
        let compactionSeq = 0;
        const messageGroup = () => {
            const last = model.groups[model.groups.length - 1];
            if (last && !last.compaction && last.step === undefined)
                return last;
            const group = { title: t('messages'), cells: [] };
            model.groups.push(group);
            return group;
        };
        // step → 分组用 Map 索引，避免每步线性扫描 model.groups（原 O(k²)）。
        const stepGroups = new Map();
        const stepGroup = (step) => {
            const existing = stepGroups.get(step);
            if (existing)
                return existing;
            const group = { title: t('stepLabel', { step }), cells: [], step };
            stepGroups.set(step, group);
            model.groups.push(group);
            return group;
        };
        const pushCompaction = (cell) => {
            compactionSeq += 1;
            model.groups.push({
                title: t('compaction', { seq: compactionSeq }),
                cells: [cell],
                compaction: true,
            });
        };
        const ordered = [...items].filter(isRecord).sort((left, right) => Number(left.created_seq ?? 0) - Number(right.created_seq ?? 0)
            || Number(left.item_index ?? 0) - Number(right.item_index ?? 0));
        for (let i = 0; i < ordered.length; i += 1) {
            const item = ordered[i];
            const payload = payloadOf(item);
            const rawKind = rawKindOf(item, payload);
            if (rawKind === 'terminal' || rawKind === 'turn_terminal' || rawKind === 'llm_stream_retry') {
                if (rawKind === 'llm_stream_retry' || trajectoryFailure(payload, String(item.status ?? payload.status ?? ''))) {
                    pending.push({ item, payload, retry: rawKind === 'llm_stream_retry', scope: scopeOf(item, payload) });
                }
                continue;
            }
            if ((rawKind === 'assistant_message' || rawKind === 'system_message' || rawKind === 'system') && isCompactionMarkerPayload(payload))
                continue;
            // progress/workspace_update/model_usage 是传输态或统计
            // 通知，成功的轮次终止只是生命周期回执；后端已不再入账，这里跳过历史
            // 遗留数据（不占用记录索引）。
            if (rawKind === 'progress' || rawKind === 'workspace_update'
                || rawKind === 'model_usage'
                || rawKind === 'thread_turn_started' || rawKind === 'model_call')
                continue;
            // 历史脏数据：队列瞬态（含 user_withdrew 等取消回执）、会话写入器重复
            // 落账的工具结果（tool_call 条目已合并参数与结果）、命令子会话生命周期、
            // 知识检索与 hook 通知 —— 对轨迹都没有可读价值，一律不占记录索引。
            if (rawKind === 'queue' || rawKind.startsWith('queue_')
                || rawKind === 'tool_message' || rawKind.startsWith('command_session')
                || rawKind === 'knowledge_request' || rawKind.startsWith('hook/'))
                continue;
            const kind = resolveKind(rawKind, payload);
            const cell = buildCell(item, payload, kind, index);
            index += 1;
            // tool_call 与紧随其后的 tool_result 合并成一条记录。
            if (kind === 'tool' && rawKind === 'tool_call') {
                const callId = payload.tool_call_id;
                for (let j = i + 1; j < ordered.length; j += 1) {
                    const candidate = ordered[j];
                    const candidatePayload = payloadOf(candidate);
                    if (rawKindOf(candidate, candidatePayload) !== 'tool_result')
                        continue;
                    if (callId !== undefined && candidatePayload.tool_call_id !== callId)
                        continue;
                    const resultRaw = summarizeToolResult(candidatePayload);
                    if (resultRaw.trim()) {
                        cell.result = resultRaw;
                        cell.outputDetail = resultRaw;
                        defineLazy(cell, 'resultPreviewMarkdown', () => markdownPreviewText(resultRaw));
                    }
                    const candidateStatus = typeof candidate.status === 'string' ? candidate.status : '';
                    cell.status = candidateStatus || 'completed';
                    cell.failure = trajectoryFailure(candidatePayload, candidateStatus);
                    if (isToolFailure(candidatePayload, candidateStatus))
                        cell.isError = true;
                    const candidateStart = firstTime(candidatePayload, STARTED_KEYS)
                        ?? firstTime(candidate, ['created_time']);
                    const candidateEnd = firstTime(candidatePayload, COMPLETED_KEYS)
                        ?? firstTime(candidate, ['updated_time']);
                    if (candidateStart !== null && candidateEnd !== null) {
                        cell.timeSeconds = Math.max(0, (candidateEnd - candidateStart) / 1000);
                    }
                    ordered.splice(j, 1);
                    break;
                }
            }
            if (kind === 'message') {
                const scope = scopeOf(item, payload);
                assistants.set(`${scope}:${payload.model_round ?? 0}`, cell);
                lastAssistants.set(scope, cell);
                const step = payload.model_round;
                if (typeof step === 'number' && Number.isFinite(step)) {
                    stepGroup(step).cells.push(cell);
                }
                else {
                    messageGroup().cells.push(cell);
                }
            }
            else if (kind === 'compacted') {
                pushCompaction(cell);
            }
            else if (kind === 'tool' || kind === 'subtool') {
                if (kind === 'tool' && !cell.result && !cell.failure?.message && !cell.failure?.code && (cell.status === 'running' || failedStatus(cell.status ?? ''))) {
                    unfinishedTools.push({ cell, scope: scopeOf(item, payload) });
                }
                const step = payload.model_round;
                if (typeof step === 'number' && Number.isFinite(step)) {
                    stepGroup(step).cells.push(cell);
                }
                else {
                    const last = model.groups[model.groups.length - 1];
                    if (last && last.step !== undefined && !last.compaction) {
                        last.cells.push(cell);
                    }
                    else {
                        stepGroup(1).cells.push(cell);
                    }
                }
            }
            else {
                messageGroup().cells.push(cell);
            }
        }
        for (const event of pending) {
            const { item, payload, scope } = event;
            const target = assistants.get(`${scope}:${payload.model_round ?? 0}`)
                ?? (event.retry ? undefined : lastAssistants.get(scope));
            if (event.retry) {
                if (target)
                    target.retry = trajectoryRetry({ trajectory_retry: payload });
                continue;
            }
            const failure = trajectoryFailure(payload, String(item.status ?? payload.status ?? 'failed'));
            if (!failure)
                continue;
            const endedAt = firstTime(payload, COMPLETED_KEYS) ?? firstTime(item, ['updated_time', 'created_time']);
            if (target)
                applyTrajectoryFailure(target, failure, endedAt);
            else {
                // Admission can fail before a model request exists. Keep that failure
                // visible instead of incorrectly attaching it to a different execution.
                const fallback = buildCell(item, payload, 'context', index++);
                if (!fallback.text)
                    Object.defineProperty(fallback, 'text', { value: failure.message ?? failure.code ?? t('stateFailed'), enumerable: true, configurable: true });
                applyTrajectoryFailure(fallback, failure, endedAt);
                messageGroup().cells.push(fallback);
            }
            for (const tool of unfinishedTools) {
                if (tool.scope !== scope)
                    continue;
                applyTrajectoryFailure(tool.cell, { status: 'interrupted', code: 'INTERRUPTED', message: t('toolInterrupted') }, endedAt);
            }
        }
        for (const cell of assistants.values()) {
            if (cell.retry && cell.status !== 'running')
                cell.retry = { ...cell.retry, willRetry: false };
        }
    }
    return turns;
}



export function requestIdentity(turn, groupTitle) {
    return `${turn ?? 'none'}${REQUEST_SEPARATOR}${groupTitle}`;
}
function addUsage(target, usage) {
    if (!usage)
        return;
    for (const key of ['input', 'cacheRead', 'cacheWrite', 'output', 'think']) {
        const value = usage[key];
        if (typeof value === 'number' && Number.isFinite(value)) {
            target[key] = (target[key] ?? 0) + value;
        }
    }
}
/** 按会话顺序给「第 N 步」组编号并累计用量。 */
export function buildRequestNumbers(turns) {
    const requests = [];
    const cumulative = {};
    for (const turn of turns) {
        for (const group of turn.groups) {
            if (group.compaction || group.step === undefined)
                continue;
            const messageCell = group.cells.find(cell => cell.kind === 'message');
            if (!messageCell)
                continue;
            addUsage(cumulative, messageCell.usage);
            const status = messageCell.isError
                ? 'error'
                : messageCell.status === 'running' || messageCell.metrics?.completedTime == null ? 'running' : 'complete';
            group.requestNumber = requests.length + 1;
            requests.push({
                identity: requestIdentity(turn.turn, group.title),
                number: requests.length + 1,
                turn: turn.turn,
                step: group.step,
                groupTitle: group.title,
                status,
                startedAt: messageCell.startedAt,
                completedAt: messageCell.metrics?.completedTime ?? null,
                usage: messageCell.usage,
                cumulativeUsage: { ...cumulative },
                assistantIndex: messageCell.index,
                failure: messageCell.failure,
                retry: messageCell.retry,
            });
        }
    }
    return requests;
}
/* ------------------------------------------------------------------ */
/* 时间线投影                                                          */
/* ------------------------------------------------------------------ */
function laneFor(kind) {
    if (kind === 'tool' || kind === 'subtool')
        return 2;
    if (kind === 'message' || kind === 'compacted')
        return 1;
    return 0;
}
function finite(value) {
    return value !== null && value !== undefined && Number.isFinite(value);
}
function cellRange(cell) {
    if (!finite(cell.startedAt))
        return null;
    const durationMs = finite(cell.timeSeconds) ? Math.max(0, cell.timeSeconds * 1000) : 0;
    return { start: cell.startedAt, end: cell.startedAt + durationMs };
}
function ttftFractionFor(cell) {
    const metrics = cell.metrics;
    if (!metrics?.timingRecorded)
        return null;
    const { stepStartTime, firstTokenTime, completedTime } = metrics;
    if (!finite(stepStartTime) || !finite(firstTokenTime) || !finite(completedTime))
        return null;
    const total = completedTime - stepStartTime;
    if (total <= 0)
        return null;
    return Math.min(1, Math.max(0, (firstTokenTime - stepStartTime) / total));
}
function spanFor(cell, start, end) {
    return {
        start,
        end,
        index: cell.index,
        isError: cell.isError === true,
        kind: cell.kind,
        label: cell.text,
        lane: laneFor(cell.kind),
        ttftFraction: ttftFractionFor(cell),
    };
}
/** 把每条记录投影到稳定的三通道时间线。 */
function deriveTimelineUninstrumented(turns, mode = 'sequence') {
    if (mode !== 'sequence') {
        return deriveTimedTimeline(turns, mode === 'duration' || mode === 'actual', mode === 'duration');
    }
    const spans = [];
    const turnBoundaries = [];
    for (const turn of turns) {
        const cells = turn.groups.flatMap(group => group.cells);
        if (cells.length === 0)
            continue;
        if (turn.turn !== null) {
            turnBoundaries.push({ turn: turn.turn, time: spans.length });
        }
        spans.push(...cells.map((cell, offset) => spanFor(cell, spans.length + offset, spans.length + offset + 1)));
    }
    if (spans.length === 0)
        return null;
    return { start: 0, end: spans.length, spans, turnBoundaries };
}
/**
 * W4-L instrumentation seam: trajectory layout is the cost of projecting the
 * ledger onto the timeline, so it is recorded once per call. Both the sequence
 * and the timed layout run inside the measured span.
 */
export function deriveTimeline(turns, mode = 'sequence') { return deriveTimelineUninstrumented(turns, mode); }
function deriveTimedTimeline(turns, actualDuration, compressIdle) {
    const timedTurns = turns.flatMap((turn) => {
        const rawSpans = turn.groups.flatMap(group => group.cells.flatMap((cell) => {
            const range = cellRange(cell);
            if (range === null)
                return [];
            return [{ ...spanFor(cell, range.start, range.end) }];
        }));
        return rawSpans.length === 0 ? [] : [{ turn: turn.turn, rawSpans }];
    });
    const rawSpans = timedTurns.flatMap(turn => turn.rawSpans);
    if (rawSpans.length === 0)
        return null;
    const removedIdleBySpan = new Map();
    let removedIdle = 0;
    let coveredUntil = null;
    for (const span of [...rawSpans].sort((left, right) => left.start - right.start || left.end - right.end)) {
        if (compressIdle && coveredUntil !== null && span.start > coveredUntil) {
            removedIdle += span.start - coveredUntil;
        }
        removedIdleBySpan.set(span, removedIdle);
        coveredUntil = coveredUntil === null ? span.end : Math.max(coveredUntil, span.end);
    }
    const spans = [];
    const turnBoundaries = [];
    // 用循环累加求 min/max，避免对大数组做 Math.min(...) / Math.max(...) 展开。
    let start = Infinity;
    let end = -Infinity;
    for (const turn of timedTurns) {
        let turnStart = Infinity;
        for (const span of turn.rawSpans) {
            const offset = removedIdleBySpan.get(span) ?? 0;
            const projectedStart = span.start - offset;
            const projectedEnd = (actualDuration ? span.end : span.start) - offset;
            spans.push({ ...span, start: projectedStart, end: projectedEnd });
            if (projectedStart < turnStart)
                turnStart = projectedStart;
            if (projectedStart < start)
                start = projectedStart;
            if (projectedEnd > end)
                end = projectedEnd;
        }
        if (turn.turn !== null) {
            turnBoundaries.push({ turn: turn.turn, time: turnStart });
        }
    }
    return { start, end, spans, turnBoundaries };
}
/** 选区覆盖的记录索引集合。 */
export function timelineFocusIndexes(turns, range, mode = 'sequence') {
    const model = deriveTimeline(turns, mode);
    return new Set(model?.spans
        .filter(span => span.start <= range.end && span.end >= range.start)
        .map(span => span.index) ?? []);
}
/* ------------------------------------------------------------------ */
/* 台账记录与折叠                                                      */
/* ------------------------------------------------------------------ */
/** 轮次列表 → 扁平台账记录。 */
export function flattenRecords(turns) {
    const records = [];
    for (const turn of turns) {
        const turnStart = records.length;
        for (const group of turn.groups) {
            const groupStart = records.length;
            for (const cell of group.cells) {
                records.push({
                    turn: turn.turn,
                    section: 0,
                    group: group.title,
                    groupStart: false,
                    turnStart: false,
                    turnEnd: false,
                    cell,
                });
            }
            if (records.length > groupStart)
                records[groupStart].groupStart = true;
        }
        // Group identities annotate rows; they must not reorder durable events.
        const ordered = records.slice(turnStart).sort((left, right) => left.cell.index - right.cell.index);
        for (let index = 0; index < ordered.length; index += 1)
            records[turnStart + index] = ordered[index];
        if (records.length > turnStart) {
            records[turnStart].turnStart = true;
            records[records.length - 1].turnEnd = true;
        }
    }
    return records;
}
function recordNeedle(record) {
    return [
        record.cell.text,
        record.cell.toolName ?? '',
        record.cell.inputDetail ?? '',
        record.cell.outputDetail ?? '',
        record.cell.result ?? '',
        record.cell.failure?.code ?? '',
        record.cell.failure?.message ?? '',
        record.cell.retry?.message ?? '',
    ].join('\n').toLowerCase();
}
/** 搜索过滤：文本不匹配的记录整行隐藏。 */
export function filterRecords(records, query) {
    const needle = query.trim().toLowerCase();
    if (!needle)
        return records;
    return records.filter(record => recordNeedle(record).includes(needle));
}
/** Bounded, linear fold facts, keyed by user turn and stable assistant record ID. */
export function buildFoldGroups(records) {
    const turns = new Map();
    const assistants = new Map();
    const toolOwners = new Map();
    const requestGroups = new Set();
    let assistant = null;
    for (let index = 0; index < records.length; index += 1) {
        const record = records[index];
        const tool = record.cell.kind === 'tool' || record.cell.kind === 'subtool';
        if (record.cell.kind === 'message')
            requestGroups.add(requestIdentity(record.turn, record.group));
        if (record.turn !== null) {
            let turn = turns.get(record.turn);
            if (record.cell.kind !== 'system') {
                if (!turn) {
                    turn = { first: index, hidden: 0, steps: new Set(), tools: 0, turnEnd: record.turnEnd };
                    turns.set(record.turn, turn);
                }
                else {
                    turn.hidden += 1;
                    if (record.group)
                        turn.steps.add(record.group);
                    if (tool)
                        turn.tools += 1;
                }
            }
            if (turn)
                turn.turnEnd = record.turnEnd;
        }
        if (tool && assistant && assistant.turn === record.turn) {
            let group = assistants.get(assistant.key);
            if (!group) {
                group = { end: index + 1, count: 0, names: [] };
                assistants.set(assistant.key, group);
            }
            group.end = index + 1;
            group.count += 1;
            const name = record.cell.toolName || record.cell.text.split(' · ')[0] || '';
            if (name && group.names.length < 8 && !group.names.includes(name))
                group.names.push(name);
            toolOwners.set(record.cell.index, assistant.key);
        }
        else {
            assistant = record.cell.kind === 'message'
                ? { key: trajectoryRecordId(record.cell), turn: record.turn } : null;
        }
    }
    for (const [turn, group] of turns) {
        for (const step of group.steps) {
            if (!requestGroups.has(requestIdentity(turn, step)))
                group.steps.delete(step);
        }
    }
    return { turns, assistants, toolOwners };
}
/** Preserve headers and first content; calls fold only the following tool rows. */
export function collapseTurnRecords(records, collapsedTurns, collapsedAssistants, t) {
    if (collapsedTurns.size === 0 && collapsedAssistants.size === 0)
        return records;
    const groups = buildFoldGroups(records);
    const output = [];
    for (let index = 0; index < records.length; index += 1) {
        const record = records[index];
        const turn = record.turn === null ? undefined : groups.turns.get(record.turn);
        if (record.turn !== null && collapsedTurns.has(record.turn) && turn && turn.hidden > 0) {
            if (record.cell.kind === 'system')
                output.push(record);
            else if (turn.first === index) {
                output.push({ ...record, turnEnd: false });
                output.push({ ...record, groupStart: false, turnStart: false, turnEnd: turn.turnEnd,
                    collapsedSummary: t('summaryTurn', { steps: turn.steps.size, tools: turn.tools }),
                    collapsedSummaryKind: 'turn' });
            }
            continue;
        }
        const key = trajectoryRecordId(record.cell);
        const calls = record.cell.kind === 'message' && collapsedAssistants.has(key)
            ? groups.assistants.get(key) : undefined;
        if (calls) {
            output.push({ ...record, turnEnd: false });
            const names = calls.names.length ? ` · ${calls.names.join(', ')}` : '';
            output.push({ ...record, groupStart: false, turnStart: false,
                turnEnd: records[calls.end - 1].turnEnd,
                collapsedSummary: t('summaryAssistantTools', { count: calls.count }) + names,
                collapsedSummaryKind: 'assistant' });
            index = calls.end - 1;
        }
        else
            output.push(record);
    }
    return output;
}
const ROW_PX = 30;
const ROW_SUMMARY_PX = 20;
/** 固定行高的虚拟行布局（一条记录一行）。 */
export function groupVirtualRows(records) {
    const rows = [];
    records.forEach((record, index) => {
        rows.push({
            record,
            height: record.collapsedSummary ? ROW_SUMMARY_PX : ROW_PX,
            key: `${record.turn ?? 'none'}-${record.group}-${record.cell.recordId}-${index}`,
        });
    });
    return { rows };
}
/** 台账记录的稳定标识。 */
export function trajectoryRecordId(cell) {
    return cell.recordId || String(cell.index);
}

/** Root turns own the display group; each item keeps its execution identity. */
export function attachTurnItems(snapshot) {
  if (!isRecord(snapshot)) return [];
  const turns = Array.isArray(snapshot.turns) ? snapshot.turns.filter(isRecord) : [];
  const byId = new Map(turns.map(turn => [String(turn.turn_id ?? ''), turn]));
  const grouped = new Map();
  for (const turn of turns) {
    const root = String(turn.root_turn_id || turn.turn_id || '');
    if (!grouped.has(root)) {
      const owner = byId.get(root) ?? turn;
      grouped.set(root, { ...owner, turn_id: root, root_turn_id: root, items: [] });
    }
  }
  const items = new Map();
  const add = (item, owner) => {
    if (!isRecord(item)) return;
    const execution = String(item.turn_id || payloadOf(item).turn_id || owner?.turn_id || '');
    const root = String(item.root_turn_id || byId.get(execution)?.root_turn_id || owner?.root_turn_id || execution);
    const turn = grouped.get(root);
    if (!turn) return;
    if (!item.turn_id && execution) item = { ...item, turn_id: execution };
    const id = String(item.item_id ?? '');
    if (id) {
      const previous = items.get(id);
      if (previous) {
        if (Number(item.revision ?? 0) < Number(previous.item.revision ?? 0)) return;
        previous.turn.items[previous.index] = item;
        previous.item = item;
        return;
      }
      items.set(id, { item, turn, index: turn.items.length });
    }
    turn.items.push(item);
  };
  for (const turn of turns) {
    for (const item of Array.isArray(turn.items) ? turn.items : []) add(item, turn);
  }
  for (const item of Array.isArray(snapshot.items) ? snapshot.items : []) add(item);

  // Failed/running assistants may only have durable text blocks. Hydrate just
  // missing fields from this already bounded snapshot, without padding offsets
  // or overwriting a completed response/reasoning with an older stream tail.
  const fields = new Map();
  for (const block of Array.isArray(snapshot.blocks) ? snapshot.blocks : []) {
    if (!isRecord(block)) continue;
    const data = isRecord(block.data) ? block.data : block;
    const id = String(block.item_id ?? data.item_id ?? '');
    const entry = items.get(id);
    if (!entry) continue;
    const payload = payloadOf(entry.item);
    for (const field of ['content', 'reasoning']) {
      const present = field === 'content' ? contentToText(payload.content).trim()
        : [payload.reasoning, payload.reasoning_content, payload.thinking, payload.thought,
          payload.reasoningText, payload.thinking_content, metaOf(payload).reasoning_content,
          metaOf(payload).reasoning, metaOf(payload).thinking].some(value => contentToText(value).trim());
      if (present || typeof data[field] !== 'string' || !data[field]) continue;
      const key = `${id}\u0000${field}`;
      let chunks = fields.get(key);
      if (!chunks) {
        chunks = { entry, field, blocks: [] };
        fields.set(key, chunks);
      }
      chunks.blocks.push({ text: data[field], offset: data[`${field}_offset`],
        index: Number(block.block_index ?? data.block_index ?? 0) });
    }
  }
  for (const { entry, field, blocks } of fields.values()) {
    blocks.sort((a, b) => a.index - b.index);
    const parts = [];
    let end = 0;
    for (const block of blocks) {
      const offset = Number.isSafeInteger(block.offset) && block.offset >= 0 ? block.offset : end;
      if (offset > end) parts.push('\n…\n');
      if (offset + block.text.length <= end) continue;
      parts.push(block.text.slice(Math.max(0, end - offset)));
      end = offset + block.text.length;
    }
    const original = entry.turn.items[entry.index];
    entry.turn.items[entry.index] = { ...original, payload: { ...payloadOf(original), [field]: parts.join('') } };
  }
  return [...grouped.values()].sort((a, b) => Number(a.user_turn_index ?? 0) - Number(b.user_turn_index ?? 0));
}
