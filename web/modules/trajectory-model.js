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
    stats.interaction_duration_s ?? stats.interactionDurationS
      ?? stats.duration_s ?? stats.durationSeconds,
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

function buildCell(item, payload, kind, index) {
  payload = eventPayloadOf(payload);
  const status = typeof item.status === "string" ? item.status : "";
  const recordId = typeof item.item_id === "string" ? item.item_id : String(index);
  const createdMs = firstTime(item, ["created_time"]) ?? firstTime(payload, ["created_time"]);
  const updatedMs = firstTime(item, ["updated_time"]);
  let startedAt = firstTime(payload, STARTED_KEYS) ?? createdMs;
  const completedAt = firstTime(payload, COMPLETED_KEYS) ?? updatedMs;
  const recordedDuration = kind === "message" ? assistantRecordedDurationSeconds(payload) : null;
  if (kind === "message" && recordedDuration !== null
    && firstTime(payload, STARTED_KEYS) === null
    && firstTime(payload, COMPLETED_KEYS) === null
    && completedAt !== null) {
    startedAt = completedAt - recordedDuration * 1000;
  }

  const cell = {
    index,
    recordId,
    kind,
    text: "",
    timeSeconds: null,
    startedAt,
  };
  const usage = extractUsage(payload);

  if (kind === "message") {
    const contentText = contentToText(payload.content);
    const meta = isRecord(payload.meta) ? payload.meta : {};
    const thinkingText = [
      payload.reasoning, payload.reasoning_content, payload.thinking, payload.thought,
      payload.reasoningText, payload.thinking_content,
      meta.reasoning_content, meta.reasoning, meta.thinking,
    ].map(contentToText).find((value) => value.trim()) || "";
    const toolCalls = payload.tool_calls;
    if (!contentText.trim() && Array.isArray(toolCalls) && toolCalls.length > 0) {
      cell.text = "";
      cell.toolCallOnly = true;
    } else {
      cell.text = markdownPreviewText(contentText);
      cell.previewMarkdown = contentText.trim() ? contentText : undefined;
    }
    const prefill =
      typeof payload.prefill_duration_s === "number" && Number.isFinite(payload.prefill_duration_s)
        ? payload.prefill_duration_s
        : null;
    const decode =
      typeof payload.decode_duration_s === "number" && Number.isFinite(payload.decode_duration_s)
        ? payload.decode_duration_s
        : null;
    // Durable assistant rows often receive `updated_time` only after the
    // child/tool work and final replay are committed. Using that bookkeeping
    // timestamp makes the purple model span cover the whole child run. Prefer
    // the provider/orchestrator interaction duration when explicit assistant
    // start/end fields are absent; this is the same own-step duration DSH uses.
    cell.timeSeconds = recordedDuration !== null
      && firstTime(payload, STARTED_KEYS) === null
      && firstTime(payload, COMPLETED_KEYS) === null
      ? recordedDuration
      : startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : prefill !== null && decode !== null
          ? prefill + decode
          : null;
    // Assistant rows contain the model response.  The persisted row does not
    // carry the prompt that produced it, so exposing `content` as both input
    // and output makes the overview misleading.  Keep the response in the
    // output section and leave input absent until a real request payload is
    // available.
    cell.inputDetail = undefined;
    cell.outputDetail = contentText.trim() ? contentText : undefined;
    cell.thinkingDetail = thinkingText.trim() ? thinkingText : undefined;
    cell.usage = usage;
    const ttft =
      typeof payload.ttft_ms === "number" && Number.isFinite(payload.ttft_ms) ? payload.ttft_ms : null;
    cell.metrics = {
      timingRecorded: startedAt !== null && completedAt !== null,
      stepStartTime: startedAt,
      firstTokenTime: ttft !== null && startedAt !== null ? startedAt + ttft : null,
      completedTime: completedAt,
      usageProvided: usage !== undefined,
      outputTokens: usage?.output ?? null,
    };
    cell.isError = status === "failed" || status === "error";
  } else if (kind === "tool" || kind === "subtool") {
    const runtime = isRecord(payload.runtime) ? payload.runtime : undefined;
    const runtimeState = runtime && isRecord(runtime.agent_state) ? runtime.agent_state : undefined;
    const isSubagentRun = kind === "subtool" && runtime !== undefined;
    const subagentLabel = isSubagentRun
      ? firstNonEmptyText(
        payload.spawn_label, payload.label, runtime?.spawn_label, runtime?.label,
        runtimeState?.label, "subagent_run",
      )
      : "";
    const subagentTask = isSubagentRun
      ? firstNonEmptyText(
        payload.task, payload.user_message, runtime?.user_message,
        runtime?.task, runtimeState?.user_message,
      )
      : "";
    const subagentSummary = isSubagentRun
      ? firstNonEmptyText(
        runtime?.summary, runtime?.latest_message, runtimeState?.message,
        runtimeState?.assistant_message, payload.summary, payload.message,
      )
      : "";
    if (isSubagentRun) {
      // DSH keeps delegated work as a distinct subtool span. Preserve the
      // child label, task and final report instead of rendering an empty
      // `subagent_run` placeholder beside the parent tool call.
      cell.text = subagentLabel;
      cell.toolName = subagentLabel;
      cell.inputDetail = subagentTask || undefined;
      cell.outputDetail = subagentSummary || undefined;
      cell.result = subagentSummary || undefined;
      if (subagentSummary) cell.resultPreviewMarkdown = markdownPreviewText(subagentSummary);
      const runtimeStart = firstTime(runtime ?? {}, STARTED_KEYS);
      const runtimeEnd = firstTime(runtime ?? {}, COMPLETED_KEYS);
      if (runtimeStart !== null) cell.startedAt = runtimeStart;
      if (runtimeStart !== null && runtimeEnd !== null) {
        cell.timeSeconds = Math.max(0, (runtimeEnd - runtimeStart) / 1000);
      }
      cell.isError = runtime?.failed === true || runtime?.status === "failed" || status === "failed";
      return cell;
    }
    const name =
      typeof payload.tool === "string" && payload.tool.trim()
        ? payload.tool
        : typeof payload.name === "string" && payload.name.trim()
          ? payload.name
          : "";
    const args = payload.args;
    const argsRaw =
      typeof args === "string"
        ? args
        : isRecord(args) || Array.isArray(args)
          ? JSON.stringify(args, null, 2)
          : "";
    const resultRaw = summarizeToolResult(payload);
    cell.text = name;
    cell.toolName = name || undefined;
    cell.callId = typeof payload.tool_call_id === "string" ? payload.tool_call_id : undefined;
    cell.inputDetail = argsRaw.trim() ? argsRaw : undefined;
    cell.outputDetail = resultRaw.trim() ? resultRaw : undefined;
    cell.result = resultRaw.trim() ? resultRaw : undefined;
    cell.resultPreviewMarkdown = resultRaw.trim() ? markdownPreviewText(resultRaw) : undefined;
    const schema = payload.schema ?? payload.tool_schema;
    if (typeof schema === "string" && schema.trim()) cell.schemaDetail = schema;
    else if (isRecord(schema)) cell.schemaDetail = JSON.stringify(schema, null, 2);
    cell.isError = isToolFailure(payload, status);
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : null;
  } else if (kind === "compacted") {
    const summaryText = contentToText(
      payload.summary_text ?? payload.summaryText ?? payload.summary_model_output ??
      payload.summaryModelOutput ?? payload.summary ??
      (payload.content === "history" ? "" : payload.content)
    );
    const text = summaryText.trim() ? summaryText : "";
    cell.text = text;
    cell.previewMarkdown = text.trim() ? text : undefined;
    cell.outputDetail = text.trim() ? text : undefined;
    cell.thinkingDetail = contentToText(
      payload.raw_output ?? payload.rawOutput ?? payload.summary_model_output ?? payload.summaryModelOutput
    ).trim() || undefined;
    // A compaction row is a workflow result; its summary is output, not a
    // second copy of an input prompt.
    cell.inputDetail = undefined;
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : null;
  } else {
    let contentText = contentToText(payload.content);
    if (!contentText.trim()) {
      for (const key of ["summary", "message", "reason", "stage", "description", "code"]) {
        const value = payload[key];
        if (typeof value === "string" && value.trim()) {
          contentText = value;
          break;
        }
      }
    }
    cell.text = markdownPreviewText(contentText);
    cell.previewMarkdown = contentText.trim() ? contentText : undefined;
    cell.inputDetail = contentText.trim() ? contentText : undefined;
    // User/context/system events are instantaneous input markers. Falling back
    // to updated_time here stretches the input lane across the whole turn.
    cell.timeSeconds = startedAt !== null ? 0 : null;
  }

  return cell;
}

