<template>
  <el-dialog
    v-model="dialogVisible"
    class="messenger-dialog messenger-timeline-detail-dialog"
    :title="dialogTitle"
    width="760px"
    destroy-on-close
  >
    <div v-if="loading" class="messenger-timeline-detail-loading">
      {{ t('common.loading') }}
    </div>
    <div v-else class="messenger-timeline-detail-panel">
      <div class="messenger-timeline-detail-toolbar">
        <div class="messenger-timeline-detail-meta">{{ detailMeta }}</div>
        <button
          class="messenger-inline-btn messenger-timeline-detail-export-btn"
          type="button"
          :disabled="!sessionDetail"
          :title="t('messenger.timeline.detail.export')"
          :aria-label="t('messenger.timeline.detail.export')"
          @click="exportTimelineDetail"
        >
          <i class="fa-solid fa-download" aria-hidden="true"></i>
          <span>{{ t('messenger.timeline.detail.export') }}</span>
        </button>
      </div>

      <div class="messenger-timeline-detail-section">
        <div class="messenger-timeline-detail-label-row">
          <label class="messenger-timeline-detail-label">{{ t('messenger.timeline.detail.question') }}</label>
          <div v-if="roundOptions.length" class="messenger-timeline-detail-round-picker">
            <span class="messenger-timeline-detail-round-picker-label">
              {{ t('messenger.timeline.detail.userRound') }}
            </span>
            <select v-model.number="selectedRound" class="messenger-timeline-detail-round-select">
              <option v-for="item in roundOptions" :key="item.value" :value="item.value">
                {{ item.label }}
              </option>
            </select>
          </div>
        </div>
        <div class="messenger-timeline-detail-question">{{ detailQuestion }}</div>
      </div>

      <div class="messenger-timeline-detail-section messenger-timeline-detail-section-events">
        <label class="messenger-timeline-detail-label">{{ t('messenger.timeline.detail.events') }}</label>
        <div class="messenger-timeline-detail-filters">
          <select v-model="eventTypeFilter" class="messenger-timeline-detail-filter-select">
            <option value="">{{ t('messenger.timeline.detail.filterAllTypes') }}</option>
            <option v-for="item in eventTypeOptions" :key="item" :value="item">
              {{ item }}
            </option>
          </select>
          <input
            v-model.trim="keywordFilter"
            class="messenger-timeline-detail-filter-input"
            type="text"
            :placeholder="t('messenger.timeline.detail.filterKeyword')"
          />
          <div class="messenger-timeline-detail-filter-stats">{{ filterStats }}</div>
        </div>

        <div v-if="!filteredEvents.length" class="messenger-timeline-detail-empty">
          {{ t('messenger.timeline.detail.noEvents') }}
        </div>
        <div v-else ref="eventsContainerRef" class="messenger-timeline-detail-events">
          <details
            v-for="item in filteredEvents"
            :key="item.key"
            class="messenger-timeline-detail-event-item"
            :data-round="item.round"
          >
            <summary class="messenger-timeline-detail-event-summary">
              <span class="messenger-timeline-detail-event-time">[{{ item.timestampLabel }}]</span>
              <span class="messenger-timeline-detail-event-type">#{{ item.order }} {{ item.eventType }}</span>
              <span class="messenger-timeline-detail-event-title">{{ item.title }}</span>
              <span class="messenger-timeline-detail-event-round">
                {{ t('messenger.timeline.detail.round', { round: item.round }) }}
              </span>
            </summary>
            <pre class="messenger-timeline-detail-event-raw">{{ item.raw }}</pre>
          </details>
        </div>
      </div>
    </div>

    <template #footer>
      <el-button @click="dialogVisible = false">{{ t('common.close') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { getSession as getChatSessionApi, getSessionEvents as getChatSessionEventsApi } from '@/api/chat';
import { getCurrentLanguage, useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

type TimelineDetailRoundEvent = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
  timestamp?: unknown;
};

type TimelineDetailRound = {
  user_round?: unknown;
  round?: unknown;
  events?: TimelineDetailRoundEvent[];
};

type TimelineDetailEventItem = {
  key: string;
  order: number;
  round: number;
  eventType: string;
  timestampLabel: string;
  title: string;
  raw: string;
  searchText: string;
  rawEvent: TimelineDetailRoundEvent;
};

type TimelineRoundOption = {
  value: number;
  label: string;
};

type TimelineDetailSession = {
  id: string;
  title: string;
  agentId: string;
  agentName: string;
  createdAt: unknown;
  updatedAt: unknown;
  lastMessageAt: unknown;
  messageCount: number;
  historyIncomplete: boolean;
  messages: Record<string, unknown>[];
};

type TimelineExportLine = Record<string, unknown>;

const TIMELINE_DETAIL_EVENT_TITLE_MAX_LENGTH = 120;

const props = defineProps<{
  visible: boolean;
  sessionId: string;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const dialogVisible = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const loading = ref(false);
const sessionDetail = ref<TimelineDetailSession | null>(null);
const rounds = ref<TimelineDetailRound[]>([]);
const running = ref(false);
const lastEventId = ref(0);
const eventTypeFilter = ref('');
const keywordFilter = ref('');
const selectedRound = ref(0);
const eventsContainerRef = ref<HTMLElement | null>(null);

let requestToken = 0;

const resetFilters = () => {
  eventTypeFilter.value = '';
  keywordFilter.value = '';
};

const resetDetailState = () => {
  loading.value = false;
  sessionDetail.value = null;
  rounds.value = [];
  running.value = false;
  lastEventId.value = 0;
  selectedRound.value = 0;
  resetFilters();
};

const normalizeTimestamp = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? 0 : value.getTime();
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const date = new Date(text);
  return Number.isNaN(date.getTime()) ? 0 : date.getTime();
};

const unwrapEventData = (payload: unknown): unknown => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    return payload;
  }
  const source = payload as Record<string, unknown>;
  const hasSessionId = typeof source.session_id === 'string' && source.session_id.trim().length > 0;
  const hasTimestamp = typeof source.timestamp === 'string' && source.timestamp.trim().length > 0;
  const inner = source.data;
  if (hasSessionId && hasTimestamp && inner && typeof inner === 'object') {
    return inner;
  }
  return payload;
};

