import { defineStore } from 'pinia';

export type CommandSessionRuntimeStatus = 'running' | 'failed_to_start' | 'exited';
export type CommandSessionRuntimeStream = 'stdout' | 'stderr' | 'pty';

export type CommandSessionRuntimeEntry = {
  commandSessionId: string;
  toolCallId: string;
  userId: string;
  sessionId: string;
  workspaceId: string;
  commandIndex: number | null;
  command: string;
  cwd: string;
  shell: string;
  launchMode: string;
  tty: boolean;
  interactive: boolean;
  status: CommandSessionRuntimeStatus;
  seq: number;
  startedAt: string;
  updatedAt: string;
  endedAt: string;
  exitCode: number | null;
  timedOut: boolean;
  error: string;
  stdoutBytes: number;
  stderrBytes: number;
  ptyBytes: number;
  stdoutDroppedBytes: number;
  stderrDroppedBytes: number;
  ptyDroppedBytes: number;
  stdoutTail: string;
  stderrTail: string;
  ptyTail: string;
};

const OUTPUT_TAIL_MAX_CHARS = 24000;

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.trunc(value);
    }
    if (typeof value !== 'string') continue;
    const parsed = Number.parseInt(value.trim(), 10);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
};

const normalizeOptionalCount = (...values: unknown[]): number | null => {
  const parsed = normalizeOptionalInt(...values);
  if (parsed === null || parsed < 0) return null;
  return parsed;
};

const normalizeCount = (...values: unknown[]): number => normalizeOptionalCount(...values) ?? 0;

const normalizeFlag = (...values: unknown[]): boolean => {
  for (const value of values) {
    if (typeof value === 'boolean') return value;
    if (typeof value !== 'string') continue;
    const normalized = value.trim().toLowerCase();
    if (normalized === 'true') return true;
    if (normalized === 'false') return false;
  }
  return false;
};

const normalizeStatus = (value: unknown): CommandSessionRuntimeStatus => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'failed_to_start') return 'failed_to_start';
  if (normalized === 'exited') return 'exited';
  return 'running';
};

const normalizeStream = (value: unknown): CommandSessionRuntimeStream => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized.includes('err')) return 'stderr';
  if (normalized === 'pty') return 'pty';
  return 'stdout';
};

const clampTailText = (
  current: string,
  delta: string
): { value: string; droppedChars: number } => {
  const next = `${current || ''}${delta || ''}`;
  if (next.length <= OUTPUT_TAIL_MAX_CHARS) {
    return { value: next, droppedChars: 0 };
  }
  const droppedChars = next.length - OUTPUT_TAIL_MAX_CHARS;
  return {
    value: next.slice(droppedChars),
    droppedChars
  };
};

const ensureSessionIndex = (state: CommandSessionsState, sessionId: string): string[] => {
  if (!state.sessionIndex[sessionId]) {
    state.sessionIndex[sessionId] = [];
  }
  return state.sessionIndex[sessionId];
};

const defaultEntry = (
  commandSessionId: string,
  sessionId: string
): CommandSessionRuntimeEntry => ({
  commandSessionId,
  toolCallId: '',
  userId: '',
  sessionId,
  workspaceId: '',
  commandIndex: null,
  command: '',
  cwd: '',
  shell: '',
  launchMode: '',
  tty: false,
  interactive: false,
  status: 'running',
  seq: 0,
  startedAt: '',
  updatedAt: '',
  endedAt: '',
  exitCode: null,
  timedOut: false,
  error: '',
  stdoutBytes: 0,
  stderrBytes: 0,
  ptyBytes: 0,
  stdoutDroppedBytes: 0,
  stderrDroppedBytes: 0,
  ptyDroppedBytes: 0,
  stdoutTail: '',
  stderrTail: '',
  ptyTail: ''
});

const resolveSessionId = (fallbackSessionId: string, payload: Record<string, unknown>): string =>
  normalizeText(payload.session_id ?? payload.sessionId ?? fallbackSessionId);