/* ------------------------------------------------------------------ */
/* 布局折叠                                                            */
/* ------------------------------------------------------------------ */

/** 把快照的轮次列表折叠成轨迹布局。 */
export function buildTrajectoryLayout(rawTurns, t) {
  const turns = [];
  let index = 0;

  for (const rawTurn of Array.isArray(rawTurns) ? rawTurns : []) {
    if (!isRecord(rawTurn)) continue;
    let turnNo = null;
    for (const key of USER_TURN_KEYS) {
      const value = rawTurn[key];
      if (typeof value === "number" && Number.isFinite(value)) {
        turnNo = value;
        break;
      }
    }
    const items = Array.isArray(rawTurn.items) ? rawTurn.items : [];

    const model = { turn: turnNo, groups: [] };
    turns.push(model);

    let compactionSeq = 0;
    const messageGroup = () => {
      const last = model.groups[model.groups.length - 1];
      if (last && !last.compaction && last.step === undefined) return last;
      const group = { title: t("messages"), cells: [] };
      model.groups.push(group);
      return group;
    };
    const stepGroup = (step) => {
      const existing = model.groups.find((group) => group.step === step);
      if (existing) return existing;
      const group = { title: t("stepLabel", { step }), cells: [], step };
      model.groups.push(group);
      return group;
    };
    const pushCompaction = (cell) => {
      compactionSeq += 1;
      model.groups.push({
        title: t("compaction", { seq: compactionSeq }),
        cells: [cell],
        compaction: true,
      });
    };

    const ordered = [...items]
      .filter(isRecord)
      .sort(
        (left, right) =>
          Number(left.created_seq ?? 0) - Number(right.created_seq ?? 0) ||
          Number(left.item_index ?? 0) - Number(right.item_index ?? 0)
      );

    for (let i = 0; i < ordered.length; i += 1) {
      const item = ordered[i];
      const payload = payloadOf(item);
      const rawKind = rawKindOf(item, payload);
      // Compatibility rows for manual compaction duplicate the real
      // `compaction` workflow item. They carry the summary as assistant/system
      // content and must never appear as an ordinary transcript row.
      if ((rawKind === "assistant_message" || rawKind === "system_message" || rawKind === "system") &&
          isCompactionMarkerPayload(payload)) continue;
      // Transport and lifecycle records are not user-visible trajectory spans.
      // They are already represented by the model/tool records in the same
      // round; retaining them makes the bridge timeline longer than the hive.
      if (
        rawKind === "progress" || rawKind === "workspace_update"
        || rawKind === "model_usage" || rawKind === "llm_stream_retry"
        || rawKind === "thread_turn_started" || rawKind === "model_call"
      ) continue;
      if (
        rawKind === "queue" || rawKind.startsWith("queue_")
        || rawKind === "tool_message" || rawKind.startsWith("command_session")
        || rawKind === "knowledge_request" || rawKind.startsWith("hook/")
      ) continue;
      if ((rawKind === "terminal" || rawKind === "turn_terminal")
        && !payload.error && item.status === "completed") continue;
      const kind = resolveKind(rawKind, payload);
      const cell = buildCell(item, payload, kind, index);
      index += 1;

      // tool_call 与紧随其后的 tool_result 合并成一条记录。
      if (kind === "tool" && rawKind === "tool_call") {
        const callId = payload.tool_call_id;
        for (let j = i + 1; j < ordered.length; j += 1) {
          const candidate = ordered[j];
          const candidatePayload = payloadOf(candidate);
          if (rawKindOf(candidate, candidatePayload) !== "tool_result") continue;
          if (callId !== undefined && candidatePayload.tool_call_id !== callId) continue;
          const resultRaw = summarizeToolResult(candidatePayload);
          if (resultRaw.trim()) {
            cell.result = resultRaw;
            cell.outputDetail = resultRaw;
            cell.resultPreviewMarkdown = markdownPreviewText(resultRaw);
          }
          if (isToolFailure(candidatePayload, candidate.status)) cell.isError = true;
          const candidateStart =
            firstTime(candidatePayload, STARTED_KEYS) ?? firstTime(candidate, ["created_time"]);
          const candidateEnd =
            firstTime(candidatePayload, COMPLETED_KEYS) ?? firstTime(candidate, ["updated_time"]);
          if (candidateStart !== null && candidateEnd !== null) {
            cell.timeSeconds = Math.max(0, (candidateEnd - candidateStart) / 1000);
          }
          ordered.splice(j, 1);
          break;
        }
      }

      if (kind === "message") {
        const step = payload.model_round;
        if (typeof step === "number" && Number.isFinite(step)) {
          stepGroup(step).cells.push(cell);
        } else {
          messageGroup().cells.push(cell);
        }
      } else if (kind === "compacted") {
        pushCompaction(cell);
      } else if (kind === "tool" || kind === "subtool") {
        const step = payload.model_round;
        if (typeof step === "number" && Number.isFinite(step)) {
          stepGroup(step).cells.push(cell);
        } else {
          const last = model.groups[model.groups.length - 1];
          if (last && last.step !== undefined && !last.compaction) {
            last.cells.push(cell);
          } else {
            stepGroup(1).cells.push(cell);
          }
        }
      } else {
        messageGroup().cells.push(cell);
      }
    }
  }

  return turns;
}

