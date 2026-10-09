<template>
  <div class="thread-trajectory">
    <div class="thread-trajectory-toolbar" role="toolbar" :aria-label="t('messenger.trajectory.title')">
      <div class="thread-trajectory-toolbar-inner">
        <div class="thread-trajectory-toolbar-head">
          <button
            class="tt-icon-btn"
            type="button"
            :title="t('messenger.trajectory.back')"
            :aria-label="t('messenger.trajectory.back')"
            @click="goBack"
          >
            <i class="fa-solid fa-arrow-left" aria-hidden="true"></i>
          </button>
          <span class="thread-trajectory-title">{{ t('messenger.trajectory.title') }}</span>
          <span v-if="sessionId" class="thread-trajectory-session" :title="sessionId">{{ sessionId }}</span>
        </div>
        <div class="thread-trajectory-toolbar-actions">
          <div class="tt-switch" role="group">
            <button
              class="tt-switch-btn"
              :class="{ 'is-active': !durationMode }"
              type="button"
              @click="durationMode = false"
            >
              {{ t('messenger.trajectory.toolbar.sequence') }}
            </button>
            <button
              class="tt-switch-btn"
              :class="{ 'is-active': durationMode }"
              type="button"
              @click="durationMode = true"
            >
              {{ t('messenger.trajectory.toolbar.duration') }}
            </button>
          </div>
          <button class="tt-text-btn" type="button" @click="toggleAllTurns">
            {{ allTurnsCollapsed ? t('messenger.trajectory.toolbar.expandTurns') : t('messenger.trajectory.toolbar.collapseTurns') }}
          </button>
          <button class="tt-text-btn" type="button" @click="toggleAllCalls">
            {{ collapseCalls ? t('messenger.trajectory.toolbar.expandCalls') : t('messenger.trajectory.toolbar.collapseCalls') }}
          </button>
          <label class="tt-search">
            <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
            <input
              v-model="searchQuery"
              type="search"
              :placeholder="t('messenger.trajectory.toolbar.search')"
            />
          </label>
        </div>
      </div>
    </div>

    <div class="thread-trajectory-timeline">
      <div class="tt-timeline-labels">
        <span v-for="lane in TIMELINE_LANES" :key="lane.labelKey" :style="{ top: `${lane.top}px` }">
          {{ t(lane.labelKey) }}
        </span>
      </div>
      <div
        ref="trackRef"
        class="tt-timeline-track"
        @pointerdown="onTrackPointerDown"
        @pointermove="onTrackPointerMove"
        @pointerleave="onTrackPointerLeave"
        @dblclick="clearSelection"
      >
        <div class="tt-timeline-lanes">
          <span
            v-for="span in timelineSpans"
            :key="span.recordId"
            class="tt-timeline-span"
            :class="spanClasses(span)"
            :style="spanStyle(span)"
            :title="spanTooltip(span)"
            @click.stop="focusSpan(span)"
          ></span>
        </div>
        <div class="tt-timeline-boundaries">
          <span
            v-for="boundary in timelineBoundaries"
            :key="boundary.turnKey"
            class="tt-timeline-boundary"
            :style="{ left: `${boundary.left}%` }"
          ></span>
        </div>
        <div v-if="hoverLine !== null" class="tt-timeline-hoverline" :style="{ left: `${hoverLine}%` }"></div>
      </div>
    </div>

    <div class="thread-trajectory-body">
      <div class="thread-trajectory-ledger">
        <div v-if="loading" class="thread-trajectory-state">{{ t('common.loading') }}</div>
        <div v-else-if="loadError" class="thread-trajectory-state is-error">{{ loadError }}</div>
        <div v-else-if="!visibleTurns.length" class="thread-trajectory-state">
          {{ t('messenger.trajectory.empty') }}
        </div>
        <table v-else class="thread-trajectory-table">
          <colgroup>
            <col class="thread-trajectory-col-event" />
            <col class="thread-trajectory-col-content" />
          </colgroup>
          <tbody>
            <template v-for="turn in visibleTurns" :key="turn.key">
              <tr class="thread-trajectory-turn-row">
                <th colspan="2">
                  <div class="thread-trajectory-turn-inner">
                    <button class="thread-trajectory-turn-toggle" type="button" @click="toggleTurn(turn.key)">
                      <i
                        :class="turn.collapsed ? 'fa-solid fa-caret-right' : 'fa-solid fa-caret-down'"
                        aria-hidden="true"
                      ></i>
                      <span>{{ turn.label }}</span>
                    </button>
                    <div class="thread-trajectory-turn-columns">
                      <span class="thread-trajectory-turn-column">{{ formatNumber(turn.usage.input) }}</span>
                      <span class="thread-trajectory-turn-column">{{ formatNumber(turn.usage.output) }}</span>
                      <span class="thread-trajectory-turn-column">{{ formatNumber(turn.usage.reasoning) }}</span>
                      <span class="thread-trajectory-turn-column">{{ formatSeconds(turn.timeSeconds) }}</span>
                    </div>
                  </div>
                </th>
              </tr>
              <template v-if="!turn.collapsed">
                <template v-for="group in turn.groups" :key="group.key">
                  <tr class="thread-trajectory-group-row">
                    <td colspan="2" class="thread-trajectory-group-cell">
                      <span class="thread-trajectory-group-title">{{ group.title }}</span>
                      <span v-if="group.description" class="thread-trajectory-group-desc">{{ group.description }}</span>
                    </td>
                  </tr>
                  <tr
                    v-for="record in group.records"
                    :key="record.recordId"
                    class="thread-trajectory-record-row"
                    :class="recordClasses(record)"
                    @click="selectRecord(record.recordId)"
                  >
                    <td class="thread-trajectory-event-cell">
                      <div class="thread-trajectory-event">
                        <span class="thread-trajectory-index">#{{ record.index }}</span>
                        <span class="thread-trajectory-kind" :class="`is-${record.kind}`">
                          <i :class="kindIcon(record.kind)" aria-hidden="true"></i>
                          <span>{{ t(kindLabelKey(record.kind)) }}</span>
                        </span>
                      </div>
                    </td>
                    <td class="thread-trajectory-content-cell">
                      <div class="thread-trajectory-content">
                        <span class="thread-trajectory-text" :class="{ 'is-error': record.isError }">
                          {{ record.text || t('messenger.trajectory.emptyValue') }}
                        </span>
                        <span v-if="record.resultPreview" class="thread-trajectory-result" :title="record.resultPreview">
                          {{ record.resultPreview }}
                        </span>
                      </div>
                    </td>
                  </tr>
                  <tr v-if="collapseCalls && group.toolCount > 0" class="thread-trajectory-group-row">
                    <td colspan="2" class="thread-trajectory-collapsed-cell">
                      {{ t('messenger.trajectory.collapsed.callsSummary', { count: group.toolCount }) }}
                      <template v-if="group.description"> · {{ group.description }}</template>
                    </td>
                  </tr>
                </template>
              </template>
            </template>
          </tbody>
        </table>
        <div v-if="hasMore && !loading" class="thread-trajectory-more">
          <button class="tt-text-btn" type="button" :disabled="loadingMore" @click="loadEarlier">
            {{ loadingMore ? t('common.loading') : t('messenger.trajectory.loadEarlier') }}
          </button>
        </div>
      </div>

      <aside v-if="selectedRecord" class="thread-trajectory-inspector" :style="{ width: `${INSPECTOR_WIDTH}px` }">
        <div class="tt-inspector-tabs">
          <button
            v-for="tab in INSPECTOR_TABS"
            :key="tab.key"
            class="tt-inspector-tab"
            :class="{ 'is-active': activeTab === tab.key }"
            type="button"
            @click="activeTab = tab.key"
          >
            {{ t(tab.labelKey) }}
          </button>
          <span class="tt-inspector-spacer"></span>
          <button class="tt-icon-btn" type="button" :title="t('messenger.trajectory.inspector.close')" @click="clearSelection">
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
        <div class="tt-inspector-body">
          <template v-if="activeTab === 'overview'">
            <div v-for="row in overviewRows" :key="row.label" class="tt-inspector-row">
              <span class="tt-inspector-label">{{ row.label }}</span>
              <span class="tt-inspector-value">{{ row.value }}</span>
            </div>
          </template>
          <template v-else-if="activeTab === 'timing'">
            <div v-for="row in timingRows" :key="row.label" class="tt-inspector-row">
              <span class="tt-inspector-label">{{ row.label }}</span>
              <span class="tt-inspector-value">{{ row.value }}</span>
            </div>
          </template>
          <template v-else-if="activeTab === 'usage'">
            <div v-for="row in usageRows" :key="row.label" class="tt-inspector-row">
              <span class="tt-inspector-label">{{ row.label }}</span>
              <span class="tt-inspector-value">{{ row.value }}</span>
            </div>
          </template>
          <template v-else>
            <template v-if="selectedRecord.inputDetail">
              <div class="tt-inspector-section">{{ t('messenger.trajectory.payload.input') }}</div>
              <pre class="tt-inspector-pre">{{ selectedRecord.inputDetail }}</pre>
            </template>
            <template v-if="selectedRecord.thinkingDetail">
              <div class="tt-inspector-section">{{ t('messenger.trajectory.payload.thinking') }}</div>
              <pre class="tt-inspector-pre">{{ selectedRecord.thinkingDetail }}</pre>
            </template>
            <template v-if="selectedRecord.outputDetail">
              <div class="tt-inspector-section">{{ t('messenger.trajectory.payload.output') }}</div>
              <pre class="tt-inspector-pre">{{ selectedRecord.outputDetail }}</pre>
            </template>
            <div v-if="!selectedRecord.inputDetail && !selectedRecord.thinkingDetail && !selectedRecord.outputDetail" class="tt-inspector-row">
              <span class="tt-inspector-value">{{ t('messenger.trajectory.emptyValue') }}</span>
            </div>
          </template>
        </div>
      </aside>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { getThreadLogTurns } from '@/api/chat';
