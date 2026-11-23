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
        ref="taskPanelRef"
        class="composer-status-panel composer-task-popover"
        data-testid="composer-background-task-panel"
        @click.stop
      >
        <div class="composer-status-panel__head">
          <strong>{{ t('chat.composer.backgroundTasks') }}</strong>
          <span>{{ t('chat.composer.backgroundTasksCount', { count: taskCount }) }}</span>
        </div>
        <!-- 任务行放独立滚动区：条目多或展开多条输出时，浮层自己不越过视口。 -->
        <div class="composer-task-body" data-scroll-shell>
          <div class="composer-task-list" data-scroll-body @scroll="handleScrollSync">
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
                  v-if="!task.synthetic"
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
                <!-- 输出框高度封顶：超出部分在框内滚动，默认跟随最新输出；滚动条自绘，任何平台都看得见。 -->
                <div class="composer-task-output" data-scroll-shell>
                  <pre
                    data-task-output
                    data-scroll-body
                    tabindex="0"
                    @scroll="handleOutputScroll"
                  >{{ taskOutput(task) || t('chat.composer.backgroundTasksNoOutput') }}</pre>
                  <div class="composer-scroll-track" aria-hidden="true">
                    <div class="composer-scroll-thumb" data-scroll-thumb @pointerdown="startThumbDrag"></div>
                  </div>
                </div>
              </div>
            </div>
            <div v-if="taskCount > visibleTasks.length" class="composer-status-panel__more">
              {{ t('chat.composer.backgroundTasksMore', { count: taskCount - visibleTasks.length }) }}
            </div>
          </div>
          <div class="composer-scroll-track" aria-hidden="true">
            <div class="composer-scroll-thumb" data-scroll-thumb @pointerdown="startThumbDrag"></div>
          </div>
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
        :class="{ 'is-active': activeSubagentCount > 0 }"
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
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import MessageSubagentPanel from '@/components/chat/MessageSubagentPanel.vue';
import { getSessionCommandSessions, terminateSessionCommandSession } from '@/api/chat';
import { registerWsGlobalEventHandler } from '@/utils/ws';
import { getSessionMessages } from '@/stores/chatRuntimeState';
import { threadViewSlots } from '@/realtime/chat/threadViewStore';
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
/* 面板是「看一眼最新输出」的入口：截取尾部若干行即可，超出部分交给滚动条。 */
const OUTPUT_LINES = 200;
const OUTPUT_CHARS = 6000;
const OUTPUT_FOLLOW_THRESHOLD = 8;

type DockKind = 'tasks' | 'subagents';

const props = defineProps<{ sessionId?: string }>();
const { t } = useI18n();
const chatStore = useChatStore();
const commandStore = useCommandSessionStore();
const sessionId = computed(() => String(props.sessionId || chatStore.activeSessionId || '').trim());
/**
 * 命令会话事件在云端可能以“无请求归属”的方式到达，broker 列表也可能只在
 * 结束时才更新；这里额外从当前会话的进行中工具条目推断显式后台命令，
 * 让底部指示器在命令一开始就显示计数。普通前台执行与 yield 自动脱离不计入。
 */
type ComposerTaskRow = CommandSessionRuntimeEntry & { synthetic?: boolean };
const runningCommandRuns = computed<ComposerTaskRow[]>(() => {
  const cached = sessionId.value === String(chatStore.activeSessionId || '').trim()
    ? chatStore.messages
    : getSessionMessages(sessionId.value);
  const slots = threadViewSlots(sessionId.value) || [];
  const fromView = slots.flatMap(slot => [slot.assistant]).filter(Boolean);
  const rows: ComposerTaskRow[] = [];
  const seenItems = new Set<string>();
  for (const message of [...fromView, ...(Array.isArray(cached) ? cached : [])]) {
    const items = Array.isArray(message?.process_items) ? message.process_items : [];
    for (const item of items) {
      if (!item || item.kind !== 'tool_call' || String(item.status || '') !== 'running') continue;
      const payload = item.payload || {};
      const itemId = String(item.item_id || '');
      if (itemId && seenItems.has(itemId)) continue;
      if (itemId) seenItems.add(itemId);
      const tool = String(payload.tool || payload.tool_name || item.tool || '');
      if (!/exec|command|shell|terminal|bash/i.test(tool)) continue;
      const background = payload.run_in_background ?? payload.background
        ?? payload.args?.run_in_background ?? payload.args?.background;
      if (background !== true && background !== 'true') continue;
      const command = String(payload.command || payload.cmd || payload.args?.command || payload.args?.cmd || '').trim();
      rows.push({
        synthetic: true,
        background: true,
        commandSessionId: `tool:${String(item.item_id || command || rows.length)}`,
        toolCallId: String(item.tool_call_id || payload.tool_call_id || ''),
        userId: '', sessionId: sessionId.value, workspaceId: '',
        commandIndex: null,
        command: command || t('chat.composer.backgroundTasksCount', { count: 1 }),
        cwd: String(payload.cwd || ''),
        shell: '', launchMode: '', tty: false, interactive: false,
        status: 'running', seq: 0, startedAt: '', updatedAt: '', endedAt: '',
        exitCode: null, timedOut: false, error: '',
        stdoutBytes: 0, stderrBytes: 0, ptyBytes: 0,
        stdoutDroppedBytes: 0, stderrDroppedBytes: 0, ptyDroppedBytes: 0,
        stdoutTail: '', stderrTail: '', ptyTail: '', outputTail: ''
      });
    }
  }
  return rows;
});
const tasks = computed<ComposerTaskRow[]>(() => {
  const known = commandStore.listBySession(sessionId.value)
    .filter(task => task.status === 'running' && task.background);
  const knownToolCalls = new Set(
    known.map(task => String(task.toolCallId || '').trim()).filter(Boolean)
  );
  const inferred = runningCommandRuns.value.filter(run => {
    const toolCallId = String(run.toolCallId || '').trim();
    return !toolCallId || !knownToolCalls.has(toolCallId);
  });
  return [...known, ...inferred];
});
/**
 * 命令会话的 start/delta/exit 可能以“无请求归属”的方式到达（broker 重放、
 * 请求切换），多路复用找不到 request 时会把它们交给这个全局处理器，
 * 指示器才不会长期停在 0。
 */