const fallbackEventDataText = (value: unknown): string => {
  if (value === null || value === undefined) {
    return '';
  }
  if (typeof value === 'string') {
    return value;
  }
  if (Array.isArray(value)) {
    return `[array(${value.length})]`;
  }
  if (typeof value === 'object') {
    try {
      const source = value as Record<string, unknown>;
      const keys = Object.keys(source);
      if (keys.length === 0) {
        return '{}';
      }
      const preview: Record<string, unknown> = {};
      keys.slice(0, 8).forEach((key) => {
        const field = source[key];
        if (field === null || field === undefined) {
          preview[key] = field;
          return;
        }
        if (typeof field === 'string' || typeof field === 'number' || typeof field === 'boolean') {
          preview[key] = field;
          return;
        }
        if (typeof field === 'bigint') {
          preview[key] = field.toString();
          return;
        }
        if (Array.isArray(field)) {
          preview[key] = `[array(${field.length})]`;
          return;
        }
        preview[key] = '[object]';
      });
      if (keys.length > 8) {
        preview.__extra_keys__ = keys.length - 8;
      }
      const text = JSON.stringify(preview);
      return typeof text === 'string' ? text : '{...}';
    } catch {
      return '{...}';
    }
  }
  return String(value);
};

// Keep timeline rendering readable even if payload has circular refs/BigInt.
const safeStringifyEventData = (value: unknown, pretty = false): string => {
  const seen = new WeakSet<object>();
  try {
    const text = JSON.stringify(
      value ?? null,
      (_key, current: unknown) => {
        if (typeof current === 'bigint') {
          return current.toString();
        }
        if (typeof current === 'function') {
          return `[Function ${current.name || 'anonymous'}]`;
        }
        if (typeof current === 'symbol') {
          return String(current);
        }
        if (current instanceof Error) {
          return {
            name: current.name,
            message: current.message,
            stack: current.stack
          };
        }
        if (current && typeof current === 'object') {
          const objectValue = current as object;
          if (seen.has(objectValue)) {
            return '[Circular]';
          }
          seen.add(objectValue);
          if (current instanceof Map) {
            return Object.fromEntries(current.entries());
          }
          if (current instanceof Set) {
            return Array.from(current.values());
          }
        }
        return current;
      },
      pretty ? 2 : undefined
    );
    return typeof text === 'string' ? text : fallbackEventDataText(value);
  } catch {
    return fallbackEventDataText(value);
  }
};

