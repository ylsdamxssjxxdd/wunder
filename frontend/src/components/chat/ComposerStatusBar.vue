<template>
  <div class="composer-status-bar">
    <div
      class="composer-status-dock"
      @mouseenter="openDock('tasks')"
      @mouseleave="scheduleDockClose('tasks')"
      @focusin="openDock('tasks')"
      @focusout="handleFocusOut('tasks', $event)"
    >
      <button
        class="composer-status-trigger"
        :class="{ 'is-active': taskCount > 0 }"
        type="button"
        data-testid="composer-background-tasks"
        :title="taskHint"
        :aria-label="taskHint"
        :aria-expanded="taskPanelVisible"
        @click.stop="toggleDock('tasks')"
      >
        <i class="fa-solid fa-terminal composer-status-icon" aria-hidden="true"></i>
        <span>{{ compactCount(taskCount) }}</span>
      </button>
      <div
        v-if="taskPanelVisible"
        class="composer-status-panel"
        data-testid="composer-background-task-panel"
        @click.stop
      >
        <div class="composer-status-panel__head">
          <strong>{{ t('chat.composer.backgroundTasks') }}</strong>
          <span>{{ t('chat.composer.backgroundTasksCount', { count: taskCount }) }}</span>
        </div>
        <div v-for="task in visibleTasks" :key="task.commandSessionId" class="composer-task-row">
          <div class="composer-task-row__main">
            <button
              class="composer-task-row__open"
              type="button"
              :title="task.command"
              :aria-expanded="expandedTaskIds.has(task.commandSessionId)"
              @click="toggleTask(task.commandSessionId)"
            >
              <span class="composer-task-dot" aria-hidden="true"></span>
              <span class="composer-task-command">{{ task.command }}</span>
              <span class="composer-task-elapsed">{{ elapsed(task) }}</span>
            </button>
            <button
              class="composer-task-stop"
              type="button"
              :disabled="terminatingTaskIds.has(task.commandSessionId)"
              :title="t('chat.composer.backgroundTasksTerminate')"
              :aria-label="t('chat.composer.backgroundTasksTerminate')"
              @click="terminateTask(task)"
            >
              <i class="fa-solid fa-xmark" aria-hidden="true"></i>
            </button>
          </div>
          <div v-if="expandedTaskIds.has(task.commandSessionId)" class="composer-task-detail">
            <div class="composer-task-cwd" :title="task.cwd">{{ task.cwd }}</div>
            <pre>{{ taskOutput(task) || t('chat.composer.backgroundTasksNoOutput') }}</pre>
          </div>
        </div>
        <div v-if="taskCount > visibleTasks.length" class="composer-status-panel__more">
          {{ t('chat.composer.backgroundTasksMore', { count: taskCount - visibleTasks.length }) }}
        </div>
      </div>
    </div>

    <div
      class="composer-status-dock"
      @mouseenter="openDock('subagents')"
      @mouseleave="scheduleDockClose('subagents')"
      @focusin="openDock('subagents')"
      @focusout="handleFocusOut('subagents', $event)"
    >
      <button
        class="composer-status-trigger"
        :class="{ 'is-active': subagents.length > 0 }"
        type="button"
        data-testid="composer-subagents"
        :title="subagentHint"
        :aria-label="subagentHint"
        :aria-expanded="subagentPanelVisible"
        @click.stop="toggleDock('subagents')"
      >
        <i class="fa-solid fa-robot composer-status-icon" aria-hidden="true"></i>
        <span>{{ compactCount(subagents.length) }}</span>
      </button>
      <div
        v-show="subagentPanelVisible"
        class="composer-status-panel composer-subagent-popover"
        data-testid="composer-subagent-panel"
        @click.stop
      >
        <MessageSubagentPanel :session-id="sessionId" :items="visibleSubagents" />
        <div v-if="subagents.length > visibleSubagents.length" class="composer-status-panel__more">
          {{ t('chat.composer.subagentsMore', { count: subagents.length - visibleSubagents.length }) }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import MessageSubagentPanel from '@/components/chat/MessageSubagentPanel.vue';
import { getSessionCommandSessions, terminateSessionCommandSession } from '@/api/chat';
import { getSessionMessages } from '@/stores/chatRuntimeState';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import {
  useCommandSessionStore,
  type CommandSessionRuntimeEntry
} from '@/stores/commandSessions';
import { normalizeMessageSubagents } from '@/stores/chatStats';
import { onSubagentPoolChanged } from '@/utils/subagentPoolEvents';
import { isSubagentItemActive } from '@/utils/subagentRuntime';

const MAX_ROWS = 6;
const OUTPUT_LINES = 10;
const OUTPUT_CHARS = 1200;

type DockKind = 'tasks' | 'subagents';

const props = defineProps<{ sessionId?: string }>();
const { t } = useI18n();
const chatStore = useChatStore();
const commandStore = useCommandSessionStore();
const sessionId = computed(() => String(props.sessionId || chatStore.activeSessionId || '').trim());
const tasks = computed(() => commandStore.listBySession(sessionId.value).filter(task => task.status === 'running'));
const visibleTasks = computed(() => tasks.value.slice(0, MAX_ROWS));
const taskCount = computed(() => tasks.value.length);
const loadedSubagents = ref<Record<string, unknown>[]>([]);
const projectedSubagents = computed<Record<string, unknown>[]>(() => {
  void chatStore.runtimeProjectionVersionBySession?.[sessionId.value];
  const cached = sessionId.value === String(chatStore.activeSessionId || '').trim()
    ? chatStore.messages
    : getSessionMessages(sessionId.value);
  const messages = Array.isArray(cached) ? cached : [];
  return normalizeMessageSubagents(messages.flatMap(message =>
    Array.isArray(message?.subagents) ? message.subagents : []
  ));
});
const subagents = computed<Record<string, unknown>[]>(() =>
  normalizeMessageSubagents([...projectedSubagents.value, ...loadedSubagents.value])
);
const visibleSubagents = computed(() => subagents.value.slice(0, MAX_ROWS));
const expandedTaskIds = ref<Set<string>>(new Set());
const terminatingTaskIds = ref<Set<string>>(new Set());
const taskDockOpen = ref(false);
const subagentDockOpen = ref(false);
const nowMs = ref(Date.now());
const closeTimers: Record<DockKind, ReturnType<typeof setTimeout> | null> = { tasks: null, subagents: null };
let subagentLoadSerial = 0;

const taskPanelVisible = computed(() => taskDockOpen.value && taskCount.value > 0);
const subagentPanelVisible = computed(() => subagentDockOpen.value && subagents.value.length > 0);
const compactCount = (count: number): string => count > 9 ? '9+' : String(count);
const taskHint = computed(() => taskCount.value > 0
  ? t('chat.composer.backgroundTasksHint', { count: taskCount.value })
  : t('chat.composer.backgroundTasksEmpty'));
const subagentHint = computed(() => subagents.value.length > 0
  ? t('chat.composer.subagentsHint', { count: subagents.value.length })
  : t('chat.subagentPool.empty'));

const clearCloseTimer = (kind: DockKind) => {
  if (closeTimers[kind]) clearTimeout(closeTimers[kind]!);
  closeTimers[kind] = null;
};
const openDock = (kind: DockKind) => {
  clearCloseTimer(kind);
  const other: DockKind = kind === 'tasks' ? 'subagents' : 'tasks';
  clearCloseTimer(other);
  if (kind === 'tasks') {
    taskDockOpen.value = true;
    subagentDockOpen.value = false;
  } else {
    subagentDockOpen.value = true;
    taskDockOpen.value = false;
  }
};
const scheduleDockClose = (kind: DockKind) => {
  clearCloseTimer(kind);
  closeTimers[kind] = setTimeout(() => {
    closeTimers[kind] = null;
    if (kind === 'tasks') taskDockOpen.value = false;
    else subagentDockOpen.value = false;
  }, 200);
};
const toggleDock = (kind: DockKind) => {
  const open = kind === 'tasks' ? taskDockOpen.value : subagentDockOpen.value;
  if (open) scheduleDockClose(kind);
  else openDock(kind);
};
const handleFocusOut = (kind: DockKind, event: FocusEvent) => {
  const current = event.currentTarget as HTMLElement | null;
  if (!current?.contains(event.relatedTarget as Node | null)) scheduleDockClose(kind);
};

const toggleTask = (id: string) => {
  const next = new Set(expandedTaskIds.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedTaskIds.value = next;
};
const elapsed = (task: CommandSessionRuntimeEntry): string => {
  const started = Date.parse(task.startedAt);
  const seconds = Number.isFinite(started) ? Math.max(0, Math.floor((nowMs.value - started) / 1000)) : 0;
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m${String(seconds % 60).padStart(2, '0')}s`;
  return `${Math.floor(seconds / 3600)}h${String(Math.floor((seconds % 3600) / 60)).padStart(2, '0')}m`;
};
const taskOutput = (task: CommandSessionRuntimeEntry): string => {
  const text = task.ptyTail || [task.stdoutTail, task.stderrTail].filter(Boolean).join('\n') || task.error;
  const lines = text.replace(/\r/g, '').split('\n').slice(-OUTPUT_LINES).join('\n');
  return lines.length > OUTPUT_CHARS ? `…${lines.slice(-OUTPUT_CHARS)}` : lines;
};
const loadTasks = async (target: string) => {
  if (!target) {
    commandStore.clearSession(target);
    return;
  }
  try {
    const response = await getSessionCommandSessions(target);
    if (sessionId.value !== target) return;
    const items = response?.data?.data?.items;
    commandStore.hydrateSession(target, Array.isArray(items) ? items : []);
  } catch {
    // The live stream remains authoritative; a failed initial read must not clear it.
  }
};
const terminateTask = async (task: CommandSessionRuntimeEntry) => {
  const target = sessionId.value;
  const id = task.commandSessionId;
  if (!target || !id || terminatingTaskIds.value.has(id)) return;
  terminatingTaskIds.value = new Set(terminatingTaskIds.value).add(id);
  try {
    await terminateSessionCommandSession(target, id);
    await loadTasks(target);
  } catch (error: any) {
    ElMessage.error(String(error?.response?.data?.detail || error?.message || t('chat.composer.backgroundTasksFailed')));
  } finally {
    const next = new Set(terminatingTaskIds.value);
    next.delete(id);
    terminatingTaskIds.value = next;
  }
};

const loadSubagents = async (target: string, force = false) => {
  const serial = ++subagentLoadSerial;
  if (!target) {
    loadedSubagents.value = [];
    return;
  }
  try {
    const items = await chatStore.refreshSessionSubagents(target, { force });
    if (serial === subagentLoadSerial && sessionId.value === target) {
      loadedSubagents.value = normalizeMessageSubagents(items);
    }
  } catch {
    if (serial === subagentLoadSerial && sessionId.value === target) loadedSubagents.value = [];
  }
};

const removeSubagentListener = onSubagentPoolChanged(changedSession => {
  if (changedSession === sessionId.value) void loadSubagents(changedSession, true);
});
const clock = setInterval(() => {
  if (taskCount.value > 0) nowMs.value = Date.now();
}, 1000);
const subagentReconcile = setInterval(() => {
  if (subagents.value.some(item => isSubagentItemActive(item)) && sessionId.value) {
    void loadSubagents(sessionId.value, true);
  }
}, 3000);
watch(sessionId, target => {
  expandedTaskIds.value = new Set();
  terminatingTaskIds.value = new Set();
  taskDockOpen.value = false;
  subagentDockOpen.value = false;
  void loadTasks(target);
  void loadSubagents(target);
}, { immediate: true });
onBeforeUnmount(() => {
  clearInterval(clock);
  clearInterval(subagentReconcile);
  clearCloseTimer('tasks');
  clearCloseTimer('subagents');
  removeSubagentListener();
});
</script>

<style scoped>
.composer-status-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  box-sizing: border-box;
  flex: 0 0 auto;
  height: 30px;
  margin-left: 10px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
}
/* 状态格本身不建立定位上下文：浮层贴住动作行左缘展开，
   与加号菜单同一条左边线，也不会探到侧栏底下。 */
.composer-status-dock { position: static; }
.composer-status-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  box-sizing: border-box;
  gap: 5px;
  width: 40px;
  height: 26px;
  padding: 0 5px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
}
.composer-status-trigger:hover { background: var(--mz-hover, rgba(15, 23, 42, 0.05)); }
.composer-status-trigger.is-active { color: var(--mz-primary, var(--ui-accent-ink)); }
.composer-status-trigger > span { line-height: 16px; }
.composer-status-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: 0 0 16px;
  width: 16px;
  height: 16px;
  font-size: 16px;
  line-height: 16px;
}
.composer-status-panel {
  position: absolute;
  z-index: 40;
  /* 定位上下文是 `.composer-action-row`：左缘与加号菜单对齐，宽度不超出动作行。 */
  left: 0;
  bottom: calc(100% + 6px);
  box-sizing: border-box;
  width: min(380px, 100%);
  max-width: calc(100vw - 32px);
  padding: 8px;
  border: 1px solid var(--mz-border, #e5e7eb);
  border-radius: 12px;
  background: var(--mz-surface, #fff);
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.14);
  color: var(--mz-text, #1f2937);
}
.composer-status-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 20px;
  margin-bottom: 4px;
  font-size: 11px;
}
.composer-status-panel__head span, .composer-status-panel__more { color: var(--mz-text-muted, #8a8f99); }
.composer-status-panel__more { padding: 5px 4px 2px; font-size: 10px; }
.composer-task-row { min-width: 0; }
.composer-task-row__main { display: flex; align-items: center; min-width: 0; }
.composer-task-row__open {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1 1 auto;
  min-width: 0;
  height: 30px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.composer-task-dot { flex: 0 0 auto; width: 5px; height: 5px; border-radius: 50%; background: var(--mz-primary, #c37); }
.composer-task-command { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
.composer-task-elapsed { flex: 0 0 auto; color: var(--mz-text-muted, #8a8f99); font-size: 10px; }
.composer-task-stop {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: 0 0 auto;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  cursor: pointer;
}
.composer-task-stop:hover { background: #fef2f2; color: #dc2626; }
.composer-task-stop:disabled { opacity: 0.5; cursor: default; }
.composer-task-detail { margin: 0 0 6px 12px; padding: 6px 8px; border-radius: 6px; background: var(--mz-panel, #f8fafc); }
.composer-task-cwd { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--mz-text-muted, #8a8f99); font-size: 9px; }
.composer-task-detail pre { max-height: 108px; margin: 4px 0 0; overflow: hidden; white-space: pre-wrap; overflow-wrap: anywhere; font: 9px/1.4 var(--mz-font-mono, monospace); }
.composer-subagent-popover { padding: 0; overflow: hidden; }
.composer-subagent-popover :deep(.subagent-panel) { margin: 0; border: 0; border-radius: 0; background: transparent; }
.composer-subagent-popover :deep(.subagent-panel__list) { max-height: 280px; }
.composer-subagent-popover .composer-status-panel__more { padding: 6px 12px 8px; }
</style>