const unregisterCommandEvents = registerWsGlobalEventHandler((type, dataText) => {
  if (!String(type || '').startsWith('command_session_')) return;
  let payload: any = null;
  try { payload = JSON.parse(dataText); } catch { return; }
  const inner = payload?.data && typeof payload.data === 'object' ? payload.data : payload;
  if (!inner || typeof inner !== 'object') return;
  const target = String(payload?.session_id || inner.session_id || sessionId.value || '').trim();
  if (!target) return;
  if (type === 'command_session_delta') {
    commandStore.appendDelta(
      target,
      inner.command_session_id || inner.commandSessionId,
      inner.stream,
      inner.delta,
      inner
    );
    return;
  }
  commandStore.upsertSnapshot(target, inner);
});
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
/** 只有仍在工作的子智能体才点亮图标；已完成的仍计入数字，供回看结果。 */
const activeSubagentCount = computed(() =>
  subagents.value.reduce((count, item) => (isSubagentItemActive(item) ? count + 1 : count), 0)
);
const expandedTaskIds = ref<Set<string>>(new Set());
const taskPanelRef = ref<HTMLElement | null>(null);
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
    // 浮层出现后再量一次尺寸：首次挂载时 clientHeight/scrollHeight 才有值。
    void nextTick(syncAllScrollThumbs);
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
  // 展开时定位到最新输出；之后由 watch 跟随，用户上滚即停止跟随。
  void nextTick(pinTaskOutput);
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
  const allLines = text.replace(/\r/g, '').split('\n');
  let truncated = allLines.length > OUTPUT_LINES;
  let tail = allLines.slice(-OUTPUT_LINES).join('\n');
  if (tail.length > OUTPUT_CHARS) {
    tail = tail.slice(-OUTPUT_CHARS);
    const cut = tail.indexOf('\n');
    if (cut >= 0 && cut < 200) tail = tail.slice(cut + 1);
    truncated = true;
  }
  return truncated ? `…\n${tail}` : tail;
};
/**
 * 应用全局隐藏了原生滚动条（styles/base.css），命令输出与任务列表改用自绘细滚动条：
 * 有溢出才显示、位置随滚动实时更新、按住可拖动，任何平台都能看出「还有内容」。
 */
/** 只找直接子级：任务行里的输出框也带滚动区，避免把内层滚条当成外层。 */
const directChild = (root: HTMLElement, selector: string): HTMLElement | null => {
  for (const child of Array.from(root.children)) {
    if (child.matches(selector)) return child as HTMLElement;
  }
  return null;
};
const scrollBodyOf = (shell: HTMLElement | null): HTMLElement | null =>
  shell ? directChild(shell, '[data-scroll-body]') : null;
