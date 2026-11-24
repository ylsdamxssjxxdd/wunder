<template>
  <div
    class="input-container input-container--world"
    :class="{ dragover: dragActive }"
    @dragenter="handleDragEnter"
    @dragover="handleDragOver"
    @dragleave="handleDragLeave"
    @drop="handleDrop"
  >
    <input
      ref="fileInputRef"
      class="composer-file-input"
      type="file"
      multiple
      tabindex="-1"
      data-testid="chat-composer-file-input"
      @change="handleComposerFileInputChange"
    />
    <!-- 手机上传文件与拍照共用工作目录落地链路；输入常驻 DOM，iOS 才能在选择器点击手势里直接打开。 -->
    <input
      v-if="phoneLayout"
      ref="phonePhotoInputRef"
      class="composer-file-input"
      type="file"
      accept="image/*"
      capture="environment"
      tabindex="-1"
      @change="handleComposerFileInputChange"
    />

    <div v-if="showUploadArea" class="upload-preview" data-testid="chat-composer-attachments">
      <div class="upload-preview-list">
        <div
          v-for="attachment in attachments"
          :key="attachment.id"
          class="upload-preview-item"
          :title="attachment.workspace_path || attachment.name"
          :class="{
            'upload-preview-item--video': attachment.type === 'video' || attachment.type === 'gif',
            'is-active': (attachment.type === 'video' || attachment.type === 'gif') && isVideoControlOpen(attachment.id),
            'is-processing': isAttachmentProcessing(attachment.id)
          }"
        >
          <div
            class="upload-preview-main"
            :class="{ 'upload-preview-main--button': attachment.type === 'video' || attachment.type === 'gif' }"
            :role="attachment.type === 'video' || attachment.type === 'gif' ? 'button' : undefined"
            :tabindex="attachment.type === 'video' || attachment.type === 'gif' ? 0 : undefined"
            @click="(attachment.type === 'video' || attachment.type === 'gif') && toggleVideoControl(attachment.id)"
            @keydown.enter.prevent="(attachment.type === 'video' || attachment.type === 'gif') && toggleVideoControl(attachment.id)"
            @keydown.space.prevent="(attachment.type === 'video' || attachment.type === 'gif') && toggleVideoControl(attachment.id)"
          >
            <div class="upload-preview-visual" aria-hidden="true">
              <img
                v-if="attachment.preview_url"
                class="upload-preview-image"
                :src="attachment.preview_url"
                alt=""
                draggable="false"
                @error="handleAttachmentPreviewError(attachment.id)"
              />
              <img
                v-else-if="attachment.type === 'workspace'"
                class="upload-preview-doc-icon"
                :src="resolveComposerAttachmentIcon(attachment.name)"
                alt=""
                draggable="false"
              />
              <i
                v-else
                :class="['fa-solid', resolveAttachmentIconClass(attachment), 'upload-preview-icon']"
              ></i>
            </div>
            <div class="upload-preview-copy">
              <span class="upload-preview-name" :title="attachment.name">{{ attachment.name }}</span>
              <span v-if="resolveAttachmentMeta(attachment)" class="upload-preview-meta">
                {{ resolveAttachmentMeta(attachment) }}
              </span>
              <span
                v-if="(attachment.type === 'video' || attachment.type === 'gif') && attachment.warnings?.length"
                class="upload-preview-warning"
                :title="attachment.warnings[0]"
              >
                {{ attachment.warnings[0] }}
              </span>
            </div>
          </div>
          <button
            class="upload-preview-remove"
            type="button"
            :title="t('common.remove')"
            :aria-label="t('common.remove')"
            @click.stop="removeAttachment(attachment.id)"
          >
            <i class="fa-solid fa-xmark upload-preview-remove-icon" aria-hidden="true"></i>
          </button>
          <div
            v-if="(attachment.type === 'video' || attachment.type === 'gif') && isVideoControlOpen(attachment.id)"
            class="upload-preview-video-controls"
          >
            <label v-if="attachment.type === 'video'" class="upload-preview-video-field">
              <span class="upload-preview-video-label">
                {{ t('chat.attachments.video.frameRate') }}
              </span>
              <input
                class="upload-preview-video-input"
                type="number"
                min="0.1"
                max="12"
                step="0.25"
                :value="resolveVideoFrameRateInput(attachment.id)"
                @input="handleVideoFrameRateInput(attachment.id, $event)"
                @keydown.enter.prevent="applyVideoFrameRate(attachment.id)"
              />
            </label>
            <label v-else class="upload-preview-video-field">
              <span class="upload-preview-video-label">
                {{ t('chat.attachments.gif.frameStep') }}
              </span>
              <input
                class="upload-preview-video-input"
                type="number"
                min="0"
                max="120"
                step="1"
                :value="resolveGifFrameStepInput(attachment.id)"
                @input="handleGifFrameStepInput(attachment.id, $event)"
                @keydown.enter.prevent="applyGifFrameStep(attachment.id)"
              />
            </label>
            <button
              class="upload-preview-video-apply"
              type="button"
              :disabled="!attachment.source_public_path || isAttachmentProcessing(attachment.id)"
              @click="attachment.type === 'video' ? applyVideoFrameRate(attachment.id) : applyGifFrameStep(attachment.id)"
            >
              {{ attachment.type === 'video' ? t('chat.attachments.video.reextract') : t('chat.attachments.gif.reextract') }}
            </button>
            <div class="upload-preview-video-summary">
              {{ resolveVideoControlSummary(attachment) }}
            </div>
          </div>
        </div>
      </div>
      <div v-if="composerBusy > 0" class="upload-preview-status" role="status">
        {{ workspaceDropBusy > 0 ? t('chat.workspaceDrop.uploading') : t('chat.attachments.processing', { count: attachmentBusy }) }}
      </div>
    </div>

    <!-- 排队条只承载输入队列：当前轮还在跑时发出的消息先停在这里，可插话 / 编辑 / 删除 / 拖动排序。
         等待全局并发槽位的轮次已经算「发出」，留在聊天时间线里显示排队中。 -->
    <ComposerQueueStrip
      :session-id="activeQueueSessionId"
      @edit="handleQueueTurnEdit"
      @withdrawn="handleQueueTurnWithdrawn"
    />

    <!-- 输入卡与桌面一致：白底 + 细边 + 圆角，编辑器与底部工具行共用这一张底。 -->
    <div class="input-box input-box--world" @mousedown="handleComposerSurfacePointerDown">
      <textarea
        data-testid="chat-composer-input"
        v-model="inputText"
        ref="inputRef"
        class="composer-textarea"
        :placeholder="inputPlaceholder"
        rows="1"
        @input="handleInput"
        @click="syncCaretPosition"
        @keyup="syncCaretPosition"
        @keydown="handleInputKeydown"
        @paste="handleComposerPaste"
      />

      <!-- 录音中的计时文本随麦克风进入工具栏（对齐桌面 composer.slint:610-618）。 -->
      <div v-if="voiceTranscribing" class="composer-voice-indicator">
        <span class="composer-voice-rings" aria-hidden="true">
          <span></span><span></span><span></span>
        </span>
        <span>{{ voiceTranscribingLabel }}</span>
      </div>

      <div class="composer-action-row">
        <div class="composer-action-group">
          <!-- 「+」放上传文件与预设问题（点一下即填入草稿）；常驻状态格只列后台任务与子智能体。 -->
          <div ref="plusMenuAnchorRef" class="composer-anchor">
            <button
              class="composer-plus-btn"
              type="button"
              data-testid="chat-composer-plus"
              :class="{ 'is-active': plusPanelVisible }"
              :title="t('chat.composer.moreActions')"
              :aria-label="t('chat.composer.moreActions')"
              :aria-expanded="plusPanelVisible"
              @click.stop="togglePlusMenu"
            >
              <i class="fa-solid fa-plus composer-plus-icon" aria-hidden="true"></i>
            </button>

            <div v-if="plusPanelVisible" ref="plusPanelRef" class="composer-panel composer-panel--plus" @click.stop>
              <!-- 桌面一栏：上传文件 + 预设问题；输入 `/` 时同一栏让位给命令建议。 -->
              <div v-if="commandSuggestionsVisible" class="composer-plus-suggestions" role="listbox">
                <button
                  v-for="(item, index) in commandSuggestions"
                  :key="item.command"
                  class="command-menu-item"
                  :class="{ active: index === commandMenuIndex }"
                  type="button"
                  role="option"
                  :aria-selected="index === commandMenuIndex"
                  @mousedown.prevent="applyCommandSuggestion(index)"
                  @mouseenter="setCommandMenuIndex(index)"
                >
                  <span class="command-menu-name">{{ item.command }}</span>
                  <span class="command-menu-desc">{{ item.description }}</span>
                </button>
                <div class="command-menu-hint">{{ t('chat.commandMenu.hint') }}</div>
              </div>
              <!-- 手机对齐安卓版：第一页三个入口（预设问题 / 上传文件 / 拍照），
                   预设问题再进第二页列表，返回键在第二页标题行。 -->
              <template v-else-if="phoneLayout && phonePlusPage === 0">
                <div class="composer-panel-title">{{ t('chat.composer.addToChat') }}</div>
                <button class="composer-plus-action" type="button" @click="openPhonePresetPage">
                  <i class="fa-solid fa-wand-magic-sparkles composer-plus-action-icon" aria-hidden="true"></i>
                  <span>{{ t('chat.commandMenu.presetQuestions') }}</span>
                  <i class="fa-solid fa-chevron-right composer-plus-action-caret" aria-hidden="true"></i>
                </button>
                <button class="composer-plus-action" type="button" @click="pickWorkspaceFiles">
                  <i class="fa-solid fa-paperclip composer-plus-action-icon" aria-hidden="true"></i>
                  <span>{{ t('chat.composer.uploadFile') }}</span>
                </button>
                <button class="composer-plus-action" type="button" @click="pickPhonePhoto">
                  <i class="fa-solid fa-camera composer-plus-action-icon" aria-hidden="true"></i>
                  <span>{{ t('chat.composer.takePhoto') }}</span>
                </button>
              </template>
              <template v-else>
                <button
                  v-if="!phoneLayout"
                  class="composer-plus-action"
                  type="button"
                  data-testid="chat-composer-upload-files"
                  @click="pickWorkspaceFiles"
                >
                  <i class="fa-solid fa-paperclip composer-plus-action-icon" aria-hidden="true"></i>
                  <span>{{ t('chat.composer.uploadFile') }}</span>
                </button>
                <div v-if="phoneLayout" class="composer-plus-page-head">
                  <button
                    class="composer-plus-back-btn"
                    type="button"
                    :title="t('chat.commandMenu.presetQuestions')"
                    :aria-label="t('chat.commandMenu.presetQuestions')"
                    @click="phonePlusPage = 0"
                  >
                    <i class="fa-solid fa-chevron-left" aria-hidden="true"></i>
                  </button>
                  <span class="composer-plus-page-title">{{ t('chat.commandMenu.presetQuestions') }}</span>
                </div>
                <div v-else class="composer-panel-title">{{ t('chat.commandMenu.presetQuestions') }}</div>
                <ComposerPresetQuestions
                  v-if="presetQuestionItems.length"
                  :items="presetQuestionItems"
                  @pick="applyPresetQuestion"
                />
                <div v-else class="composer-panel-hint">{{ t('chat.composer.presetEmpty') }}</div>
              </template>
            </div>
          </div>

          <ComposerStatusBar :session-id="targetSessionId" />
        </div>

          <div class="composer-action-group composer-action-group--end">
            <!-- 右组（对齐桌面 composer.slint:637-663）：模型 + 上下文占用 → 发送/停止。
                 占用统计不再单独占一个图标位：触发器上的大脑就是占用图标（按占用率填充），
                 数字与进度在浮层里展开。数据仍取自**唯一**一份投影 sessionContextUsage.ts。 -->
            <!-- 手机把模型入口移到顶部标题区（对齐安卓版），输入区不再占模型位。 -->
            <div v-if="!phoneLayout" ref="modelMenuAnchorRef" class="composer-anchor">
              <button
                class="composer-model-trigger"
                type="button"
                :class="{ 'is-active': modelMenuVisible }"
                :title="modelTriggerTitle"
                :aria-label="modelTriggerTitle"
                :aria-expanded="modelMenuVisible"
                @click.stop="toggleModelMenu"
              >
                <ContextUsageIcon class="composer-model-usage-icon" :ratio="contextUsage.ratio" :compacted="contextUsage.compacted" />
                <span class="composer-model-name">{{ modelTriggerLabel }}</span>
                <span v-if="reasoningEffortCompactLabel" class="composer-model-effort">
                  {{ reasoningEffortCompactLabel }}
                </span>
                <span v-if="modelSwitching" class="composer-model-spinner" aria-hidden="true"></span>
                <i v-else class="fa-solid fa-chevron-down composer-caret" aria-hidden="true"></i>
              </button>
              <ModelPickerPopover
                v-if="modelMenuVisible"
                :items="modelOptions"
                :active-model-id="composerModelName"
                :context-usage="contextUsage"
                :loading="modelCatalog.loading"
                :failed="modelCatalog.failed"
                :busy="modelSwitching"
                @select="selectModel"
                @retry="reloadModelCatalog"
                @close="closeComposerPanels"
              />
            </div>

            <button
              class="composer-send-btn"
              data-testid="chat-composer-send"
              :data-mode="stopButtonActive ? 'stop' : 'send'"
              type="button"
              :disabled="!canSendOrStop"
              :title="sendButtonTitle"
              :aria-label="sendButtonTitle"
              @click="handleSendOrStop"
            >
              <i v-if="stopButtonActive" class="fa-solid fa-stop composer-send-icon" aria-hidden="true"></i>
              <i v-else class="fa-solid fa-arrow-up composer-send-icon" aria-hidden="true"></i>
            </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import ComposerPresetQuestions from '@/components/chat/ComposerPresetQuestions.vue';
