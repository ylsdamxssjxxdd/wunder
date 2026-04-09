export type BeeroomMissionSubagentItem = {
  key: string;
  sessionId: string;
  runId: string;
  runKind: string;
  requestedBy: string;
  spawnedBy: string;
  agentId: string;
  title: string;
  label: string;
  status: string;
  summary: string;
  userMessage: string;
  assistantMessage: string;
  errorMessage: string;
  updatedTime: number;
  terminal: boolean;
  failed: boolean;
  depth: number | null;
  role: string;
  controlScope: string;
  spawnMode: string;
  strategy: string;
  dispatchLabel: string;
  controllerSessionId: string;
  parentSessionId: string;
  parentTurnRef: string;
  parentUserRound: number | null;
  parentModelRound: number | null;
  workflowItems: unknown[];
};

export type BeeroomSessionEventRecord = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
  title?: unknown;
  timestamp?: unknown;
  timestamp_ms?: unknown;
};

type BeeroomSessionEventRound = {
  events?: BeeroomSessionEventRecord[];
};

type BeeroomParentTurnKey = {
  userRound: number;
  modelRound: number;
};

const ACTIVE_BEEROOM_SUBAGENT_STATUS_VALUES = ['running', 'waiting', 'queued', 'accepted', 'cancelling'] as const;

export const ACTIVE_BEEROOM_SUBAGENT_STATUSES = new Set<string>(ACTIVE_BEEROOM_SUBAGENT_STATUS_VALUES);

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeOptionalCount = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeSubagentUpdatedTime = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value > 1_000_000_000_000 ? value / 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed / 1000 : 0;
};

const normalizeSubagentStatus = (value: unknown): string => {
  const normalized = normalizeText(value).toLowerCase();
  return normalized || 'running';
};

const decodeParentTurnRef = (value: unknown): BeeroomParentTurnKey | null => {
  const text = normalizeText(value);
  if (!text) return null;
  const match = /^subagent_turn:(\d+):(\d+)$/i.exec(text);
  if (!match) return null;
  const userRound = Number(match[1]);
  const modelRound = Number(match[2]);
  if (!Number.isFinite(userRound) || userRound <= 0 || !Number.isFinite(modelRound) || modelRound <= 0) {
    return null;
  }
  return { userRound, modelRound };
};

const resolveParentTurnKey = (item: Pick<
  BeeroomMissionSubagentItem,
  'parentTurnRef' | 'parentUserRound' | 'parentModelRound'
>): BeeroomParentTurnKey | null => {
  const parentUserRound = Number(item.parentUserRound || 0);
  const parentModelRound = Number(item.parentModelRound || 0);
  if (parentUserRound > 0 && parentModelRound > 0) {
    return {
      userRound: parentUserRound,
      modelRound: parentModelRound
    };
  }
  return decodeParentTurnRef(item.parentTurnRef);
};

const compareParentTurnKeys = (left: BeeroomParentTurnKey, right: BeeroomParentTurnKey) =>
  left.userRound - right.userRound || left.modelRound - right.modelRound;

const buildSubagentIdentity = (item: Pick<BeeroomMissionSubagentItem, 'key' | 'sessionId' | 'runId'>) =>
  normalizeText(item.runId || item.sessionId || item.key);

const shouldPreferIncomingSubagent = (
  current: BeeroomMissionSubagentItem,
  incoming: BeeroomMissionSubagentItem
) => {
  const currentMoment = Number(current.updatedTime || 0);
  const incomingMoment = Number(incoming.updatedTime || 0);
  if (incomingMoment !== currentMoment) {
    return incomingMoment > currentMoment;
  }
  if (incoming.terminal !== current.terminal) {
    return incoming.terminal;
  }
  if (incoming.failed !== current.failed) {
    return incoming.failed;
  }
  const incomingActive = ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(incoming.status);
  const currentActive = ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(current.status);
  if (incomingActive !== currentActive) {
    return incomingActive;
  }
  if (incoming.assistantMessage !== current.assistantMessage) {
    return Boolean(incoming.assistantMessage);
  }
  if (incoming.summary !== current.summary) {
    return Boolean(incoming.summary);
  }
  return false;
};

