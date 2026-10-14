<template>
  <aside
    ref="sidebarEl"
    class="messenger-sidebar"
    data-testid="messenger-sidebar"
    :aria-label="t('messenger.sidebar.workspaceGroup')"
  >
    <div ref="workspaceRegionEl" class="messenger-sidebar-workspace-region" :style="workspaceRegionStyle">
      <div class="messenger-sidebar-group-title">{{ t('messenger.sidebar.workspaceGroup') }}</div>

      <!-- §6.3 工作区即行：云端工作区与本地工作区是同一份可拖拽条目列表（真正的同级，
           云端行也能拖）。线程树是云端工作区的子级，紧跟云端行渲染——云端行被拖到哪，
           它的线程就跟到哪（蛹同构的工作区/线程树）。
           本地行没有检测到工作区投影的设备不出条目（见 interlinkWorkspaceRows）；
           点击行把文件区切到对应目录（URL `?node=&ws=`），行顺序是蜂巢账号级偏好
           （云端存储），不回写本地设备。 -->
      <template v-for="entry in orderedWorkspaceEntries" :key="entry.key">
        <template v-if="entry.kind === 'cloud'">
          <div
            class="messenger-sidebar-workspace-row"
            :class="{
              'is-selected': !interlinkRemoteSelected,
              'is-dragging': dragKey === entry.key,
              'is-drop-target': dropTargetKey === entry.key
            }"
            role="button"
            tabindex="0"
            draggable="true"
            :aria-label="workspaceName"
            @click="handleWorkspaceRowClick"
            @keydown.enter.prevent="handleWorkspaceRowClick"
            @dragstart="handleRowDragStart(entry, $event)"
            @dragend="handleRowDragEnd"
            @dragover.prevent="handleRowDragOver(entry)"
            @dragleave="handleRowDragLeave(entry)"
            @drop.prevent="handleRowDrop(entry)"
          >
            <i
              class="messenger-sidebar-workspace-icon"
              :class="cloudRowSelected ? 'fa-solid fa-cloud' : 'fa-regular fa-cloud'"
              aria-hidden="true"
            ></i>
            <span class="messenger-sidebar-workspace-name" :title="workspaceName">{{ workspaceName }}</span>
            <span class="messenger-sidebar-workspace-meta">{{ workspaceMetaLabel }}</span>
            <!-- 唯一的「新建线程」入口（左栏顶部的「新任务」按钮已移除），所以常驻可见、不再悬停才出现。 -->
            <button
              class="messenger-sidebar-workspace-menu"
              type="button"
              :disabled="newTaskDisabled"
              :title="t('chat.newSession')"
              :aria-label="t('chat.newSession')"
              @click.stop="handleNewTask"
            >
              <i class="fa-solid fa-plus" aria-hidden="true"></i>
            </button>
          </div>

          <!-- 云端线程树：云端工作区的子级。打开线程同时把目录目标切回云端。 -->
          <!-- 线程是云端工作区的内容：只有云端工作区处于选中态才展开线程树；
               选中别的工作区行时收起，回到参考端「选中最上个工作区才铺开线程」的形态。 -->
          <MessengerThreadTree
            v-if="cloudRowSelected"
            :items="threads"
            :active-session-id="activeSessionId"
            :agent-id="agentIdForApi"
            @open="handleThreadOpen"
            @trajectory="emit('thread-trajectory', $event)"
            @rename="emit('rename-thread', $event)"
            @archive="emit('archive-thread', $event)"
          />
        </template>

        <button
          v-else
          class="messenger-sidebar-workspace-row"
          :class="{
            'is-selected': isRowSelected(entry),
            'is-dragging': dragKey === entry.key,
            'is-drop-target': dropTargetKey === entry.key
          }"
          type="button"
          draggable="true"
          :aria-label="t('interlink.workspace.rowAria', { name: entry.label })"
          @click="selectWorkspaceRow(entry)"
          @dragstart="handleRowDragStart(entry, $event)"
          @dragend="handleRowDragEnd"
          @dragover.prevent="handleRowDragOver(entry)"
          @dragleave="handleRowDragLeave(entry)"
          @drop.prevent="handleRowDrop(entry)"
        >
          <i
            class="messenger-sidebar-workspace-icon"
            :class="isRowSelected(entry) ? 'fa-solid fa-folder' : 'fa-regular fa-folder'"
            aria-hidden="true"
          ></i>
          <span class="messenger-sidebar-workspace-name" :title="entry.label">
            {{ entry.label }}
          </span>
        </button>
      </template>
    </div>

    <div
      ref="splitterEl"
      class="messenger-sidebar-splitter"
      :class="{ 'is-dragging': isResizing }"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('messenger.sidebar.resize')"
      tabindex="0"
      @pointerdown.prevent="startResize"
      @dblclick.prevent="resetResize"
      @keydown.up.prevent="nudgeResize(-24)"
      @keydown.down.prevent="nudgeResize(24)"
    ></div>

    <!-- Lower region: cloud working-directory interaction area (B2). -->
    <div class="messenger-sidebar-files-region" :style="filesRegionStyle">
      <WorkspaceFilesPanel />
    </div>

    <div ref="footerEl" class="messenger-sidebar-footer">
      <button
        class="messenger-sidebar-settings"
        :class="{ 'is-active': settingsActive }"
        type="button"
        :title="t('messenger.sidebar.settings')"
        @click="emit('open-settings')"
      >
        <!-- 对齐桌面参考主页面：按钮上直接展示当前用户（头像 + 名称），点击呼出设置窗口。 -->
        <span class="messenger-sidebar-avatar" :style="currentUserAvatarStyle" aria-hidden="true">
          <img
            v-if="currentUserAvatarImageUrl"
            class="messenger-sidebar-avatar-image"
            :src="currentUserAvatarImageUrl"
            alt=""
          />
          <span v-else class="messenger-sidebar-avatar-text">{{ avatarLabel(currentUsername) }}</span>
        </span>
        <span class="messenger-sidebar-settings-name">{{ currentUsername || t('user.guest') }}</span>
      </button>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import MessengerThreadTree from './MessengerThreadTree.vue';