import ComposerStatusBar from '@/components/chat/ComposerStatusBar.vue';
import ContextUsageIcon from '@/components/chat/ContextUsageIcon.vue';
import ModelPickerPopover from '@/components/chat/ModelPickerPopover.vue';

import { processChatMediaAttachment } from '@/api/chat';
import { createWunderWorkspaceDir, uploadWunderWorkspace } from '@/api/workspace';
import {
  clearComposerDraftState,
  readComposerDraftState,
  writeComposerDraftState,
  type ComposerDraftAttachment
} from '@/components/chat/composerDraftCache';
import {
  composerModelCatalog,
  ensureComposerModelCatalog,
  type ComposerModelOption
} from '@/components/chat/composerModelCatalog';
import { useI18n } from '@/i18n';
import ComposerQueueStrip from '@/components/chat/ComposerQueueStrip.vue';
import { scheduleChatQueueRefresh } from '@/stores/chatQueueState';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import {
  collectWorkspaceClipboardFiles,
  collectWorkspaceDropTree,
  resolveEmptyDropDirectories,
  type WorkspaceDroppedFile,
  type WorkspaceDropEntry,
  type WorkspaceDroppedTree
} from '@/utils/workspaceDrop';
import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { clearWorkspaceDragPaths, hasWorkspaceDragPaths, readWorkspaceDragPaths } from '@/components/chat/workspaceDrag';
import {
  createComposerImagePreview,
  normalizeClipboardDroppedFile,
  resolveComposerAttachmentIcon,
  revokeComposerImagePreview
} from '@/components/chat/composerAttachments';

import { useSessionContextUsage } from '@/views/messenger/sessionContextUsage';
import { readChatLiveContextUsage } from '@/realtime/chat/chatLiveContextUsage';
// B2 hand-off: the sidebar file area queues workspace references here.
import {
  pendingWorkspaceChatReferences,
  takeWorkspaceChatReferences
} from '@/views/messenger/workspace/workspaceChatReference';