/* ------------------------------------------------------------------ */
/* 请求编号                                                            */
/* ------------------------------------------------------------------ */

export function requestIdentity(turn, groupTitle) {
  return `${turn ?? "none"}${REQUEST_SEPARATOR}${groupTitle}`;
}

function addUsage(target, usage) {
  if (!usage) return;
  for (const key of ["input", "cacheRead", "cacheWrite", "output", "think"]) {
    const value = usage[key];
    if (typeof value === "number" && Number.isFinite(value)) {
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
      if (group.compaction || group.step === undefined) continue;
      const messageCell = group.cells.find((cell) => cell.kind === "message");
      if (!messageCell) continue;
      addUsage(cumulative, messageCell.usage);
      const status = messageCell.isError
        ? "error"
        : messageCell.metrics?.completedTime == null
          ? "running"
          : "complete";
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
      });
    }
  }
  return requests;
}

/* ------------------------------------------------------------------ */
/* 时间线投影                                                          */
/* ------------------------------------------------------------------ */

function laneFor(kind) {
  if (kind === "tool" || kind === "subtool") return 2;
  if (kind === "message" || kind === "compacted") return 1;
  return 0;
}

function finite(value) {
  return value !== null && value !== undefined && Number.isFinite(value);
}

function cellRange(cell) {
  if (!finite(cell.startedAt)) return null;
  const durationMs = finite(cell.timeSeconds) ? Math.max(0, cell.timeSeconds * 1000) : 0;
  return { start: cell.startedAt, end: cell.startedAt + durationMs };
}

