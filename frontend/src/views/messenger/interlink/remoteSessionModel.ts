// AI生成
/**
 * 远程会话视图的帧归一化（方案 §7.4）。
 *
 * 通道语义是 Snapshot → Delta → Replay 的同构：首包是本地线程当前投影的序列化快照，
 * 之后是增量。转发帧的 payload 内部结构属于「本地引擎公开事件流」，契约只冻结了外层信封
 * `{v,type,thread_id,seq,payload}`，所以这里对负载做**防御式归一**：能识别的取文本，
 * 识别不了的计入 dropped 计数并在界面上说明，绝不因为一个陌生帧让视图变空白。
 *
 * 隐私红线（§7.4）：消息正文只活在组件态，刷新即重快照；本模块不写任何存储、
 * 不进 localStorage / pinia / 服务端缓存。
 */

import type { InterlinkRemoteFrame } from '@/api/interlink';
import { REMOTE_MESSAGE_MAX_RENDERED } from './interlinkNodeModel';

export type RemoteMessageRole = 'user' | 'assistant' | 'tool' | 'system' | 'unknown';

export type RemoteMessage = {
  id: string;
  role: RemoteMessageRole;
  text: string;
  status: string;
  ts: number;
};

export type RemoteSessionState = {
  messages: RemoteMessage[];
  /** 已应用的最后一个帧序号，用于丢帧/重帧判断。 */
  lastSeq: number;
  /** 无法归一的帧数量（界面显式说明，不静默吞掉）。 */
  droppedFrames: number;
  gotSnapshot: boolean;
};

export const createRemoteSessionState = (): RemoteSessionState => ({
  messages: [],
  lastSeq: -1,
  droppedFrames: 0,
  gotSnapshot: false
});

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const asText = (value: unknown): string => (typeof value === 'string' ? value : '');

const ROLE_ALIASES: Record<string, RemoteMessageRole> = {
  user: 'user',
  human: 'user',
  input: 'user',
  assistant: 'assistant',
  agent: 'assistant',
  model: 'assistant',
  output: 'assistant',
  tool: 'tool',
  tool_result: 'tool',
  system: 'system'
};

const resolveRole = (raw: Record<string, unknown>): RemoteMessageRole => {
  const candidate = asText(raw.role) || asText(raw.speaker) || asText(raw.kind) || asText(raw.sender);
  return ROLE_ALIASES[candidate.trim().toLowerCase()] || 'unknown';
};

/** 从消息负载里抽正文：字符串 / 块数组 / 常见字段名逐级兜底。 */
const extractText = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) {
    return value
      .map((item) => {
        if (typeof item === 'string') return item;
        const record = asRecord(item);
        const text =
          asText(record.text) ||
          asText(record.content) ||
          asText(record.delta) ||
          asText(record.summary);
        return text;
      })
      .filter(Boolean)
      .join('');
  }
  const record = asRecord(value);
  if (!Object.keys(record).length) return '';
  return (
    asText(record.text) ||
    asText(record.content) ||
    asText(record.message) ||
    asText(record.delta) ||
    asText(record.output)
  );
};

const pickMessageSource = (raw: Record<string, unknown>): Record<string, unknown> | null => {
  const nested = asRecord(raw.message ?? raw.item ?? raw.entry ?? raw.data);
  return Object.keys(nested).length ? nested : raw;
};

const buildMessage = (raw: Record<string, unknown>, index: number): RemoteMessage | null => {
  const source = pickMessageSource(raw);
  const text = extractText(source.content ?? source.text ?? source.message ?? source.data ?? source);
  const id =
    asText(source.id) ||
    asText(source.message_id) ||
    asText(source.event_id) ||
    asText(source.turn_id) ||
    `remote_${index}`;
  if (!text && !asText(source.status)) return null;
  return {
    id,
    role: resolveRole(source),
    text,
    status: asText(source.status),
    ts: Number(source.created_at ?? source.ts) || Date.now()
  };
};

const trimMessages = (messages: RemoteMessage[]): RemoteMessage[] =>
  messages.length > REMOTE_MESSAGE_MAX_RENDERED
    ? messages.slice(messages.length - REMOTE_MESSAGE_MAX_RENDERED)
    : messages;