const props = defineProps({
  loading: {
    type: Boolean,
    default: false
  },
  demoMode: {
    type: Boolean,
    default: false
  },
  inquiryActive: {
    type: Boolean,
    default: false
  },
  inquirySelection: {
    type: Array,
    default: () => []
  },
  sendKey: {
    type: String,
    default: 'enter'
  },
  draftKey: {
    type: String,
    default: ''
  },
  sessionId: {
    type: String,
    default: ''
  },
  // 语音录音与审批选择目前没有输入区入口（审批在设置页「智能体」里配置），
  // 契约保留：`voiceRecording`/`voiceTranscribing` 仍驱动转写提示与发送快照，
  // `approvalMode` 仍随消息上送。
  voiceSupported: {
    type: Boolean,
    default: false
  },
  voiceRecording: {
    type: Boolean,
    default: false
  },
  voiceDurationMs: {
    type: Number,
    default: 0
  },
  voiceTranscribing: {
    type: Boolean,
    default: false
  },
  approvalMode: {
    type: String,
    default: ''
  },
  approvalModeEditable: {
    type: Boolean,
    default: false
  },
  approvalModeSyncing: {
    type: Boolean,
    default: false
  },
  modelName: {
    type: String,
    default: ''
  },
  /**
   * Model switch executor owned by the messenger controller: applies the model
   * to the current thread and reports the outcome back to the popover.
   */
  applyModel: {
    type: Function,
    default: null
  },
  contextMessages: {
    type: Array,
    default: () => []
  },
  presetQuestions: {
    type: Array,
    default: () => []
  },
  workspaceAgentId: {
    type: String,
    default: ''
  },
  workspaceContainerId: {
    type: [Number, String],
    default: 1
  },
  reasoningEffort: {
    type: String,
    default: 'default'
  },
  /**
   * 手机布局（云端版 <= 720px）：模型入口上移到顶部标题区，输入区「+」改为
   * 预设问题 / 上传文件 / 拍照三入口，对齐安卓版。
   */
  phoneLayout: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits([
  'send',
  'stop',
  'new-thread',
  'open-thread',
  'toggle-voice-record',
  'update:approval-mode'
]);

const normalizeOptionalNumber = (value: unknown): number | null => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
};

const inputText = ref('');
const inputRef = ref(null);
const attachments = ref<ComposerDraftAttachment[]>([]);
const attachmentBusy = ref(0);
const workspaceDropBusy = ref(0);
const dragActive = ref(false);
const dragCounter = ref(0);
const modelMenuAnchorRef = ref<HTMLElement | null>(null);
const plusMenuAnchorRef = ref<HTMLElement | null>(null);
const plusPanelRef = ref<HTMLElement | null>(null);
// 文件选择器常驻 DOM；手机相机使用单独输入。
const fileInputRef = ref<HTMLInputElement | null>(null);
const phonePhotoInputRef = ref<HTMLInputElement | null>(null);
const phonePlusPage = ref(0);
const modelMenuVisible = ref(false);
const plusMenuVisible = ref(false);
const modelSwitching = ref(false);
type ReasoningEffort = 'default' | 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh';
const normalizeReasoningEffort = (value: unknown): ReasoningEffort => {
  const normalized = String(value || '').trim().toLowerCase();
  return (['none', 'minimal', 'low', 'medium', 'high', 'xhigh'] as string[]).includes(normalized)
    ? normalized as ReasoningEffort
    : 'default';
};
const reasoningEffort = ref<ReasoningEffort>(normalizeReasoningEffort(props.reasoningEffort));
const caretPosition = ref(0);
const commandMenuIndex = ref(0);
const commandMenuDismissed = ref(false);
const expandedVideoAttachmentId = ref('');
const videoFrameRateDrafts = ref<Record<string, string>>({});
const gifFrameStepDrafts = ref<Record<string, string>>({});
const attachmentProcessingIds = ref<string[]>([]);
const DRAFT_PERSIST_DEBOUNCE_MS = 240;
let draftPersistTimer: ReturnType<typeof setTimeout> | null = null;
const { t } = useI18n();
const chatStore = useChatStore();
const targetSessionId = computed(() => String(props.sessionId || chatStore.activeSessionId || '').trim());

type AttachmentPayload = {
  type: string;
  name: string;
  content: string;
  mime_type?: string;
  public_path?: string;
};

type ProcessedMediaAttachment = {
  name?: string;
  content?: string;
  content_type?: string;
  mime_type?: string;
  public_path?: string;
};

type ProcessedMediaResponse = {
  kind?: string;
  name?: string;
  source_public_path?: string;
  duration_ms?: number;
  requested_frame_rate?: number;
  applied_frame_rate?: number;
  requested_frame_step?: number;
  applied_frame_step?: number;
  total_frame_count?: number;
  frame_count?: number;
  has_audio?: boolean;
  warnings?: string[];
  attachments?: ProcessedMediaAttachment[];
};

type ApprovalModeOption = {
  value: string;
  label: string;
  description: string;
  icon: string;
};

type SendKeyMode = 'enter' | 'ctrl_enter' | 'none';

type SlashCommandDefinition = {
  command: string;
  aliases: string[];
  descriptionKey: string;
};
// 1-8 rows of 14px text at 1.45 line-height, then the textarea scrolls internally.
const INPUT_MAX_HEIGHT = 168;
const resolveDraftKey = (): string => String(props.draftKey || '').trim();
const resolveKeyboardKeyCode = (event: KeyboardEvent): number =>
  Number(
    (
      event as KeyboardEvent & {
        keyCode?: number;
        which?: number;
      }
    ).keyCode ??
      (
        event as KeyboardEvent & {
          keyCode?: number;
          which?: number;
        }
      ).which ??
      0
  );
const isEnterKeyboardEvent = (event: KeyboardEvent): boolean => {
  const key = String(event.key || '').toLowerCase();
  const code = String(event.code || '').toLowerCase();
  const keyCode = resolveKeyboardKeyCode(event);
  return (
    key === 'enter' ||
    key === 'return' ||
    code === 'enter' ||
    code === 'numpadenter' ||
    keyCode === 13 ||
    keyCode === 10
  );
};
const hasPrimarySendModifier = (event: KeyboardEvent): boolean =>
  Boolean(
    event.ctrlKey ||
      event.metaKey ||
      event.getModifierState?.('Control') ||
      event.getModifierState?.('Meta')
  );
const hasBackupSendModifier = (event: KeyboardEvent): boolean =>
  Boolean(event.altKey && !hasPrimarySendModifier(event));
// Active durable session record; shared by the reasoning-effort sync watcher and
// the toolbar context-usage projection (same source as `activeSessionRecord`).
const resolveCurrentSession = (): Record<string, unknown> | null => {
  const activeSessionId = targetSessionId.value;
  if (!activeSessionId) {
    return null;
  }
  return (
    (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).find(
    (item) => String((item as Record<string, unknown> | null)?.id || '').trim() === activeSessionId
    ) as Record<string, unknown> | undefined
  ) ?? null;
};

const composerBusy = computed(() => attachmentBusy.value + workspaceDropBusy.value);
const showUploadArea = computed(() => attachments.value.length > 0 || composerBusy.value > 0);
const chatBusyMessage = computed(() =>
  workspaceDropBusy.value > 0 ? t('chat.workspaceDrop.uploading') : t('chat.attachments.busy')
);
const composerModelName = computed(() => String(props.modelName || '').trim());
const composerModelMissing = computed(() => {
  const name = composerModelName.value;
  if (!name) return true;
  return name === t('desktop.system.modelUnnamed');
});
// §8.3 trigger: model icon + model name (13px) + reasoning effort (gray) + caret.
const composerModelDisplayName = computed(() =>
  composerModelMissing.value ? t('chat.composer.modelUnset') : composerModelName.value
);
const modelCatalog = computed(() => composerModelCatalog.value);
const modelOptions = computed<ComposerModelOption[]>(() =>
  Array.isArray(composerModelCatalog.value.items) ? composerModelCatalog.value.items : []
);
const modelTriggerLabel = computed(() => composerModelDisplayName.value);
// 触发器同时承载模型与占用：悬浮给出「切换模型: <模型> · 上下文占用 12k / 128k」。
const modelTriggerTitle = computed(
  () => `${t('chat.composer.modelSelect')}: ${modelTriggerLabel.value} · ${contextUsageTitle.value}`
);
const reasoningEffortCompactLabel = computed(() => {
  if (reasoningEffort.value === 'default') return '';
  const option = reasoningEffortOptions.value.find((item) => item.value === reasoningEffort.value);
  const label = String(option?.label || reasoningEffort.value);
  const splitIndex = ['（', '(']
    .map((marker) => label.indexOf(marker))
    .filter((index) => index > 0)
    .sort((left, right) => left - right)[0];
  return typeof splitIndex === 'number' ? label.slice(0, splitIndex).trim() || label : label;
});
// 占用统计只有 `sessionContextUsage.ts` 一份实现：模型触发器上的大脑、百分比和
// 浮层里的明细都读同一个投影，同一屏不会漂出两个百分比。usage 帧不进入消息
// 投影，因此这里优先采用 WS 上最新观测到的实时快照，避免整轮停留在旧值或 0。
const liveContextUsage = computed(() => readChatLiveContextUsage(targetSessionId.value));
const contextUsage = useSessionContextUsage({
  scope: () => `${targetSessionId.value}:${composerModelName.value}`,
  messages: () => (Array.isArray(props.contextMessages) ? props.contextMessages : []),
  session: () => resolveCurrentSession(),
  loading: () => Boolean(props.loading),
  modelName: () => composerModelName.value,
  observedUsedTokens: () => liveContextUsage.value?.used ?? null,
  observedTotalTokens: () => liveContextUsage.value?.total ?? null
});
const contextUsageTitle = computed(() => {
  const counts = String(contextUsage.value.counts || '').trim();
  return counts ? `${t('profile.stats.contextTokens')} ${counts}` : t('profile.stats.contextTokens');
});
const hasInquirySelection = computed(
  () => Array.isArray(props.inquirySelection) && props.inquirySelection.length > 0
);
const sendShortcutHint = computed(() => {
  if (props.sendKey === 'ctrl_enter') return t('chat.input.sendHintCtrlEnterAlt');
  if (props.sendKey === 'enter') return t('chat.input.sendHintEnterAlt');
  return '';
});
const sendButtonTitle = computed(() => {
  if (stopButtonActive.value) return t('common.stop');
  return sendShortcutHint.value ? `${t('chat.input.send')} · ${sendShortcutHint.value}` : t('chat.input.send');
});
const reasoningEffortOptions = computed(() =>
  (['default', 'none', 'minimal', 'low', 'medium', 'high', 'xhigh'] as ReasoningEffort[]).map((value) => ({
    value,
    label: t(`desktop.system.reasoningEffort.${value}`)
  }))
);
// 普通输入显示通用提示；问询轮次改成当前问题的输入提示。
const inputPlaceholder = computed(() =>
  props.inquiryActive ? t('chat.input.inquiryPlaceholder') : t('chat.input.placeholder')
);
const voiceTranscribingLabel = computed(() => t('messenger.world.voice.transcribing'));
// §8.4 three approval tiers mapped onto the real `approval_mode` values.
// `suggest` is the strictest gate the backend exposes (every write and command
// asks for approval); there is no separate read-only mode to map to.
const APPROVAL_MODES: Array<{
  value: string;
  labelKey: string;
  descriptionKey: string;
  icon: string;
}> = [
  {
    value: 'full_auto',
    labelKey: 'chat.composer.approval.full_auto',
    descriptionKey: 'chat.composer.approval.full_autoHint',
    icon: 'fa-solid fa-bolt'
  },
  {
    value: 'auto_edit',
    labelKey: 'chat.composer.approval.auto_edit',
    descriptionKey: 'chat.composer.approval.auto_editHint',
    icon: 'fa-solid fa-shield-halved'
  },
  {
    value: 'suggest',
    labelKey: 'chat.composer.approval.suggest',
    descriptionKey: 'chat.composer.approval.suggestHint',
    icon: 'fa-solid fa-hand'
  }
];
const approvalOptions = computed<ApprovalModeOption[]>(() =>
  APPROVAL_MODES.map((mode) => ({
    value: mode.value,
    label: t(mode.labelKey),
    description: t(mode.descriptionKey),
    icon: mode.icon
  }))
);
const approvalModeValue = computed(() => {
  const candidate = String(props.approvalMode || '').trim().toLowerCase();
  const matched = approvalOptions.value.find((item) => item.value === candidate);
  return matched?.value || 'full_auto';
});
const voiceRecording = computed(() => Boolean(props.voiceRecording));
const voiceTranscribing = computed(() => Boolean(props.voiceTranscribing));
const hasSendableContent = computed(() =>
  Boolean(inputText.value.trim()) || attachments.value.length > 0 || hasInquirySelection.value
);
const stopButtonActive = computed(() => Boolean(props.loading) && !hasSendableContent.value);
const canSendOrStop = computed(() => {

  if (stopButtonActive.value) return true;
  if (composerBusy.value > 0) return false;
  return hasSendableContent.value;
});
const slashCommandDefinitions: SlashCommandDefinition[] = [
  { command: '/new', aliases: ['/reset'], descriptionKey: 'chat.commandMenu.new' },
  { command: '/stop', aliases: ['/cancel'], descriptionKey: 'chat.commandMenu.stop' },
  { command: '/goal', aliases: [], descriptionKey: 'chat.commandMenu.goal' },
  { command: '/compact', aliases: [], descriptionKey: 'chat.commandMenu.compact' },
  { command: '/help', aliases: ['/?'], descriptionKey: 'chat.commandMenu.help' }
];

const commandQuery = computed(() => {
  const raw = String(inputText.value || '');
  const cursor = Math.max(0, Math.min(caretPosition.value, raw.length));
  const beforeCursor = raw.slice(0, cursor);
  const trimmedLeading = beforeCursor.replace(/^\s+/, '');
  if (!trimmedLeading.startsWith('/')) {
    return null;
  }
  const token = trimmedLeading.split(/\s+/, 1)[0];
  if (!/^\/[a-zA-Z?]*$/.test(token)) {
    return null;
  }
  if (trimmedLeading.length > token.length) {
    return null;
  }
  return token.slice(1).toLowerCase();
});

const commandSuggestions = computed(() => {
  const query = commandQuery.value;
  if (query === null) {
    return [];
  }
  return slashCommandDefinitions
    .filter((item) => {
      if (!query) {
        return true;
      }
      const keywords = [item.command, ...item.aliases].map((value) =>
        value.replace(/^\//, '').toLowerCase()
      );
      return keywords.some((value) => value.startsWith(query));
    })
    .map((item) => ({
      command: item.command,
      description: t(item.descriptionKey)
    }));
});

const commandSuggestionsVisible = computed(
  () => !commandMenuDismissed.value && commandSuggestions.value.length > 0
);

const presetQuestionItems = computed(() => normalizeAgentPresetQuestions(props.presetQuestions));

const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

const resolveUploadError = (error, fallback) =>
  error?.response?.data?.detail || error?.message || fallback;

const isAttachmentProcessing = (id: string): boolean => attachmentProcessingIds.value.includes(id);

const markAttachmentProcessing = (id: string, active: boolean) => {
  const normalized = String(id || '').trim();
  if (!normalized) return;
  const next = attachmentProcessingIds.value.filter((item) => item !== normalized);
  if (active) next.push(normalized);
  attachmentProcessingIds.value = next;
};

const formatFrameRate = (value: unknown): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '1';
  const fixed = parsed >= 1 ? parsed.toFixed(parsed >= 10 ? 0 : 2) : parsed.toFixed(2);
  return fixed.replace(/\.?0+$/, '');
};

const resolveAttachmentIconClass = (attachment: ComposerDraftAttachment): string => {
  if (attachment.type === 'image') return 'fa-image';
  if (attachment.type === 'audio') return 'fa-music';
  if (attachment.type === 'video') return 'fa-film';
  if (attachment.type === 'gif') return 'fa-photo-film';
  return 'fa-file-lines';
};

const resolveAttachmentMeta = (attachment: ComposerDraftAttachment): string => {
  if (attachment.workspace_path) return attachment.workspace_path;
  if (isAttachmentProcessing(attachment.id)) {
    return attachment.type === 'gif'
      ? t('chat.attachments.gif.processingSingle')
      : t('chat.attachments.video.processingSingle');
  }
  if (attachment.type === 'audio') {
    return t('chat.attachments.audio.ready');
  }
  if (attachment.type === 'video') {
    return t('chat.attachments.video.meta', {
      frames: Number(attachment.frame_count || 0),
      fps: formatFrameRate(attachment.applied_frame_rate || attachment.requested_frame_rate || 1),
      audio: attachment.has_audio
        ? t('chat.attachments.video.metaAudioYes')
        : t('chat.attachments.video.metaAudioNo')
    });
  }
  if (attachment.type === 'gif') {
    return t('chat.attachments.gif.meta', {
      selected: Number(attachment.frame_count || 0),
      total: Number(attachment.total_frame_count || attachment.frame_count || 0),
      step: Number(attachment.applied_frame_step ?? attachment.requested_frame_step ?? 0)
    });
  }
  if (attachment.converter) {
    return t('chat.attachments.document.ready');
  }
  return '';
};

const isVideoControlOpen = (id: string): boolean =>
  String(expandedVideoAttachmentId.value || '') === String(id || '');

const toggleVideoControl = (id: string) => {
  const normalized = String(id || '').trim();
  if (!normalized) return;
  expandedVideoAttachmentId.value = isVideoControlOpen(normalized) ? '' : normalized;
};

const resolveVideoFrameRateInput = (id: string): string => {
  const normalized = String(id || '').trim();
  if (!normalized) return '1';
  const existing = videoFrameRateDrafts.value[normalized];
  if (String(existing || '').trim()) return String(existing);
  const current = attachments.value.find((item) => item.id === normalized);
  return formatFrameRate(current?.requested_frame_rate || current?.applied_frame_rate || 1);
};

const handleVideoFrameRateInput = (id: string, event: Event) => {
  const normalized = String(id || '').trim();
  if (!normalized) return;
  videoFrameRateDrafts.value = {
    ...videoFrameRateDrafts.value,
    [normalized]: String((event.target as HTMLInputElement | null)?.value || '')
  };
};

const resolveGifFrameStepInput = (id: string): string => {
  const normalized = String(id || '').trim();
  if (!normalized) return '0';
  const existing = gifFrameStepDrafts.value[normalized];
  if (String(existing || '').trim()) return String(existing);
  const current = attachments.value.find((item) => item.id === normalized);
  return String(Number(current?.requested_frame_step ?? current?.applied_frame_step ?? 0));
};

const handleGifFrameStepInput = (id: string, event: Event) => {
  const normalized = String(id || '').trim();
  if (!normalized) return;
  gifFrameStepDrafts.value = {
    ...gifFrameStepDrafts.value,
    [normalized]: String((event.target as HTMLInputElement | null)?.value || '')
  };
};

const resolveVideoControlSummary = (attachment: ComposerDraftAttachment): string => {
  if (!attachment.source_public_path) {
    return attachment.type === 'gif'
      ? t('chat.attachments.gif.controlUnavailable')
      : t('chat.attachments.video.controlUnavailable');
  }
  if (attachment.type === 'gif') {
    return t('chat.attachments.gif.controlSummary', {
      requested: Number(attachment.requested_frame_step ?? 0),
      applied: Number(attachment.applied_frame_step ?? attachment.requested_frame_step ?? 0),
      selected: Number(attachment.frame_count || 0),
      total: Number(attachment.total_frame_count || attachment.frame_count || 0)
    });
  }
  return t('chat.attachments.video.controlSummary', {
    requested: formatFrameRate(attachment.requested_frame_rate || 1),
    applied: formatFrameRate(attachment.applied_frame_rate || attachment.requested_frame_rate || 1),
    frames: Number(attachment.frame_count || 0)
  });
};

const collectPayloadAttachments = (attachment: ComposerDraftAttachment): AttachmentPayload[] => {
  // Workspace references are serialized into the question at send time.
  if (attachment.workspace_path) return [];
  const derived = Array.isArray(attachment.derived_attachments)
    ? attachment.derived_attachments.flatMap((item) => collectPayloadAttachments(item))
    : [];
  if (derived.length) return derived;
  const content = String(attachment.content || '');
  const publicPath = String(attachment.public_path || '').trim();
  if (!content.trim() && !publicPath) {
    return [];
  }
  const payload: AttachmentPayload = {
    type: attachment.type,
    name: attachment.name,
    content
  };
  if (attachment.mime_type) {
    payload.mime_type = attachment.mime_type;
  }
  if (publicPath) {
    payload.public_path = publicPath;
  }
  return [payload];
};

// Keep only fields the backend needs so UI-only state never leaks into requests.
const buildAttachmentPayload = () => attachments.value.flatMap((item) => collectPayloadAttachments(item));

const resizeInput = () => {
  const el = inputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  const nextHeight = Math.min(el.scrollHeight, INPUT_MAX_HEIGHT);
  el.style.height = `${nextHeight}px`;
  el.style.overflowY = el.scrollHeight > INPUT_MAX_HEIGHT ? 'auto' : 'hidden';
};

const syncCaretPosition = () => {
  const el = inputRef.value;
  const fallback = String(inputText.value || '').length;
  const selectionStart = Number(el?.selectionStart);
  caretPosition.value = Number.isFinite(selectionStart) ? selectionStart : fallback;
};

const handleInput = () => {
  commandMenuDismissed.value = false;
  resetHistoryRecall();
  resizeInput();
  syncCaretPosition();
};

const focusComposerInputAtEnd = () => {
  void nextTick(() => {
    const el = inputRef.value;
    if (!el) return;
    const cursor = String(inputText.value || '').length;
    if (typeof el.focus === 'function') {
      el.focus({ preventScroll: true });
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(cursor, cursor);
    }
    caretPosition.value = cursor;
  });
};

// 卡片内除控件外的空白（含底部工具行空位）都算命中输入区，和桌面一致。
const isComposerControlTarget = (target: EventTarget | null): boolean => {
  const element = target instanceof Element ? target : null;
  return Boolean(element?.closest(
    'button, a, input, textarea, select, label, [role="button"], [role="menuitem"], [role="option"], '
    + '[contenteditable="true"], .composer-panel, .command-menu'
  ));
};
const handleComposerSurfacePointerDown = (event: MouseEvent) => {
  if (event.button !== 0 || isComposerControlTarget(event.target)) return;
  // 阻止本次按下的默认失焦，否则浏览器会在事件结束后把焦点交还给 body。
  event.preventDefault();
  focusComposerInputAtEnd();
};

const activeQueueSessionId = computed(() => targetSessionId.value);

// 撤下一条还没开跑的轮次：引擎侧它从未进过会话历史，本地占位必须一起消失。
const handleQueueTurnWithdrawn = (item: {
  clientMessageId?: string;
  rootTurnId?: string;
  userTurnId?: string;
  queueId?: string;
}) => {
  if (!item) return;
  chatStore.dropParkedQueueTurn(item);
};

// 排队条目被「编辑」撤下后放回输入框：文本追加，附件按已转换的媒体条目还原。
const handleQueueTurnEdit = (item: { content?: string; attachments?: unknown[] }) => {
  const content = String(item?.content || '').trim();
  const restored = (Array.isArray(item?.attachments) ? item.attachments : [])
    .map((entry) => normalizeProcessedMediaAttachment(entry))
    .filter(Boolean) as ComposerDraftAttachment[];
  if (content) {
    const draft = String(inputText.value || '');
    inputText.value = draft.trim() ? `${draft}\n${content}` : content;
  }
  if (restored.length) {
    attachments.value = [...attachments.value, ...restored];
  }
  void nextTick(() => {
    resizeInput();
    syncVideoAttachmentDrafts();
  });
  focusComposerInputAtEnd();
};

const recallableUserMessages = computed<string[]>(() => {
  const list = Array.isArray(props.contextMessages) ? props.contextMessages : [];
  const output: string[] = [];
  for (const message of list) {
    if (!message || typeof message !== 'object') continue;
    const source = message as Record<string, unknown>;
    if (String(source.role || '').toLowerCase() !== 'user') continue;
    const status = String(source.status || '').trim().toLowerCase();
    if (status === 'queued' || status === 'failed' || status === 'cancelled') continue;
    const content = String(source.content || '').trim();
    if (!content) continue;
    if (output[output.length - 1] === content) continue;
    output.push(content);
  }
  // 最近的一条排在最前：上箭头从下标 0 开始递增，第一次按就该回到最后发过的那条。
  // 去重仍按时间正序做（只合并相邻重复），所以放在反转之前。
  output.reverse();
  return output;
});

const historyRecallIndex = ref(-1);
const historyRecallStash = ref('');

const resetHistoryRecall = () => {
  historyRecallIndex.value = -1;
  historyRecallStash.value = '';
};

const isCaretAtInputStart = () => {
  const el = inputRef.value;
  if (!el || typeof el.selectionStart !== 'number') return caretPosition.value <= 0;
  return el.selectionStart === el.selectionEnd && el.selectionStart <= 0;
};

const isCaretAtInputEnd = () => {
  const el = inputRef.value;
  const length = String(inputText.value || '').length;
  if (!el || typeof el.selectionStart !== 'number') return caretPosition.value >= length;
  return el.selectionStart === el.selectionEnd && el.selectionStart >= length;
};

const applyHistoryRecall = (index: number) => {
  const value = recallableUserMessages.value[index];
  if (typeof value !== 'string') return;
  historyRecallIndex.value = index;
  inputText.value = value;
  commandMenuDismissed.value = false;
  void nextTick(() => resizeInput());
  focusComposerInputAtEnd();
};

// 上箭头逐条找回本会话发过的消息，下箭头往回走；命令面板可见时仍归面板使用。
const handleHistoryRecallKey = (event) => {
  const history = recallableUserMessages.value;
  if (!history.length) return false;
  const recalling = historyRecallIndex.value >= 0;
  if (event.key === 'ArrowUp') {
    if (!recalling && String(inputText.value || '').trim() && !isCaretAtInputStart()) {
      return false;
    }
    const next = historyRecallIndex.value + 1;
    if (next >= history.length) return false;
    if (!recalling) historyRecallStash.value = inputText.value;
    event.preventDefault();
    applyHistoryRecall(next);
    return true;
  }
  if (event.key === 'ArrowDown') {
    if (!recalling || !isCaretAtInputEnd()) return false;
    event.preventDefault();
    const next = historyRecallIndex.value - 1;
    if (next < 0) {
      const stash = historyRecallStash.value;
      resetHistoryRecall();
      inputText.value = stash;
      void nextTick(() => resizeInput());
      focusComposerInputAtEnd();
      return true;
    }
    applyHistoryRecall(next);
    return true;
  }
  return false;
};

const focusComposerInputAt = (cursor: number) => {
  void nextTick(() => {
    const el = inputRef.value;
    const current = String(inputText.value || '');
    const safeCursor = Math.max(0, Math.min(cursor, current.length));
    if (!el) return;
    if (typeof el.focus === 'function') {
      el.focus();
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(safeCursor, safeCursor);
    }
    caretPosition.value = safeCursor;
  });
};

const insertTextIntoComposer = (text: unknown, mode: 'append' | 'cursor' = 'append') => {
  const normalized = String(text || '').trim();
  if (!normalized) return;
  const current = String(inputText.value || '');
  let nextCursor = 0;
  if (mode === 'cursor' && current) {
    const el = inputRef.value;
    const start = Number.isFinite(el?.selectionStart) ? Math.max(0, Number(el.selectionStart)) : current.length;
    const end = Number.isFinite(el?.selectionEnd) ? Math.max(0, Number(el.selectionEnd)) : start;
    const beforeRaw = current.slice(0, start);
    const afterRaw = current.slice(end);
    const before = beforeRaw && !/\s$/.test(beforeRaw) ? `${beforeRaw} ` : beforeRaw;
    const after = afterRaw && !/^\s/.test(afterRaw) ? ` ${afterRaw}` : afterRaw;
    inputText.value = `${before}${normalized}${after}`;
    nextCursor = before.length + normalized.length;
  } else {
    inputText.value = current.trim()
      ? `${current.replace(/\s*$/, '')}\n${normalized}`
      : normalized;
    nextCursor = String(inputText.value || '').length;
  }
  commandMenuDismissed.value = false;
  flushPersistDraftState();
  void nextTick(() => {
    resizeInput();
    focusComposerInputAt(nextCursor);
  });
};

const appendTextToComposer = (text: unknown) => {
  insertTextIntoComposer(text, 'append');
};

const setCommandMenuIndex = (index) => {
  const total = commandSuggestions.value.length;
  if (total <= 0) {
    commandMenuIndex.value = 0;
    return;
  }
  commandMenuIndex.value = Math.max(0, Math.min(index, total - 1));
};

const moveCommandMenuIndex = (delta) => {
  const total = commandSuggestions.value.length;
  if (total <= 0) {
    commandMenuIndex.value = 0;
    return;
  }
  const next = (commandMenuIndex.value + delta + total) % total;
  commandMenuIndex.value = next;
};

const applyCommandSuggestion = (index = commandMenuIndex.value) => {
  const item = commandSuggestions.value[index];
  if (!item) {
    return false;
  }
  const leading = String(inputText.value || '').match(/^\s*/)?.[0] || '';
  inputText.value = `${leading}${item.command} `;
  commandMenuDismissed.value = false;
  nextTick(() => {
    resizeInput();
    const el = inputRef.value;
    if (!el) return;
    const cursor = inputText.value.length;
    if (typeof el.focus === 'function') {
      el.focus();
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(cursor, cursor);
    }
    caretPosition.value = cursor;
  });
  return true;
};

const handleInputKeydown = async (event) => {
  if (isEnterKeyboardEvent(event)) {
    await handleEnterKeydown(event);
    return;
  }
  if (event.key === 'Escape') {
    if (hasOpenComposerPanel()) {
      event.preventDefault();
      closeComposerPanels();
      return;
    }
  }
  if (!commandSuggestionsVisible.value) {
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      handleHistoryRecallKey(event);
    }
    return;
  }
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    moveCommandMenuIndex(1);
    return;
  }
  if (event.key === 'ArrowUp') {
    event.preventDefault();
    moveCommandMenuIndex(-1);
    return;
  }
  if (event.key === 'Tab') {
    event.preventDefault();
    applyCommandSuggestion();
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    commandMenuDismissed.value = true;
  }
};

