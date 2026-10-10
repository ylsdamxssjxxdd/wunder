/**
 * 线程轨迹的数据模型：把线程日志快照折叠成 轮次 → 步骤组 → 记录，
 * 并给出时间线投影、请求编号、折叠与虚拟行布局。
 */

export type TrajKind = 'system' | 'user' | 'context' | 'compacted' | 'message' | 'tool' | 'subtool'

export interface TrajUsage {
  input?: number
  cacheRead?: number
  cacheWrite?: number
  output?: number
  think?: number
}

export interface TrajMetrics {
  timingRecorded: boolean
  stepStartTime: number | null
  firstTokenTime: number | null
  completedTime: number | null
  usageProvided: boolean
  outputTokens: number | null
}

export interface TrajCell {
  index: number
  recordId: string
  kind: TrajKind
  text: string
  previewMarkdown?: string
  inputDetail?: string
  outputDetail?: string
  thinkingDetail?: string
  schemaDetail?: string
  result?: string
  resultPreviewMarkdown?: string
  callId?: string
  toolName?: string
  isError?: boolean
  toolCallOnly?: boolean
  timeSeconds: number | null
  startedAt: number | null
  metrics?: TrajMetrics
  usage?: TrajUsage
}

export interface TrajGroupModel {
  title: string
  cells: TrajCell[]
  step?: number
  compaction?: boolean
  requestNumber?: number
}

export interface TrajTurnModel {
  turn: number | null
  groups: TrajGroupModel[]
}

export interface TrajRequestNumber {
  identity: string
  number: number
  turn: number | null
  step: number
  groupTitle: string
  status: 'complete' | 'running' | 'error'
  startedAt: number | null
  completedAt: number | null
  usage?: TrajUsage
  cumulativeUsage: TrajUsage
  assistantIndex: number
}

export type TimelineMode = 'sequence' | 'duration' | 'time' | 'actual'

export interface TimelineSpan {
  start: number
  end: number
  index: number
  isError: boolean
  kind: TrajKind
  label: string
  lane: number
  ttftFraction: number | null
}

export interface TimelineTurnBoundary {
  turn: number
  time: number
}

export interface TimelineModel {
  start: number
  end: number
  spans: TimelineSpan[]
  turnBoundaries: TimelineTurnBoundary[]
}

export interface TableRecord {
  turn: number | null
  section: number
  group: string
  groupStart: boolean
  turnStart: boolean
  turnEnd: boolean
  cell: TrajCell
  collapsedSummary?: string
  collapsedSummaryKind?: 'turn' | 'assistant'
}

export interface VirtualRow {
  record: TableRecord
  height: number
  key: string
}

export type Translate = (key: string, params?: Record<string, string | number>) => string

/* ------------------------------------------------------------------ */
/* 常量                                                                */
/* ------------------------------------------------------------------ */

const REQUEST_SEPARATOR = '\u0000'
const TEXT_PREVIEW_SOURCE_LIMIT = 2048
const TEXT_PREVIEW_LIMIT = 512
const USER_TURN_KEYS = ['user_turn_index', 'user_round', 'turn_index', 'turn'] as const
const STARTED_KEYS = ['started_at', 'start_time', 'created_at'] as const
const COMPLETED_KEYS = ['completed_at', 'end_time', 'finished_at'] as const
const CONTEXT_RAW_KINDS = new Set([
  'queue', 'plan', 'approval', 'terminal', 'context', 'note', 'status', 'context_message',
])

/* ------------------------------------------------------------------ */
/* 格式化                                                              */
/* ------------------------------------------------------------------ */

/** 整数毫秒 + 千分位，如「11,049 毫秒」。 */
export function formatDurationMillis(milliseconds: number | null, t: Translate): string {
  if (milliseconds === null || !Number.isFinite(milliseconds)) return t('trajectory.unavailable')
  return t('trajectory.durationMillis', { value: Math.round(milliseconds).toLocaleString('en-US') })
}

export function formatElapsedSeconds(seconds: number | null, t: Translate): string {
  if (seconds === null || !Number.isFinite(seconds)) return t('trajectory.unavailable')
  return formatDurationMillis(seconds * 1000, t)
}

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

function pad3(value: number): string {
  return String(value).padStart(3, '0')
}