import { useI18n } from '@/i18n';

type Json = Record<string, unknown>;

type TrajKind = 'system' | 'user' | 'context' | 'compacted' | 'message' | 'tool' | 'subtool';
type InspectorTab = 'overview' | 'timing' | 'usage' | 'payload';

interface TrajUsage {
  input: number;
  cacheRead: number;
  cacheWrite: number;
  output: number;
  reasoning: number;
}

interface TrajCell {
  index: number;
  recordId: string;
  kind: TrajKind;
  text: string;
  turn: number | null;
  step: number | null;
  startedAt: number | null;
  completedAt: number | null;
  timeSeconds: number | null;
  ttftMs: number | null;
  decodeSpeed: number | null;
  isError: boolean;
  isFirstOfTurn: boolean;
  toolName: string;
  callId: string;
  status: string;
  inputDetail: string;
  outputDetail: string;
  thinkingDetail: string;
  resultPreview: string;
  usage: TrajUsage | null;
  searchText: string;
}

interface TrajGroup {
  key: string;
  title: string;
  description: string;
  toolCount: number;
  records: TrajCell[];
}

interface TrajTurn {
  key: string;
  label: string;
  collapsed: boolean;
  usage: TrajUsage;
  timeSeconds: number | null;
  recordCount: number;
  groups: TrajGroup[];
}