import WorkspaceFilesPanel from '@/views/messenger/workspace/WorkspaceFilesPanel.vue';
import { useInterlinkNodes } from '@/views/messenger/interlink/useInterlinkNodes';
import { useInterlinkNodeTarget } from '@/views/messenger/interlink/interlinkNodeTarget';
import {
  fetchInterlinkWorkspaceOrder,
  interlinkDeviceTarget,
  saveInterlinkWorkspaceOrder
} from '@/api/interlink';
import {
  interlinkWorkspaceRows,
  type InterlinkWorkspaceRow
} from '@/views/messenger/interlink/interlinkNodeModel';
import {
  applyWorkspaceOrder,
  moveWorkspaceRow
} from '@/views/messenger/interlink/interlinkWorkspaceOrder';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { buildTaskList } from '@/views/messenger/taskList';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{
  'new-task': [];
  'select-workspace': [];
  'open-thread': [id: string];
  'thread-trajectory': [id: string];
  'rename-thread': [id: string];
  'archive-thread': [id: string];
  'open-settings': [];
}>();

const t = props.controller.t;
const chatStore = props.controller.chatStore;
const activeSessionId = computed(() => String(chatStore.activeSessionId || ''));
const agentIdForApi = props.controller.activeAgentIdForApi;
const newTaskDisabled = computed(
  () => Boolean(props.controller.creatingAgentSession?.value) || Boolean(props.controller.isMessengerInteractionBlocked?.value)
);
const settingsActive = computed(() => Boolean(props.controller.showChatSettingsView?.value));

// 设置按钮头像：复用与站点头部同一份用户头像投影，不新造接口。
const avatarLabel = props.controller.avatarLabel;
const currentUsername = props.controller.currentUsername;
const currentUserAvatarImageUrl = props.controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = props.controller.currentUserAvatarStyle;

// ---------------------------------------------------------------- workspace

const WORKSPACE_SPLIT_STORAGE_KEY = 'messenger:sidebar:split';
const MIN_SPLIT = 0.3;
const MAX_SPLIT = 0.8;
const DEFAULT_SPLIT = 0.6;

const workspaceName = computed(() => t('messenger.workspace.defaultName'));

// 搜索栏已移除（对齐桌面）：线程树恢复为不过滤。
// 注意：`controller.keywordInput` 仍是 sessionHub 搜索（中间栏 sections 过滤）的唯一输入源，
// 其 store 字段、debounce watcher 与 controller 内过滤逻辑保留未删，等主智能体决定是否重新挂载入口。
const threads = computed(() =>
  buildTaskList(chatStore.sessions || [], String(agentIdForApi?.value || ''), t('chat.newSession'))
);
const workspaceMetaLabel = computed(() => {
  const total = threads.value.length;
  if (!total) return t('messenger.status.idle');
  return t('messenger.sidebar.threadCount', { count: total });
});

// ------------------------------------------------- 本地工作区行（§6.3）

const interlink = useInterlinkNodes({});
const {
  target: interlinkTarget,
  workspaceId: interlinkWorkspaceId,
  isRemote: interlinkRemoteSelected,
  writeTarget: writeInterlinkTarget,
  backToCloud: backToCloudWorkspace
} = useInterlinkNodeTarget();

/** 节点 → 工作区行的映射是纯函数；上限在模型里统一收敛，不一次铺开。 */
const workspaceRows = computed(() => interlinkWorkspaceRows(interlink.nodes.value || []));