const stringifyEventData = (payload: unknown, pretty = false): string => {
  const resolved = unwrapEventData(payload);
  if (typeof resolved === 'string') {
    return resolved;
  }
  return safeStringifyEventData(resolved, pretty);
};

const truncateText = (value: unknown): string => {
  const text = String(value || '')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) {
    return '';
  }
  if (text.length <= TIMELINE_DETAIL_EVENT_TITLE_MAX_LENGTH) {
    return text;
  }
  return `${text.slice(0, TIMELINE_DETAIL_EVENT_TITLE_MAX_LENGTH)}...`;
};

// Extract readable scalar text from nested summary/error objects.
const extractEventTitleText = (value: unknown, depth = 0): string => {
  if (value === null || value === undefined || depth > 3) {
    return '';
  }
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    typeof value === 'bigint'
  ) {
    return String(value).trim();
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const text = extractEventTitleText(item, depth + 1);
      if (text) {
        return text;
      }
    }
    return '';
  }
  if (typeof value !== 'object') {
    return '';
  }
  const source = value as Record<string, unknown>;
  for (const key of [
    'summary',
    'message',
    'question',
    'reason',
    'error',
    'tool',
    'tool_name',
    'toolName',
    'name',
    'model',
    'model_name',
    'stage',
    'status',
    'code',
    'title'
  ]) {
    const text = extractEventTitleText(source[key], depth + 1);
    if (text) {
      return text;
    }
  }
  return '';
};

const resolveEventTitle = (eventType: string, payload: unknown): string => {
  const normalizedType = String(eventType || '')
    .trim()
    .toLowerCase();
  const data = unwrapEventData(payload);
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const source = data as Record<string, unknown>;
    const candidates =
      normalizedType === 'user_input'
        ? [
            source.message,
            source.question,
            source.input,
            source.content,
            source.summary,
            source.error,
            source.reason,
            source.tool,
            source.tool_name,
            source.toolName,
            source.name,
            source.model,
            source.model_name,
            source.stage,
            source.status
          ]
        : [
            source.summary,
            source.message,
            source.question,
            source.error,
            source.reason,
            source.tool,
            source.tool_name,
            source.toolName,
            source.name,
            source.model,
            source.model_name,
            source.stage,
            source.status
          ];
    for (const candidate of candidates) {
      const title = truncateText(extractEventTitleText(candidate));
      if (title) {
        return title;
      }
    }
  }
  if (typeof data === 'string') {
    const title = truncateText(data);
    if (title) {
      return title;
    }
  }
  const fallback = truncateText(stringifyEventData(data, false));
  return fallback || '-';
};

const formatEventTimestamp = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) {
    return '-';
  }
  const parsed = new Date(text);
  if (Number.isNaN(parsed.getTime())) {
    return text;
  }
  return parsed.toLocaleString(getCurrentLanguage());
};

const normalizeRoundIndex = (value: unknown, fallback: number): number => {
  const parsed = Number.parseInt(String(value ?? fallback), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return Math.max(1, fallback);
  }
  return parsed;
};