const resolveSendKeyMode = (): SendKeyMode =>
  props.sendKey === 'ctrl_enter' || props.sendKey === 'none' ? props.sendKey : 'enter';

const normalizeFiniteNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return null;
  return parsed;
};

const normalizeProcessedMediaAttachment = (
  value: unknown,
  fallbackType = 'file'
): ComposerDraftAttachment | null => {
  if (!value || typeof value !== 'object') return null;
  const source = value as Record<string, unknown>;
  const name = String(source.name || '').trim();
  const content = String(source.content || '');
  const publicPath = String(source.public_path || '').trim();
  const mimeType = String(source.content_type ?? source.mime_type ?? '').trim();
  const inferredType = mimeType.startsWith('image/')
    ? 'image'
    : mimeType.startsWith('audio/')
      ? 'audio'
      : fallbackType;
  if (!name || (!content.trim() && !publicPath)) return null;
  const attachment: ComposerDraftAttachment = {
    id: buildAttachmentId(),
    type: inferredType,
    name,
    content
  };
  if (mimeType) attachment.mime_type = mimeType;
  if (publicPath) attachment.public_path = publicPath;
  return attachment;
};

const normalizeDraftAttachment = (value: unknown): ComposerDraftAttachment | null => {
  if (!value || typeof value !== 'object') return null;
  const source = value as Record<string, unknown>;
  const id = String(source.id || '').trim();
  const type = String(source.type || '').trim();
  const name = String(source.name || '').trim();
  const content = String(source.content || '');
  const publicPath = String(source.public_path || '').trim();
  const derivedAttachments = Array.isArray(source.derived_attachments)
    ? (source.derived_attachments
        .map((item) => normalizeDraftAttachment(item))
        .filter(Boolean) as ComposerDraftAttachment[])
    : [];
  if (!id || !type || !name) return null;
  const workspacePath = type === 'workspace' ? normalizeWorkspacePath(source.workspace_path) : '';
  if (!content.trim() && !publicPath && !workspacePath && derivedAttachments.length === 0) return null;
  const attachment: ComposerDraftAttachment = {
    id,
    type,
    name,
    content
  };
  if (workspacePath) attachment.workspace_path = workspacePath;
  const previewUrl = String(source.preview_url || '').trim();
  if (previewUrl) attachment.preview_url = previewUrl;
  const mimeType = String(source.mime_type ?? source.content_type ?? '').trim();
  if (mimeType) attachment.mime_type = mimeType;
  const converter = String(source.converter || '').trim();
  if (converter) attachment.converter = converter;
  if (publicPath) attachment.public_path = publicPath;
  const sourcePublicPath = String(source.source_public_path || '').trim();
  if (sourcePublicPath) attachment.source_public_path = sourcePublicPath;
  if (derivedAttachments.length > 0) {
    attachment.derived_attachments = derivedAttachments;
  }
  const requestedFrameRate = normalizeFiniteNumber(source.requested_frame_rate);
  if (requestedFrameRate !== null && requestedFrameRate > 0) {
    attachment.requested_frame_rate = requestedFrameRate;
  }
  const appliedFrameRate = normalizeFiniteNumber(source.applied_frame_rate);
  if (appliedFrameRate !== null && appliedFrameRate > 0) {
    attachment.applied_frame_rate = appliedFrameRate;
  }
  const requestedFrameStep = normalizeFiniteNumber(source.requested_frame_step);
  if (requestedFrameStep !== null && requestedFrameStep >= 0) {
    attachment.requested_frame_step = requestedFrameStep;
  }
  const appliedFrameStep = normalizeFiniteNumber(source.applied_frame_step);
  if (appliedFrameStep !== null && appliedFrameStep >= 0) {
    attachment.applied_frame_step = appliedFrameStep;
  }
  const totalFrameCount = normalizeFiniteNumber(source.total_frame_count);
  if (totalFrameCount !== null && totalFrameCount >= 0) {
    attachment.total_frame_count = totalFrameCount;
  }
  const durationMs = normalizeFiniteNumber(source.duration_ms);
  if (durationMs !== null && durationMs >= 0) {
    attachment.duration_ms = durationMs;
  }
  const frameCount = normalizeFiniteNumber(source.frame_count);
  if (frameCount !== null && frameCount >= 0) {
    attachment.frame_count = frameCount;
  }
  if (source.has_audio === true) {
    attachment.has_audio = true;
  }
  if (Array.isArray(source.warnings)) {
    const warnings = source.warnings
      .map((item) => String(item || '').trim())
      .filter((item) => item);
    if (warnings.length > 0) {
      attachment.warnings = warnings;
    }
  }
  return attachment;
};