// ---------------------------------------------- 工作区条目排序（云端偏好，§6.3）

/** 云端工作区与本地工作区在侧栏是同一份条目列表：同级、同一套拖拽排序。 */
type WorkspaceSidebarEntry =
  | { kind: 'cloud'; key: string }
  | ({ kind: 'local' } & InterlinkWorkspaceRow);

const CLOUD_ENTRY_KEY = 'cloud';
const cloudSidebarEntry: WorkspaceSidebarEntry = { kind: 'cloud', key: CLOUD_ENTRY_KEY };

const workspaceOrder = ref<string[]>([]);
const workspaceEntries = computed<WorkspaceSidebarEntry[]>(() => [
  cloudSidebarEntry,
  ...workspaceRows.value.map((row): WorkspaceSidebarEntry => ({ kind: 'local', ...row }))
]);
const orderedWorkspaceEntries = computed<WorkspaceSidebarEntry[]>(() =>
  applyWorkspaceOrder(workspaceEntries.value, workspaceOrder.value)
);
const dragKey = ref('');
const dropTargetKey = ref('');
let orderSaveTimer: number | null = null;

onMounted(() => {
  // 排序是偏好：加载失败就按自然顺序展示，不阻塞侧栏。
  void fetchInterlinkWorkspaceOrder()
    .then((keys) => {
      workspaceOrder.value = keys;
    })
    .catch(() => undefined);
});

const handleRowDragStart = (entry: { key: string }, event: DragEvent): void => {
  dragKey.value = entry.key;
  dropTargetKey.value = '';
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move';
    // Firefox 需要 setData 才会真正进入拖拽。
    try {
      event.dataTransfer.setData('text/plain', entry.key);
    } catch {
      // 老内核不支持时不影响本地状态驱动的拖拽。
    }
  }
};

const handleRowDragOver = (entry: { key: string }): void => {
  if (dragKey.value && entry.key !== dragKey.value) dropTargetKey.value = entry.key;
};

const handleRowDragLeave = (entry: { key: string }): void => {
  if (dropTargetKey.value === entry.key) dropTargetKey.value = '';
};

const handleRowDragEnd = (): void => {
  dragKey.value = '';
  dropTargetKey.value = '';
};

const handleRowDrop = (entry: { key: string }): void => {
  const dragged = dragKey.value;
  handleRowDragEnd();
  if (!dragged || dragged === entry.key) return;
  const next = moveWorkspaceRow(
    orderedWorkspaceEntries.value.map((item) => item.key),
    dragged,
    entry.key
  );
  if (!next) return;
  workspaceOrder.value = next;
  scheduleOrderSave();
};

/** 合并保存：连拖多次只落一次库；失败不打断使用（本会话顺序已生效）。 */
const scheduleOrderSave = (): void => {
  if (orderSaveTimer) window.clearTimeout(orderSaveTimer);
  orderSaveTimer = window.setTimeout(() => {
    orderSaveTimer = null;
    void saveInterlinkWorkspaceOrder(workspaceOrder.value).catch(() => undefined);
  }, 600);
};

const isRowSelected = (row: InterlinkWorkspaceRow): boolean => {
  if (!interlinkRemoteSelected.value) return false;
  if (interlinkTarget.value !== interlinkDeviceTarget(row.deviceId)) return false;
  const ws = interlinkWorkspaceId.value;
  if (!ws) {
    // 缺省 ws＝该设备的默认目录：高亮该设备的第一行，避免「远程目标却无选中行」。
    const first = workspaceRows.value.find(
      (item) => interlinkDeviceTarget(item.deviceId) === interlinkTarget.value
    );
    return first?.key === row.key;
  }
  return ws === row.workspaceId;
};

/** 云端工作区行的选中态：没有远程本地目标时云端行即选中，同时控制图标实心化与线程树展开。 */
const cloudRowSelected = computed(() => !interlinkRemoteSelected.value);

// 目标自愈（§6.3）：URL 里残留的本地目标若对应不到任何工作区行（设备从未同步
// 投影、工作区已删除、设备被吊销等），一律回落云端——文件区绝不停留在一个
// 已不存在的本地目录上。等节点列表完成首次加载后再判，避免把有效深链误杀。
const targetHasRow = computed(() => {
  if (!interlinkRemoteSelected.value) return true;
  const target = interlinkTarget.value;
  const ws = interlinkWorkspaceId.value;
  return workspaceRows.value.some(
    (row) => interlinkDeviceTarget(row.deviceId) === target && (!ws || row.workspaceId === ws)
  );
});

watch(
  () => interlinkRemoteSelected.value && interlink.lastLoadedAt.value > 0 && !targetHasRow.value,
  (stale) => {
    if (stale) backToCloudWorkspace();
  },
  { immediate: true }
);