const normalizeRounds = (value: unknown): TimelineDetailRound[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .map((item) => {
      const source =
        item && typeof item === 'object' && !Array.isArray(item)
          ? (item as Record<string, unknown>)
          : {};
      const events = Array.isArray(source.events)
        ? source.events
            .filter((event) => event && typeof event === 'object' && !Array.isArray(event))
            .map((event) => event as TimelineDetailRoundEvent)
        : [];
      return {
        user_round: source.user_round,
        round: source.round,
        events
      } as TimelineDetailRound;
    })
    .filter((item) => Array.isArray(item.events) && item.events.length > 0);
};

const isDefaultHiddenEventType = (eventType: string): boolean => {
  const normalized = String(eventType || '')
    .trim()
    .toLowerCase();
  return normalized.endsWith('_delta') || normalized === 'context_usage';
};

const normalizeSession = (sessionId: string, value: unknown): TimelineDetailSession => {
  const source =
    value && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const messages = Array.isArray(source.messages)
    ? source.messages
        .filter((item) => item && typeof item === 'object' && !Array.isArray(item))
        .map((item) => item as Record<string, unknown>)
    : [];
  return {
    id: String(source.id || sessionId),
    title: String(source.title || ''),
    agentId: String(source.agent_id || ''),
    agentName: String(source.agent_name || ''),
    createdAt: source.created_at,
    updatedAt: source.updated_at,
    lastMessageAt: source.last_message_at,
    messageCount: messages.length,
    historyIncomplete: Boolean(source.history_incomplete),
    messages
  };
};

const resolveQuestion = (session: TimelineDetailSession | null): string => {
  if (!session) {
    return t('messenger.timeline.detail.questionEmpty');
  }
  for (const message of session.messages) {
    if (String(message?.role || '').trim() !== 'user') continue;
    const content = String(message?.content || '').trim();
    if (content) {
      return content;
    }
  }
  const fallback = String(session.title || '').trim();
  if (fallback) {
    return fallback;
  }
  return t('messenger.timeline.detail.questionEmpty');
};

const resolveQuestionFromEventPayload = (payload: unknown): string => {
  const data = unwrapEventData(payload);
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const source = data as Record<string, unknown>;
    const candidate =
      source.message ||
      source.question ||
      source.input ||
      source.content ||
      source.prompt ||
      source.text;
    const text = String(candidate || '').trim();
    if (text) {
      return text;
    }
  }
  if (typeof data === 'string') {
    return data.trim();
  }
  return '';
};

const formatMetaTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) {
    return '-';
  }
  return new Date(ts).toLocaleString(getCurrentLanguage());
};

const events = computed<TimelineDetailEventItem[]>(() => {
  const result: TimelineDetailEventItem[] = [];
  let order = 0;
  rounds.value.forEach((round, roundIndex) => {
    const roundIndexValue = normalizeRoundIndex(round?.user_round ?? round?.round, roundIndex + 1);
    const eventList = Array.isArray(round?.events) ? round.events : [];
    eventList.forEach((event, eventIndex) => {
      order += 1;
      const eventType = String(event?.event || event?.type || 'unknown').trim() || 'unknown';
      const raw = stringifyEventData(event?.data, true);
      const title = resolveEventTitle(eventType, event?.data);
      const timestampLabel = formatEventTimestamp(event?.timestamp);
      const searchText = `${eventType} ${title} ${stringifyEventData(event?.data, false)}`.toLowerCase();
      result.push({
        key: `${roundIndexValue}-${eventType}-${order}-${eventIndex}`,
        order,
        round: roundIndexValue,
        eventType,
        timestampLabel,
        title,
        raw,
        searchText,
        rawEvent: event
      });
    });
  });
  return result;
});

const roundOptions = computed<TimelineRoundOption[]>(() => {
  const values = new Set<number>();
  rounds.value.forEach((round, roundIndex) => {
    values.add(normalizeRoundIndex(round?.user_round ?? round?.round, roundIndex + 1));
  });
  return Array.from(values)
    .sort((left, right) => left - right)
    .map((value) => ({
      value,
      label: t('messenger.timeline.detail.round', { round: value })
    }));
});

