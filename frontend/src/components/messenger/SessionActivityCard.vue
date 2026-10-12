<template>
  <section class="session-activity-card" :aria-label="t('messenger.activity.ariaLabel')">
    <header class="session-activity-head">
      <button
        type="button"
        class="session-activity-tab"
        :class="{ 'is-active': activeTab === 'sessions' }"
        :aria-pressed="activeTab === 'sessions'"
        @click="activeTab = 'sessions'"
      >
        {{ t('messenger.activity.tab.sessions') }}
      </button>
      <button
        type="button"
        class="session-activity-tab"
        :class="{ 'is-active': activeTab === 'credits' }"
        :aria-pressed="activeTab === 'credits'"
        @click="activeTab = 'credits'"
      >
        {{ t('messenger.activity.tab.credits') }}
      </button>
    </header>

    <div class="session-activity-grid" role="img" :aria-label="summaryLabel">
      <span
        v-for="cell in flatCells"
        :key="cell.date"
        class="session-activity-cell"
        :class="cellClass(cell)"
        :title="cellTitle(cell)"
      ></span>
    </div>

    <div class="session-activity-months" aria-hidden="true">
      <span
        v-for="label in grid.monthLabels"
        :key="`month-${label.month}-${label.column}`"
        class="session-activity-month"
        :style="{ gridColumnStart: String(label.column + 1) }"
      >
        {{ monthLabel(label.month) }}
      </span>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import { getDailyActivity } from '@/api/chat';
import { useI18n } from '@/i18n';
import {
  ACTIVITY_WEEK_COLUMNS,
  buildActivityGrid,
  formatActivityCount,
  formatDayKey,
  type ActivityCell,
  type DailyActivityPoint
} from '@/utils/dailyActivityGrid';

type ActivityTab = 'sessions' | 'credits';

type DailyActivityPayload = {
  today?: unknown;
  sessions?: unknown;
  credits?: unknown;
};

// One card per new thread; a short-lived shared result keeps repeated thread
// switches off the database without freezing the day buckets.
const CACHE_TTL_MS = 60_000;
let cachedActivity: { at: number; payload: DailyActivityPayload } | null = null;

const { t, language } = useI18n();

const localTodayKey = (): string => {
  const now = new Date();
  return formatDayKey(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()));
};

const asPoints = (value: unknown): DailyActivityPoint[] =>
  (Array.isArray(value) ? value : [])
    .map((raw) => {
      const item = (raw || {}) as Record<string, unknown>;
      return { date: item.date, value: Number(item.value) || 0 };
    })
    .filter((item) => typeof item.date === 'string' && item.value > 0);

const activeTab = ref<ActivityTab>('sessions');
const todayKey = ref(localTodayKey());
const sessionPoints = ref<DailyActivityPoint[]>([]);
const creditPoints = ref<DailyActivityPoint[]>([]);
let requestController: AbortController | null = null;

const applyPayload = (payload: DailyActivityPayload | null): void => {
  const today = String(payload?.today || '').trim();
  todayKey.value = today || localTodayKey();
  sessionPoints.value = asPoints(payload?.sessions);
  creditPoints.value = asPoints(payload?.credits);
};

const loadActivity = async (): Promise<void> => {
  if (cachedActivity && Date.now() - cachedActivity.at < CACHE_TTL_MS) {
    applyPayload(cachedActivity.payload);
    return;
  }
  const controller = new AbortController();
  requestController = controller;
  try {
    const { data } = await getDailyActivity({}, { signal: controller.signal });
    const payload = ((data as Record<string, unknown>)?.data || {}) as DailyActivityPayload;
    cachedActivity = { at: Date.now(), payload };
    applyPayload(payload);
  } catch {
    // A failed read keeps the all-empty grid; the thread stays usable.
    cachedActivity = null;
  } finally {
    if (requestController === controller) {
      requestController = null;
    }
  }
};

const grid = computed(() =>
  buildActivityGrid(
    activeTab.value === 'sessions' ? sessionPoints.value : creditPoints.value,
    todayKey.value,
    ACTIVITY_WEEK_COLUMNS
  )
);

const flatCells = computed(() =>
  grid.value.columns.reduce<ActivityCell[]>((cells, column) => cells.concat(column.cells), [])
);

const summaryLabel = computed(() =>
  activeTab.value === 'sessions'
    ? t('messenger.activity.summarySessions', {
        count: formatActivityCount(grid.value.total),
        days: grid.value.activeDays
      })
    : t('messenger.activity.summaryCredits', {
        count: formatActivityCount(grid.value.total),
        days: grid.value.activeDays
      })
);

const cellClass = (cell: ActivityCell): Record<string, boolean> => ({
  'is-future': cell.future,
  [`level-${cell.level}`]: true
});

const cellTitle = (cell: ActivityCell): string => {
  const dateLabel = cell.isToday
    ? `${t('messenger.activity.todayLabel')} · ${cell.date}`
    : cell.date;
  if (cell.future) {
    return t('messenger.activity.cellFuture', { date: dateLabel });
  }
  if (cell.value <= 0) {
    return t('messenger.activity.cellEmpty', { date: dateLabel });
  }
  return activeTab.value === 'sessions'
    ? t('messenger.activity.cellSessions', { date: dateLabel, count: formatActivityCount(cell.value) })
    : t('messenger.activity.cellCredits', { date: dateLabel, count: formatActivityCount(cell.value) });
};

const monthFormatter = computed(
  () =>
    new Intl.DateTimeFormat(language.value || undefined, {
      month: 'short',
      timeZone: 'UTC'
    })
);
const monthLabel = (month: number): string =>
  monthFormatter.value.format(new Date(Date.UTC(2000, month, 1)));

onMounted(() => {
  void loadActivity();
});

onBeforeUnmount(() => {
  requestController?.abort();
  requestController = null;
});
</script>