const mergeSubagentPair = (
  current: BeeroomMissionSubagentItem,
  incoming: BeeroomMissionSubagentItem
): BeeroomMissionSubagentItem => {
  const preferred = shouldPreferIncomingSubagent(current, incoming) ? incoming : current;
  const fallback = preferred === incoming ? current : incoming;
  return {
    ...fallback,
    ...preferred,
    key: preferred.key || fallback.key,
    sessionId: preferred.sessionId || fallback.sessionId,
    runId: preferred.runId || fallback.runId,
    runKind: preferred.runKind || fallback.runKind,
    requestedBy: preferred.requestedBy || fallback.requestedBy,
    spawnedBy: preferred.spawnedBy || fallback.spawnedBy,
    agentId: preferred.agentId || fallback.agentId,
    title: preferred.title || fallback.title,
    label: preferred.label || fallback.label,
    status: preferred.status || fallback.status,
    summary: preferred.summary || fallback.summary,
    userMessage: preferred.userMessage || fallback.userMessage,
    assistantMessage: preferred.assistantMessage || fallback.assistantMessage,
    errorMessage: preferred.errorMessage || fallback.errorMessage,
    updatedTime: Math.max(Number(current.updatedTime || 0), Number(incoming.updatedTime || 0)),
    terminal: preferred.terminal,
    failed: preferred.failed,
    depth: preferred.depth ?? fallback.depth,
    role: preferred.role || fallback.role,
    controlScope: preferred.controlScope || fallback.controlScope,
    spawnMode: preferred.spawnMode || fallback.spawnMode,
    strategy: preferred.strategy || fallback.strategy,
    dispatchLabel: preferred.dispatchLabel || fallback.dispatchLabel,
    controllerSessionId: preferred.controllerSessionId || fallback.controllerSessionId,
    parentSessionId: preferred.parentSessionId || fallback.parentSessionId,
    parentTurnRef: preferred.parentTurnRef || fallback.parentTurnRef,
    parentUserRound: preferred.parentUserRound ?? fallback.parentUserRound,
    parentModelRound: preferred.parentModelRound ?? fallback.parentModelRound,
    workflowItems:
      Array.isArray(preferred.workflowItems) && preferred.workflowItems.length > 0
        ? preferred.workflowItems
        : fallback.workflowItems
  };
};

export const sortBeeroomMissionSubagentItems = (
  items: BeeroomMissionSubagentItem[]
): BeeroomMissionSubagentItem[] =>
  [...items].sort((left, right) => {
    const activeDiff =
      Number(ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(right.status)) -
      Number(ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(left.status));
    if (activeDiff !== 0) return activeDiff;
    const updatedDiff = Number(right.updatedTime || 0) - Number(left.updatedTime || 0);
    if (updatedDiff !== 0) return updatedDiff;
    return buildSubagentIdentity(left).localeCompare(buildSubagentIdentity(right));
  });

export const mergeBeeroomMissionSubagentItems = (
  ...sources: Array<BeeroomMissionSubagentItem[] | null | undefined>
): BeeroomMissionSubagentItem[] => {
  const merged = new Map<string, BeeroomMissionSubagentItem>();
  sources.forEach((items) => {
    (Array.isArray(items) ? items : []).forEach((item) => {
      const identity = buildSubagentIdentity(item);
      if (!identity) return;
      const current = merged.get(identity);
      merged.set(identity, current ? mergeSubagentPair(current, item) : item);
    });
  });
  return sortBeeroomMissionSubagentItems(Array.from(merged.values()));
};

const resolveSessionEventName = (event: BeeroomSessionEventRecord): string =>
  normalizeText(event?.event ?? event?.type).toLowerCase();

const resolveSessionEventPayload = (event: BeeroomSessionEventRecord): Record<string, unknown> => {
  const source = asRecord(event?.data);
  return source || {};
};

export const resolveBeeroomSessionEventTimestamp = (event: BeeroomSessionEventRecord): number => {
  const payload = resolveSessionEventPayload(event);
  const timestampMs = Number(event?.timestamp_ms ?? payload.timestamp_ms ?? 0);
  if (Number.isFinite(timestampMs) && timestampMs > 0) {
    return timestampMs / 1000;
  }
  const numeric = Number(event?.timestamp ?? payload.timestamp ?? 0);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
  }
  const iso = normalizeText(event?.timestamp);
  if (!iso) return 0;
  const parsed = Date.parse(iso);
  return Number.isFinite(parsed) ? parsed / 1000 : 0;
};

export const flattenBeeroomSessionEventRounds = (rounds: unknown): BeeroomSessionEventRecord[] => {
  const items: BeeroomSessionEventRecord[] = [];
  (Array.isArray(rounds) ? rounds : []).forEach((round) => {
    const source = round as BeeroomSessionEventRound;
    if (!Array.isArray(source?.events)) return;
    source.events.forEach((event) => {
      if (!event || typeof event !== 'object') return;
      items.push(event);
    });
  });
  return items;
};