const roundQuestionMap = computed(() => {
  const result = new Map<number, string>();
  rounds.value.forEach((round, roundIndex) => {
    const roundIndexValue = normalizeRoundIndex(round?.user_round ?? round?.round, roundIndex + 1);
    const eventList = Array.isArray(round?.events) ? round.events : [];
    let fallbackQuestion = '';
    for (const event of eventList) {
      const eventType = String(event?.event || event?.type || '')
        .trim()
        .toLowerCase();
      const question = resolveQuestionFromEventPayload(event?.data);
      if (!question) {
        continue;
      }
      if (eventType === 'user_input' || eventType === 'received') {
        result.set(roundIndexValue, question);
        fallbackQuestion = '';
        break;
      }
      if (!fallbackQuestion) {
        fallbackQuestion = question;
      }
    }
    if (!result.has(roundIndexValue) && fallbackQuestion) {
      result.set(roundIndexValue, fallbackQuestion);
    }
  });

  const userMessages = (sessionDetail.value?.messages || [])
    .filter((item) => String(item?.role || '').trim() === 'user')
    .map((item) => String(item?.content || '').trim())
    .filter((item) => item.length > 0);
  roundOptions.value.forEach((item, index) => {
    if (!result.has(item.value) && userMessages[index]) {
      result.set(item.value, userMessages[index]);
    }
  });
  return result;
});

const eventTypeOptions = computed(() => {
  const types = new Set<string>();
  events.value.forEach((item) => {
    if (item.eventType) {
      types.add(item.eventType);
    }
  });
  return Array.from(types).sort((left, right) => left.localeCompare(right));
});

const filteredEvents = computed(() => {
  const selectedType = String(eventTypeFilter.value || '').trim();
  const keyword = String(keywordFilter.value || '')
    .trim()
    .toLowerCase();
  return events.value.filter((item) => {
    if (selectedType && item.eventType !== selectedType) {
      return false;
    }
    if (!selectedType && isDefaultHiddenEventType(item.eventType)) {
      return false;
    }
    if (!keyword) {
      return true;
    }
    return item.searchText.includes(keyword);
  });
});

const exportEvents = computed(() => {
  const selectedType = String(eventTypeFilter.value || '').trim();
  const keyword = String(keywordFilter.value || '').trim();
  if (selectedType || keyword) {
    return filteredEvents.value;
  }
  return events.value.filter((item) => !isDefaultHiddenEventType(item.eventType));
});

const dialogTitle = computed(() => {
  const sessionId = String(sessionDetail.value?.id || props.sessionId || '').trim();
  return sessionId
    ? t('messenger.timeline.detail.titleWithId', { id: sessionId })
    : t('messenger.timeline.detail.title');
});

const detailQuestion = computed(() => {
  if (selectedRound.value > 0) {
    const question = String(roundQuestionMap.value.get(selectedRound.value) || '').trim();
    if (question) {
      return question;
    }
  }
  return resolveQuestion(sessionDetail.value);
});

const resolveSessionAgentDisplay = (session: TimelineDetailSession): string => {
  const name = String(session.agentName || '').trim();
  const id = String(session.agentId || '').trim();
  if (name && id && name !== id) {
    return `${name} (${id})`;
  }
  if (name) {
    return name;
  }
  if (id) {
    return id;
  }
  return '-';
};

