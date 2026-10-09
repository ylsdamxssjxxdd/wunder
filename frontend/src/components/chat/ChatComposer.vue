<template>
  <div
    class="input-container input-container--world"
    :class="{ dragover: dragActive }"
    @dragenter="handleDragEnter"
    @dragover="handleDragOver"
    @dragleave="handleDragLeave"
    @drop="handleDrop"
  >
    <div v-if="showUploadArea" class="upload-preview">
      <div class="upload-preview-list">
        <div
          v-for="attachment in attachments"
          :key="attachment.id"
          class="upload-preview-item"
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
            <i
              :class="['fa-solid', resolveAttachmentIconClass(attachment), 'upload-preview-icon']"
              aria-hidden="true"
            ></i>
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
      <div v-if="attachmentBusy > 0" class="upload-preview-status">
        {{ t('chat.attachments.processing', { count: attachmentBusy }) }}
      </div>
    </div>

    <div v-if="references.length" class="upload-preview workspace-quote-preview">
      <div class="upload-preview-list">
        <div
          v-for="reference in references"
          :key="reference.id"
          class="upload-preview-item workspace-quote-item"
        >
          <div class="upload-preview-main">
            <i
              :class="[
                'fa-solid',
                reference.isDir ? 'fa-folder' : 'fa-file-lines',
                'upload-preview-icon'
              ]"
              aria-hidden="true"
            ></i>
            <div class="upload-preview-copy">
              <span class="upload-preview-name" :title="reference.path">{{ reference.name }}</span>
              <span class="upload-preview-meta">@{{ reference.path }}</span>
            </div>
          </div>
          <button
            class="upload-preview-remove"
            type="button"
            :title="t('chat.composer.referenceRemove')"
            :aria-label="t('chat.composer.referenceRemove')"
            @click.stop="removeReference(reference.id)"
          >
            <i class="fa-solid fa-xmark upload-preview-remove-icon" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </div>

    <!-- 输入框是唯一的「卡片」：白底 + 细边 + 圆角；工具行不再共用这张底。 -->
    <div class="input-box input-box--world">
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
      />

      <!-- 录音中的计时文本随麦克风进入工具栏（对齐桌面 composer.slint:610-618）。 -->
      <div v-if="voiceTranscribing" class="composer-voice-indicator">
        <span class="composer-voice-rings" aria-hidden="true">
          <span></span><span></span><span></span>
        </span>
        <span>{{ voiceTranscribingLabel }}</span>
      </div>
    </div>

    <div class="composer-action-row">
      <div class="composer-action-group">
        <!-- 命令 / 预设问题 / 录音 收进「+」：常驻行只留工作目录与审批模式。
             输入 `/` 仍就地展开命令列表；面板内三段各自开合，互不遮挡。 -->
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

          <div v-if="plusPanelVisible" class="composer-panel composer-panel--plus" @click.stop>
            <div class="composer-plus-row" @mouseenter="hoverPlusSubmenu('command')">
              <button
                class="composer-plus-item"
                type="button"
                :class="{ 'is-active': commandMenuOpen }"
                :aria-expanded="commandMenuOpen"
                @click.stop="toggleCommandMenu"
              >
                <i class="fa-solid fa-terminal composer-plus-item-icon" aria-hidden="true"></i>
                <span class="composer-plus-item-label">{{ t('chat.composer.commands') }}</span>
                <i class="fa-solid fa-chevron-right composer-plus-item-caret" aria-hidden="true"></i>
              </button>
              <div v-if="commandPanelVisible && !presetMenuVisible" class="composer-plus-flyout" role="listbox">
                <button
                  v-for="(item, index) in commandPanelItems"
                  :key="item.command"
                  class="command-menu-item"
                  :class="{ active: !commandMenuOpen && index === commandMenuIndex }"
                  type="button"
                  role="option"
                  :aria-selected="!commandMenuOpen && index === commandMenuIndex"
                  @mousedown.prevent="applyCommandSuggestion(index)"
                  @mouseenter="setCommandMenuIndex(index)"
                >
                  <span class="command-menu-name">{{ item.command }}</span>
                  <span class="command-menu-desc">{{ item.description }}</span>
                </button>
                <div class="command-menu-hint">{{ t('chat.commandMenu.hint') }}</div>
              </div>
            </div>

            <template v-if="presetQuestionItems.length">
              <div class="composer-plus-row" @mouseenter="hoverPlusSubmenu('preset')">
                <button
                  class="composer-plus-item"
                  type="button"
                  :class="{ 'is-active': presetMenuVisible }"
                  :disabled="stopButtonActive"
                  :aria-expanded="presetMenuVisible"
                  @click.stop="togglePresetMenu"
                >
                  <i
                    class="fa-solid fa-wand-magic-sparkles composer-plus-item-icon"
                    aria-hidden="true"
                  ></i>
                  <span class="composer-plus-item-label">{{
                    t('chat.commandMenu.presetQuestions')
                  }}</span>
                  <i class="fa-solid fa-chevron-right composer-plus-item-caret" aria-hidden="true"></i>
                </button>
                <ComposerPresetQuestions
                  v-if="presetMenuVisible"
                  class="composer-plus-flyout"
                  :items="presetQuestionItems"
                  :disabled="stopButtonActive"
                  @pick="applyPresetQuestion"
                />
              </div>
            </template>

            <div class="composer-plus-row" @mouseenter="hoverPlusSubmenu('none')">
              <button
                v-if="voiceSupported"
                class="composer-plus-item"
                type="button"
                :class="{ 'is-recording': voiceRecording }"
                :disabled="composerBusy > 0 || stopButtonActive || voiceTranscribing"
                :title="voiceButtonTitle"
                @click.stop="handleToggleVoiceRecord"
              >
                <i
                  :class="[
                    voiceRecording
                      ? 'fa-solid fa-stop'
                      : voiceTranscribing
                        ? 'fa-solid fa-waveform-lines'
                        : 'fa-solid fa-microphone',
                    'composer-plus-item-icon'
                  ]"
                  aria-hidden="true"
                ></i>
                <span class="composer-plus-item-label">{{ t('messenger.world.voice.title') }}</span>
                <span v-if="voiceRecording" class="composer-voice-timer" :title="voiceRecordingLabel">
                  {{ formatVoiceDurationLabel(props.voiceDurationMs) }}
                </span>
              </button>
            </div>
          </div>
        </div>

        <ComposerStatusBar :workspace-name="workspaceName" />

        <div v-if="showApprovalModeSelector" ref="approvalMenuAnchorRef" class="composer-anchor">
          <button
            class="composer-approval-trigger"
            type="button"
            :class="{ 'is-active': approvalMenuVisible }"
            :title="approvalTriggerTitle"
            :aria-label="approvalTriggerTitle"
            :aria-expanded="approvalMenuVisible"
            :disabled="approvalModeSyncing"
            @click.stop="toggleApprovalMenu"
          >
            <i :class="[approvalTriggerIcon, 'composer-approval-icon']" aria-hidden="true"></i>
            <span class="composer-approval-label">{{ approvalModeLabel }}</span>
            <i class="fa-solid fa-chevron-down composer-caret" aria-hidden="true"></i>
          </button>
          <div v-if="approvalMenuVisible" class="composer-panel composer-panel--approval" @click.stop>
            <div class="composer-panel-title">{{ t('chat.composer.approval') }}</div>
            <button
              v-for="option in approvalOptions"
              :key="option.value"
              class="composer-panel-item composer-panel-item--radio"
              :class="{ 'is-selected': option.value === approvalModeValue }"
              type="button"
              role="menuitemradio"
              :aria-checked="option.value === approvalModeValue"
              @click="selectApprovalMode(option.value)"
            >
              <span class="composer-panel-radio" aria-hidden="true">
                <i v-if="option.value === approvalModeValue" class="fa-solid fa-check"></i>
              </span>
              <span class="composer-panel-main">
                <span class="composer-panel-item-label">{{ option.label }}</span>
                <span class="composer-panel-desc">{{ option.description }}</span>
              </span>
            </button>
            <div class="composer-panel-hint">{{ t('chat.composer.approval.hint') }}</div>
          </div>
        </div>
      </div>

        <div class="composer-action-group composer-action-group--end">
          <!-- 右组（对齐桌面 composer.slint:637-663）：模型 + 上下文占用 → 发送/停止。
               占用统计不再单独占一个图标位：触发器上的大脑就是占用图标（按占用率填充），
               数字与进度在浮层里展开。数据仍取自**唯一**一份投影 sessionContextUsage.ts。 -->
          <div ref="modelMenuAnchorRef" class="composer-anchor">
            <button
              class="composer-model-trigger"
              type="button"
              :class="{ 'is-active': modelMenuVisible }"
              :title="modelTriggerTitle"
              :aria-label="modelTriggerTitle"
              :aria-expanded="modelMenuVisible"
              @click.stop="toggleModelMenu"
            >
              <ContextUsageIcon class="composer-model-usage-icon" :ratio="contextUsage.ratio" />
              <span class="composer-model-name">{{ modelTriggerLabel }}</span>
              <span v-if="reasoningEffortCompactLabel" class="composer-model-effort">
                {{ reasoningEffortCompactLabel }}
              </span>
              <span class="composer-model-usage" :class="contextUsage.level" data-testid="composer-context-percent">
                {{ contextUsage.percentText }}
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
              :reasoning-effort="reasoningEffort"
              :reasoning-effort-options="reasoningEffortOptions"
              @select="selectModel"
              @effort="selectReasoningEffort"
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
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import ComposerPresetQuestions from '@/components/chat/ComposerPresetQuestions.vue';
import ComposerStatusBar from '@/components/chat/ComposerStatusBar.vue';
import ContextUsageIcon from '@/components/chat/ContextUsageIcon.vue';
import ModelPickerPopover from '@/components/chat/ModelPickerPopover.vue';