function ttftFractionFor(cell) {
  const metrics = cell.metrics;
  if (!metrics?.timingRecorded) return null;
  const { stepStartTime, firstTokenTime, completedTime } = metrics;
  if (!finite(stepStartTime) || !finite(firstTokenTime) || !finite(completedTime)) return null;
  const total = completedTime - stepStartTime;
  if (total <= 0) return null;
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
export function deriveTimeline(turns, mode = "sequence") {
  if (mode !== "sequence") {
    return deriveTimedTimeline(turns, mode === "duration" || mode === "actual", mode === "duration");
  }
  const spans = [];
  const turnBoundaries = [];
  for (const turn of turns) {
    const cells = turn.groups.flatMap((group) => group.cells);
    if (cells.length === 0) continue;
    if (turn.turn !== null) {
      turnBoundaries.push({ turn: turn.turn, time: spans.length });
    }
    spans.push(
      ...cells.map((cell, offset) => spanFor(cell, spans.length + offset, spans.length + offset + 1))
    );
  }
  if (spans.length === 0) return null;
  return { start: 0, end: spans.length, spans, turnBoundaries };
}

function deriveTimedTimeline(turns, actualDuration, compressIdle) {
  const timedTurns = turns.flatMap((turn) => {
    const rawSpans = turn.groups.flatMap((group) =>
      group.cells.flatMap((cell) => {
        const range = cellRange(cell);
        if (range === null) return [];
        return [{ ...spanFor(cell, range.start, range.end) }];
      })
    );
    return rawSpans.length === 0 ? [] : [{ turn: turn.turn, rawSpans }];
  });
  const rawSpans = timedTurns.flatMap((turn) => turn.rawSpans);
  if (rawSpans.length === 0) return null;

  const removedIdleBySpan = new Map();
  let removedIdle = 0;
  let coveredUntil = null;
  for (const span of [...rawSpans].sort(
    (left, right) => left.start - right.start || left.end - right.end
  )) {
    if (compressIdle && coveredUntil !== null && span.start > coveredUntil) {
      removedIdle += span.start - coveredUntil;
    }
    removedIdleBySpan.set(span, removedIdle);
    coveredUntil = coveredUntil === null ? span.end : Math.max(coveredUntil, span.end);
  }

  const spans = [];
  const turnBoundaries = [];
  for (const turn of timedTurns) {
    const projected = turn.rawSpans.map((span) => {
      const offset = removedIdleBySpan.get(span) ?? 0;
      return {
        ...span,
        start: span.start - offset,
        end: (actualDuration ? span.end : span.start) - offset,
      };
    });
    spans.push(...projected);
    if (turn.turn !== null) {
      turnBoundaries.push({
        turn: turn.turn,
        time: Math.min(...projected.map((span) => span.start)),
      });
    }
  }

  return {
    start: Math.min(...spans.map((span) => span.start)),
    end: Math.max(...spans.map((span) => span.end)),
    spans,
    turnBoundaries,
  };
}

/** 选区覆盖的记录索引集合。 */
export function timelineFocusIndexes(turns, range, mode = "sequence") {
  const model = deriveTimeline(turns, mode);
  return new Set(
    model?.spans
      .filter((span) => span.start <= range.end && span.end >= range.start)
      .map((span) => span.index) ?? []
  );
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
      if (records.length > groupStart) records[groupStart].groupStart = true;
    }
    // Group identities annotate rows; they must not reorder durable events.
    const ordered = records.slice(turnStart).sort((left, right) => left.cell.index - right.cell.index);
    for (let index = 0; index < ordered.length; index += 1) records[turnStart + index] = ordered[index];
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
    record.cell.toolName ?? "",
    record.cell.inputDetail ?? "",
    record.cell.outputDetail ?? "",
    record.cell.result ?? "",
  ]
    .join("\n")
    .toLowerCase();
}

