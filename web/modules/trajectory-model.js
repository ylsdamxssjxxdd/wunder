// 线程轨迹的数据模型（对齐蜂巢 trajectoryModel.ts）：
// 把线程日志快照折叠成 轮次 → 步骤组 → 记录，并给出时间线投影、
// 请求编号、折叠与虚拟行布局。纯函数实现，不持有 DOM。

export const TRAJ_KINDS = ["system", "user", "context", "compacted", "message", "tool", "subtool"];

const REQUEST_SEPARATOR = "\u0000";
const TEXT_PREVIEW_SOURCE_LIMIT = 2048;
const TEXT_PREVIEW_LIMIT = 512;
const USER_TURN_KEYS = ["user_turn_index", "user_round", "turn_index", "turn"];
const STARTED_KEYS = ["started_at", "start_time", "created_at"];
const COMPLETED_KEYS = ["completed_at", "end_time", "finished_at"];
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

const payloadOf = (item) => (isRecord(item.payload) ? item.payload : {});

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

function extractUsage(payload) {
  const usage = {};
  const sources = [];
  const stats = payload.stats;
  if (isRecord(stats)) sources.push(stats);
  const messageStats = metaOf(payload).message_stats;
  if (isRecord(messageStats)) sources.push(messageStats);
  if (isRecord(payload.usage)) sources.push(payload.usage);
  if (sources.length === 0) return undefined;
  const pick = (keys) => {
    for (const source of sources) {
      for (const key of keys) {
        const value = source[key];
        if (typeof value === "number" && Number.isFinite(value)) return value;
      }
    }
    return undefined;
  };
  usage.input = pick(["input_tokens", "prompt_tokens"]);
  usage.cacheRead = pick(["cached_input_tokens", "cache_read_input_tokens", "cached_tokens"]);
  usage.cacheWrite = pick(["cache_creation_input_tokens"]);
  usage.output = pick(["output_tokens", "completion_tokens"]);
  usage.think = pick(["reasoning_tokens"]);
  return Object.values(usage).some((value) => value !== undefined) ? usage : undefined;
}

function resolveKind(rawKind, payload) {
  if (metaOf(payload).type === "system_prompt") return "system";
  if (rawKind === "assistant_message" || rawKind === "assistant") return "message";
  if (rawKind === "subagent" || rawKind === "subagent_message") return "subtool";
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

/* ------------------------------------------------------------------ */
/* 单元格构建                                                          */
/* ------------------------------------------------------------------ */

function buildCell(item, payload, kind, index) {
  const status = typeof item.status === "string" ? item.status : "";
  const recordId = typeof item.item_id === "string" ? item.item_id : String(index);
  const createdMs = firstTime(item, ["created_time"]) ?? firstTime(payload, ["created_time"]);
  const updatedMs = firstTime(item, ["updated_time"]);
  const startedAt = firstTime(payload, STARTED_KEYS) ?? createdMs;
  const completedAt = firstTime(payload, COMPLETED_KEYS) ?? updatedMs;

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
    const thinkingText = contentToText(payload.reasoning);
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
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : prefill !== null && decode !== null
          ? prefill + decode
          : null;
    cell.inputDetail = contentText.trim() ? contentText : undefined;
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
    const errorValue = payload.error ?? payload.is_error;
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
    cell.isError =
      errorValue === true ||
      (errorValue !== undefined && errorValue !== null && errorValue !== false) ||
      status === "failed" ||
      status === "error";
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : null;
  } else if (kind === "compacted") {
    const summaryText = contentToText(payload.summary ?? payload.content);
    const text = summaryText.trim() ? summaryText : "";
    cell.text = text;
    cell.previewMarkdown = text.trim() ? text : undefined;
    cell.inputDetail = text.trim() ? text : undefined;
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : null;
  } else {
    const contentText = contentToText(payload.content);
    cell.text = markdownPreviewText(contentText);
    cell.previewMarkdown = contentText.trim() ? contentText : undefined;
    cell.inputDetail = contentText.trim() ? contentText : undefined;
    cell.timeSeconds =
      startedAt !== null && completedAt !== null
        ? Math.max(0, (completedAt - startedAt) / 1000)
        : null;
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
          if (candidatePayload.error === true) cell.isError = true;
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

function countTools(records) {
  return records.filter((record) => record.cell.kind === "tool" || record.cell.kind === "subtool")
    .length;
}

/** 轮次折叠：每轮只留首条记录并附摘要行。 */
export function collapseTurnRecords(records, collapsedTurns, collapsedAssistants, t) {
  const turnRecords = new Map();
  for (const record of records) {
    const bucket = turnRecords.get(record.turn);
    if (bucket) bucket.push(record);
    else turnRecords.set(record.turn, [record]);
  }

  const output = [];
  let currentTurn;
  let turnCollapsed = false;
  for (const record of records) {
    if (record.turn !== currentTurn) {
      currentTurn = record.turn;
      turnCollapsed = collapsedTurns.has(record.turn);
    }
    if (turnCollapsed) {
      if (!record.turnStart) continue;
      const bucket = turnRecords.get(record.turn) ?? [];
      const steps = new Set(
        bucket.map((candidate) => candidate.group).filter((title) => title.length > 0)
      ).size;
      output.push({
        ...record,
        collapsedSummary: t("summaryTurn", { steps, tools: countTools(bucket) }),
        collapsedSummaryKind: "turn",
      });
      continue;
    }
    const assistantKey = requestIdentity(record.turn, record.group);
    if (collapsedAssistants.has(assistantKey)) {
      const bucket = (turnRecords.get(record.turn) ?? []).filter(
        (candidate) => candidate.group === record.group
      );
      if (bucket[0]?.cell.index !== record.cell.index) continue;
      output.push({
        ...record,
        collapsedSummary: t("summaryAssistantTools", { count: countTools(bucket) }),
        collapsedSummaryKind: "assistant",
      });
      continue;
    }
    output.push(record);
  }
  return output;
}

export const ROW_PX = 30;
export const ROW_SUMMARY_PX = 20;

/** 固定行高的虚拟行布局（一条记录一行）。 */
export function groupVirtualRows(records) {
  const rows = [];
  records.forEach((record, index) => {
    rows.push({
      record,
      height: record.collapsedSummary ? ROW_SUMMARY_PX : ROW_PX,
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