/** YYYY-MM-DD HH:mm:ss.SSS（本地时区）。 */
export function formatStartedAt(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return ''
  const date = new Date(ms)
  if (Number.isNaN(date.getTime())) return ''
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} `
    + `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}.`
    + `${pad3(date.getMilliseconds())}`
}

/** HH:mm:ss.SSS（本地时区）。 */
export function formatClock(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return ''
  const date = new Date(ms)
  if (Number.isNaN(date.getTime())) return ''
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}.${pad3(date.getMilliseconds())}`
}

/** 有界的纯文本预览：去掉 markdown 标记并截断。 */
export function markdownPreviewText(text: string): string {
  const bounded = text.length > TEXT_PREVIEW_SOURCE_LIMIT
    ? `${text.slice(0, TEXT_PREVIEW_SOURCE_LIMIT)}…`
    : text
  const plain = bounded
    .replace(/```[^\n]*\n?([\s\S]*?)```/g, (match: string): string => {
      const body = match.replace(/^```[^\n]*\n?/, '').replace(/```\s*$/, '')
      return body.trim()
    })
    .replace(/`([^`\n]+)`/g, '$1')
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]+)\]\(([^)]*)\)/g, '$1')
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/^\s{0,3}>\s?/gm, '')
    .replace(/^(\s*)[-*+]\s+(?:\[[ xX]\]\s+)?/gm, '$1')
    .replace(/^\s*\d+\.\s+/gm, '')
    .replace(/(\*\*\*|\*\*|__|~~)/g, '')
    .replace(/<[^>]+>/g, '')
  return plain.length > TEXT_PREVIEW_LIMIT ? `${plain.slice(0, TEXT_PREVIEW_LIMIT)}…` : plain
}

/* ------------------------------------------------------------------ */
/* 快照字段读取                                                        */
/* ------------------------------------------------------------------ */

type RawRecord = Record<string, unknown>

function isRecord(value: unknown): value is RawRecord {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function payloadOf(item: RawRecord): RawRecord {
  return isRecord(item.payload) ? item.payload : {}
}

function metaOf(payload: RawRecord): RawRecord {
  return isRecord(payload.meta) ? payload.meta : {}
}

function rawKindOf(item: RawRecord, payload: RawRecord): string {
  if (typeof item.kind === 'string' && item.kind) return item.kind
  const eventType = payload.event_type
  if (typeof eventType === 'string' && eventType) return eventType
  const metaType = metaOf(payload).type
  return typeof metaType === 'string' ? metaType : ''
}

/** 时间字段统一为毫秒时间戳；秒级数值按 1e11 阈值启发式放大。 */
function timestampMs(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.abs(value) < 1e11 ? value * 1000 : value
  }
  if (typeof value === 'string' && value) {
    const parsed = Date.parse(value)
    if (!Number.isNaN(parsed)) return parsed
  }
  return null
}

function firstTime(source: RawRecord, keys: readonly string[]): number | null {
  for (const key of keys) {
    const ms = timestampMs(source[key])
    if (ms !== null) return ms
  }
  return null
}

function contentToText(value: unknown): string {
  if (typeof value === 'string') return value
  if (Array.isArray(value)) {
    return value.map((block): string => {
      if (typeof block === 'string') return block
      if (isRecord(block)) {
        const text = block.text ?? block.content ?? block.thinking
        return typeof text === 'string' ? text : ''
      }
      return ''
    }).filter(part => part.length > 0).join('\n')
  }
  if (isRecord(value)) {
    const text = value.text ?? value.content
    if (typeof text === 'string') return text
    return JSON.stringify(value, null, 2)
  }
  return value === null || value === undefined ? '' : String(value)
}

function summarizeToolResult(payload: RawRecord): string {
  const data = payload.data
  if (typeof data === 'string') return data
  if (isRecord(data)) {
    const nested = data.result ?? data.output ?? data.content ?? data.text
    if (typeof nested === 'string') return nested
    if (typeof data.summary === 'string') return data.summary
  }
  const fallback = payload.result ?? payload.output ?? payload.content
  return contentToText(fallback)
}

function extractUsage(payload: RawRecord): TrajUsage | undefined {
  const usage: TrajUsage = {}
  const sources: RawRecord[] = []
  const stats = payload.stats
  if (isRecord(stats)) sources.push(stats)
  const messageStats = metaOf(payload).message_stats
  if (isRecord(messageStats)) sources.push(messageStats)
  if (isRecord(payload.usage)) sources.push(payload.usage)
  if (sources.length === 0) return undefined
  const pick = (keys: string[]): number | undefined => {
    for (const source of sources) {
      for (const key of keys) {
        const value = source[key]
        if (typeof value === 'number' && Number.isFinite(value)) return value
      }
    }
    return undefined
  }
  usage.input = pick(['input_tokens', 'prompt_tokens'])
  usage.cacheRead = pick(['cached_input_tokens', 'cache_read_input_tokens', 'cached_tokens'])
  usage.cacheWrite = pick(['cache_creation_input_tokens'])
  usage.output = pick(['output_tokens', 'completion_tokens'])
  usage.think = pick(['reasoning_tokens'])
  return Object.values(usage).some(value => value !== undefined) ? usage : undefined
}

function resolveKind(rawKind: string, payload: RawRecord): TrajKind {
  if (metaOf(payload).type === 'system_prompt') return 'system'
  if (rawKind === 'assistant_message' || rawKind === 'assistant') return 'message'
  if (rawKind === 'subagent' || rawKind === 'subagent_message') return 'subtool'
  if (rawKind === 'tool_message' || rawKind === 'tool_call'
    || rawKind === 'tool_result' || rawKind === 'tool') return 'tool'
  if (rawKind === 'compaction' || rawKind === 'compacted' || rawKind === 'compaction_summary') {
    return 'compacted'
  }
  if (rawKind === 'user_message' || rawKind === 'user') return 'user'
  if (rawKind === 'system_message' || rawKind === 'system') return 'system'
  if (CONTEXT_RAW_KINDS.has(rawKind)) return 'context'
  return 'context'
}

/* ------------------------------------------------------------------ */
/* 单元格构建                                                          */
/* ------------------------------------------------------------------ */

function buildCell(item: RawRecord, payload: RawRecord, kind: TrajKind, index: number): TrajCell {
  const status = typeof item.status === 'string' ? item.status : ''
  const recordId = typeof item.item_id === 'string' ? item.item_id : String(index)
  const createdMs = firstTime(item, ['created_time']) ?? firstTime(payload, ['created_time'])
  const updatedMs = firstTime(item, ['updated_time'])
  const startedAt = firstTime(payload, STARTED_KEYS) ?? createdMs
  const completedAt = firstTime(payload, COMPLETED_KEYS) ?? updatedMs

  const cell: TrajCell = {
    index,
    recordId,
    kind,
    text: '',
    timeSeconds: null,
    startedAt,
  }
  const usage = extractUsage(payload)

  if (kind === 'message') {
    const contentText = contentToText(payload.content)
    const thinkingText = contentToText(payload.reasoning)
    const toolCalls = payload.tool_calls
    if (!contentText.trim() && Array.isArray(toolCalls) && toolCalls.length > 0) {
      cell.text = ''
      cell.toolCallOnly = true
    } else {
      cell.text = markdownPreviewText(contentText)
      cell.previewMarkdown = contentText.trim() ? contentText : undefined
    }
    const prefill = typeof payload.prefill_duration_s === 'number'
      && Number.isFinite(payload.prefill_duration_s) ? payload.prefill_duration_s : null
    const decode = typeof payload.decode_duration_s === 'number'
      && Number.isFinite(payload.decode_duration_s) ? payload.decode_duration_s : null
    cell.timeSeconds = startedAt !== null && completedAt !== null
      ? Math.max(0, (completedAt - startedAt) / 1000)
      : prefill !== null && decode !== null ? prefill + decode : null
    cell.inputDetail = contentText.trim() ? contentText : undefined
    cell.outputDetail = contentText.trim() ? contentText : undefined
    cell.thinkingDetail = thinkingText.trim() ? thinkingText : undefined
    cell.usage = usage
    const ttft = typeof payload.ttft_ms === 'number' && Number.isFinite(payload.ttft_ms)
      ? payload.ttft_ms
      : null
    cell.metrics = {
      timingRecorded: startedAt !== null && completedAt !== null,
      stepStartTime: startedAt,
      firstTokenTime: ttft !== null && startedAt !== null ? startedAt + ttft : null,
      completedTime: completedAt,
      usageProvided: usage !== undefined,
      outputTokens: usage?.output ?? null,
    }
    cell.isError = status === 'failed' || status === 'error'
  } else if (kind === 'tool' || kind === 'subtool') {
    const name = typeof payload.tool === 'string' && payload.tool.trim()
      ? payload.tool
      : typeof payload.name === 'string' && payload.name.trim() ? payload.name : ''
    const args = payload.args
    const argsRaw = typeof args === 'string' ? args : isRecord(args) || Array.isArray(args)
      ? JSON.stringify(args, null, 2)
      : ''
    const resultRaw = summarizeToolResult(payload)
    const errorValue = payload.error ?? payload.is_error
    cell.text = name
    cell.toolName = name || undefined
    cell.callId = typeof payload.tool_call_id === 'string' ? payload.tool_call_id : undefined
    cell.inputDetail = argsRaw.trim() ? argsRaw : undefined
    cell.outputDetail = resultRaw.trim() ? resultRaw : undefined
    cell.result = resultRaw.trim() ? resultRaw : undefined
    cell.resultPreviewMarkdown = resultRaw.trim() ? markdownPreviewText(resultRaw) : undefined
    const schema = payload.schema ?? payload.tool_schema
    if (typeof schema === 'string' && schema.trim()) cell.schemaDetail = schema
    else if (isRecord(schema)) cell.schemaDetail = JSON.stringify(schema, null, 2)
    cell.isError = errorValue === true
      || (errorValue !== undefined && errorValue !== null && errorValue !== false)
      || status === 'failed' || status === 'error'
    cell.timeSeconds = startedAt !== null && completedAt !== null
      ? Math.max(0, (completedAt - startedAt) / 1000)
      : null
  } else if (kind === 'compacted') {
    const summaryText = contentToText(payload.summary ?? payload.content)
    const text = summaryText.trim() ? summaryText : ''
    cell.text = text
    cell.previewMarkdown = text.trim() ? text : undefined
    cell.inputDetail = text.trim() ? text : undefined
    cell.timeSeconds = startedAt !== null && completedAt !== null
      ? Math.max(0, (completedAt - startedAt) / 1000)
      : null
  } else {
    const contentText = contentToText(payload.content)
    cell.text = markdownPreviewText(contentText)
    cell.previewMarkdown = contentText.trim() ? contentText : undefined
    cell.inputDetail = contentText.trim() ? contentText : undefined
    cell.timeSeconds = startedAt !== null && completedAt !== null
      ? Math.max(0, (completedAt - startedAt) / 1000)
      : null
  }

  return cell
}

/* ------------------------------------------------------------------ */
/* 布局折叠                                                            */
/* ------------------------------------------------------------------ */

/** 把快照的轮次列表折叠成轨迹布局。 */
export function buildTrajectoryLayout(rawTurns: unknown, t: Translate): TrajTurnModel[] {
  const turns: TrajTurnModel[] = []
  let index = 0

  for (const rawTurn of Array.isArray(rawTurns) ? rawTurns : []) {
    if (!isRecord(rawTurn)) continue
    let turnNo: number | null = null
    for (const key of USER_TURN_KEYS) {
      const value = rawTurn[key]
      if (typeof value === 'number' && Number.isFinite(value)) {
        turnNo = value
        break
      }
    }
    const items = Array.isArray(rawTurn.items) ? rawTurn.items : []

    const model: TrajTurnModel = { turn: turnNo, groups: [] }
    turns.push(model)

    let compactionSeq = 0
    const messageGroup = (): TrajGroupModel => {
      const last = model.groups[model.groups.length - 1]
      if (last && !last.compaction && last.step === undefined) return last
      const group: TrajGroupModel = { title: t('trajectory.messages'), cells: [] }
      model.groups.push(group)
      return group
    }
    const stepGroup = (step: number): TrajGroupModel => {
      const existing = model.groups.find(group => group.step === step)
      if (existing) return existing
      const group: TrajGroupModel = { title: t('trajectory.stepLabel', { step }), cells: [], step }
      model.groups.push(group)
      return group
    }
    const pushCompaction = (cell: TrajCell): void => {
      compactionSeq += 1
      model.groups.push({
        title: t('trajectory.compaction', { seq: compactionSeq }),
        cells: [cell],
        compaction: true,
      })
    }

    const ordered = [...items].filter(isRecord).sort((left, right) =>
      Number(left.created_seq ?? 0) - Number(right.created_seq ?? 0)
      || Number(left.item_index ?? 0) - Number(right.item_index ?? 0))

    for (let i = 0; i < ordered.length; i += 1) {
      const item = ordered[i]
      const payload = payloadOf(item)
      const rawKind = rawKindOf(item, payload)
      const kind = resolveKind(rawKind, payload)
      const cell = buildCell(item, payload, kind, index)
      index += 1

      // tool_call 与紧随其后的 tool_result 合并成一条记录。
      if (kind === 'tool' && rawKind === 'tool_call') {
        const callId = payload.tool_call_id
        for (let j = i + 1; j < ordered.length; j += 1) {
          const candidate = ordered[j]
          const candidatePayload = payloadOf(candidate)
          if (rawKindOf(candidate, candidatePayload) !== 'tool_result') continue
          if (callId !== undefined && candidatePayload.tool_call_id !== callId) continue
          const resultRaw = summarizeToolResult(candidatePayload)
          if (resultRaw.trim()) {
            cell.result = resultRaw
            cell.outputDetail = resultRaw
            cell.resultPreviewMarkdown = markdownPreviewText(resultRaw)
          }
          if (candidatePayload.error === true) cell.isError = true
          const candidateStart = firstTime(candidatePayload, STARTED_KEYS)
            ?? firstTime(candidate, ['created_time'])
          const candidateEnd = firstTime(candidatePayload, COMPLETED_KEYS)
            ?? firstTime(candidate, ['updated_time'])
          if (candidateStart !== null && candidateEnd !== null) {
            cell.timeSeconds = Math.max(0, (candidateEnd - candidateStart) / 1000)
          }
          ordered.splice(j, 1)
          break
        }
      }

      if (kind === 'message') {
        const step = payload.model_round
        if (typeof step === 'number' && Number.isFinite(step)) {
          stepGroup(step).cells.push(cell)
        } else {
          messageGroup().cells.push(cell)
        }
      } else if (kind === 'compacted') {
        pushCompaction(cell)
      } else if (kind === 'tool' || kind === 'subtool') {
        const step = payload.model_round
        if (typeof step === 'number' && Number.isFinite(step)) {
          stepGroup(step).cells.push(cell)
        } else {
          const last = model.groups[model.groups.length - 1]
          if (last && last.step !== undefined && !last.compaction) {
            last.cells.push(cell)
          } else {
            stepGroup(1).cells.push(cell)
          }
        }
      } else {
        messageGroup().cells.push(cell)
      }
    }
  }

  return turns
}

/* ------------------------------------------------------------------ */
/* 请求编号                                                            */
/* ------------------------------------------------------------------ */

export function requestIdentity(turn: number | null, groupTitle: string): string {
  return `${turn ?? 'none'}${REQUEST_SEPARATOR}${groupTitle}`
}

function addUsage(target: TrajUsage, usage: TrajUsage | undefined): void {
  if (!usage) return
  for (const key of ['input', 'cacheRead', 'cacheWrite', 'output', 'think'] as const) {
    const value = usage[key]
    if (typeof value === 'number' && Number.isFinite(value)) {
      target[key] = (target[key] ?? 0) + value
    }
  }
}

/** 按会话顺序给「第 N 步」组编号并累计用量。 */
export function buildRequestNumbers(turns: readonly TrajTurnModel[]): TrajRequestNumber[] {
  const requests: TrajRequestNumber[] = []
  const cumulative: TrajUsage = {}
  for (const turn of turns) {
    for (const group of turn.groups) {
      if (group.compaction || group.step === undefined) continue
      const messageCell = group.cells.find(cell => cell.kind === 'message')
      if (!messageCell) continue
      addUsage(cumulative, messageCell.usage)
      const status: TrajRequestNumber['status'] = messageCell.isError
        ? 'error'
        : messageCell.metrics?.completedTime == null ? 'running' : 'complete'
      group.requestNumber = requests.length + 1
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
      })
    }
  }
  return requests
}

/* ------------------------------------------------------------------ */
/* 时间线投影                                                          */
/* ------------------------------------------------------------------ */

function laneFor(kind: TrajKind): number {
  if (kind === 'tool' || kind === 'subtool') return 2
  if (kind === 'message' || kind === 'compacted') return 1
  return 0
}

function finite(value: number | null | undefined): value is number {
  return value !== null && value !== undefined && Number.isFinite(value)
}

function cellRange(cell: TrajCell): { start: number; end: number } | null {
  if (!finite(cell.startedAt)) return null
  const durationMs = finite(cell.timeSeconds) ? Math.max(0, cell.timeSeconds * 1000) : 0
  return { start: cell.startedAt, end: cell.startedAt + durationMs }
}

function ttftFractionFor(cell: TrajCell): number | null {
  const metrics = cell.metrics
  if (!metrics?.timingRecorded) return null
  const { stepStartTime, firstTokenTime, completedTime } = metrics
  if (!finite(stepStartTime) || !finite(firstTokenTime) || !finite(completedTime)) return null
  const total = completedTime - stepStartTime
  if (total <= 0) return null
  return Math.min(1, Math.max(0, (firstTokenTime - stepStartTime) / total))
}

function spanFor(cell: TrajCell, start: number, end: number): TimelineSpan {
  return {
    start,
    end,
    index: cell.index,
    isError: cell.isError === true,
    kind: cell.kind,
    label: cell.text,
    lane: laneFor(cell.kind),
    ttftFraction: ttftFractionFor(cell),
  }
}

/** 把每条记录投影到稳定的三通道时间线。 */
export function deriveTimeline(
  turns: readonly TrajTurnModel[],
  mode: TimelineMode = 'sequence',
): TimelineModel | null {
  if (mode !== 'sequence') {
    return deriveTimedTimeline(turns, mode === 'duration' || mode === 'actual', mode === 'duration')
  }
  const spans: TimelineSpan[] = []
  const turnBoundaries: TimelineTurnBoundary[] = []
  for (const turn of turns) {
    const cells = turn.groups.flatMap(group => group.cells)
    if (cells.length === 0) continue
    if (turn.turn !== null) {
      turnBoundaries.push({ turn: turn.turn, time: spans.length })
    }
    spans.push(...cells.map((cell, offset): TimelineSpan =>
      spanFor(cell, spans.length + offset, spans.length + offset + 1)))
  }
  if (spans.length === 0) return null
  return { start: 0, end: spans.length, spans, turnBoundaries }
}

function deriveTimedTimeline(
  turns: readonly TrajTurnModel[],
  actualDuration: boolean,
  compressIdle: boolean,
): TimelineModel | null {
  const timedTurns = turns.flatMap((turn) => {
    const rawSpans = turn.groups.flatMap(group => group.cells.flatMap((cell): TimelineSpan[] => {
      const range = cellRange(cell)
      if (range === null) return []
      return [{ ...spanFor(cell, range.start, range.end) }]
    }))
    return rawSpans.length === 0 ? [] : [{ turn: turn.turn, rawSpans }]
  })
  const rawSpans = timedTurns.flatMap(turn => turn.rawSpans)
  if (rawSpans.length === 0) return null

  const removedIdleBySpan = new Map<TimelineSpan, number>()
  let removedIdle = 0
  let coveredUntil: number | null = null
  for (const span of [...rawSpans].sort((left, right) =>
    left.start - right.start || left.end - right.end)) {
    if (compressIdle && coveredUntil !== null && span.start > coveredUntil) {
      removedIdle += span.start - coveredUntil
    }
    removedIdleBySpan.set(span, removedIdle)
    coveredUntil = coveredUntil === null ? span.end : Math.max(coveredUntil, span.end)
  }

  const spans: TimelineSpan[] = []
  const turnBoundaries: TimelineTurnBoundary[] = []
  for (const turn of timedTurns) {
    const projected = turn.rawSpans.map((span): TimelineSpan => {
      const offset = removedIdleBySpan.get(span) ?? 0
      return {
        ...span,
        start: span.start - offset,
        end: (actualDuration ? span.end : span.start) - offset,
      }
    })
    spans.push(...projected)
    if (turn.turn !== null) {
      turnBoundaries.push({
        turn: turn.turn,
        time: Math.min(...projected.map(span => span.start)),
      })
    }
  }

  return {
    start: Math.min(...spans.map(span => span.start)),
    end: Math.max(...spans.map(span => span.end)),
    spans,
    turnBoundaries,
  }
}

/** 选区覆盖的记录索引集合。 */
export function timelineFocusIndexes(
  turns: readonly TrajTurnModel[],
  range: { start: number; end: number },
  mode: TimelineMode = 'sequence',
): ReadonlySet<number> {
  const model = deriveTimeline(turns, mode)
  return new Set(
    model?.spans
      .filter(span => span.start <= range.end && span.end >= range.start)
      .map(span => span.index) ?? [],
  )
}

/* ------------------------------------------------------------------ */
/* 台账记录与折叠                                                      */
/* ------------------------------------------------------------------ */

/** 轮次列表 → 扁平台账记录。 */
export function flattenRecords(turns: readonly TrajTurnModel[]): TableRecord[] {
  const records: TableRecord[] = []
  for (const turn of turns) {
    const turnStart = records.length
    for (const group of turn.groups) {
      const groupStart = records.length
      for (const cell of group.cells) {
        records.push({
          turn: turn.turn,
          section: 0,
          group: group.title,
          groupStart: false,
          turnStart: false,
          turnEnd: false,
          cell,
        })
      }
      if (records.length > groupStart) records[groupStart].groupStart = true
    }
    if (records.length > turnStart) {
      records[turnStart].turnStart = true
      records[records.length - 1].turnEnd = true
    }
  }
  return records
}

function recordNeedle(record: TableRecord): string {
  return [
    record.cell.text,
    record.cell.toolName ?? '',
    record.cell.inputDetail ?? '',
    record.cell.outputDetail ?? '',
    record.cell.result ?? '',
  ].join('\n').toLowerCase()
}

/** 搜索过滤：文本不匹配的记录整行隐藏。 */
export function filterRecords(
  records: readonly TableRecord[],
  query: string,
): readonly TableRecord[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return records
  return records.filter(record => recordNeedle(record).includes(needle))
}

function countTools(records: readonly TableRecord[]): number {
  return records.filter(record =>
    record.cell.kind === 'tool' || record.cell.kind === 'subtool').length
}

/** 轮次折叠：每轮只留首条记录并附摘要行。 */
export function collapseTurnRecords(
  records: readonly TableRecord[],
  collapsedTurns: ReadonlySet<number | null>,
  collapsedAssistants: ReadonlySet<string>,
  t: Translate,
): readonly TableRecord[] {
  const turnRecords = new Map<number | null, TableRecord[]>()
  for (const record of records) {
    const bucket = turnRecords.get(record.turn)
    if (bucket) bucket.push(record)
    else turnRecords.set(record.turn, [record])
  }

  const output: TableRecord[] = []
  let currentTurn: number | null | undefined
  let turnCollapsed = false
  for (const record of records) {
    if (record.turn !== currentTurn) {
      currentTurn = record.turn
      turnCollapsed = collapsedTurns.has(record.turn)
    }
    if (turnCollapsed) {
      if (!record.turnStart) continue
      const bucket = turnRecords.get(record.turn) ?? []
      const steps = new Set(
        bucket.map(candidate => candidate.group).filter(title => title.length > 0),
      ).size
      output.push({
        ...record,
        collapsedSummary: t('trajectory.summaryTurn', { steps, tools: countTools(bucket) }),
        collapsedSummaryKind: 'turn',
      })
      continue
    }
    const assistantKey = requestIdentity(record.turn, record.group)
    if (collapsedAssistants.has(assistantKey)) {
      const bucket = (turnRecords.get(record.turn) ?? [])
        .filter(candidate => candidate.group === record.group)
      if (bucket[0]?.cell.index !== record.cell.index) continue
      output.push({
        ...record,
        collapsedSummary: t('trajectory.summaryAssistantTools', { count: countTools(bucket) }),
        collapsedSummaryKind: 'assistant',
      })
      continue
    }
    output.push(record)
  }
  return output
}

const ROW_PX = 30
const ROW_SUMMARY_PX = 20

/** 固定行高的虚拟行布局（一条记录一行）。 */
export function groupVirtualRows(
  records: readonly TableRecord[],
): { rows: VirtualRow[] } {
  const rows: VirtualRow[] = []
  records.forEach((record, index) => {
    rows.push({
      record,
      height: record.collapsedSummary ? ROW_SUMMARY_PX : ROW_PX,
      key: `${record.turn ?? 'none'}-${record.group}-${record.cell.recordId}-${index}`,
    })
  })
  return { rows }
}

/** 台账记录的稳定标识。 */
export function trajectoryRecordId(cell: TrajCell): string {
  return cell.recordId || String(cell.index)
}