export const normalizeBeeroomMissionSubagentItem = (
  value: unknown
): BeeroomMissionSubagentItem | null => {
  const source = asRecord(value);
  if (!source) return null;
  const sessionId = normalizeText(source.session_id ?? source.sessionId);
  const runId = normalizeText(source.run_id ?? source.runId);
  const key = runId || sessionId;
  if (!key) return null;
  const status = normalizeSubagentStatus(source.status);
  const detail = asRecord(source.detail) || asRecord(source.metadata) || {};
  const parentTurnRef = normalizeText(
    source.parent_turn_ref ?? source.parentTurnRef ?? detail.parent_turn_ref ?? detail.parentTurnRef
  );
  const decodedParentTurn = decodeParentTurnRef(parentTurnRef);
  const parentUserRound =
    normalizeOptionalCount(
      source.parent_user_round ?? source.parentUserRound ?? detail.parent_user_round ?? detail.parentUserRound
    ) ?? decodedParentTurn?.userRound ?? null;
  const parentModelRound =
    normalizeOptionalCount(
      source.parent_model_round ?? source.parentModelRound ?? detail.parent_model_round ?? detail.parentModelRound
    ) ?? decodedParentTurn?.modelRound ?? null;
  const userMessage = normalizeText(
    source.user_message ??
      source.userMessage ??
      detail.user_message ??
      detail.userMessage
  );
  const assistantMessage = normalizeText(
    source.assistant_message ??
      source.assistantMessage ??
      detail.assistant_message ??
      detail.assistantMessage
  );
  const errorMessage = normalizeText(
    source.error_message ??
      source.errorMessage ??
      detail.error_message ??
      detail.errorMessage ??
      source.error
  );
  const summary = normalizeText(source.summary ?? assistantMessage ?? errorMessage);
  const title = normalizeText(source.title) || normalizeText(source.label) || sessionId || runId || 'subagent';
  const updatedTime = normalizeSubagentUpdatedTime(
    source.updated_time ??
      source.updatedTime ??
      source.finished_time ??
      source.finishedTime ??
      source.started_time ??
      source.startedTime
  );

  return {
    key,
    sessionId,
    runId,
    runKind: normalizeText(source.run_kind ?? source.runKind),
    requestedBy: normalizeText(source.requested_by ?? source.requestedBy),
    spawnedBy: normalizeText(source.spawned_by ?? source.spawnedBy),
    agentId: normalizeText(source.agent_id ?? source.agentId),
    title,
    label: normalizeText(source.label ?? source.spawn_label ?? source.spawnLabel),
    status,
    summary,
    userMessage,
    assistantMessage,
    errorMessage,
    updatedTime,
    terminal: Boolean(source.terminal),
    failed: Boolean(source.failed),
    depth: normalizeOptionalCount(source.depth ?? detail.depth),
    role: normalizeText(source.role ?? detail.role),
    controlScope: normalizeText(source.control_scope ?? source.controlScope ?? detail.control_scope),
    spawnMode: normalizeText(source.spawn_mode ?? source.spawnMode ?? detail.spawn_mode),
    strategy: normalizeText(source.strategy ?? detail.strategy),
    dispatchLabel: normalizeText(
      source.dispatch_label ?? source.dispatchLabel ?? detail.dispatch_label ?? source.label
    ),
    controllerSessionId: normalizeText(
      source.controller_session_id ?? source.controllerSessionId ?? detail.controller_session_id
    ),
    parentSessionId: normalizeText(source.parent_session_id ?? source.parentSessionId),
    parentTurnRef,
    parentUserRound,
    parentModelRound,
    workflowItems: []
  };
};

export const collectBeeroomHistoricalSubagentItems = (
  events: BeeroomSessionEventRecord[],
  options: { latestTurnOnly?: boolean } = {}
): BeeroomMissionSubagentItem[] => {
  const historical = events
    .filter((event) => {
      const name = resolveSessionEventName(event);
      return name === 'subagent_dispatch_item_update' || name === 'subagent_status';
    })
    .map((event) => {
      const payload = resolveSessionEventPayload(event);
      const normalized = normalizeBeeroomMissionSubagentItem(payload);
      if (!normalized) return null;
      if (Number(normalized.updatedTime || 0) > 0) {
        return normalized;
      }
      return {
        ...normalized,
        updatedTime: resolveBeeroomSessionEventTimestamp(event)
      };
    })
    .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item));

  if (!historical.length) {
    return [];
  }

  let filtered = historical;
  if (options.latestTurnOnly) {
    let latestTurn: BeeroomParentTurnKey | null = null;
    filtered.forEach((item) => {
      const turn = resolveParentTurnKey(item);
      if (!turn) return;
      if (!latestTurn || compareParentTurnKeys(turn, latestTurn) > 0) {
        latestTurn = turn;
      }
    });
    if (latestTurn) {
      filtered = filtered.filter((item) => {
        const turn = resolveParentTurnKey(item);
        return Boolean(turn) && compareParentTurnKeys(turn, latestTurn as BeeroomParentTurnKey) === 0;
      });
    }
  }

  return mergeBeeroomMissionSubagentItems(filtered);
};