const detailMeta = computed(() => {
  const session = sessionDetail.value;
  if (!session) {
    return '';
  }
  const parts = [
    t('messenger.timeline.detail.metaSessionId', { id: session.id || '-' }),
    t('messenger.timeline.detail.metaAgent', { agent: resolveSessionAgentDisplay(session) }),
    t('messenger.timeline.detail.metaCreatedAt', { time: formatMetaTime(session.createdAt) }),
    t('messenger.timeline.detail.metaUpdatedAt', {
      time: formatMetaTime(session.updatedAt || session.lastMessageAt)
    }),
    t('messenger.timeline.detail.metaMessageCount', { count: session.messageCount }),
    t('messenger.timeline.detail.metaRoundCount', { count: rounds.value.length }),
    t('messenger.timeline.detail.metaEventCount', { count: events.value.length })
  ];
  if (running.value) {
    parts.push(t('messenger.timeline.detail.running'));
  }
  if (session.historyIncomplete) {
    parts.push(t('messenger.timeline.detail.historyIncomplete'));
  }
  if (lastEventId.value > 0) {
    parts.push(t('messenger.timeline.detail.metaLastEventId', { id: lastEventId.value }));
  }
  return parts.join(' · ');
});

const filterStats = computed(() =>
  t('messenger.timeline.detail.filterStats', {
    visible: filteredEvents.value.length,
    total: events.value.length
  })
);

const sanitizeFilenamePart = (value: unknown, fallback: string): string => {
  const text = String(value || '').trim();
  const safe = text.replace(/[\\/:*?"<>|]+/g, '_');
  return safe || fallback;
};

const buildExportFilename = (sessionId: string): string => {
  const safeSessionId = sanitizeFilenamePart(sessionId, 'session');
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  return `timeline-detail-${safeSessionId}-${timestamp}.jsonl`;
};

const buildEventSummary = (eventType: string, payload: unknown): Record<string, unknown> => {
  const data = unwrapEventData(payload);
  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    return {};
  }
  const source = data as Record<string, unknown>;
  const summary: Record<string, unknown> = {};
  for (const key of [
    'stage',
    'summary',
    'message',
    'question',
    'trace_id',
    'model_round',
    'tool',
    'tool_name',
    'stop_reason',
    'ok'
  ]) {
    const value = source[key];
    if (value !== undefined && value !== null && String(value).trim() !== '') {
      summary[key] = value;
    }
  }
  if (eventType === 'tool_call' && source.args && typeof source.args === 'object') {
    summary.args = source.args;
  }
  if (eventType === 'tool_result' && source.meta && typeof source.meta === 'object') {
    summary.meta = source.meta;
  }
  if (eventType === 'llm_output' && source.usage && typeof source.usage === 'object') {
    summary.usage = source.usage;
  }
  return summary;
};

const normalizeExportTimestamp = (value: unknown): string => {
  const text = String(value || '').trim();
  if (text) {
    const parsed = new Date(text);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toISOString();
    }
  }
  const timestampMs = normalizeTimestamp(value);
  if (timestampMs > 0) {
    return new Date(timestampMs).toISOString();
  }
  return '';
};

const buildTimelineExportLines = (): TimelineExportLine[] => {
  const output: TimelineExportLine[] = [];
  const uniqueEventTypes = new Set<string>();
  exportEvents.value.forEach((item, index) => {
    const event = item.rawEvent;
    const eventType = item.eventType || 'unknown';
    uniqueEventTypes.add(eventType);
    output.push({
      record_type: 'event',
      order: index + 1,
      round: item.round,
      event: eventType,
      timestamp: normalizeExportTimestamp(event?.timestamp),
      timestamp_ms: normalizeTimestamp(event?.timestamp),
      title: item.title,
      summary: buildEventSummary(eventType, event?.data),
      data: unwrapEventData(event?.data)
    });
  });

  const session = sessionDetail.value;
  output.unshift({
    record_type: 'meta',
    export_schema_version: 3,
    export_format: 'jsonl',
    exported_at: new Date().toISOString(),
    summary: {
      question: detailQuestion.value,
      round_count: rounds.value.length,
      event_count: output.length,
      event_types: Array.from(uniqueEventTypes).sort((left, right) => left.localeCompare(right)),
      running: running.value,
      last_event_id: lastEventId.value
    },
    session
  });

  return output;
};