interface TimelineSpan {
  recordId: string;
  index: number;
  kind: TrajKind;
  lane: number;
  left: number;
  width: number;
  isError: boolean;
  equalDuration: boolean;
  durationSeconds: number | null;
  startedAt: number | null;
}

const TURNS_LIMIT = 100;
const HISTORY_TURNS_PAGE = 50;
const INSPECTOR_WIDTH = 360;
const TIMELINE_LANES = [
  { labelKey: 'messenger.trajectory.lane.input', top: 7 },
  { labelKey: 'messenger.trajectory.lane.model', top: 21 },
  { labelKey: 'messenger.trajectory.lane.tools', top: 35 }
];
const INSPECTOR_TABS: Array<{ key: InspectorTab; labelKey: string }> = [
  { key: 'overview', labelKey: 'messenger.trajectory.inspector.overview' },
  { key: 'timing', labelKey: 'messenger.trajectory.inspector.timing' },
  { key: 'usage', labelKey: 'messenger.trajectory.inspector.usage' },
  { key: 'payload', labelKey: 'messenger.trajectory.inspector.payload' }
];
const KIND_LABEL_KEY: Record<TrajKind, string> = {
  system: 'messenger.trajectory.kind.system',
  user: 'messenger.trajectory.kind.user',
  context: 'messenger.trajectory.kind.context',
  compacted: 'messenger.trajectory.kind.compacted',
  message: 'messenger.trajectory.kind.message',
  tool: 'messenger.trajectory.kind.tool',
  subtool: 'messenger.trajectory.kind.subtool'
};
const KIND_ICON: Record<TrajKind, string> = {
  system: 'fa-solid fa-gear',
  user: 'fa-solid fa-user',
  context: 'fa-solid fa-layer-group',
  compacted: 'fa-solid fa-compress',
  message: 'fa-solid fa-comment-dots',
  tool: 'fa-solid fa-wrench',
  subtool: 'fa-solid fa-diagram-project'
};

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

const sessionId = computed(() => {
  const raw = Array.isArray(route.query.session) ? route.query.session[0] : route.query.session;
  return typeof raw === 'string' ? raw.trim() : '';
});

const rawTurns = ref<Json[]>([]);
const loading = ref(false);
const loadingMore = ref(false);
const loadError = ref('');
const hasMore = ref(false);
const nextBefore = ref<number | null>(null);

const collapsedTurns = ref<Set<string>>(new Set());
const collapseCalls = ref(false);
const durationMode = ref(true);
const searchQuery = ref('');
const selectedRecordId = ref<string | null>(null);
const timelineFocus = ref<Set<number> | null>(null);
const activeTab = ref<InspectorTab>('overview');
const hoverLine = ref<number | null>(null);
const trackRef = ref<HTMLElement | null>(null);