const normalizeEntryPayload = (
  fallbackSessionId: string,
  payload: unknown
): CommandSessionRuntimeEntry | null => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
  const source = payload as Record<string, unknown>;
  const commandSessionId = normalizeText(
    source.command_session_id ?? source.commandSessionId
  );
  if (!commandSessionId) return null;
  const sessionId = resolveSessionId(fallbackSessionId, source);
  return {
    commandSessionId,
    toolCallId: normalizeText(source.tool_call_id ?? source.toolCallId),
    userId: normalizeText(source.user_id ?? source.userId),
    sessionId,
    workspaceId: normalizeText(source.workspace_id ?? source.workspaceId),
    commandIndex: normalizeOptionalCount(source.command_index, source.commandIndex),
    command: normalizeText(source.command),
    cwd: normalizeText(source.cwd),
    shell: normalizeText(source.shell),
    launchMode: normalizeText(source.launch_mode ?? source.launchMode),
    tty: normalizeFlag(source.tty),
    interactive: normalizeFlag(source.interactive),
    status: normalizeStatus(source.status),
    seq: normalizeCount(source.seq),
    startedAt: normalizeText(source.started_at ?? source.startedAt),
    updatedAt: normalizeText(source.updated_at ?? source.updatedAt),
    endedAt: normalizeText(source.ended_at ?? source.endedAt),
    exitCode: normalizeOptionalInt(
      source.exit_code,
      source.exitCode,
      source.returncode
    ),
    timedOut: normalizeFlag(source.timed_out, source.timedOut),
    error: normalizeText(source.error),
    stdoutBytes: normalizeCount(source.stdout_bytes, source.stdoutBytes),
    stderrBytes: normalizeCount(source.stderr_bytes, source.stderrBytes),
    ptyBytes: normalizeCount(source.pty_bytes, source.ptyBytes),
    stdoutDroppedBytes: normalizeCount(
      source.stdout_dropped_bytes,
      source.stdoutDroppedBytes
    ),
    stderrDroppedBytes: normalizeCount(
      source.stderr_dropped_bytes,
      source.stderrDroppedBytes
    ),
    ptyDroppedBytes: normalizeCount(source.pty_dropped_bytes, source.ptyDroppedBytes),
    stdoutTail: String(source.stdout_tail ?? source.stdoutTail ?? ''),
    stderrTail: String(source.stderr_tail ?? source.stderrTail ?? ''),
    ptyTail: String(source.pty_tail ?? source.ptyTail ?? '')
  };
};

type CommandSessionsState = {
  entries: Record<string, CommandSessionRuntimeEntry>;
  sessionIndex: Record<string, string[]>;
  hydratedSessions: Record<string, boolean>;
};

