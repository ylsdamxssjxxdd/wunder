// W2-E: render-source fallback (transcript -> turn rows).
//
// The chat page renders turn rows exclusively from the v2 durable thread state
// (`buildChatThreadMaterializedSlots`). When the atomic thread snapshot fails or
// its deadline aborts it, `hydrateChatThreadRuntime` is skipped, the registry has
// no entry for the session, and the panel settles on the yearly activity grid
// even though the bounded transcript (limit=80) was already fetched.
//
// This adapter builds the exact row shape the page already consumes
// (`{key, rootTurnId, kind, user:{key,sourceIndex,message}, assistant:{...}}`)
// from that transcript. The row key keeps the `turn:` prefix so the virtual
// window keeps its two-row height estimate (`resolveVirtualMessageHeight`).
//
// v1 and v2 render keys never share a namespace (v2 bubbles carry `tturn:`,
// legacy messages resolve to `runtime:`/`legacy:`), so a fallback row can never
// be de-duplicated against a v2 row. The two sources are therefore mutually
// exclusive by construction in `resolveAgentTurnRenderRows`; they are never
// concatenated.

import { resolveChatRuntimeRenderableKey } from '@/realtime/chat/chatRuntimeMessageKeys';

/** `wunder_chat_*` localStorage switch, fail-open like `isWindowingEnabled`. */
export const TRANSCRIPT_FALLBACK_ENABLED_KEY = 'wunder_chat_transcript_fallback';

/** Row key prefix shared with the v2 projection (`turn:<turnId>`). */
export const LEGACY_TURN_ROW_KEY_PREFIX = 'turn:legacy:';

export type AgentTurnRenderSource = 'window' | 'transcript_fallback' | 'empty';

export interface AgentTurnRowMessage {
  key: string;
  sourceIndex: number;
  message: Record<string, unknown>;
}

export interface AgentTurnRow {
  key: string;
  rootTurnId: string;
  kind: 'turn';
  user: AgentTurnRowMessage;
  assistant: AgentTurnRowMessage;
}

export interface AgentTurnRenderPlan {
  source: AgentTurnRenderSource;
  rows: AgentTurnRow[];
}

const EMPTY_ROWS: AgentTurnRow[] = Object.freeze([]) as unknown as AgentTurnRow[];

/**
 * Read the fallback switch. Unset / unreadable means enabled (fail-open), the
 * same contract as `wunder_chat_windowing`. `0`/`false` forces the pure grid
 * behaviour for a one-key rollback.
 */
export const isTranscriptFallbackEnabled = (): boolean => {
  try {
    const storage = (globalThis as { localStorage?: { getItem?: (key: string) => string | null } })?.localStorage;
    if (!storage || typeof storage.getItem !== 'function') return true;
    const raw = storage.getItem(TRANSCRIPT_FALLBACK_ENABLED_KEY);
    if (!raw) return true;
    return raw !== '0' && String(raw).toLowerCase() !== 'false';
  } catch (error) {
    return true;
  }
};

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null;

const readRole = (message: Record<string, unknown>): string =>
  String(message?.role ?? '').trim().toLowerCase();

/** The greeting bubble is owned by `resolveSyntheticGreetingRenderable`; never emit it twice. */
export const isTranscriptGreetingMessage = (message: Record<string, unknown> | null | undefined): boolean => {
  if (!message) return false;
  if (message.isGreeting === true || message.is_greeting === true) return true;
  const display = asRecord(message.display);
  return Boolean(display && (display.isGreeting === true || display.is_greeting === true));
};

/** Internal bookkeeping rows must not become a user turn. */
export const isTranscriptHiddenMessage = (message: Record<string, unknown> | null | undefined): boolean => {
  if (!message) return false;
  if (message.hiddenInternal === true || message.hidden === true) return true;
  const meta = asRecord(message.meta);
  if (!meta) return false;
  if (meta.hidden === true || meta.internal_user === true) return true;
  return String(meta.type ?? '').trim() === 'model_context_internal';
};

const buildEmptyAssistantMessage = (): Record<string, unknown> => ({ role: 'assistant', content: '' });

interface PendingTurn {
  sourceIndex: number;
  userMessage: Record<string, unknown>;
  assistants: Array<{ sourceIndex: number; message: Record<string, unknown> }>;
}

/** Multiple assistant records between two user inputs belong to one page slot. */
const mergeAssistantMessages = (
  entries: Array<{ sourceIndex: number; message: Record<string, unknown> }>
): { sourceIndex: number; message: Record<string, unknown> } | null => {
  if (!entries.length) return null;
  if (entries.length === 1) return entries[0];
  const first = entries[0];
  const collect = (field: string): string =>
    entries.map((entry) => String(entry.message?.[field] ?? '')).filter(Boolean).join('\n\n');
  const collectList = (field: string): unknown[] =>
    entries.flatMap((entry) => (Array.isArray(entry.message?.[field]) ? (entry.message?.[field] as unknown[]) : []));
  return {
    sourceIndex: first.sourceIndex,
    message: {
      ...first.message,
      content: collect('content'),
      reasoning: collect('reasoning'),
      workflowItems: collectList('workflowItems'),
      timeline: collectList('timeline'),
      subagents: collectList('subagents')
    }
  };
};