const syncVideoAttachmentDrafts = () => {
  const nextVideo: Record<string, string> = {};
  const nextGif: Record<string, string> = {};
  attachments.value.forEach((attachment) => {
    if (attachment.type !== 'video' && attachment.type !== 'gif') return;
    const id = String(attachment.id || '').trim();
    if (!id) return;
    if (attachment.type === 'video') {
      const existing = String(videoFrameRateDrafts.value[id] || '').trim();
      nextVideo[id] =
        existing ||
        formatFrameRate(attachment.requested_frame_rate || attachment.applied_frame_rate || 1);
      return;
    }
    const existing = String(gifFrameStepDrafts.value[id] || '').trim();
    nextGif[id] =
      existing || String(Number(attachment.requested_frame_step ?? attachment.applied_frame_step ?? 0));
  });
  videoFrameRateDrafts.value = nextVideo;
  gifFrameStepDrafts.value = nextGif;
  if (
    expandedVideoAttachmentId.value &&
    !nextVideo[expandedVideoAttachmentId.value] &&
    !nextGif[expandedVideoAttachmentId.value]
  ) {
    expandedVideoAttachmentId.value = '';
  }
};

const replaceAttachment = (id: string, nextAttachment: ComposerDraftAttachment) => {
  const normalized = String(id || '').trim();
  attachments.value = attachments.value.map((item) =>
    item.id === normalized ? nextAttachment : item
  );
  syncVideoAttachmentDrafts();
};