const scrollThumbOf = (shell: HTMLElement | null): HTMLElement | null => {
  const track = shell ? directChild(shell, '.composer-scroll-track') : null;
  return track?.firstElementChild instanceof HTMLElement ? track.firstElementChild : null;
};
const syncScrollThumb = (shell: HTMLElement | null) => {
  const body = scrollBodyOf(shell);
  const thumb = scrollThumbOf(shell);
  if (!body || !thumb) return;
  const viewport = body.clientHeight;
  const total = body.scrollHeight;
  if (viewport <= 0 || total <= viewport + 1) {
    thumb.style.opacity = '0';
    thumb.style.height = '0px';
    return;
  }
  const height = Math.max(20, Math.round((viewport * viewport) / total));
  const maxTop = Math.max(0, viewport - height);
  const scrollable = Math.max(1, total - viewport);
  const progress = Math.min(1, Math.max(0, body.scrollTop / scrollable));
  thumb.style.opacity = '1';
  thumb.style.height = `${height}px`;
  thumb.style.transform = 'translateY(' + Math.round(progress * maxTop) + 'px)';
};
const syncAllScrollThumbs = () => {
  taskPanelRef.value?.querySelectorAll<HTMLElement>('[data-scroll-shell]')
    .forEach(shell => syncScrollThumb(shell));
};
const handleScrollSync = (event: Event) => {
  const el = event.currentTarget as HTMLElement | null;
  if (!el) return;
  syncScrollThumb(el.closest<HTMLElement>('[data-scroll-shell]'));
};
/** 用户手动上滚查看历史时停止跟随，回到底部后重新跟随最新输出。 */
const handleOutputScroll = (event: Event) => {
  const el = event.currentTarget as HTMLElement | null;
  if (!el) return;
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= OUTPUT_FOLLOW_THRESHOLD;
  el.dataset.follow = atBottom ? '1' : '0';
  syncScrollThumb(el.closest<HTMLElement>('[data-scroll-shell]'));
};
const pinTaskOutput = () => {
  const panel = taskPanelRef.value;
  if (!panel) return;
  panel.querySelectorAll<HTMLElement>('[data-task-output]').forEach(el => {
    if (el.dataset.follow !== '0') el.scrollTop = el.scrollHeight;
  });
  syncAllScrollThumbs();
};
const startThumbDrag = (event: PointerEvent) => {
  const thumb = event.currentTarget as HTMLElement | null;
  const body = scrollBodyOf(thumb?.closest<HTMLElement>('[data-scroll-shell]') ?? null);
  if (!thumb || !body) return;
  event.preventDefault();
  const thumbHeight = thumb.offsetHeight || 20;
  const span = Math.max(1, body.clientHeight - thumbHeight);
  const top = body.getBoundingClientRect().top;
  const move = (pointer: PointerEvent) => {
    const ratio = Math.min(1, Math.max(0, (pointer.clientY - top - thumbHeight / 2) / span));
    body.scrollTop = ratio * (body.scrollHeight - body.clientHeight);
  };
  const stop = () => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', stop);
    window.removeEventListener('pointercancel', stop);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', stop);
  window.addEventListener('pointercancel', stop);
  move(event);
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
/*
 * 运行态只由事件与进行中工具条目推断，不用 broker 列表回灌：云端在命令
 * 运行期返回的快照仍是 exited，回灌会把刚出现的 running 立刻抹掉。
 */
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
/* 命令输出会持续追加：展开着输出框时把视图钉在最新一行。 */
watch(tasks, () => {
  if (!taskPanelVisible.value || expandedTaskIds.value.size === 0) return;
  void nextTick(pinTaskOutput);
});
window.addEventListener('resize', syncAllScrollThumbs);
onBeforeUnmount(() => {
  window.removeEventListener('resize', syncAllScrollThumbs);
  unregisterCommandEvents();
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
  flex: 0 0 auto;
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
/* 任务浮层：头部固定，任务行单独滚动，浮层高度不越过视口。 */
.composer-task-popover {
  display: flex;
  flex-direction: column;
  max-height: min(58vh, 420px);
  overflow: hidden;
}
/* 任务行滚动区：外层承担 flex 高度，内层只负责滚动，滚条贴浮层内边距。 */
.composer-task-body { position: relative; flex: 1 1 auto; min-height: 0; display: flex; }
.composer-task-list { flex: 1 1 auto; min-width: 0; min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
/* 输出框：高度封顶，超出在框内滚动；文字不压在滚条下面。 */
.composer-task-output { position: relative; margin-top: 4px; }
.composer-task-output pre {
  max-height: min(240px, 34vh);
  margin: 0;
  padding-right: 8px;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font: 10px/1.45 var(--mz-font-mono, monospace);
}
/* 自绘滚条（全局隐藏了原生滚动条）：只在有溢出时显示，可按住拖动。 */
.composer-scroll-track {
  position: absolute;
  top: 0;
  right: -6px;
  bottom: 0;
  width: 6px;
  pointer-events: none;
}
.composer-scroll-thumb {
  width: 4px;
  height: 0;
  margin: 0 auto;
  border-radius: 2px;
  background: rgba(15, 23, 42, 0.2);
  opacity: 0;
  pointer-events: auto;
  cursor: grab;
  transition: background 0.12s ease;
}
.composer-scroll-thumb:hover { background: rgba(15, 23, 42, 0.38); }
.composer-scroll-thumb:active { cursor: grabbing; background: rgba(15, 23, 42, 0.5); }
.composer-subagent-popover { padding: 0; overflow: hidden; }
.composer-subagent-popover :deep(.subagent-panel) { margin: 0; border: 0; border-radius: 0; background: transparent; }
.composer-subagent-popover :deep(.subagent-panel__list) { max-height: 280px; }
.composer-subagent-popover .composer-status-panel__more { padding: 6px 12px 8px; }
</style>