// Keep command terminal runtime separate from workflow message strings to reduce hot-path churn.
export const useCommandSessionStore = defineStore('commandSessions', {
  state: (): CommandSessionsState => ({
    entries: {},
    sessionIndex: {},
    hydratedSessions: {}
  }),
  getters: {
    getById: (state) => (commandSessionId: string): CommandSessionRuntimeEntry | null => {
      const normalizedId = normalizeText(commandSessionId);
      return normalizedId ? state.entries[normalizedId] || null : null;
    },
    listBySession: (state) => (sessionId: string): CommandSessionRuntimeEntry[] => {
      const normalizedSessionId = normalizeText(sessionId);
      if (!normalizedSessionId) return [];
      const ids = state.sessionIndex[normalizedSessionId] || [];
      return ids
        .map((id) => state.entries[id])
        .filter(Boolean)
        .sort((left, right) => {
          const leftIndex = left.commandIndex ?? Number.MAX_SAFE_INTEGER;
          const rightIndex = right.commandIndex ?? Number.MAX_SAFE_INTEGER;
          return leftIndex - rightIndex || left.seq - right.seq;
        });
    }
  },
  actions: {
    reset(): void {
      this.entries = {};
      this.sessionIndex = {};
      this.hydratedSessions = {};
    },
    clearSession(sessionId: string): void {
      const normalizedSessionId = normalizeText(sessionId);
      if (!normalizedSessionId) return;
      const ids = this.sessionIndex[normalizedSessionId] || [];
      ids.forEach((id) => {
        delete this.entries[id];
      });
      delete this.sessionIndex[normalizedSessionId];
      delete this.hydratedSessions[normalizedSessionId];
    },
    hydrateSession(sessionId: string, snapshots: unknown[]): void {
      const normalizedSessionId = normalizeText(sessionId);
      if (!normalizedSessionId) return;
      const nextIds = new Set<string>();
      if (Array.isArray(snapshots)) {
        snapshots.forEach((snapshot) => {
          const normalized = normalizeEntryPayload(normalizedSessionId, snapshot);
          if (!normalized) return;
          this.upsertSnapshot(normalizedSessionId, normalized);
          nextIds.add(normalized.commandSessionId);
        });
      }
      const previousIds = this.sessionIndex[normalizedSessionId] || [];
      previousIds.forEach((id) => {
        if (!nextIds.has(id)) {
          delete this.entries[id];
        }
      });
      this.sessionIndex[normalizedSessionId] = Array.from(nextIds);
      this.hydratedSessions[normalizedSessionId] = true;
    },
    upsertSnapshot(sessionId: string, payload: unknown): CommandSessionRuntimeEntry | null {
      const normalized = normalizeEntryPayload(sessionId, payload);
      if (!normalized) return null;
      const existing = this.entries[normalized.commandSessionId];
      const existingSeq = existing?.seq ?? -1;
      const nextSeq = normalized.seq;
      const preserveTerminalState = existing && nextSeq < existingSeq;
      const next: CommandSessionRuntimeEntry = {
        ...(existing || defaultEntry(normalized.commandSessionId, normalized.sessionId)),
        ...(preserveTerminalState
          ? {
              toolCallId: normalized.toolCallId || existing.toolCallId,
              userId: normalized.userId || existing.userId,
              sessionId: normalized.sessionId || existing.sessionId,
              workspaceId: normalized.workspaceId || existing.workspaceId,
              commandIndex:
                normalized.commandIndex === null ? existing.commandIndex : normalized.commandIndex,
              command: normalized.command || existing.command,
              cwd: normalized.cwd || existing.cwd,
              shell: normalized.shell || existing.shell,
              launchMode: normalized.launchMode || existing.launchMode,
              tty: normalized.tty || existing.tty,
              interactive: normalized.interactive || existing.interactive,
              stdoutTail: normalized.stdoutTail || existing.stdoutTail,
              stderrTail: normalized.stderrTail || existing.stderrTail,
              ptyTail: normalized.ptyTail || existing.ptyTail,
              stdoutBytes: Math.max(existing.stdoutBytes, normalized.stdoutBytes),
              stderrBytes: Math.max(existing.stderrBytes, normalized.stderrBytes),
              ptyBytes: Math.max(existing.ptyBytes, normalized.ptyBytes),
              stdoutDroppedBytes: Math.max(
                existing.stdoutDroppedBytes,
                normalized.stdoutDroppedBytes
              ),
              stderrDroppedBytes: Math.max(
                existing.stderrDroppedBytes,
                normalized.stderrDroppedBytes
              ),
              ptyDroppedBytes: Math.max(existing.ptyDroppedBytes, normalized.ptyDroppedBytes),
              updatedAt: normalized.updatedAt || existing.updatedAt
            }
          : normalized)
      };
      this.entries[normalized.commandSessionId] = next;
      const sessionIds = ensureSessionIndex(this.$state, next.sessionId);
      if (!sessionIds.includes(next.commandSessionId)) {
        sessionIds.push(next.commandSessionId);
      }
      return next;
    },
    appendDelta(
      sessionId: string,
      commandSessionId: string,
      stream: unknown,
      delta: unknown,
      meta: Record<string, unknown> = {}
    ): CommandSessionRuntimeEntry | null {
      const normalizedCommandSessionId = normalizeText(commandSessionId);
      const textDelta = String(delta || '');
      if (!normalizedCommandSessionId || !textDelta) {
        return null;
      }
      const streamName = normalizeStream(stream);
      const existing = this.entries[normalizedCommandSessionId];
      const normalizedSessionId = normalizeText(sessionId || existing?.sessionId);
      if (!normalizedSessionId) {
        return null;
      }
      const next = {
        ...(existing || defaultEntry(normalizedCommandSessionId, normalizedSessionId))
      };
      if (!next.sessionId) {
        next.sessionId = normalizedSessionId;
      }
      const command = normalizeText(meta.command);
      if (command && !next.command) {
        next.command = command;
      }
      const commandIndex = normalizeOptionalCount(meta.command_index, meta.commandIndex);
      if (commandIndex !== null && next.commandIndex === null) {
        next.commandIndex = commandIndex;
      }
      next.updatedAt = new Date().toISOString();
      next.seq = Math.max(next.seq + 1, normalizeCount(meta.seq));

      if (streamName === 'stderr') {
        const tail = clampTailText(next.stderrTail, textDelta);
        next.stderrTail = tail.value;
        next.stderrBytes = next.stderrBytes + textDelta.length;
        next.stderrDroppedBytes = next.stderrDroppedBytes + tail.droppedChars;
      } else if (streamName === 'pty') {
        const tail = clampTailText(next.ptyTail, textDelta);
        next.ptyTail = tail.value;
        next.ptyBytes = next.ptyBytes + textDelta.length;
        next.ptyDroppedBytes = next.ptyDroppedBytes + tail.droppedChars;
      } else {
        const tail = clampTailText(next.stdoutTail, textDelta);
        next.stdoutTail = tail.value;
        next.stdoutBytes = next.stdoutBytes + textDelta.length;
        next.stdoutDroppedBytes = next.stdoutDroppedBytes + tail.droppedChars;
      }

      this.entries[normalizedCommandSessionId] = next;
      const sessionIds = ensureSessionIndex(this.$state, next.sessionId);
      if (!sessionIds.includes(normalizedCommandSessionId)) {
        sessionIds.push(normalizedCommandSessionId);
      }
      return next;
    }
  }
});