const buildDraftAttachments = (): ComposerDraftAttachment[] =>
  attachments.value
    .map((item) => normalizeDraftAttachment(item))
    .filter(Boolean) as ComposerDraftAttachment[];

const persistDraftStateByKey = (key: string) => {
  const normalizedKey = String(key || '').trim();
  if (!normalizedKey) return;
  const content = String(inputText.value || '');
  const normalizedAttachments = buildDraftAttachments();
  if (!content.trim() && normalizedAttachments.length === 0) {
    clearComposerDraftState(normalizedKey);
    return;
  }
  writeComposerDraftState(normalizedKey, {
    content,
    attachments: normalizedAttachments
  });
};

const persistDraftState = () => {
  persistDraftStateByKey(resolveDraftKey());
};

const clearScheduledDraftPersist = () => {
  if (draftPersistTimer === null) return;
  clearTimeout(draftPersistTimer);
  draftPersistTimer = null;
};

const flushPersistDraftState = (key = resolveDraftKey()) => {
  clearScheduledDraftPersist();
  persistDraftStateByKey(key);
};

const schedulePersistDraftState = () => {
  clearScheduledDraftPersist();
  draftPersistTimer = setTimeout(() => {
    draftPersistTimer = null;
    persistDraftState();
  }, DRAFT_PERSIST_DEBOUNCE_MS);
};

const restoreDraftStateByKey = (key: string) => {
  // 换会话/换草稿时召回游标必须归零，否则会从上一个会话的下标继续推进，
  // 轻则跳条，重则越界后上键完全失效。
  resetHistoryRecall();
  const normalizedKey = String(key || '').trim();
  if (!normalizedKey) {
    inputText.value = '';
    attachments.value = [];
    attachmentProcessingIds.value = [];
    expandedVideoAttachmentId.value = '';
    videoFrameRateDrafts.value = {};
    gifFrameStepDrafts.value = {};
    commandMenuDismissed.value = false;
    caretPosition.value = 0;
    void nextTick(() => {
      resizeInput();
      syncCaretPosition();
    });
    return;
  }
  const cached = readComposerDraftState(normalizedKey);
  inputText.value = String(cached?.content || '');
  attachments.value = Array.isArray(cached?.attachments)
    ? (cached!.attachments
        .map((item) => normalizeDraftAttachment(item))
        .filter(Boolean) as ComposerDraftAttachment[])
    : [];
  attachmentProcessingIds.value = [];
  syncVideoAttachmentDrafts();
  commandMenuDismissed.value = false;
  void nextTick(() => {
    resizeInput();
    syncCaretPosition();
  });
};

const handleEnterKeydown = async (event) => {
  if (event.isComposing) {
    return;
  }
  const mode = resolveSendKeyMode();
  if (mode === 'none') {
    return;
  }
  if (mode === 'ctrl_enter') {
    if (hasBackupSendModifier(event) || hasPrimarySendModifier(event)) {
      event.preventDefault();
      await handleSend();
    }
    return;
  }
  if (event.shiftKey) {
    return;
  }
  if (hasBackupSendModifier(event) || hasPrimarySendModifier(event)) {
    event.preventDefault();
    await handleSend();
    return;
  }
  event.preventDefault();
  await handleSend();
};


const resetInputHeight = () => {
  const el = inputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  el.style.overflowY = 'hidden';
};

const hasFileDrag = (event): boolean => {
  const transfer = event?.dataTransfer;
  if (!transfer) return false;
  if (hasWorkspaceDragPaths(transfer)) return true;
  if (transfer.files && transfer.files.length > 0) return true;
  if (transfer.items && transfer.items.length > 0) {
    const items = Array.from(transfer.items) as DataTransferItem[];
    if (items.some((item) => String(item?.kind || '').toLowerCase() === 'file')) {
      return true;
    }
  }
  const types = Array.from(transfer.types || []).map((item) => String(item || ''));
  return types.includes('Files') || types.includes('application/x-moz-file');
};

const normalizeWorkspaceContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 0;
  return Math.min(10, Math.max(0, parsed));
};

const resolveWorkspaceDropPath = (path: unknown): string => {
  const normalized = normalizeWorkspacePath(path);
  return normalized;
};

const resolveComposerWorkspaceContainerId = (): number => {
  const explicit = String(props.workspaceContainerId ?? '').trim();
  if (explicit) {
    return normalizeWorkspaceContainerId(explicit);
  }
  return 1;
};

const resolveUploadedWorkspacePaths = (payload: unknown): string[] => {
  const source = payload && typeof payload === 'object' ? payload as Record<string, unknown> : {};
  return Array.isArray(source.files)
    ? source.files.map((item) => normalizeWorkspacePath(item)).filter(Boolean)
    : [];
};

const buildWorkspaceFileNotice = (paths: string[]): string => {
  const normalized = paths.map((item) => normalizeWorkspacePath(item)).filter(Boolean);
  const displayPaths = normalized;
  if (!displayPaths.length) return '';
  const lines = [
    t('chat.workspaceDrop.noticeHeader', { count: displayPaths.length })
  ];
  displayPaths.forEach((path, index) => {
    lines.push(`${index + 1}. ${path}`);
  });
  lines.push(t('chat.workspaceDrop.noticeFooter'));
  return lines.join('\n');
};

const addWorkspaceFileReferences = (paths: string[], sources: WorkspaceDroppedFile[] = []) => {
  const existing = new Set(attachments.value.map(item => item.workspace_path).filter(Boolean));
  const additions: ComposerDraftAttachment[] = [];
  for (const [index, rawPath] of paths.entries()) {
    const path = normalizeWorkspacePath(rawPath);
    if (!path || existing.has(path)) continue;
    existing.add(path);
    const source = sources[index]?.file;
    const previewUrl = source ? createComposerImagePreview(source) : '';
    additions.push({
      id: buildAttachmentId(),
      type: 'workspace',
      name: path.split('/').pop() || path,
      content: '',
      workspace_path: path,
      ...(source?.type ? { mime_type: source.type } : {}),
      ...(previewUrl ? { preview_url: previewUrl } : {})
    });
  }
  if (!additions.length) return;
  attachments.value = [...attachments.value, ...additions];
  flushPersistDraftState();
  focusComposerInputAtEnd();
};

const appendWorkspacePathsToComposer = (paths: string[]) => {
  const normalized = Array.from(new Set(paths.map(path => normalizeWorkspacePath(path)).filter(Boolean)));
  if (!normalized.length) return;
  appendTextToComposer(normalized.join('\n'));
};

const UPLOAD_REQUEST_MAX_FILES = 100;
const UPLOAD_REQUEST_MAX_BYTES = 200 * 1024 * 1024;
const COMPOSER_ATTACHMENT_ROOT = 'temp/chat_attachments';

const buildComposerAttachmentPath = (): string => {
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  return `${COMPOSER_ATTACHMENT_ROOT}/${timestamp}-${Math.random().toString(36).slice(2, 10)}`;
};

const collectDroppedWorkspaceTree = async (
  dataTransfer: DataTransfer | null | undefined
): Promise<WorkspaceDroppedTree> => {
  const items = Array.from(dataTransfer?.items || []);
  const entries: WorkspaceDropEntry[] = [];
  const flatFiles: WorkspaceDroppedFile[] = [];
  items.forEach((item) => {
    const entry = typeof item.webkitGetAsEntry === 'function' ? item.webkitGetAsEntry() : null;
    if (entry) {
      entries.push(entry as unknown as WorkspaceDropEntry);
      return;
    }
    const file = item.getAsFile();
    if (file) flatFiles.push({ file, relativePath: file.name || 'upload' });
  });
  if (entries.length) {
    const tree = await collectWorkspaceDropTree(entries);
    return { ...tree, files: [...tree.files, ...flatFiles] };
  }
  const transferFiles = Array.from(dataTransfer?.files || []).map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name || 'upload'
  }));
  const files = transferFiles.length
    ? transferFiles
    : flatFiles;
  return { files, directories: [], unreadable: 0, depthSkipped: 0 };
};

const createEmptyWorkspaceDirectories = async (directories: string[]): Promise<number> => {
  let failed = 0;
  for (let index = 0; index < directories.length; index += 4) {
    const batch = directories.slice(index, index + 4);
    const results = await Promise.allSettled(
      batch.map((directory) => createWunderWorkspaceDir({ path: normalizeWorkspacePath(directory) }))
    );
    failed += results.filter((result) => result.status === 'rejected').length;
  }
  return failed;
};

/**
 * 附件上传按「文件数 + 总字节」切分请求：单次请求不再受 1 GiB 限制，
 * 大目录不会整批失败；分片全部结束后只发一次刷新事件。
 */
const uploadDroppedFilesToWorkspace = async (
  items: WorkspaceDroppedFile[],
  targetPath: string,
  extraPaths: string[] = []
): Promise<string[]> => {
  const fileItems = items.filter((item) => Boolean(item?.file));
  if (!fileItems.length) return [];
  const agentId = String(props.workspaceAgentId || '').trim();
  const containerId = resolveComposerWorkspaceContainerId();
  const uploadedPaths: string[] = [];
  let lastTreeVersion: unknown;
  let batch: WorkspaceDroppedFile[] = [];
  let batchBytes = 0;

  const flushBatch = async () => {
    if (!batch.length) return;
    const formData = new FormData();
    formData.append('path', resolveWorkspaceDropPath(targetPath));
    if (agentId) {
      formData.append('agent_id', agentId);
    }
    // No `container_id` field: the cloud workspace is a single per-user root.
    batch.forEach((item) => {
      const file = item.file;
      formData.append('files', file, file.name || 'upload');
      formData.append('relative_paths', normalizeWorkspacePath(item.relativePath || file.name || 'upload'));
    });
    const response = await uploadWunderWorkspace(formData);
    uploadedPaths.push(...resolveUploadedWorkspacePaths(response?.data));
    lastTreeVersion = response?.data?.tree_version;
    batch = [];
    batchBytes = 0;
  };

  try {
    for (const item of fileItems) {
      const size = Number(item.file.size) || 0;
      if (batch.length && (batch.length >= UPLOAD_REQUEST_MAX_FILES || batchBytes + size > UPLOAD_REQUEST_MAX_BYTES)) {
        await flushBatch();
      }
      batch.push(item);
      batchBytes += size;
    }
    await flushBatch();
  } finally {
    // 分片中途失败时也要让已上传的部分出现在工作目录里。
    if (uploadedPaths.length || extraPaths.length) {
      const refreshDetail = {
        reason: 'composer-drop-upload',
        containerId,
        paths: [...uploadedPaths, ...extraPaths],
        treeVersion: lastTreeVersion,
        tree_version: lastTreeVersion
      };
      emitWorkspaceRefresh(
        agentId
          ? {
              ...refreshDetail,
              agentId,
              agent_id: agentId
            }
          : refreshDetail
      );
    }
  }
  return uploadedPaths;
};