const saveBlobUrl = (url: string, filename: string) => {
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.style.display = 'none';
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
};

const loadTimelineDetail = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) {
    return;
  }
  const currentToken = ++requestToken;
  loading.value = true;
  sessionDetail.value = null;
  rounds.value = [];
  running.value = false;
  lastEventId.value = 0;
  resetFilters();
  try {
    const [sessionRes, eventsRes] = await Promise.all([
      getChatSessionApi(targetId),
      getChatSessionEventsApi(targetId).catch(() => null)
    ]);
    if (currentToken !== requestToken) {
      return;
    }
    const sessionData = (sessionRes?.data as { data?: unknown } | undefined)?.data;
    sessionDetail.value = normalizeSession(targetId, sessionData);
    const eventPayload = (eventsRes?.data as { data?: Record<string, unknown> } | undefined)?.data;
    rounds.value = normalizeRounds(eventPayload?.rounds);
    running.value = Boolean(eventPayload?.running);
    const parsedLastEventId = Number.parseInt(String(eventPayload?.last_event_id ?? 0), 10);
    lastEventId.value = Number.isFinite(parsedLastEventId) && parsedLastEventId > 0 ? parsedLastEventId : 0;
  } catch (error) {
    if (currentToken !== requestToken) {
      return;
    }
    dialogVisible.value = false;
    showApiError(error, t('messenger.timeline.detail.loadFailed'));
  } finally {
    if (currentToken === requestToken) {
      loading.value = false;
    }
  }
};

const exportTimelineDetail = () => {
  const session = sessionDetail.value;
  if (!session) {
    return;
  }
  try {
    const lines = buildTimelineExportLines();
    const payload = lines.map((item) => JSON.stringify(item)).join('\n');
    const blob = new Blob([payload], {
      type: 'application/x-ndjson;charset=utf-8'
    });
    const objectUrl = URL.createObjectURL(blob);
    saveBlobUrl(objectUrl, buildExportFilename(session.id));
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
    ElMessage.success(t('messenger.timeline.detail.exported'));
  } catch (error) {
    const detail = String((error as { message?: string })?.message || t('common.requestFailed'));
    ElMessage.error(t('messenger.timeline.detail.exportFailed', { message: detail }));
  }
};

const scrollToSelectedRound = () => {
  if (!selectedRound.value) {
    return;
  }
  const container = eventsContainerRef.value;
  if (!container) {
    return;
  }
  const selector = `.messenger-timeline-detail-event-item[data-round="${selectedRound.value}"]`;
  const target = container.querySelector<HTMLElement>(selector);
  if (!target) {
    return;
  }
  container
    .querySelectorAll<HTMLElement>('.messenger-timeline-detail-event-item.is-round-target')
    .forEach((node) => node.classList.remove('is-round-target'));
  target.classList.add('is-round-target');
  target.scrollIntoView({ block: 'start', behavior: 'smooth' });
  window.setTimeout(() => {
    target.classList.remove('is-round-target');
  }, 1400);
};

watch(
  [() => dialogVisible.value, () => props.sessionId],
  ([visible, sessionId]) => {
    const targetId = String(sessionId || '').trim();
    if (!visible || !targetId) {
      return;
    }
    void loadTimelineDetail(targetId);
  },
  { immediate: true }
);

watch(
  roundOptions,
  (options) => {
    if (!options.length) {
      selectedRound.value = 0;
      return;
    }
    const selectedValid = options.some((item) => item.value === selectedRound.value);
    if (!selectedValid) {
      selectedRound.value = options[options.length - 1]?.value || 0;
    }
  },
  { immediate: true }
);

watch(
  [selectedRound, () => filteredEvents.value.length],
  () => {
    void nextTick(() => {
      scrollToSelectedRound();
    });
  },
  { flush: 'post' }
);

watch(
  () => dialogVisible.value,
  (visible) => {
    if (visible) {
      return;
    }
    requestToken += 1;
    resetDetailState();
  }
);
</script>