/** 搜索过滤：文本不匹配的记录整行隐藏。 */
export function filterRecords(records, query) {
  const needle = String(query || "").trim().toLowerCase();
  if (!needle) return records;
  return records.filter((record) => recordNeedle(record).includes(needle));
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

export const ROW_PX = 30;
export const ROW_SUMMARY_PX = 20;

/** 固定行高的虚拟行布局（一条记录一行）。 */
export function groupVirtualRows(records, rowHeight = ROW_PX, summaryHeight = ROW_SUMMARY_PX) {
  const rows = [];
  records.forEach((record, index) => {
    rows.push({
      record,
      height: record.collapsedSummary ? summaryHeight : rowHeight,
      key: `${record.turn ?? "none"}-${record.group}-${record.cell.recordId}-${index}`,
    });
  });
  return { rows };
}

/** 台账记录的稳定标识。 */
export function trajectoryRecordId(cell) {
  return cell.recordId || String(cell.index);
}

/**
 * 快照的 items 与 turns 是两个平铺数组；把 items 按 turn_id 归组挂到各自的
 * 轮次上，布局模型才能读到每轮的记录。没有 turn_id 的条目挂在首个轮次前。
 */
export function attachTurnItems(payload) {
  const source = payload && typeof payload === "object" ? payload : null;
  if (!Array.isArray(source?.turns)) return [];
  const turns = source.turns.filter((turn) => !!turn && typeof turn === "object");
  const items = Array.isArray(source?.items) ? source.items : [];
  if (items.length === 0) return turns;
  const buckets = new Map();
  for (const item of items) {
    const turnId = typeof item?.turn_id === "string" ? item.turn_id : "";
    if (!turnId) continue;
    const bucket = buckets.get(turnId);
    if (bucket) bucket.push(item);
    else buckets.set(turnId, [item]);
  }
  for (const turn of turns) {
    const turnId = typeof turn.turn_id === "string" ? turn.turn_id : "";
    turn.items = turnId ? buckets.get(turnId) ?? [] : [];
  }
  return turns;
}