/**
 * 文件选择、剪贴板粘贴与拖拽统一落到 temp/chat_attachments 的独立批次目录，
 * 再以实际附件路径随下一条消息发送；重名文件不会跨批覆盖。
 */
const ingestComposerFiles = async (tree: WorkspaceDroppedTree) => {
  const files = (Array.isArray(tree?.files) ? tree.files : []).filter((item) => Boolean(item?.file));
  const emptyDirectories = resolveEmptyDropDirectories(tree?.directories || [], files);
  if (!files.length && !emptyDirectories.length) return;
  const attachmentPath = buildComposerAttachmentPath();
  const emptyAttachmentDirectories = emptyDirectories.map(
    (directory) => `${attachmentPath}/${directory}`
  );
  closeComposerPanels();
  workspaceDropBusy.value += 1;
  try {
    if (emptyAttachmentDirectories.length) {
      const failed = await createEmptyWorkspaceDirectories(emptyAttachmentDirectories);
      if (failed > 0) {
        ElMessage.warning(t('messenger.filesArea.uploadEmptyDirFailed', { count: failed }));
      }
    }
    if (!files.length) {
      emitWorkspaceRefresh({
        reason: 'composer-drop-upload',
        containerId: resolveComposerWorkspaceContainerId(),
        paths: emptyAttachmentDirectories
      });
      ElMessage.success(t('messenger.filesArea.uploadEmptyDirsDone'));
      return;
    }
    const uploadedPaths = await uploadDroppedFilesToWorkspace(
      files,
      attachmentPath,
      emptyAttachmentDirectories
    );
    addWorkspaceFileReferences(uploadedPaths, files);
    ElMessage.success(t('chat.workspaceDrop.uploaded', { count: uploadedPaths.length }));
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.workspaceDrop.failed')));
  } finally {
    workspaceDropBusy.value = Math.max(0, workspaceDropBusy.value - 1);
  }
};

// 文件选择、拖拽和粘贴都走工作目录上传；侧栏文件区继续使用同一 /workspace 契约。
const handleDragEnter = (event) => {
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  dragCounter.value += 1;
  dragActive.value = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = composerBusy.value > 0
      ? 'none'
      : hasWorkspaceDragPaths(event.dataTransfer)
        ? 'copy'
        : 'copy';
  }
};

const handleDragOver = (event) => {
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = composerBusy.value > 0
      ? 'none'
      : hasWorkspaceDragPaths(event.dataTransfer)
        ? 'copy'
        : 'copy';
  }
};

const handleDragLeave = (event) => {
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  dragCounter.value = Math.max(0, dragCounter.value - 1);
  if (dragCounter.value === 0) {
    dragActive.value = false;
  }
};

const handleDrop = async (event) => {
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  dragCounter.value = 0;
  dragActive.value = false;
  const workspacePaths = readWorkspaceDragPaths(event.dataTransfer);
  if (workspacePaths.length) {
    clearWorkspaceDragPaths();
    if (composerBusy.value > 0) {
      ElMessage.warning(chatBusyMessage.value);
      return;
    }
    closeComposerPanels();
    appendWorkspacePathsToComposer(workspacePaths);
    ElMessage.success(t('chat.workspaceDrop.pathsInserted', { count: workspacePaths.length }));
    return;
  }
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  const droppedTree = await collectDroppedWorkspaceTree(event.dataTransfer);
  if (!droppedTree.files.length && !droppedTree.directories.length) return;
  if (droppedTree.depthSkipped > 0) {
    ElMessage.warning(t('messenger.filesArea.uploadDepthSkipped', { count: droppedTree.depthSkipped }));
  }
  if (droppedTree.unreadable > 0) {
    ElMessage.warning(t('messenger.filesArea.uploadUnreadable', { count: droppedTree.unreadable }));
  }
  await ingestComposerFiles(droppedTree);
};

const handleComposerPaste = async (event: ClipboardEvent) => {
  const pastedTree = collectWorkspaceClipboardFiles(event.clipboardData);
  pastedTree.files = pastedTree.files.map(normalizeClipboardDroppedFile);
  if (!pastedTree.files.length) return;
  event.preventDefault();
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  closeComposerPanels();
  await ingestComposerFiles(pastedTree);
};

const requestMediaProcessing = async (formData: FormData): Promise<ProcessedMediaResponse> => {
  const response = await processChatMediaAttachment(formData);
  return (response?.data?.data || {}) as ProcessedMediaResponse;
};

const buildVideoDraftAttachment = (
  filename: string,
  payload: ProcessedMediaResponse,
  attachmentId?: string,
  forcedType?: 'video' | 'gif'
): ComposerDraftAttachment => {
  const derivedAttachments = Array.isArray(payload.attachments)
    ? (payload.attachments
        .map((item) => normalizeProcessedMediaAttachment(item))
        .filter(Boolean) as ComposerDraftAttachment[])
    : [];
  if (!derivedAttachments.length) {
    throw new Error(t('chat.attachments.emptyResult'));
  }
  const nextAttachment: ComposerDraftAttachment = {
    id: attachmentId || buildAttachmentId(),
    type: forcedType || (String(payload.kind || '').trim() === 'gif' ? 'gif' : 'video'),
    name: String(payload.name || filename || '').trim() || filename,
    content: '',
    source_public_path: String(payload.source_public_path || '').trim() || undefined,
    derived_attachments: derivedAttachments,
    requested_frame_rate: Number.isFinite(payload.requested_frame_rate)
      ? Number(payload.requested_frame_rate)
      : 1,
    applied_frame_rate: Number.isFinite(payload.applied_frame_rate)
      ? Number(payload.applied_frame_rate)
      : undefined,
    requested_frame_step: Number.isFinite(payload.requested_frame_step)
      ? Number(payload.requested_frame_step)
      : undefined,
    applied_frame_step: Number.isFinite(payload.applied_frame_step)
      ? Number(payload.applied_frame_step)
      : undefined,
    total_frame_count: Number.isFinite(payload.total_frame_count)
      ? Number(payload.total_frame_count)
      : undefined,
    duration_ms: Number.isFinite(payload.duration_ms) ? Number(payload.duration_ms) : undefined,
    frame_count: Number.isFinite(payload.frame_count)
      ? Number(payload.frame_count)
      : derivedAttachments.filter((item) => item.type === 'image').length,
    has_audio: payload.has_audio === true
  };
  if (Array.isArray(payload.warnings) && payload.warnings.length > 0) {
    nextAttachment.warnings = payload.warnings;
  }
  return nextAttachment;
};