const selectWorkspaceRow = (row: InterlinkWorkspaceRow): void => {
  writeInterlinkTarget(interlinkDeviceTarget(row.deviceId), row.workspaceId);
};

const handleWorkspaceRowClick = (): void => {
  // 回到云端工作区上下文：文件区跟随 `?node=` 切回云端目录。
  backToCloudWorkspace();
  emit('select-workspace');
};

/** 线程是云端工作区的内容：打开线程（含新建）即把目录目标切回云端，
 * 文件区跟着回到云端工作目录——不会停在某个本地工作区的只读投影上。 */
const handleThreadOpen = (id: string): void => {
  backToCloudWorkspace();
  emit('open-thread', id);
};

const handleNewTask = (): void => {
  backToCloudWorkspace();
  emit('new-task');
};

// ------------------------------------------------------------------ resizer

const sidebarEl = ref<HTMLElement | null>(null);
const workspaceRegionEl = ref<HTMLElement | null>(null);
const splitterEl = ref<HTMLElement | null>(null);
const footerEl = ref<HTMLElement | null>(null);
const splitRatio = ref(DEFAULT_SPLIT);
const isResizing = ref(false);
/** 按下分隔条时，指针相对分隔条顶边的偏移：拖拽全程保持不变，
 *  否则分隔条顶边会直接吸到指针上，整条跳到指针下方。 */
let resizeGrabOffset = 0;

try {
  const stored = Number.parseFloat(String(localStorage.getItem(WORKSPACE_SPLIT_STORAGE_KEY) || ''));
  if (Number.isFinite(stored) && stored >= MIN_SPLIT && stored <= MAX_SPLIT) {
    splitRatio.value = stored;
  }
} catch {
  // Keep the default ratio when storage is unavailable.
}

const workspaceRegionStyle = computed(() => ({ flex: `${splitRatio.value} 1 0%` }));
const filesRegionStyle = computed(() => ({ flex: `${1 - splitRatio.value} 1 0%` }));

const persistSplit = () => {
  try {
    localStorage.setItem(WORKSPACE_SPLIT_STORAGE_KEY, splitRatio.value.toFixed(3));
  } catch {
    // Preference only.
  }
};

/** 可伸缩轨道高度＝侧栏高度减去固定的分隔条与页脚：分栏比例只作用于这段轨道，
 *  取整个侧栏高度会把固定的页脚也算进比例，按下瞬间分栏会整体跳一下。 */
const resizableTrackHeight = (): number => {
  const rect = sidebarEl.value?.getBoundingClientRect();
  if (!rect || rect.height <= 0) return 0;
  const splitterHeight = splitterEl.value?.offsetHeight ?? 0;
  const footerHeight = footerEl.value?.offsetHeight ?? 0;
  return Math.max(1, rect.height - splitterHeight - footerHeight);
};

const handleResizeMove = (event: PointerEvent) => {
  if (!isResizing.value) return;
  const rect = sidebarEl.value?.getBoundingClientRect();
  if (!rect || rect.height <= 0) return;
  const track = resizableTrackHeight();
  if (track <= 0) return;
  // 轨道内高度＝指针位置减去按下时记录的抓取偏移：分隔条跟着指针平移，
  // 而不是让顶边吸到指针上（那样整条会瞬间移到指针下方）。
  const pointerTrackY = event.clientY - rect.top - resizeGrabOffset;
  const next = Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, pointerTrackY / track));
  splitRatio.value = next;
};

const stopResize = () => {
  if (!isResizing.value) return;
  isResizing.value = false;
  window.removeEventListener('pointermove', handleResizeMove);
  persistSplit();
};

const startResize = (event: PointerEvent) => {
  isResizing.value = true;
  // 记录抓取点：指针相对分隔条顶边（即可伸缩轨道底边）的偏移，拖拽全程保持，
  // 分隔条跟着指针平移而不产生跳变。
  const splitterRect = splitterEl.value?.getBoundingClientRect();
  resizeGrabOffset = splitterRect ? event.clientY - splitterRect.top : 0;
  (event.currentTarget as HTMLElement | null)?.focus?.();
  window.addEventListener('pointermove', handleResizeMove);
  window.addEventListener('pointerup', stopResize, { once: true });
};

const resetResize = () => {
  splitRatio.value = DEFAULT_SPLIT;
  persistSplit();
};

const nudgeResize = (delta: number) => {
  const track = resizableTrackHeight();
  if (track <= 0) return;
  splitRatio.value = Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, splitRatio.value + delta / track));
  persistSplit();
};

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', handleResizeMove);
  window.removeEventListener('pointerup', stopResize);
  if (orderSaveTimer) window.clearTimeout(orderSaveTimer);
  orderSaveTimer = null;
});
</script>