import { processChatMediaAttachment, convertChatAttachment } from '@/api/chat';
import { uploadWunderWorkspace } from '@/api/workspace';
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
import {
  COMPOSER_REFERENCE_MAX,
  buildComposerSendContent,
  mergeComposerReferences,
  normalizeComposerReferences,
  removeComposerReference,
  type ComposerReference
} from '@/components/chat/composerReferences';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { clearWorkspaceDragPaths, hasWorkspaceDragPaths, readWorkspaceDragPaths } from '@/components/chat/workspaceDrag';
import { workspaceDisplayNameOverride } from '@/views/messenger/workspaceDisplayName';
import { useSessionContextUsage } from '@/views/messenger/sessionContextUsage';
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
  }
});

const emit = defineEmits([
  'send',
  'stop',
  'new-thread',
  'open-thread',
  'toggle-voice-record',
  'update:approval-mode',
  'update:reasoning-effort'
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
const approvalMenuAnchorRef = ref<HTMLElement | null>(null);
const modelMenuAnchorRef = ref<HTMLElement | null>(null);
const plusMenuAnchorRef = ref<HTMLElement | null>(null);
const approvalMenuVisible = ref(false);
const modelMenuVisible = ref(false);
const presetMenuVisible = ref(false);
const plusMenuVisible = ref(false);
// 命令按钮把命令面板钉住（桌面 `root.command-open`）；输入即交回 `/` 建议链路。
const commandMenuOpen = ref(false);
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

const IMAGE_MIME_TYPES = new Set([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/bmp',
  'image/webp',
  'image/wmf',
  'image/emf',
  'image/x-wmf',
  'image/x-emf',
  'application/x-msmetafile',
  'application/emf',
  'application/x-emf'
]);

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

type DirectoryReaderLike = {
  readEntries: (
    successCallback: (entries: FileSystemEntryLike[]) => void,
    errorCallback?: (reason: DOMException) => void
  ) => void;
};

type FileSystemEntryLike = {
  isFile?: boolean;
  isDirectory?: boolean;
  name?: string;
  file?: (successCallback: (file: File) => void, errorCallback?: (reason: DOMException) => void) => void;
  createReader?: () => DirectoryReaderLike;
};

type DataTransferItemLike = DataTransferItem & {
  webkitGetAsEntry?: () => FileSystemEntryLike | null;
};

type WorkspaceDroppedFile = {
  file: File;
  relativePath: string;
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
const MAX_WORKSPACE_UPLOAD_BYTES = 1024 * 1024 * 1024;
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
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
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
const showUploadArea = computed(() => attachments.value.length > 0 || attachmentBusy.value > 0);
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
// 浮层里的明细都读同一个投影，同一屏不会漂出两个百分比。
const contextUsage = useSessionContextUsage({
  scope: () => `${String(chatStore.activeSessionId || '').trim()}:${composerModelName.value}`,
  messages: () => (Array.isArray(props.contextMessages) ? props.contextMessages : []),
  session: () => resolveCurrentSession(),
  loading: () => Boolean(props.loading),
  modelName: () => composerModelName.value
});
const contextUsageTitle = computed(() => {
  const counts = String(contextUsage.value.counts || '').trim();
  return counts ? `${t('profile.stats.contextTokens')} ${counts}` : t('profile.stats.contextTokens');
});
// 输入卡的状态面只剩「这次消息发到哪个工作目录」；在线与占用都不再单独占一行。
const workspaceName = computed(
  () => String(workspaceDisplayNameOverride.value || '').trim() || t('messenger.workspace.defaultName')
);

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
// 输入框不再放占位文字：发送目标已经由工具栏左组的工作目录 chip 说明。
const inputPlaceholder = computed(() =>
  props.inquiryActive ? t('chat.input.inquiryPlaceholder') : ''
);
const formatVoiceDurationLabel = (durationMs: unknown): string => {
  const value = Number(durationMs);
  if (!Number.isFinite(value) || value <= 0) {
    return '0:00';
  }
  const totalSeconds = Math.max(1, Math.round(value / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
};
const voiceButtonTitle = computed(() => {
  if (props.voiceRecording) return t('messenger.world.voice.stop');
  if (props.voiceTranscribing) return t('messenger.world.voice.transcribing');
  return t('messenger.world.voice.start');
});
const voiceRecordingLabel = computed(() =>
  t('messenger.world.voice.recording', {
    duration: formatVoiceDurationLabel(props.voiceDurationMs)
  })
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
const approvalModeLabel = computed(
  () => approvalOptions.value.find((item) => item.value === approvalModeValue.value)?.label || ''
);
const approvalTriggerIcon = computed(
  () => approvalOptions.value.find((item) => item.value === approvalModeValue.value)?.icon || 'fa-solid fa-bolt'
);
const showApprovalModeSelector = computed(() => Boolean(props.approvalModeEditable));
const approvalModeSyncing = computed(() => props.approvalModeSyncing);
const approvalTriggerTitle = computed(() =>
  approvalModeSyncing.value
    ? t('chat.composer.approval.syncing')
    : `${t('chat.composer.approval')}: ${approvalModeLabel.value}`
);
const voiceSupported = computed(() => Boolean(props.voiceSupported));
const voiceRecording = computed(() => Boolean(props.voiceRecording));
const voiceTranscribing = computed(() => Boolean(props.voiceTranscribing));
const stopButtonActive = computed(() => Boolean(props.loading));
const canSendOrStop = computed(() => {

  if (stopButtonActive.value) return true;
  if (composerBusy.value > 0) return false;
  return (
    Boolean(inputText.value.trim()) ||
    attachments.value.length > 0 ||
    hasInquirySelection.value
  );
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

// 命令面板两条入口共用一份 DOM：输入 `/` 走建议过滤，命令按钮钉住时列全部命令。
const allCommandItems = computed(() =>
  slashCommandDefinitions.map((item) => ({
    command: item.command,
    description: t(item.descriptionKey)
  }))
);
const commandPanelItems = computed(() =>
  commandMenuOpen.value ? allCommandItems.value : commandSuggestions.value
);
const commandPanelVisible = computed(() => commandMenuOpen.value || commandSuggestionsVisible.value);
const presetQuestionItems = computed(() => normalizeAgentPresetQuestions(props.presetQuestions));

const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

const resolveUploadError = (error, fallback) =>
  error?.response?.data?.detail || error?.message || fallback;

const formatBytes = (value: unknown): string => {
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B';
  }
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = bytes;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  const digits = size >= 10 || unitIndex === 0 ? 0 : 1;
  return `${size.toFixed(digits)} ${units[unitIndex]}`;
};

const resolveFileExtension = (filename) => {
  const parts = String(filename || '').trim().split('.');
  if (parts.length < 2) return '';
  return parts.pop().toLowerCase();
};

const normalizeImageMimeType = (value: unknown): string => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase()
    .split(';')[0]
    ?.trim();
  if (normalized === 'image/jpg') return 'image/jpeg';
  return normalized;
};

const inferImageMimeTypeFromExtension = (filename: unknown): string => {
  const ext = resolveFileExtension(String(filename || ''));
  switch (ext) {
    case 'png':
      return 'image/png';
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg';
    case 'gif':
      return 'image/gif';
    case 'bmp':
      return 'image/bmp';
    case 'webp':
      return 'image/webp';
    case 'wmf':
      return 'image/wmf';
    case 'emf':
      return 'image/emf';
    default:
      return '';
  }
};

const isWindowsMetafile = (file: File): boolean => {
  const ext = resolveFileExtension(file?.name);
  return ext === 'wmf' || ext === 'emf';
};

const resolveSupportedImageMimeType = (file): string => {
  const mimeType = normalizeImageMimeType(file?.type);
  if (mimeType && IMAGE_MIME_TYPES.has(mimeType)) {
    return mimeType;
  }
  const inferred = inferImageMimeTypeFromExtension(file?.name);
  return IMAGE_MIME_TYPES.has(inferred) ? inferred : '';
};

const validateImageFile = async (file: File): Promise<void> => {
  if (!file) {
    throw new Error(t('chat.attachments.imageInvalid'));
  }
  if (isWindowsMetafile(file)) {
    return;
  }
  if (typeof createImageBitmap === 'function') {
    let bitmap: ImageBitmap | null = null;
    try {
      bitmap = await createImageBitmap(file);
      return;
    } catch {
      // Fallback to object URL decode below when bitmap decoding is unavailable.
    } finally {
      bitmap?.close();
    }
  }
  const objectUrl = URL.createObjectURL(file);
  try {
    await new Promise<void>((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve();
      image.onerror = () => reject(new Error(t('chat.attachments.imageInvalid')));
      image.src = objectUrl;
    });
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
};

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
  // 一旦开始输入，命令面板交回 `/` 建议链路，避免钉住的面板抢走 Enter/Tab。
  commandMenuOpen.value = false;
  resizeInput();
  syncCaretPosition();
};

const focusComposerInputAtEnd = () => {
  void nextTick(() => {
    const el = inputRef.value;
    if (!el) return;
    const cursor = String(inputText.value || '').length;
    if (typeof el.focus === 'function') {
      el.focus();
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(cursor, cursor);
    }
    caretPosition.value = cursor;
  });
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
  const item = commandPanelItems.value[index];
  if (!item) {
    return false;
  }
  const leading = String(inputText.value || '').match(/^\s*/)?.[0] || '';
  inputText.value = `${leading}${item.command} `;
  commandMenuOpen.value = false;
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
    const hasOpenPanel =
      commandMenuOpen.value ||
      approvalMenuVisible.value ||
      modelMenuVisible.value ||
      presetMenuVisible.value;
    if (hasOpenPanel) {
      event.preventDefault();
      closeComposerPanels();
      return;
    }
  }
  if (!commandSuggestionsVisible.value) {
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
  if (!content.trim() && !publicPath && derivedAttachments.length === 0) return null;
  const attachment: ComposerDraftAttachment = {
    id,
    type,
    name,
    content
  };
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

const buildWorkspaceFileNotice = (paths: string[], items: WorkspaceDroppedFile[]): string => {
  const normalized = paths.map((item) => normalizeWorkspacePath(item)).filter(Boolean);
  const fileNames = items
    .map((item) => normalizeWorkspacePath(item.relativePath || item.file?.name || 'upload'))
    .filter(Boolean);
  const displayPaths = normalized.length ? normalized : fileNames;
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

const appendWorkspaceFileNotice = (paths: string[], items: WorkspaceDroppedFile[]) => {
  const notice = buildWorkspaceFileNotice(paths, items);
  if (!notice) return;
  appendTextToComposer(notice);
};

const readDirectoryEntries = (reader: DirectoryReaderLike): Promise<FileSystemEntryLike[]> =>
  new Promise((resolve) => {
    const entries: FileSystemEntryLike[] = [];
    const readBatch = () => {
      reader.readEntries(
        (batch: FileSystemEntryLike[]) => {
          if (!batch.length) {
            resolve(entries);
            return;
          }
          entries.push(...batch);
          readBatch();
        },
        () => resolve(entries)
      );
    };
    readBatch();
  });

const walkDroppedEntry = async (
  entry: FileSystemEntryLike,
  prefix: string
): Promise<WorkspaceDroppedFile[]> => {
  if (!entry) return [];
  if (entry.isFile) {
    const file = await new Promise<File | null>((resolve) => {
      entry.file?.((target) => resolve(target), () => resolve(null));
    });
    if (!file) return [];
    return [{ file, relativePath: `${prefix}${file.name}` }];
  }
  if (entry.isDirectory) {
    const reader = entry.createReader?.();
    if (!reader) return [];
    const nextPrefix = `${prefix}${entry.name || ''}/`;
    const children = await readDirectoryEntries(reader);
    const nested = await Promise.all(children.map((child) => walkDroppedEntry(child, nextPrefix)));
    return nested.flat();
  }
  return [];
};

const collectDroppedWorkspaceFiles = async (
  dataTransfer: DataTransfer | null | undefined
): Promise<WorkspaceDroppedFile[]> => {
  const items = Array.from(dataTransfer?.items || []) as DataTransferItemLike[];
  if (items.length) {
    const batches = await Promise.all(
      items.map((item) => {
        const entry = item.webkitGetAsEntry?.();
        if (entry) {
          return walkDroppedEntry(entry, '');
        }
        const file = item.getAsFile();
        return file ? [{ file, relativePath: file.name || 'upload' }] : [];
      })
    );
    return batches.flat();
  }
  return Array.from(dataTransfer?.files || []).map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name || 'upload'
  }));
};

const uploadDroppedFilesToWorkspace = async (items: WorkspaceDroppedFile[]): Promise<string[]> => {
  const fileList = items.map((item) => item.file).filter(Boolean);
  if (!fileList.length) return [];
  const totalBytes = fileList.reduce((sum, file) => sum + (Number(file?.size) || 0), 0);
  if (totalBytes > MAX_WORKSPACE_UPLOAD_BYTES) {
    throw new Error(t('workspace.upload.tooLarge', { limit: formatBytes(MAX_WORKSPACE_UPLOAD_BYTES) }));
  }
  const formData = new FormData();
  formData.append('path', resolveWorkspaceDropPath(''));
  const agentId = String(props.workspaceAgentId || '').trim();
  const containerId = resolveComposerWorkspaceContainerId();
  if (agentId) {
    formData.append('agent_id', agentId);
  }
  // No `container_id` field: the cloud workspace is a single per-user root.
  fileList.forEach((file, index) => {
    formData.append('files', file, file.name || 'upload');
    formData.append('relative_paths', normalizeWorkspacePath(items[index]?.relativePath || file.name || 'upload'));
  });
  const response = await uploadWunderWorkspace(formData);
  const uploadedPaths = resolveUploadedWorkspacePaths(response?.data);
  const refreshDetail = {
    reason: 'composer-drop-upload',
    containerId,
    paths: uploadedPaths,
    treeVersion: response?.data?.tree_version,
    tree_version: response?.data?.tree_version
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
  return uploadedPaths;
};

/**
 * Shared entry for "upload into the workspace" from drag/drop and from the
 * "+" menu. Busy accounting keeps the send button disabled until it settles.
 */
const ingestComposerFiles = async (items: WorkspaceDroppedFile[]) => {
  const files = (Array.isArray(items) ? items : []).filter((item) => Boolean(item?.file));
  if (!files.length) return;
  if (stopButtonActive.value) return;
  closeComposerPanels();
  workspaceDropBusy.value += 1;
  try {
    const uploadedPaths = await uploadDroppedFilesToWorkspace(files);
    appendWorkspaceFileNotice(uploadedPaths, files);
    ElMessage.success(t('chat.workspaceDrop.uploaded', { count: uploadedPaths.length || files.length }));
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.workspaceDrop.failed')));
  } finally {
    workspaceDropBusy.value = Math.max(0, workspaceDropBusy.value - 1);
  }
};

// 「上传到工作目录」的显式文件对话框随 `+` 菜单移除：入口回到左栏工作目录区
// （WorkspaceFilesPanel 的上传按钮走同一条 /workspace 上传契约），输入卡只保留拖拽上传。
const handleDragEnter = (event) => {
  if (stopButtonActive.value) return;
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  dragCounter.value += 1;
  dragActive.value = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = composerBusy.value > 0
      ? 'none'
      : hasWorkspaceDragPaths(event.dataTransfer)
        ? 'move'
        : 'copy';
  }
};

const handleDragOver = (event) => {
  if (stopButtonActive.value) return;
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = composerBusy.value > 0
      ? 'none'
      : hasWorkspaceDragPaths(event.dataTransfer)
        ? 'move'
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
  if (stopButtonActive.value) return;
  if (!hasFileDrag(event) && !hasWorkspaceDragPaths(event?.dataTransfer)) return;
  event.preventDefault();
  dragCounter.value = 0;
  dragActive.value = false;
  const workspacePaths = readWorkspaceDragPaths(event.dataTransfer);
  if (workspacePaths.length) {
    if (composerBusy.value > 0) {
      ElMessage.warning(chatBusyMessage.value);
      return;
    }
    closeComposerPanels();
    insertTextIntoComposer(workspacePaths.join('\n'), 'cursor');
    clearWorkspaceDragPaths();
    ElMessage.success(t('chat.workspaceDrop.pathsInserted', { count: workspacePaths.length }));
    return;
  }
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  const droppedItems = await collectDroppedWorkspaceFiles(event.dataTransfer);
  if (!droppedItems.length) return;
  await ingestComposerFiles(droppedItems);
};

const requestMediaProcessing = async (formData: FormData): Promise<ProcessedMediaResponse> => {
  const response = await processChatMediaAttachment(formData);
  return (response?.data?.data || {}) as ProcessedMediaResponse;
};

const buildImageDraftAttachment = (
  filename: string,
  payload: ProcessedMediaResponse
): ComposerDraftAttachment => {
  const attachment = Array.isArray(payload.attachments)
    ? payload.attachments
        .map((item) => normalizeProcessedMediaAttachment(item, 'image'))
        .find(Boolean) || null
    : null;
  if (!attachment) {
    throw new Error(t('chat.attachments.emptyResult'));
  }
  attachment.id = buildAttachmentId();
  attachment.type = 'image';
  attachment.name = attachment.name || filename;
  attachment.content = '';
  return attachment;
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

const pushAttachment = (attachment: ComposerDraftAttachment) => {
  attachments.value.push(attachment);
  syncVideoAttachmentDrafts();
};

const processImageFile = async (file: File): Promise<ComposerDraftAttachment> => {
  const formData = new FormData();
  formData.append('file', file);
  const payload = await requestMediaProcessing(formData);
  if (String(payload.kind || '').trim() === 'gif') {
    return buildVideoDraftAttachment(file.name || 'gif', payload, undefined, 'gif');
  }
  return buildImageDraftAttachment(file.name || 'image', payload);
};

/**
 * §8.2 attachment ingestion: media files go through the existing media processor,
 * every other file goes through the existing converter, and both end up as
 * ordinary composer attachments (the payload contract for `send` is untouched).
 *
 * 入口状态：`+ 更多 / 回形针`两个入口随 0.5.0 桌面对齐一并移除，云端语义下文件
 * 走左栏工作目录区（上传 + 引用到聊天）。整条管线与 chip 渲染暂时保留（无 UI 入口），
 * 交由主智能体决定是重新挂载入口还是整体移除，避免静默丢能力。
 */
const ingestAttachmentFiles = async (files: File[]) => {
  const list = (Array.isArray(files) ? files : []).filter(Boolean);
  if (!list.length) return;
  if (stopButtonActive.value) return;
  attachmentBusy.value += 1;
  try {
    for (const file of list) {
      const imageMime = resolveSupportedImageMimeType(file);
      const mime = imageMime || normalizeImageMimeType(file.type);
      try {
        if (mime.startsWith('image/')) {
          await validateImageFile(file);
          pushAttachment(await processImageFile(file));
          ElMessage.success(t('chat.attachments.imageAdded', { name: file.name }));
          continue;
        }
        if (mime.startsWith('video/') || mime.startsWith('audio/')) {
          pushAttachment(await processImageFile(file));
          ElMessage.success(t('chat.attachments.imageAdded', { name: file.name }));
          continue;
        }
        const response = await convertChatAttachment(file);
        const payload = (response?.data?.data || {}) as Record<string, unknown>;
        const content = String(payload.content || '');
        if (!content.trim()) {
          throw new Error(t('chat.attachments.emptyResult'));
        }
        pushAttachment({
          id: buildAttachmentId(),
          type: 'file',
          name: String(payload.name || file.name || '').trim() || file.name,
          content,
          mime_type: file.type || undefined,
          converter: String(payload.converter || '').trim() || undefined,
          warnings: Array.isArray(payload.warnings)
            ? payload.warnings.map((item) => String(item || ''))
            : undefined
        });
        ElMessage.success(t('chat.attachments.fileParsed', { name: file.name }));
      } catch (error) {
        ElMessage.error(resolveUploadError(error, t('chat.attachments.processFailed')));
      }
    }
  } finally {
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
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

const removeAttachment = (id) => {
  attachments.value = attachments.value.filter((item) => item.id !== id);
  markAttachmentProcessing(id, false);
  syncVideoAttachmentDrafts();
};

const clearAttachments = () => {
  attachments.value = [];
  attachmentProcessingIds.value = [];
  expandedVideoAttachmentId.value = '';
  videoFrameRateDrafts.value = {};
  gifFrameStepDrafts.value = {};
};

const closeComposerPanels = () => {
  commandMenuOpen.value = false;
  approvalMenuVisible.value = false;
  modelMenuVisible.value = false;
  presetMenuVisible.value = false;
  plusMenuVisible.value = false;
};

const closeOtherPanels = (keep: 'plus' | 'command' | 'approval' | 'model' | 'preset') => {
  if (keep !== 'command') commandMenuOpen.value = false;
  if (keep !== 'approval') approvalMenuVisible.value = false;
  if (keep !== 'model') modelMenuVisible.value = false;
  if (keep !== 'preset') presetMenuVisible.value = false;
  if (keep !== 'plus' && keep !== 'command' && keep !== 'preset') plusMenuVisible.value = false;
};

/**
 * 「+」面板的可见性：点加号，或被 `/` 建议与预设列表任一分支点亮时都要在。
 * 命令与预设在面板内各自开合，因此它们任一为真也代表面板开着。
 */
const plusPanelVisible = computed(() =>
  plusMenuVisible.value ||
  commandMenuOpen.value ||
  presetMenuVisible.value ||
  commandSuggestionsVisible.value
);

// 加号只开合面板；面板内的三段各自负责自己的开合。
const togglePlusMenu = () => {
  const next = !plusPanelVisible.value;
  closeOtherPanels('plus');
  plusMenuVisible.value = next;
  if (!next) {
    commandMenuOpen.value = false;
    presetMenuVisible.value = false;
  }
};

// 命令按钮只开面板、不发送：选中项填入草稿（与桌面 `root.draft = command.value` 一致）。
const toggleCommandMenu = () => {
  const next = !commandMenuOpen.value;
  closeOtherPanels('command');
  commandMenuOpen.value = next;
  if (next) plusMenuVisible.value = true;
  if (next) commandMenuIndex.value = 0;
};

// 预设问题：只开合列表，选中项由 applyPresetQuestion 填入草稿。
const togglePresetMenu = () => {
  if (stopButtonActive.value) return;
  const next = !presetMenuVisible.value;
  closeOtherPanels('preset');
  presetMenuVisible.value = next;
  if (next) plusMenuVisible.value = true;
};

/**
 * 「+」菜单的级联形态：悬停行就从右侧呼出子面板，移开不自动收起。
 * 语音行没有子面板，指过去即把两个子面板关掉。
 */
const hoverPlusSubmenu = (target: 'command' | 'preset' | 'none') => {
  if (!plusPanelVisible.value) return;
  if (target === 'none') {
    commandMenuOpen.value = false;
    presetMenuVisible.value = false;
    return;
  }
  if (target === 'preset') {
    if (stopButtonActive.value) return;
    commandMenuOpen.value = false;
    presetMenuVisible.value = true;
    return;
  }
  presetMenuVisible.value = false;
  // 输入 `/` 的候选已经挂在同一份子面板上，别再切成全量命令表。
  if (commandSuggestionsVisible.value) return;
  commandMenuOpen.value = true;
  commandMenuIndex.value = 0;
};

const toggleApprovalMenu = () => {
  if (approvalModeSyncing.value) return;
  const next = !approvalMenuVisible.value;
  closeOtherPanels('approval');
  approvalMenuVisible.value = next;
};

const selectApprovalMode = (value: string) => {
  const normalized = String(value || '').trim();
  approvalMenuVisible.value = false;
  if (!normalized || normalized === approvalModeValue.value) return;
  emit('update:approval-mode', normalized);
};

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

const selectReasoningEffort = async (value: ReasoningEffort) => {
  const previous = reasoningEffort.value;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  reasoningEffort.value = value;
  emit('update:reasoning-effort', reasoningEffort.value);
  // Draft sessions have no server record yet; the effort travels with the send.
  if (!sessionId) return;
  try {
    const saved = await chatStore.updateSessionReasoningEffort(sessionId, value);
    if (saved) {
      reasoningEffort.value = normalizeReasoningEffort(saved);
      emit('update:reasoning-effort', reasoningEffort.value);
    }
  } catch {
    reasoningEffort.value = previous;
    emit('update:reasoning-effort', previous);
  }
};

// ------------------------------------------- attachment pipeline (no UI entry)
// 附件（图片/视频/文档 → 媒体处理器/转换器 → 发送 payload 的 attachments）随
// 「+ 更多 / 回形针」入口一并下线：云端语义下文件走左栏工作目录区（上传 + 引用到聊天）。
// 这里保留整条管线与 chip 渲染，等待主智能体决定是重新挂载入口还是整体移除。
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
  if (stopButtonActive.value) return;
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
  // References travel as a trailing `@<relative path>` block in the user turn.
  const content = buildComposerSendContent(
    inputText.value,
    references.value,
    t('chat.composer.referenceBlockTitle')
  );
  const payloadAttachments = buildAttachmentPayload();
  if (!content && payloadAttachments.length === 0 && !hasInquirySelection.value) return;
  chatDebugLog('messenger.send', 'composer-send-emit', {
    activeSessionId: String(chatStore.activeSessionId || '').trim(),
    messageCount: Array.isArray(props.contextMessages) ? props.contextMessages.length : 0,
    contentLength: content.length,
    attachmentCount: payloadAttachments.length,
    referenceCount: references.value.length,
    approvalMode: approvalModeValue.value,
    hasInquirySelection: hasInquirySelection.value
  });
  emit('send', {
    content,
    attachments: payloadAttachments,
    reasoningEffort: reasoningEffort.value,
    approvalMode: approvalModeValue.value,
    referenceCount: references.value.length
  });
  inputText.value = '';
  commandMenuDismissed.value = false;
  caretPosition.value = 0;
  resetInputHeight();
  clearAttachments();
  references.value = [];
  flushPersistDraftState();
  focusComposerInputAtEnd();
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

const handleToggleVoiceRecord = () => {
  if (stopButtonActive.value) return;
  closeComposerPanels();
  emit('toggle-voice-record');
};

const isPointerInside = (element: HTMLElement | null, target: Node | null): boolean =>
  Boolean(element && target && element.contains(target));

const hasOpenComposerPanel = (): boolean =>
  plusPanelVisible.value ||
  commandMenuOpen.value ||
  approvalMenuVisible.value ||
  modelMenuVisible.value ||
  presetMenuVisible.value;

const handleDocumentPointerDown = (event: PointerEvent) => {
  const target = event.target as Node | null;
  if (plusPanelVisible.value && !isPointerInside(plusMenuAnchorRef.value, target)) {
    commandMenuOpen.value = false;
    presetMenuVisible.value = false;
    plusMenuVisible.value = false;
  }
  if (approvalMenuVisible.value && !isPointerInside(approvalMenuAnchorRef.value, target)) {
    approvalMenuVisible.value = false;
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
  () => [String(chatStore.activeSessionId || '').trim(), props.reasoningEffort] as const,
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
// Formal reference model: the file area pushes bounded items into a module-level
// queue; this single shallow watch drains them into removable chips. The `@path`
// block is appended to the outgoing message text on send and cleared afterwards.
const references = ref<ComposerReference[]>([]);

const removeReference = (id: string) => {
  references.value = removeComposerReference(references.value, String(id || ''));
};

const applyWorkspaceQuoteReferences = () => {
  const incoming = takeWorkspaceChatReferences();
  if (!incoming.length) return;
  const result = mergeComposerReferences(references.value, incoming);
  references.value = result.items;
  if (result.added > 0) {
    ElMessage.success(t('chat.workspaceQuote.added', { count: result.added }));
  }
  if (result.items.length >= COMPOSER_REFERENCE_MAX) {
    ElMessage.info(t('chat.composer.referenceLimit', { count: COMPOSER_REFERENCE_MAX }));
  }
};

watch(() => pendingWorkspaceChatReferences.value.length, applyWorkspaceQuoteReferences);

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
      sessionId: String(chatStore.activeSessionId || '').trim(),
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