/**
 * Pair the flat transcript into v2-shaped turn rows.
 * - user records open a row; the following assistant records fill its reply;
 * - assistant records before any user record never invent a row;
 * - greeting and hidden/internal records are dropped;
 * - a user record with no reply yet still opens a row (streaming placeholder).
 */
export const buildTranscriptTurnRows = (messages: unknown): AgentTurnRow[] => {
  const list = Array.isArray(messages) ? messages : [];
  const rows: AgentTurnRow[] = [];
  let pending: PendingTurn | null = null;

  const flush = (): void => {
    if (!pending) return;
    const merged = mergeAssistantMessages(pending.assistants);
    const assistantSourceIndex = merged ? merged.sourceIndex : pending.sourceIndex;
    const assistantMessage = merged ? merged.message : buildEmptyAssistantMessage();
    rows.push({
      key: `${LEGACY_TURN_ROW_KEY_PREFIX}${pending.sourceIndex}`,
      // No v2 turn id exists for a legacy row; keep the namespace explicit.
      rootTurnId: `legacy:${pending.sourceIndex}`,
      kind: 'turn',
      user: {
        key: resolveChatRuntimeRenderableKey(pending.userMessage, pending.sourceIndex),
        sourceIndex: pending.sourceIndex,
        message: pending.userMessage
      },
      assistant: {
        key: resolveChatRuntimeRenderableKey(assistantMessage, assistantSourceIndex),
        sourceIndex: assistantSourceIndex,
        message: assistantMessage
      }
    });
    pending = null;
  };

  for (let sourceIndex = 0; sourceIndex < list.length; sourceIndex += 1) {
    const message = asRecord(list[sourceIndex]);
    if (!message) continue;
    if (isTranscriptGreetingMessage(message) || isTranscriptHiddenMessage(message)) continue;
    const role = readRole(message);
    if (role === 'user') {
      flush();
      pending = { sourceIndex, userMessage: message, assistants: [] };
      continue;
    }
    if (role === 'assistant' && pending) {
      pending.assistants.push({ sourceIndex, message });
    }
  }
  flush();
  return rows;
};

export interface ResolveAgentTurnRenderRowsInput {
  /** v2 window projection rows (may be an empty array). */
  windowRows?: AgentTurnRow[] | null;
  /**
   * Whether the durable registry owns this session.
   * `false` means the render source is unavailable (a failed snapshot);
   * `true` with zero rows is a real, legitimately empty thread.
   */
  windowAvailable?: boolean;
  /** `chatStore.messages` (the bounded v1 transcript). */
  messages?: unknown;
  /** `wunder_chat_transcript_fallback` switch. */
  enabled?: boolean;
}

/**
 * Pick exactly one render source: window projection > transcript fallback > grid.
 * The fallback only replaces a *missing* window source (`windowAvailable` false),
 * never an empty one, so a legitimate empty thread keeps the grid.
 */
export const resolveAgentTurnRenderRows = (
  input: ResolveAgentTurnRenderRowsInput | null | undefined
): AgentTurnRenderPlan => {
  const windowRows = Array.isArray(input?.windowRows) ? (input?.windowRows as AgentTurnRow[]).filter(Boolean) : [];
  if (windowRows.length > 0) {
    return { source: 'window', rows: windowRows };
  }
  if (input?.enabled !== true) {
    return { source: 'empty', rows: EMPTY_ROWS };
  }
  if (input?.windowAvailable !== false) {
    return { source: 'empty', rows: EMPTY_ROWS };
  }
  const fallbackRows = buildTranscriptTurnRows(input?.messages);
  return fallbackRows.length > 0
    ? { source: 'transcript_fallback', rows: fallbackRows }
    : { source: 'empty', rows: EMPTY_ROWS };
};

export interface ResolveThreadActivityCardInput {
  /** `retainedMessageRenderKind`: only the agent conversation shows the grid. */
  renderKind?: string | null;
  /** True when the window projection or the fallback produced at least one row. */
  hasTurnContent?: boolean;
  sessionId?: string | null;
  historyLoading?: boolean;
}

/**
 * The yearly activity grid is the last resort: any turn row (window or fallback)
 * hides it, and history still loading must not flash it over a thread that has
 * turns.
 */
export const resolveShowThreadActivityCard = (input: ResolveThreadActivityCardInput | null | undefined): boolean => {
  if (String(input?.renderKind ?? '') !== 'agent') return false;
  if (input?.hasTurnContent === true) return false;
  const sessionId = String(input?.sessionId ?? '');
  return !sessionId || input?.historyLoading !== true;
};