const applyVideoFrameRate = async (attachmentId: string) => {
  const normalized = String(attachmentId || '').trim();
  const current = attachments.value.find((item) => item.id === normalized);
  if (!current || current.type !== 'video') return;
  const sourcePublicPath = String(current.source_public_path || '').trim();
  if (!sourcePublicPath) {
    ElMessage.warning(t('chat.attachments.video.controlUnavailable'));
    return;
  }
  attachmentBusy.value += 1;
  markAttachmentProcessing(normalized, true);
  try {
    const formData = new FormData();
    formData.append('source_public_path', sourcePublicPath);
    formData.append('frame_rate', String(resolveVideoFrameRateInput(normalized) || '1').trim() || '1');
    const nextAttachment = buildVideoDraftAttachment(
      current.name,
      await requestMediaProcessing(formData),
      normalized
    );
    replaceAttachment(normalized, nextAttachment);
    if (nextAttachment.warnings?.length) {
      ElMessage.warning(nextAttachment.warnings[0]);
    } else {
      ElMessage.success(t('chat.attachments.videoUpdated', { name: current.name }));
    }
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.attachments.processFailed')));
  } finally {
    markAttachmentProcessing(normalized, false);
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
};

const applyGifFrameStep = async (attachmentId: string) => {
  const normalized = String(attachmentId || '').trim();
  const current = attachments.value.find((item) => item.id === normalized);
  if (!current || current.type !== 'gif') return;
  const sourcePublicPath = String(current.source_public_path || '').trim();
  if (!sourcePublicPath) {
    ElMessage.warning(t('chat.attachments.gif.controlUnavailable'));
    return;
  }
  attachmentBusy.value += 1;
  markAttachmentProcessing(normalized, true);
  try {
    const formData = new FormData();
    formData.append('source_public_path', sourcePublicPath);
    formData.append('frame_step', String(resolveGifFrameStepInput(normalized) || '0').trim() || '0');
    const nextAttachment = buildVideoDraftAttachment(
      current.name,
      await requestMediaProcessing(formData),
      normalized,
      'gif'
    );
    replaceAttachment(normalized, nextAttachment);
    if (nextAttachment.warnings?.length) {
      ElMessage.warning(nextAttachment.warnings[0]);
    } else {
      ElMessage.success(t('chat.attachments.gifUpdated', { name: current.name }));
    }
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.attachments.processFailed')));
  } finally {
    markAttachmentProcessing(normalized, false);
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
};

const handleAttachmentPreviewError = (id: string) => {
  const current = attachments.value.find((item) => item.id === id);
  revokeComposerImagePreview(current?.preview_url);
  attachments.value = attachments.value.map((item) =>
    item.id === id ? { ...item, preview_url: '' } : item
  );
};

const removeAttachment = (id) => {
  const current = attachments.value.find((item) => item.id === id);
  revokeComposerImagePreview(current?.preview_url);
  attachments.value = attachments.value.filter((item) => item.id !== id);
  markAttachmentProcessing(id, false);
  syncVideoAttachmentDrafts();
};

const clearAttachments = () => {
  attachments.value.forEach((item) => revokeComposerImagePreview(item.preview_url));
  attachments.value = [];
  attachmentProcessingIds.value = [];
  expandedVideoAttachmentId.value = '';
  videoFrameRateDrafts.value = {};
  gifFrameStepDrafts.value = {};
};

const closeComposerPanels = () => {
  modelMenuVisible.value = false;
  plusMenuVisible.value = false;
};

const closeOtherPanels = (keep: 'plus' | 'model') => {
  if (keep !== 'model') modelMenuVisible.value = false;
  if (keep !== 'plus') plusMenuVisible.value = false;
};

/**
 * 「+」面板的可见性：点加号，或输入 `/` 让命令建议借这一栏时都在。
 */
const plusPanelVisible = computed(
  () => plusMenuVisible.value || commandSuggestionsVisible.value
);

// 加号只开合这一栏；桌面显示上传文件和预设问题，手机首页还提供拍照。
const togglePlusMenu = () => {
  const next = !plusPanelVisible.value;
  closeOtherPanels('plus');
  plusMenuVisible.value = next;
  // 每次打开手机面板都回到第一页（预设问题 / 上传文件 / 拍照）。
  if (next) phonePlusPage.value = 0;
  // 收起时把 `/` 唤出的建议也一起收掉，下次点「+」还是预设问题。
  if (!next) commandMenuDismissed.value = true;
};

// ------------------------------------------------- phone plus panel actions
const openPhonePresetPage = () => {
  phonePlusPage.value = 1;
};

// 面板先收起再点隐藏输入：同一个点击手势里调用，iOS/Android 都会直接调起选择器。
const pickWorkspaceFiles = () => {
  closeComposerPanels();
  fileInputRef.value?.click();
};

const pickPhonePhoto = () => {
  closeComposerPanels();
  phonePhotoInputRef.value?.click();
};

const handleComposerFileInputChange = async (event: Event) => {
  const input = event.target as HTMLInputElement | null;
  const files = Array.from(input?.files || []);
  // 立刻清空 value，同一文件连续选两次也会重新触发 change。
  if (input) input.value = '';
  if (!files.length) return;
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  await ingestComposerFiles({
    files: files.map((file) => ({ file, relativePath: file.webkitRelativePath || file.name || 'upload' })),
    directories: [],
    unreadable: 0,
    depthSkipped: 0
  });
};

// 旋转/拉宽进入手机布局时，输入区模型浮层不能悬空留着（锚点会被移除）。
watch(
  () => props.phoneLayout,
  (isPhone) => {
    if (isPhone) closeComposerPanels();
  }
);

// ------------------------------------------------------------- model switching
const reloadModelCatalog = async () => {
  await ensureComposerModelCatalog({ force: true });
};

const toggleModelMenu = () => {
  const next = !modelMenuVisible.value;
  closeOtherPanels('model');
  modelMenuVisible.value = next;
  if (next) void ensureComposerModelCatalog();
};

const selectModel = async (modelId: string) => {
  const target = String(modelId || '').trim();
  if (!target || modelSwitching.value) return;
  if (target === composerModelName.value) {
    modelMenuVisible.value = false;
    return;
  }
  const applyModel = props.applyModel as
    | ((payload: { modelId: string; reasoningEffort: string }) => Promise<{
        ok?: boolean;
        message?: string;
      }>)
    | null;
  if (typeof applyModel !== 'function') {
    ElMessage.warning(t('chat.composer.modelUnavailable'));
    return;
  }
  modelSwitching.value = true;
  try {
    const result = await applyModel({ modelId: target, reasoningEffort: reasoningEffort.value });
    if (result?.ok === false) {
      ElMessage.error(String(result?.message || t('chat.composer.modelSwitchFailed')));
      return;
    }
    modelMenuVisible.value = false;
    ElMessage.success(String(result?.message || t('chat.composer.modelSwitched', { name: target })));
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.composer.modelSwitchFailed')));
  } finally {
    modelSwitching.value = false;
  }
};

// ---------------------------------------------------------- preset questions
const applyPresetQuestion = (question: string) => {
  closeComposerPanels();
  const preset = String(question || '');
  const normalized = preset.trim();
  if (!normalized) return;
  const current = String(inputText.value || '');
  inputText.value = current.trim() ? `${current.replace(/\s*$/, '')}\n${preset}` : preset;
  commandMenuDismissed.value = false;
  nextTick(() => {
    resizeInput();
    const el = inputRef.value;
    const cursor = inputText.value.length;
    if (!el) return;
    if (typeof el.focus === 'function') {
      el.focus();
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(cursor, cursor);
    }
    caretPosition.value = cursor;
  });
};

const handleSend = async () => {
  // 会话在跑时不再静默吞掉发送：引擎会把这一轮挂进输入队列，输入区上方出现排队条；
  // 若只是等待全局并发槽位，则按普通已发送轮次留在时间线里显示排队中。
  if (voiceRecording.value) return;
  closeComposerPanels();
  if (commandSuggestionsVisible.value && applyCommandSuggestion()) {
    return;
  }
  // Prevent sending while generated attachments or workspace drop uploads are still settling.
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  const workspaceNotice = buildWorkspaceFileNotice(attachments.value.flatMap(item => item.workspace_path ? [item.workspace_path] : []));
  const content = [String(inputText.value || '').trim(), workspaceNotice].filter(Boolean).join('\n\n');
  const payloadAttachments = buildAttachmentPayload();
  if (!content && payloadAttachments.length === 0 && !hasInquirySelection.value) return;
  chatDebugLog('messenger.send', 'composer-send-emit', {
    activeSessionId: targetSessionId.value,
    messageCount: Array.isArray(props.contextMessages) ? props.contextMessages.length : 0,
    contentLength: content.length,
    attachmentCount: payloadAttachments.length,
    approvalMode: approvalModeValue.value,
    hasInquirySelection: hasInquirySelection.value
  });
  emit('send', {
    content,
    attachments: payloadAttachments,
    reasoningEffort: reasoningEffort.value,
    approvalMode: approvalModeValue.value
  });
  inputText.value = '';
  commandMenuDismissed.value = false;
  caretPosition.value = 0;
  resetHistoryRecall();
  resetInputHeight();
  clearAttachments();
  flushPersistDraftState();
  focusComposerInputAtEnd();
  // 排队补水：这一轮若被挂进队列，排队条要立刻跟上；队列还没落库时
  // queue_enter 事件会再触发一次刷新，不会漏掉。
  scheduleChatQueueRefresh(activeQueueSessionId.value, 0);
};

const handleSendOrStop = async () => {
  if (stopButtonActive.value) {
    emit('stop');
    return;
  }
  await handleSend();
};

const handleSendButtonKeydown = (event: KeyboardEvent) => {
  if (!stopButtonActive.value) {
    return;
  }
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    event.stopPropagation();
    focusComposerInputAtEnd();
  }
};

const isPointerInside = (element: HTMLElement | null, target: Node | null): boolean =>
  Boolean(element && target && element.contains(target));

const hasOpenComposerPanel = (): boolean => plusPanelVisible.value || modelMenuVisible.value;

const handleDocumentPointerDown = (event: PointerEvent) => {
  const target = event.target as Node | null;
  // 面板整栏挂在加号锚点内，点外面就把整栏收掉（`/` 建议也一起收）。
  if (plusPanelVisible.value && !isPointerInside(plusMenuAnchorRef.value, target)) {
    plusMenuVisible.value = false;
    commandMenuDismissed.value = true;
  }
  if (modelMenuVisible.value && !isPointerInside(modelMenuAnchorRef.value, target)) {
    modelMenuVisible.value = false;
  }
};

// Escape must dismiss a panel even when focus sits outside the textarea.
const handleDocumentKeydown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape' || !hasOpenComposerPanel()) return;
  closeComposerPanels();
};

onMounted(async () => {
  await nextTick();
  resizeInput();
  if (typeof document !== 'undefined') {
    document.addEventListener('pointerdown', handleDocumentPointerDown);
    document.addEventListener('keydown', handleDocumentKeydown);
  }
});

onBeforeUnmount(() => {
  flushPersistDraftState();
  closeComposerPanels();
  if (typeof document !== 'undefined') {
    document.removeEventListener('pointerdown', handleDocumentPointerDown);
    document.removeEventListener('keydown', handleDocumentKeydown);
  }
});

// Reset command suggestion state whenever command input changes.
watch(
  () => commandQuery.value,
  () => {
    commandMenuDismissed.value = false;
    commandMenuIndex.value = 0;
  }
);

// 任何来源的文本变化（草稿回填、预设问题、命令插入、发送清空）都重算输入框高度，
// 不能只靠 @input：程序化赋值不触发 input 事件。
watch(inputText, () => {
  void nextTick(resizeInput);
});

watch(
  () => commandSuggestions.value.length,
  (value) => {
    if (!value) {
      commandMenuIndex.value = 0;
      return;
    }
    if (commandMenuIndex.value >= value) {
      commandMenuIndex.value = 0;
    }
  }
);

// Clear attachments when demo mode toggles to avoid stale state.
watch(
  () => props.demoMode,
  (value) => {
    if (value) {
      clearAttachments();
    }
  }
);

watch(
  () => [targetSessionId.value, props.reasoningEffort] as const,
  () => {
    const session = resolveCurrentSession();
    reasoningEffort.value = normalizeReasoningEffort(
      session?.reasoning_effort ?? session?.reasoningEffort ?? props.reasoningEffort
    );
  },
  { immediate: true }
);

watch(
  () => props.draftKey,
  (value, previousValue) => {
    flushPersistDraftState(String(previousValue || ''));
    restoreDraftStateByKey(String(value || ''));
  },
  { immediate: true }
);

// ------------------------------------------------- workspace references (B2 -> B4)
// Workspace references are ordinary editable text, shared with draft persistence
// and the normal send path. Append without replacing the user's existing draft.
const applyWorkspaceQuoteReferences = () => {
  const incoming = takeWorkspaceChatReferences();
  if (!incoming.length) return;
  appendTextToComposer(incoming.map(item => item.path).join('\n'));
};

watch(pendingWorkspaceChatReferences, applyWorkspaceQuoteReferences, { immediate: true });

watch(
  () => inputText.value,
  () => {
    schedulePersistDraftState();
  }
);

watch(
  () => attachments.value,
  () => {
    syncVideoAttachmentDrafts();
    schedulePersistDraftState();
  },
  { deep: true }
);

watch(
  () => Boolean(props.loading),
  (next, previous) => {
    if (next === previous) return;
    chatDebugLog('chat.composer', 'loading-prop-change', {
      from: previous,
      to: next,
      sessionId: targetSessionId.value,
      canSendOrStop: canSendOrStop.value,
      attachmentBusy: attachmentBusy.value,
      voiceRecording: voiceRecording.value
    });
  },
  { immediate: true }
);

defineExpose({
  appendTextToComposer,
  focusComposerInputAtEnd
});
</script>

<style scoped src="./composerFooter.css"></style>