const asJson = (value: unknown): Json =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Json) : {};
const asText = (value: unknown): string => {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return '';
  }
};
const asNum = (value: unknown): number | null => {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
};
const pickText = (source: Json, keys: string[]): string => {
  for (const key of keys) {
    const text = asText(source[key]);
    if (text) return text;
  }
  return '';
};
const pickNum = (source: Json, keys: string[]): number | null => {
  for (const key of keys) {
    const value = asNum(source[key]);
    if (value !== null) return value;
  }
  return null;
};
const pickValue = (source: Json, keys: string[]): unknown => {
  for (const key of keys) {
    if (source[key] !== undefined && source[key] !== null) return source[key];
  }
  return undefined;
};
const contentToText = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) {
    return value
      .map((part) => {
        if (typeof part === 'string') return part;
        const record = asJson(part);
        return asText(record.text ?? record.content ?? '');
      })
      .filter(Boolean)
      .join('\n');
  }
  const record = asJson(value);
  if (Object.keys(record).length) return contentToText(record.text ?? record.content);
  return '';
};
const toMillis = (value: unknown): number | null => {
  const numeric = asNum(value);
  if (numeric !== null) return numeric < 1e12 ? Math.round(numeric * 1000) : Math.round(numeric);
  if (typeof value === 'string' && value.trim()) {
    const parsed = Date.parse(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
};
const truncate = (text: string, limit: number): string => (text.length > limit ? `${text.slice(0, limit)}…` : text);

const mapKind = (itemKind: string): TrajKind => {
  switch (itemKind) {
    case 'user_message':
      return 'user';
    case 'assistant_message':
      return 'message';
    case 'tool_call':
    case 'tool_result':
      return 'tool';
    case 'subagent_run':
      return 'subtool';
    case 'compaction':
      return 'compacted';
    case 'system_message':
      return 'system';
    default:
      return 'context';
  }
};

const extractUsage = (source: Json): TrajUsage | null => {
  const candidates: Json[] = [];
  candidates.push(asJson(source.stats));
  candidates.push(asJson(asJson(source.meta).message_stats));
  candidates.push(asJson(pickValue(source, ['usage', 'round_usage'])));
  for (const candidate of candidates) {
    const nested = asJson(candidate.usage);
    const usage = Object.keys(nested).length ? nested : candidate;
    const input = pickNum(usage, ['input', 'input_tokens', 'prompt_tokens']);
    const output = pickNum(usage, ['output', 'output_tokens', 'completion_tokens']);
    if (input === null && output === null) continue;
    return {
      input: input ?? 0,
      cacheRead: pickNum(usage, ['cache_read', 'cacheRead', 'cache_read_tokens']) ?? 0,
      cacheWrite: pickNum(usage, ['cache_write', 'cacheWrite', 'cache_write_tokens']) ?? 0,
      output: output ?? 0,
      reasoning: pickNum(usage, ['reasoning', 'reasoning_tokens', 'think_tokens']) ?? 0
    };
  }
  return null;
};

const buildCell = (item: Json, index: number, turnNo: number | null, isFirstOfTurn: boolean): TrajCell => {
  const payload = asJson(item.payload);
  const metrics: Json = {
    ...payload,
    ...asJson(payload.stats),
    ...asJson(asJson(payload.meta).message_stats)
  };
  const kind = mapKind(asText(item.kind));
  const toolName = pickText(metrics, ['tool', 'tool_name', 'name', 'toolName', 'tool_display_name']);
  const status = pickText(item, ['status']) || pickText(payload, ['status']);
  const isError =
    ['failed', 'error', 'cancelled', 'interrupted'].includes(status.toLowerCase()) ||
    asText(payload.error) !== '' ||
    payload.is_error === true;

  const question = contentToText(payload.content);
  const reasoning = contentToText(payload.reasoning ?? payload.thinking ?? payload.reasoning_content);
  const args = pickText(payload, ['args', 'arguments', 'input']);
  const result = contentToText(payload.result ?? payload.output ?? payload.content);

  let text = '';
  let inputDetail = '';
  let outputDetail = '';
  let resultPreview = '';

  if (kind === 'user') {
    text = truncate(question, 220);
    inputDetail = question;
  } else if (kind === 'message') {
    text = truncate(question || reasoning, 220);
    outputDetail = question;
  } else if (kind === 'tool' || kind === 'subtool') {
    text = [toolName || t('messenger.trajectory.kind.tool'), truncate(question, 120)].filter(Boolean).join(' · ');
    inputDetail = args || question;
    outputDetail = result;
    resultPreview = truncate(result.replace(/\s+/g, ' '), 120);
  } else if (kind === 'compacted') {
    text = t('messenger.trajectory.kind.compacted');
    inputDetail = truncate(question, 4000);
  } else {
    text = truncate(question, 220);
    inputDetail = question;
  }

  const startedAt =
    toMillis(pickValue(metrics, ['started_at', 'start_time', 'startedAt'])) ??
    toMillis(pickValue(item, ['created_time']));
  const completedAt =
    toMillis(pickValue(metrics, ['completed_at', 'end_time', 'completedAt'])) ??
    toMillis(pickValue(item, ['updated_time']));
  const decodeSeconds = pickNum(metrics, ['decode_duration_s', 'decode_duration']);
  const prefillSeconds = pickNum(metrics, ['prefill_duration_s', 'prefill_duration']);
  let timeSeconds: number | null = null;
  if (decodeSeconds !== null || prefillSeconds !== null) {
    timeSeconds = (decodeSeconds ?? 0) + (prefillSeconds ?? 0);
  } else if (startedAt !== null && completedAt !== null && completedAt >= startedAt) {
    timeSeconds = (completedAt - startedAt) / 1000;
  }

  const usage = extractUsage(payload);
  const searchText = [text, inputDetail, outputDetail, toolName, kind]
    .join(' ')
    .toLowerCase();

  return {
    index,
    recordId: asText(item.item_id) || `${asText(item.turn_id) || 'turn'}:${index}`,
    kind,
    text,
    turn: turnNo,
    step: pickNum(item, ['model_round']) ?? pickNum(payload, ['model_round']),
    startedAt,
    completedAt,
    timeSeconds,
    ttftMs: pickNum(metrics, ['ttft_ms', 'ttftMs', 'first_token_ms']),
    decodeSpeed: pickNum(metrics, ['decode_speed_tps', 'visible_decode_speed_tps', 'decodeSpeedTps']),
    isError,
    isFirstOfTurn,
    toolName,
    callId: pickText(metrics, ['tool_call_id', 'toolCallId', 'call_id', 'callId']),
    status,
    inputDetail,
    outputDetail,
    thinkingDetail: reasoning,
    resultPreview,
    usage,
    searchText
  };
};

const toolHistogram = (records: TrajCell[]): string => {
  const counts = new Map<string, number>();
  records.forEach((record) => {
    if (record.kind !== 'tool' && record.kind !== 'subtool') return;
    const name = record.toolName || record.kind;
    counts.set(name, (counts.get(name) ?? 0) + 1);
  });
  return Array.from(counts.entries())
    .map(([name, count]) => `${name}×${count}`)
    .join(' ');
};

const buildTrajectory = (input: Json[]): TrajTurn[] => {
  const ordered = [...input].sort((a, b) => {
    const ra = asNum(a.user_turn_index) ?? asNum(a.user_round) ?? 0;
    const rb = asNum(b.user_turn_index) ?? asNum(b.user_round) ?? 0;
    return ra - rb;
  });
  let cursor = 0;
  const output: TrajTurn[] = [];
  ordered.forEach((rawTurn, turnPosition) => {
    const turnNo = asNum(rawTurn.user_turn_index) ?? asNum(rawTurn.user_round);
    const turnKey = asText(rawTurn.turn_id) || `turn-${turnPosition}`;
    const items = (Array.isArray(rawTurn.items) ? rawTurn.items : [])
      .map(asJson)
      .sort((a, b) => (asNum(a.created_seq) ?? 0) - (asNum(b.created_seq) ?? 0));

    const usage: TrajUsage = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0, reasoning: 0 };
    const groups: TrajGroup[] = [];
    let currentGroup: TrajGroup | null = null;
    let timeSeconds = 0;
    let hasTime = false;
    let recordCount = 0;

    const ensureGroup = (title: string): TrajGroup => {
      const group: TrajGroup = {
        key: `${turnKey}:g${groups.length}`,
        title,
        description: '',
        toolCount: 0,
        records: []
      };
      groups.push(group);
      return group;
    };

    items.forEach((item) => {
      const kind = mapKind(asText(item.kind));
      const cell = buildCell(item, cursor, turnNo, recordCount === 0);
      cursor += 1;
      recordCount += 1;
      if (cell.usage) {
        usage.input += cell.usage.input;
        usage.cacheRead += cell.usage.cacheRead;
        usage.cacheWrite += cell.usage.cacheWrite;
        usage.output += cell.usage.output;
        usage.reasoning += cell.usage.reasoning;
      }
      if (cell.timeSeconds !== null) {
        timeSeconds += cell.timeSeconds;
        hasTime = true;
      }

      if (kind === 'user' || kind === 'system' || kind === 'context') {
        currentGroup = ensureGroup(t('messenger.trajectory.group.message'));
        currentGroup.records.push(cell);
        return;
      }
      if (kind === 'compacted') {
        currentGroup = ensureGroup(t('messenger.trajectory.group.compaction'));
        currentGroup.records.push(cell);
        return;
      }
      if (kind === 'message') {
        const step = cell.step;
        const title = step === null ? t('messenger.trajectory.group.message') : `${t('messenger.trajectory.group.step')} ${step + 1}`;
        currentGroup = ensureGroup(title);
        currentGroup.records.push(cell);
        return;
      }
      if (!currentGroup) {
        currentGroup = ensureGroup(t('messenger.trajectory.group.step'));
      }
      currentGroup.toolCount += 1;
      currentGroup.records.push(cell);
    });

    groups.forEach((group) => {
      const groupSeconds = group.records.reduce((acc, record) => acc + (record.timeSeconds ?? 0), 0);
      const histogram = toolHistogram(group.records);
      const timing = group.records.some((record) => record.timeSeconds !== null) ? `${groupSeconds.toFixed(1)} s` : '';
      group.description = [timing, histogram].filter(Boolean).join('  ');
    });

    output.push({
      key: turnKey,
      label: turnNo === null ? t('messenger.trajectory.group.message') : `${t('messenger.trajectory.turn.label')} ${turnNo}`,
      collapsed: false,
      usage,
      timeSeconds: hasTime ? timeSeconds : null,
      recordCount,
      groups
    });
  });
  return output;
};

const trajectory = computed(() => buildTrajectory(rawTurns.value));
const allCells = computed(() => trajectory.value.flatMap((turn) => turn.groups.flatMap((group) => group.records)));

const normalizedQuery = computed(() => searchQuery.value.trim().toLowerCase());

const visibleTurns = computed(() =>
  trajectory.value
    .map((turn) => {
      const query = normalizedQuery.value;
      const groups = turn.groups
        .map((group) => {
          let records = group.records;
          if (collapseCalls.value) {
            records = records.filter((record) => record.kind !== 'tool' && record.kind !== 'subtool');
          }
          if (query) {
            records = records.filter((record) => record.searchText.includes(query));
          }
          return { ...group, records };
        })
        .filter((group) => group.records.length > 0);
      return { ...turn, collapsed: collapsedTurns.value.has(turn.key), groups };
    })
    .filter((turn) => !normalizedQuery.value || turn.groups.length > 0)
);

const allTurnsCollapsed = computed(() => {
  const turns = trajectory.value;
  return turns.length > 0 && turns.every((turn) => collapsedTurns.value.has(turn.key));
});

const laneOf = (kind: TrajKind): number => {
  if (kind === 'tool' || kind === 'subtool') return 2;
  if (kind === 'message' || kind === 'compacted') return 1;
  return 0;
};

const focusedIndexes = computed(() => timelineFocus.value ?? new Set<number>());

const timelineLayout = computed(() => {
  const cells = allCells.value;
  if (!cells.length) {
    return { spans: [] as TimelineSpan[], boundaries: [] as Array<{ turnKey: string; left: number }>, useDuration: false };
  }
  const timed = cells.filter((cell) => cell.startedAt !== null && cell.timeSeconds !== null);
  const useDuration = durationMode.value && timed.length === cells.length;

  const starts: number[] = [];
  if (useDuration) {
    timed.forEach((cell) => starts.push(cell.startedAt as number));
  }
  const start = useDuration ? Math.min(...starts) : 0;
  let end = 0;
  if (useDuration) {
    cells.forEach((cell, position) => {
      end = Math.max(end, (cell.startedAt as number) + (cell.timeSeconds as number) * 1000);
    });
  }

  const spans: TimelineSpan[] = [];
  const total = cells.length;
  cells.forEach((cell, position) => {
    let left: number;
    let width: number;
    let spanStart: number | null = null;
    if (useDuration) {
      const span = Math.max(end - start, 1);
      const cellStart = cell.startedAt as number;
      const cellEnd = cellStart + (cell.timeSeconds as number) * 1000;
      left = ((cellStart - start) / span) * 100;
      width = Math.max(((cellEnd - cellStart) / span) * 100, 0.4);
      spanStart = cellStart;
    } else {
      left = (position / total) * 100;
      width = Math.max(100 / total, 0.4);
    }
    spans.push({
      recordId: cell.recordId,
      index: cell.index,
      kind: cell.kind,
      lane: laneOf(cell.kind),
      left,
      width,
      isError: cell.isError,
      equalDuration: !useDuration,
      durationSeconds: cell.timeSeconds,
      startedAt: spanStart
    });
  });

  const boundaries: Array<{ turnKey: string; left: number }> = [];
  let position = 0;
  trajectory.value.forEach((turn) => {
    const count = turn.groups.reduce((acc, group) => acc + group.records.length, 0);
    if (count > 0) {
      const boundaryPosition = position;
      const left = useDuration
        ? spans[boundaryPosition]
          ? spans[boundaryPosition].left
          : 0
        : (boundaryPosition / Math.max(cells.length, 1)) * 100;
      boundaries.push({ turnKey: turn.key, left });
    }
    position += count;
  });

  return { spans, boundaries, useDuration };
});

const timelineSpans = computed(() => timelineLayout.value.spans);
const timelineBoundaries = computed(() => timelineLayout.value.boundaries);

const selectedRecord = computed(() => allCells.value.find((cell) => cell.recordId === selectedRecordId.value) ?? null);

const spanClasses = (span: TimelineSpan): Record<string, boolean> => {
  const focused = focusedIndexes.value;
  const query = normalizedQuery.value;
  const cell = allCells.value[span.index];
  return {
    [`is-${span.kind}`]: true,
    'is-error': span.isError,
    'is-selected': selectedRecordId.value === span.recordId,
    'is-dimmed': focused.size > 0 && !focused.has(span.index),
    'is-match': Boolean(query) && Boolean(cell && cell.searchText.includes(query)),
    'is-equal': span.equalDuration
  };
};

const spanStyle = (span: TimelineSpan): Record<string, string> => ({
  left: `${span.left}%`,
  width: `${span.width}%`,
  top: `${span.lane * 14}px`
});

const spanTooltip = (span: TimelineSpan): string => {
  const parts = [
    t(kindLabelKey(span.kind)),
    span.durationSeconds === null ? '' : `${span.durationSeconds.toFixed(2)} s`,
    span.startedAt === null ? '' : formatTimestamp(span.startedAt)
  ];
  return parts.filter(Boolean).join(' · ');
};

const percentFromEvent = (event: PointerEvent): number => {
  const element = trackRef.value;
  if (!element) return 0;
  const rect = element.getBoundingClientRect();
  if (!rect.width) return 0;
  return Math.min(100, Math.max(0, ((event.clientX - rect.left) / rect.width) * 100));
};

const indexesInRange = (from: number, to: number): number[] => {
  const low = Math.min(from, to);
  const high = Math.max(from, to);
  return timelineSpans.value
    .filter((span) => span.left + span.width >= low && span.left <= high)
    .map((span) => span.index);
};

let dragStartX: number | null = null;
let dragging = false;

const onTrackPointerDown = (event: PointerEvent): void => {
  dragStartX = percentFromEvent(event);
  dragging = false;
};
const onTrackPointerMove = (event: PointerEvent): void => {
  hoverLine.value = percentFromEvent(event);
  if (dragStartX === null) return;
  const current = percentFromEvent(event);
  if (!dragging && Math.abs(current - dragStartX) < 1) return;
  dragging = true;
  timelineFocus.value = new Set(indexesInRange(dragStartX, current));
};
const onTrackPointerLeave = (): void => {
  hoverLine.value = null;
  dragStartX = null;
  dragging = false;
};

const focusSpan = (span: TimelineSpan): void => {
  timelineFocus.value = new Set([span.index]);
  selectedRecordId.value = span.recordId;
};

const clearSelection = (): void => {
  timelineFocus.value = null;
  selectedRecordId.value = null;
};

const selectRecord = (recordId: string): void => {
  const cell = allCells.value.find((item) => item.recordId === recordId);
  if (!cell) return;
  selectedRecordId.value = recordId;
  timelineFocus.value = new Set([cell.index]);
};

const toggleTurn = (key: string): void => {
  const next = new Set(collapsedTurns.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  collapsedTurns.value = next;
};

const toggleAllTurns = (): void => {
  collapsedTurns.value = allTurnsCollapsed.value
    ? new Set()
    : new Set(trajectory.value.map((turn) => turn.key));
};

const toggleAllCalls = (): void => {
  collapseCalls.value = !collapseCalls.value;
};

const formatNumber = (value: number): string => (value > 0 ? value.toLocaleString('en-US') : '—');
const formatSeconds = (value: number | null): string => (value === null ? '—' : `${value.toFixed(value < 10 ? 2 : 1)} s`);
const formatMillis = (value: number | null): string => (value === null ? '—' : `${Math.round(value)} ms`);
const formatTimestamp = (value: number | null): string => (value === null ? '—' : new Date(value).toLocaleString());
const formatSpeed = (value: number | null): string =>
  value === null ? '—' : `${value.toFixed(1)} ${t('messenger.trajectory.unit.tokensPerSecond')}`;

const kindLabelKey = (kind: TrajKind): string => KIND_LABEL_KEY[kind];
const kindIcon = (kind: TrajKind): string => KIND_ICON[kind];

const recordClasses = (record: TrajCell): Record<string, boolean> => {
  const focused = focusedIndexes.value;
  const query = normalizedQuery.value;
  return {
    'is-selected': selectedRecordId.value === record.recordId,
    'is-error': record.isError,
    'is-first-of-turn': record.isFirstOfTurn,
    'is-dimmed': focused.size > 0 && !focused.has(record.index),
    'is-match': Boolean(query) && record.searchText.includes(query)
  };
};

const overviewRows = computed(() => {
  const record = selectedRecord.value;
  if (!record) return [];
  return [
    { label: t('messenger.trajectory.overview.kind'), value: t(kindLabelKey(record.kind)) },
    { label: t('messenger.trajectory.overview.turn'), value: record.turn === null ? '—' : String(record.turn) },
    { label: t('messenger.trajectory.overview.step'), value: record.step === null ? '—' : String(record.step + 1) },
    { label: t('messenger.trajectory.overview.status'), value: record.status || '—' },
    { label: t('messenger.trajectory.overview.tool'), value: record.toolName || '—' },
    { label: t('messenger.trajectory.overview.callId'), value: record.callId || '—' },
    { label: t('messenger.trajectory.overview.duration'), value: formatSeconds(record.timeSeconds) }
  ];
});

const timingRows = computed(() => {
  const record = selectedRecord.value;
  if (!record) return [];
  return [
    { label: t('messenger.trajectory.timing.startedAt'), value: formatTimestamp(record.startedAt) },
    { label: t('messenger.trajectory.timing.completedAt'), value: formatTimestamp(record.completedAt) },
    { label: t('messenger.trajectory.timing.total'), value: formatSeconds(record.timeSeconds) },
    { label: t('messenger.trajectory.timing.ttft'), value: formatMillis(record.ttftMs) },
    { label: t('messenger.trajectory.timing.decode'), value: formatSpeed(record.decodeSpeed) }
  ];
});

const usageRows = computed(() => {
  const record = selectedRecord.value;
  const usage = record?.usage ?? null;
  if (!usage) return [{ label: t('messenger.trajectory.usage.total'), value: '—' }];
  return [
    { label: t('messenger.trajectory.usage.input'), value: usage.input.toLocaleString('en-US') },
    { label: t('messenger.trajectory.usage.cacheRead'), value: usage.cacheRead.toLocaleString('en-US') },
    { label: t('messenger.trajectory.usage.cacheWrite'), value: usage.cacheWrite.toLocaleString('en-US') },
    { label: t('messenger.trajectory.usage.output'), value: usage.output.toLocaleString('en-US') },
    { label: t('messenger.trajectory.usage.reasoning'), value: usage.reasoning.toLocaleString('en-US') }
  ];
});

const extractPayload = (response: unknown): Json => {
  const first = asJson(response);
  const body = asJson(first.data);
  const inner = asJson(body.data);
  if (Object.keys(inner).length) return inner;
  if (Object.keys(body).length) return body;
  return first;
};

const applyTurnsPayload = (payload: Json, mode: 'replace' | 'prepend'): void => {
  const turns = Array.isArray(payload.turns) ? payload.turns.map(asJson) : [];
  rawTurns.value = mode === 'prepend' ? [...turns, ...rawTurns.value] : turns;
  hasMore.value = payload.has_more === true;
  nextBefore.value = asNum(payload.next_before) ?? null;
};

const load = async (): Promise<void> => {
  const id = sessionId.value;
  if (!id) {
    loadError.value = t('messenger.trajectory.empty');
    return;
  }
  loading.value = true;
  loadError.value = '';
  try {
    const response = await getThreadLogTurns(id, { limit: TURNS_LIMIT });
    applyTurnsPayload(extractPayload(response), 'replace');
  } catch {
    loadError.value = t('messenger.trajectory.loadFailed');
  } finally {
    loading.value = false;
  }
};

const loadEarlier = async (): Promise<void> => {
  const id = sessionId.value;
  if (!id || loadingMore.value) return;
  loadingMore.value = true;
  try {
    const params: Record<string, string | number> = { limit: HISTORY_TURNS_PAGE };
    if (nextBefore.value !== null) params.before = nextBefore.value;
    const response = await getThreadLogTurns(id, params);
    applyTurnsPayload(extractPayload(response), 'prepend');
  } catch {
    // 保留已有数据，静默失败。
  } finally {
    loadingMore.value = false;
  }
};

const goBack = (): void => {
  if (window.history.length > 1) {
    router.back();
    return;
  }
  void router.push('/app/home');
};

const onKeyDown = (event: KeyboardEvent): void => {
  if (event.key === 'Escape') clearSelection();
};

watch(sessionId, (value) => {
  if (!value) return;
  rawTurns.value = [];
  clearSelection();
  void load();
});

onMounted(() => {
  window.addEventListener('keydown', onKeyDown);
  void load();
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeyDown);
});
</script>