const upsertMessage = (state: RemoteSessionState, message: RemoteMessage): void => {
  const index = state.messages.findIndex((item) => item.id === message.id);
  if (index < 0) {
    state.messages = trimMessages([...state.messages, message]);
    return;
  }
  const previous = state.messages[index];
  // 增量只补正文，已经收到的更长文本不被更短的帧覆盖。
  state.messages[index] = {
    ...previous,
    role: message.role === 'unknown' ? previous.role : message.role,
    text: message.text.length >= previous.text.length ? message.text : previous.text,
    status: message.status || previous.status,
    ts: message.ts || previous.ts
  };
};

/** delta 里的「流式片段」：追加到当前尾条助手消息，没有就开一条。 */
const appendStreamingDelta = (state: RemoteSessionState, delta: string): void => {
  const tail = state.messages[state.messages.length - 1];
  if (tail && (tail.role === 'assistant' || tail.role === 'unknown')) {
    upsertMessage(state, { ...tail, text: `${tail.text}${delta}`, status: 'streaming' });
    return;
  }
  state.messages = trimMessages([
    ...state.messages,
    { id: `remote_stream_${state.messages.length}`, role: 'assistant', text: delta, status: 'streaming', ts: Date.now() }
  ]);
};

/** 把一个远程帧并入会话状态；返回是否有可渲染变化。 */
export const applyRemoteFrame = (state: RemoteSessionState, frame: InterlinkRemoteFrame): boolean => {
  if (!frame) return false;
  const payload = asRecord(frame.payload);
  const type = String(frame.type || '').trim().toLowerCase();
  if (type === 'command') {
    // 设备侧命令生命周期通知：只有 kind/status/command_id，会话内容仍由
    // snapshot/delta 承载，因此它不改动状态、也不占用幂等序号。
    return false;
  }
  const seq = Number(frame.seq);
  if (Number.isFinite(seq) && seq >= 0 && seq <= state.lastSeq) {
    // 重复帧（服务端补水可能重发）幂等丢弃。
    return false;
  }

  if (type === 'snapshot') {
    state.gotSnapshot = true;
    const list = Array.isArray(payload.messages)
      ? payload.messages
      : Array.isArray(payload.items)
        ? payload.items
        : Array.isArray(payload.turns)
          ? payload.turns
          : null;
    if (!list) {
      state.droppedFrames += 1;
      state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
      return false;
    }
    const next: RemoteMessage[] = [];
    list.forEach((item, index) => {
      const message = buildMessage(asRecord(item), index);
      if (message) next.push(message);
    });
    // 快照权威：整段替换，避免补水后与本地残留拼接出重复气泡。
    state.messages = trimMessages(next);
    state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
    return true;
  }

  if (type === 'delta') {
    const directDelta = extractText(payload.delta ?? payload.token ?? payload.text ?? payload.content);
    // 节点的增量帧是 {event: 事件名字符串, data: 变更条目}。事件名是字符串，
    // 不能当成结构化载荷，否则会把真正的 data 记录遮住，增量永远渲染不出来。
    const structured =
      payload.message ??
      payload.item ??
      payload.data ??
      (payload.event && typeof payload.event === 'object' ? payload.event : undefined);
    if (Array.isArray(structured) || (structured && typeof structured === 'object')) {
      const message = buildMessage(asRecord(structured), state.messages.length);
      if (message) {
        upsertMessage(state, message);
        state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
        return true;
      }
    }
    if (directDelta) {
      appendStreamingDelta(state, directDelta);
      state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
      return true;
    }
    // 只有状态变化的帧（例如 run 结束）：落到尾条消息上。
    const status = asText(payload.status);
    const tail = state.messages[state.messages.length - 1];
    if (status && tail) {
      upsertMessage(state, { ...tail, status });
      state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
      return true;
    }
    state.droppedFrames += 1;
    state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
    return false;
  }

  state.lastSeq = Number.isFinite(seq) ? seq : state.lastSeq;
  return false;
};

export const resolveRemoteErrorText = (payload: Record<string, unknown>): string => {
  const record = asRecord(payload);
  return (
    asText(record.message) ||
    asText(record.error) ||
    asText(record.summary) ||
    asText(record.code) ||
    ''
  );
};
