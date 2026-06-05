<template>
  <div class="input-container" :class="{ 'input-container--world': worldStyle }">
    <ChatGoalComposer
      v-if="goalEditorVisible"
      :visible="goalEditorVisible"
      :objective="goalObjective"
      :loading="goalLoading"
      :submitting="goalSubmitting"
      :active="goalActive"
      :status="goalStatus"
      @update:objective="emit('update:goal-objective', $event)"
      @submit="emit('submit-goal')"
      @stop="emit('stop')"
      @cancel="emit('cancel-goal-editor')"
    />
    <div
      v-if="goalEditorVisible && composerContextUsageDisplay"
      class="chat-goal-context-usage"
      :class="composerContextUsageClass"
      :style="composerContextUsageStyle"
      :title="composerContextUsageTooltip"
      :aria-label="composerContextUsageTooltip"
    >
      {{ composerContextUsageDisplay }}
    </div>
    <template v-else>
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

    <div
      class="input-box"
      :class="[{ dragover: dragActive }, { 'input-box--world': worldStyle }]"
      :style="worldStyle ? worldComposerStyle : undefined"
      @dragenter="handleDragEnter"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <button
        v-if="worldStyle"
        class="chat-composer-resize-edge"
        type="button"
        :title="t('messenger.world.resize')"
        :aria-label="t('messenger.world.resize')"
        @mousedown.prevent="startWorldComposerResize"
      >
        <span class="chat-composer-resize-grip"></span>
      </button>
      <div v-if="worldStyle" class="messenger-world-toolbar chat-composer-world-toolbar">
        <div
          v-if="presetQuestionItems.length"
          ref="worldPresetCommandAnchorRef"
          class="messenger-world-tool-anchor"
          :class="{ 'is-open': isWorldCommandPanelVisible('preset') }"
          @mouseenter="handleWorldCommandAnchorMouseEnter('preset')"
          @mouseleave="handleWorldCommandAnchorMouseLeave('preset')"
          @focusin="handleWorldCommandAnchorFocusIn('preset')"
          @focusout="handleWorldCommandAnchorFocusOut('preset', $event)"
        >
          <button
            class="messenger-world-tool-btn"
            type="button"
            :class="{ active: isWorldCommandPanelVisible('preset') }"
            :disabled="stopButtonActive"
            :title="t('chat.commandMenu.presetQuestions')"
            :aria-label="t('chat.commandMenu.presetQuestions')"
            @click.prevent="toggleWorldCommandPanel('preset')"
          >
            <i class="fa-solid fa-wand-magic-sparkles chat-composer-command-btn-icon" aria-hidden="true"></i>
          </button>
          <div
            v-if="isWorldCommandPanelVisible('preset')"
            class="chat-composer-command-panel"
            @mouseenter="handleWorldCommandPanelMouseEnter('preset')"
            @mouseleave="handleWorldCommandPanelMouseLeave('preset')"
          >
            <div class="chat-composer-command-section-label">
              {{ t('chat.commandMenu.presetQuestions') }}
            </div>
            <button
              v-for="item in presetQuestionItems"
              :key="`preset-question:${item.command}`"
              class="chat-composer-command-item chat-composer-command-item--question"
              type="button"
              :title="item.command"
              @click="applyPresetQuestion(item.command)"
            >
              <span class="chat-composer-command-name">{{ item.command }}</span>
            </button>
          </div>
        </div>
        <div
          ref="worldSystemCommandAnchorRef"
          class="messenger-world-tool-anchor"
          :class="{ 'is-open': isWorldCommandPanelVisible('system') }"
          @mouseenter="handleWorldCommandAnchorMouseEnter('system')"
          @mouseleave="handleWorldCommandAnchorMouseLeave('system')"
          @focusin="handleWorldCommandAnchorFocusIn('system')"
          @focusout="handleWorldCommandAnchorFocusOut('system', $event)"
        >
          <button
            class="messenger-world-tool-btn"
            type="button"
            :class="{ active: isWorldCommandPanelVisible('system') }"
            :disabled="stopButtonActive"
            :title="t('chat.commandMenu.commands')"
            :aria-label="t('chat.commandMenu.commands')"
            @click.prevent="toggleWorldCommandPanel('system')"
          >
            <i class="fa-solid fa-terminal chat-composer-command-btn-icon" aria-hidden="true"></i>
          </button>
          <div
            v-if="isWorldCommandPanelVisible('system')"
            class="chat-composer-command-panel"
            @mouseenter="handleWorldCommandPanelMouseEnter('system')"
            @mouseleave="handleWorldCommandPanelMouseLeave('system')"
          >
            <button
              v-for="item in quickCommandItems"
              :key="item.command"
              class="chat-composer-command-item"
              type="button"
              @click="sendQuickCommand(item.command)"
            >
              <span class="chat-composer-command-name">{{ item.command }}</span>
              <span class="chat-composer-command-desc">{{ item.description }}</span>
            </button>
          </div>
        </div>
        <button
          class="messenger-world-tool-btn"
          type="button"
          :class="{
            'messenger-world-tool-btn--recording': voiceRecording,
            'messenger-world-tool-btn--transcribing': voiceTranscribing
          }"
          :title="voiceButtonTitle"
          :aria-label="voiceButtonTitle"
          :disabled="composerBusy > 0 || stopButtonActive || voiceTranscribing"
          @click="handleToggleVoiceRecord"
        >
          <i
            :class="[
              voiceRecording
                ? 'fa-solid fa-stop'
                : voiceTranscribing
                  ? 'fa-solid fa-waveform-lines'
                  : 'fa-solid fa-microphone',
              'messenger-world-tool-fa-icon'
            ]"
            aria-hidden="true"
          ></i>
        </button>
        <div
          v-if="desktopScreenshotSupported"
          ref="screenshotMenuAnchorRef"
          class="messenger-world-tool-anchor chat-screenshot-anchor"
          :class="{ 'is-open': screenshotMenuVisible }"
        >
          <button
            class="messenger-world-tool-btn chat-screenshot-toggle"
            type="button"
            :class="{ active: screenshotMenuVisible }"
            :title="t('chat.attachments.screenshot')"
            :aria-label="t('chat.attachments.screenshot')"
            :aria-expanded="screenshotMenuVisible"
            :disabled="composerBusy > 0 || voiceRecording || stopButtonActive"
            @click.stop.prevent="toggleScreenshotMenu"
          >
            <i class="fa-solid fa-camera messenger-world-tool-fa-icon" aria-hidden="true"></i>
            <i class="fa-solid fa-chevron-down chat-screenshot-caret" aria-hidden="true"></i>
          </button>
        </div>
        <div v-if="voiceRecording" class="messenger-world-voice-indicator">
          <i class="fa-solid fa-circle messenger-world-voice-indicator-dot" aria-hidden="true"></i>
          <span>{{ voiceRecordingLabel }}</span>
        </div>
        <div v-else-if="voiceTranscribing" class="messenger-world-voice-indicator messenger-world-voice-indicator--transcribing">
          <span class="messenger-world-transcribing-rings" aria-hidden="true">
            <span></span>
            <span></span>
            <span></span>
          </span>
          <span>{{ voiceTranscribingLabel }}</span>
        </div>
      </div>
      <textarea
        v-model="inputText"
        ref="inputRef"
        :class="{ 'chat-composer-input--world': worldStyle }"
        :placeholder="inputPlaceholder"
        :readonly="goalLocked"
        rows="1"
        @input="handleInput"
        @click="syncCaretPosition"
        @keyup="syncCaretPosition"
        @keydown="handleInputKeydown"
      />
      <div
        v-if="commandSuggestionsVisible"
        class="command-menu"
        :class="{ 'command-menu--world': worldStyle }"
        role="listbox"
      >
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
      <template v-if="worldStyle">
        <div class="messenger-world-footer chat-composer-world-footer">
          <button
            v-if="composerModelDisplayName && composerModelActionable"
            class="chat-composer-world-model chat-composer-world-model--action"
            type="button"
            :title="composerModelWithContextTooltip"
            :aria-label="composerModelAriaLabel"
            @click="emit('open-model-settings')"
          >
            <span class="chat-composer-world-model-text">{{ composerModelDisplayName }}</span>
            <span
              v-if="composerContextUsageDisplay"
              class="chat-composer-world-context-usage"
              :class="composerContextUsageClass"
              :style="composerContextUsageStyle"
              :title="composerContextUsageTooltip"
              :aria-label="composerContextUsageTooltip"
            >
              {{ composerContextUsageDisplay }}
            </span>
          </button>
          <div
            v-else-if="composerModelDisplayName"
            class="chat-composer-world-model"
            :title="composerModelWithContextTooltip"
            :aria-label="composerModelAriaLabel"
          >
            <span class="chat-composer-world-model-text">{{ composerModelDisplayName }}</span>
            <span
              v-if="composerContextUsageDisplay"
              class="chat-composer-world-context-usage"
              :class="composerContextUsageClass"
              :style="composerContextUsageStyle"
              :title="composerContextUsageTooltip"
              :aria-label="composerContextUsageTooltip"
            >
              {{ composerContextUsageDisplay }}
            </span>
          </div>
          <div
            v-if="showApprovalLabel && approvalLabelText"
            class="chat-composer-approval-label"
            :title="approvalLabelTooltip"
            :aria-label="approvalLabelTooltip"
          >
            {{ approvalLabelText }}
          </div>
          <div class="messenger-world-send-group">
            <label
              v-if="showApprovalModeSelector"
              class="messenger-world-approval-select-wrap"
            >
              <select
                class="messenger-world-approval-select"
                :value="approvalModeValue"
                :title="approvalModeSelectorTitle"
                :aria-label="approvalModeSelectorTitle"
                :disabled="approvalModeSyncing"
                @change="handleApprovalModeChange"
              >
                <option
                  v-for="option in approvalModeOptions"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </option>
              </select>
              <i
                class="fa-solid fa-chevron-down messenger-world-approval-select-caret"
                aria-hidden="true"
              ></i>
            </label>
            <button
              class="messenger-world-send-main"
              type="button"
              :disabled="!canSendOrStop"
              :title="stopButtonActive ? t('common.stop') : t('chat.input.send')"
              :aria-label="stopButtonActive ? t('common.stop') : t('chat.input.send')"
              @keydown="handleSendButtonKeydown"
              @click="handleSendOrStop"
            >
              <i
                v-if="stopButtonActive"
                class="fa-solid fa-stop input-icon chat-composer-world-send-stop-icon"
                aria-hidden="true"
              ></i>
              <svg v-else class="messenger-world-send-icon" aria-hidden="true">
                <use href="#send"></use>
              </svg>
            </button>
          </div>
        </div>
      </template>
      <template v-else>
        <div
          v-if="desktopScreenshotSupported"
          ref="screenshotMenuAnchorRef"
          class="chat-screenshot-anchor"
        >
          <button
            class="input-icon-btn screenshot-btn chat-screenshot-toggle"
            type="button"
            :title="t('chat.attachments.screenshot')"
            :aria-label="t('chat.attachments.screenshot')"
            :aria-expanded="screenshotMenuVisible"
            :class="{ active: screenshotMenuVisible }"
            :disabled="composerBusy > 0 || stopButtonActive"
            @click.stop.prevent="toggleScreenshotMenu"
          >
            <i class="fa-solid fa-camera input-icon" aria-hidden="true"></i>
            <i class="fa-solid fa-chevron-down chat-screenshot-caret" aria-hidden="true"></i>
          </button>
        </div>
        <button
          class="input-icon-btn send-btn"
          type="button"
          :disabled="!canSendOrStop"
          :title="stopButtonActive ? t('common.stop') : t('chat.input.send')"
          :aria-label="stopButtonActive ? t('common.stop') : t('chat.input.send')"
          @keydown="handleSendButtonKeydown"
          @click="handleSendOrStop"
        >
          <i
            v-if="stopButtonActive"
            class="fa-solid fa-stop input-icon input-icon-fill"
            aria-hidden="true"
          ></i>
          <i v-else class="fa-solid fa-paper-plane input-icon input-icon-fill" aria-hidden="true"></i>
        </button>
      </template>
    </div>

    <Teleport to="body">
      <div
        v-if="desktopScreenshotSupported && screenshotMenuVisible"
        ref="screenshotMenuPanelRef"
        class="chat-screenshot-menu chat-screenshot-menu--floating"
        :class="{ 'chat-screenshot-menu--world': worldStyle }"
        :style="screenshotMenuStyle"
      >
        <button
          v-for="option in screenshotCaptureOptions"
          :key="option.key"
          class="chat-screenshot-menu-item"
          type="button"
          @click="selectScreenshotCaptureOption(option)"
        >
          <span class="chat-screenshot-menu-item-main">
            <i :class="[option.icon, 'chat-screenshot-menu-item-icon']" aria-hidden="true"></i>
            <span>{{ option.label }}</span>
          </span>
        </button>
      </div>
    </Teleport>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import ChatGoalComposer from '@/components/chat/ChatGoalComposer.vue';

import { processChatMediaAttachment } from '@/api/chat';
import { uploadWunderWorkspace } from '@/api/workspace';
import {
  clearComposerDraftState,
  readComposerDraftState,
  writeComposerDraftState,
  type ComposerDraftAttachment
} from '@/components/chat/composerDraftCache';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { resolveAnyProviderModelPresetMaxContext } from '@/views/messenger/providerModelPresets';
import { clearWorkspaceDragPaths, hasWorkspaceDragPaths, readWorkspaceDragPaths } from '@/components/chat/workspaceDrag';
import {
  formatContextTokenCount,
  resolveStableComposerContextPair,
  resolveComposerRunningContextDisplayState,
  resolveComposerContextUsageSource
} from '@/components/chat/composerContextUsage';

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
  worldStyle: {
    type: Boolean,
    default: false
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
  showApprovalLabel: {
    type: Boolean,
    default: false
  },
  approvalLabel: {
    type: String,
    default: ''
  },
  approvalMode: {
    type: String,
    default: ''
  },
  approvalModeOptions: {
    type: Array,
    default: () => []
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
  modelJumpEnabled: {
    type: Boolean,
    default: false
  },
  modelJumpHint: {
    type: String,
    default: ''
  },
  contextUsedTokens: {
    type: [Number, String],
    default: null
  },
  contextTotalTokens: {
    type: [Number, String],
    default: null
  },
  contextMessages: {
    type: Array,
    default: () => []
  },
  goalLocked: {
    type: Boolean,
    default: false
  },
  goalEditorVisible: {
    type: Boolean,
    default: false
  },
  goalObjective: {
    type: String,
    default: ''
  },
  goalLoading: {
    type: Boolean,
    default: false
  },
  goalSubmitting: {
    type: Boolean,
    default: false
  },
  goalActive: {
    type: Boolean,
    default: false
  },
  goalStatus: {
    type: String,
    default: ''
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
  }
});

const emit = defineEmits([
  'send',
  'stop',
  'toggle-voice-record',
  'open-model-settings',
  'update:approval-mode',
  'update:goal-objective',
  'submit-goal',
  'cancel-goal-editor'
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
type WorldCommandPanelType = 'preset' | 'system';

const worldPresetCommandAnchorRef = ref<HTMLElement | null>(null);
const worldSystemCommandAnchorRef = ref<HTMLElement | null>(null);
const worldCommandPanelVisible = ref<WorldCommandPanelType | ''>('');
const worldCommandAnchorHovered = ref<WorldCommandPanelType | ''>('');
const worldCommandPanelHovered = ref<WorldCommandPanelType | ''>('');
const screenshotMenuAnchorRef = ref<HTMLElement | null>(null);
const screenshotMenuPanelRef = ref<HTMLElement | null>(null);
const screenshotMenuVisible = ref(false);
const screenshotMenuStyle = ref<Record<string, string>>({});
const caretPosition = ref(0);
const commandMenuIndex = ref(0);
const commandMenuDismissed = ref(false);
const expandedVideoAttachmentId = ref('');
const videoFrameRateDrafts = ref<Record<string, string>>({});
const gifFrameStepDrafts = ref<Record<string, string>>({});
const attachmentProcessingIds = ref<string[]>([]);
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
let worldCommandPanelCloseTimer: ReturnType<typeof setTimeout> | null = null;
const { t } = useI18n();
const chatStore = useChatStore();

const IMAGE_MIME_TYPES = new Set([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/bmp',
  'image/webp'
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

type DesktopScreenshotResult = {
  ok?: boolean;
  canceled?: boolean;
  name?: string;
  path?: string;
  mimeType?: string;
  dataUrl?: string;
  message?: string;
};

type DesktopScreenshotBridge = {
  captureScreenshot?: (
    options?: { hideWindow?: boolean; region?: boolean }
  ) => Promise<DesktopScreenshotResult | null> | DesktopScreenshotResult | null;
};

type ScreenshotCaptureOption = {
  key: string;
  hideWindow: boolean;
  region: boolean;
  icon: string;
  label: string;
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
};

type SendKeyMode = 'enter' | 'ctrl_enter' | 'none';

type SlashCommandDefinition = {
  command: string;
  aliases: string[];
  descriptionKey: string;
};
const INPUT_MAX_HEIGHT = 180;
const WORLD_COMPOSER_HEIGHT_STORAGE_KEY = 'wunder_world_composer_height';
const WORLD_COMMAND_PANEL_CLOSE_DELAY_MS = 160;
const MAX_WORKSPACE_UPLOAD_BYTES = 200 * 1024 * 1024;
const resolveDraftKey = (): string => String(props.draftKey || '').trim();
const clampWorldComposerHeight = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 188;
  return Math.min(340, Math.max(168, Math.round(parsed)));
};
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
const normalizeTokenCount = (value: unknown): number | null => {
  if (value === null || value === undefined) {
    return null;
  }
  const normalizedValue = typeof value === 'string' ? value.trim() : value;
  if (normalizedValue === '') {
    return null;
  }
  const parsed = Number(normalizedValue);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return null;
  }
  return Math.round(parsed);
};
const normalizePositiveTokenCount = (value: unknown): number | null => {
  const normalized = normalizeTokenCount(value);
  if (normalized === null || normalized <= 0) {
    return null;
  }
  return normalized;
};
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
const composerContextUsageSource = computed(() =>
  resolveComposerContextUsageSource(
    Array.isArray(props.contextMessages) ? props.contextMessages : [],
    resolveCurrentSession(),
    Boolean(props.loading)
  )
);

const composerBusy = computed(() => attachmentBusy.value + workspaceDropBusy.value);
const showUploadArea = computed(() => attachments.value.length > 0 || attachmentBusy.value > 0);
const chatBusyMessage = computed(() =>
  workspaceDropBusy.value > 0 ? t('chat.workspaceDrop.uploading') : t('chat.attachments.busy')
);
const composerModelName = computed(() => String(props.modelName || '').trim());
const composerModelJumpHint = computed(() => String(props.modelJumpHint || '').trim());
const composerModelMissing = computed(() => {
  const name = composerModelName.value;
  if (!name) return true;
  return name === t('desktop.system.modelUnnamed');
});
const composerModelDisplayName = computed(() => {
  if (composerModelMissing.value && props.modelJumpEnabled) {
    if (composerModelJumpHint.value) {
      return composerModelJumpHint.value;
    }
    return t('desktop.system.modelSetupHint');
  }
  return composerModelName.value;
});
const composerModelActionable = computed(
  () => Boolean(props.modelJumpEnabled && composerModelDisplayName.value)
);
const composerModelAriaLabel = computed(() => {
  const label = composerModelDisplayName.value || composerModelName.value;
  if (!label) return '';
  if (composerModelMissing.value && props.modelJumpEnabled && composerModelJumpHint.value) {
    return composerModelJumpHint.value;
  }
  return `${t('desktop.system.modelName')}: ${label}`;
});
const composerContextUsedTokensRaw = computed(() => {
  const fromProps = normalizePositiveTokenCount(props.contextUsedTokens);
  if (fromProps !== null) {
    return fromProps;
  }
  return composerContextUsageSource.value.contextTokens;
});
const composerContextTotalTokensRaw = computed(() => {
  const fromProps = normalizeTokenCount(props.contextTotalTokens);
  if (fromProps !== null && fromProps > 0) {
    return fromProps;
  }
  const fromSource = composerContextUsageSource.value.contextTotalTokens;
  if (fromSource !== null && fromSource > 0) {
    return fromSource;
  }
  const fromPreset = resolveAnyProviderModelPresetMaxContext(composerModelName.value);
  const normalizedPreset = normalizeTokenCount(fromPreset);
  return normalizedPreset !== null && normalizedPreset > 0 ? normalizedPreset : null;
});
const contextDisplayAssistantSignature = computed(() => {
  return composerContextUsageSource.value.assistantSignature;
});
const contextDisplayResetSignature = computed(() => {
  return composerContextUsageSource.value.contextResetSignature;
});
const contextDisplaySessionId = computed(() => String(chatStore.activeSessionId || '').trim());
const lastContextDisplaySessionId = ref<string>(contextDisplaySessionId.value);
const lastContextDisplayAssistantSignature = ref<string>(contextDisplayAssistantSignature.value);
const lastContextDisplayResetSignature = ref<string>(contextDisplayResetSignature.value);
const composerContextUsedTokensStable = ref<number | null>(null);
const composerContextTotalTokensStable = ref<number | null>(null);
const composerContextAssistantBaseTokens = ref<number | null>(null);
const composerContextAssistantRawBaseTokens = ref<number | null>(null);
const composerContextAssistantLastRawTokens = ref<number | null>(null);
watch(
  [
    contextDisplaySessionId,
    contextDisplayAssistantSignature,
    contextDisplayResetSignature,
    () => Boolean(props.loading),
    composerContextUsedTokensRaw,
    composerContextTotalTokensRaw
  ],
  ([sessionId, assistantSignature, resetSignature, loading, rawUsed, rawTotal]) => {
    const switchedSession = sessionId !== lastContextDisplaySessionId.value;
    const switchedAssistant =
      assistantSignature !== lastContextDisplayAssistantSignature.value;
    const switchedContextReset =
      Boolean(resetSignature) && resetSignature !== lastContextDisplayResetSignature.value;
    lastContextDisplaySessionId.value = sessionId;
    lastContextDisplayAssistantSignature.value = assistantSignature;
    lastContextDisplayResetSignature.value = resetSignature;
    if (switchedSession || switchedContextReset || !loading) {
      const nextPair = resolveStableComposerContextPair(rawUsed, rawTotal);
      composerContextUsedTokensStable.value = nextPair.used;
      composerContextTotalTokensStable.value = nextPair.total;
      composerContextAssistantBaseTokens.value = null;
      composerContextAssistantRawBaseTokens.value = null;
      composerContextAssistantLastRawTokens.value = null;
      return;
    }
    if (switchedAssistant) {
      const currentUsed = composerContextUsedTokensStable.value;
      const currentTotal = composerContextTotalTokensStable.value;
      const runningRaw = normalizePositiveTokenCount(
        composerContextUsageSource.value.runningContextTokens
      );
      composerContextAssistantBaseTokens.value = currentUsed;
      composerContextAssistantRawBaseTokens.value = runningRaw;
      composerContextAssistantLastRawTokens.value = runningRaw;
      const nextUsed =
        rawUsed === null ? currentUsed : currentUsed === null ? rawUsed : Math.max(currentUsed, rawUsed);
      const nextTotal =
        rawTotal === null ? currentTotal : currentTotal === null ? rawTotal : Math.max(currentTotal, rawTotal);
      const nextPair = resolveStableComposerContextPair(nextUsed, nextTotal);
      composerContextUsedTokensStable.value = nextPair.used;
      composerContextTotalTokensStable.value = nextPair.total;
      return;
    }
    if (rawUsed !== null) {
      const runningRaw = normalizePositiveTokenCount(
        composerContextUsageSource.value.runningContextTokens
      );
      const current = composerContextUsedTokensStable.value;
      if (
        composerContextUsageSource.value.runningAssistant &&
        runningRaw !== null
      ) {
        const next = resolveComposerRunningContextDisplayState({
          stableTokens: current,
          baseTokens: composerContextAssistantBaseTokens.value,
          rawBaseTokens: composerContextAssistantRawBaseTokens.value,
          lastRawTokens: composerContextAssistantLastRawTokens.value,
          runningRawTokens: runningRaw
        });
        composerContextAssistantBaseTokens.value = next.baseTokens;
        composerContextAssistantRawBaseTokens.value = next.rawBaseTokens;
        composerContextAssistantLastRawTokens.value = next.lastRawTokens;
        const nextPair = resolveStableComposerContextPair(
          next.stableTokens,
          composerContextTotalTokensStable.value
        );
        composerContextUsedTokensStable.value = nextPair.used;
        composerContextTotalTokensStable.value = nextPair.total;
        return;
      }
      const nextUsed = current === null ? rawUsed : Math.max(current, rawUsed);
      const nextPair = resolveStableComposerContextPair(
        nextUsed,
        composerContextTotalTokensStable.value
      );
      composerContextUsedTokensStable.value = nextPair.used;
      composerContextTotalTokensStable.value = nextPair.total;
    }
    if (rawTotal !== null) {
      const current = composerContextTotalTokensStable.value;
      const nextTotal = current === null ? rawTotal : Math.max(current, rawTotal);
      const nextPair = resolveStableComposerContextPair(
        composerContextUsedTokensStable.value,
        nextTotal
      );
      composerContextUsedTokensStable.value = nextPair.used;
      composerContextTotalTokensStable.value = nextPair.total;
    }
  },
  { immediate: true }
);
const composerContextUsedTokens = computed(() => composerContextUsedTokensStable.value);
const composerContextTotalTokens = computed(() => composerContextTotalTokensStable.value);
const CONTEXT_WARNING_RATIO = 0.7;
const CONTEXT_DANGER_RATIO = 0.9;
const composerContextUsageRatio = computed(() => {
  const used = composerContextUsedTokens.value;
  const total = composerContextTotalTokens.value;
  if (used === null || total === null || total <= 0) return null;
  return Math.max(0, used) / total;
});
const composerContextUsageClass = computed(() => {
  const ratio = composerContextUsageRatio.value;
  if (ratio === null) return '';
  if (ratio >= CONTEXT_DANGER_RATIO) return 'is-danger';
  if (ratio >= CONTEXT_WARNING_RATIO) return 'is-warning';
  return '';
});
const composerContextUsageStyle = computed<Record<string, string>>(() => {
  const ratio = composerContextUsageRatio.value;
  if (ratio === null || ratio < CONTEXT_WARNING_RATIO) {
    return {};
  }
  // Lerp from amber to red as context usage approaches the hard limit.
  const ratioSpan = Math.max(0.01, CONTEXT_DANGER_RATIO - CONTEXT_WARNING_RATIO);
  const progress = Math.min(Math.max((ratio - CONTEXT_WARNING_RATIO) / ratioSpan, 0), 1);
  const red = Math.round(217 + (220 - 217) * progress);
  const green = Math.round(119 + (38 - 119) * progress);
  const blue = Math.round(6 + (38 - 6) * progress);
  return { color: `rgb(${red}, ${green}, ${blue})` };
});
const composerContextUsageDisplay = computed(() => {
  if (!props.worldStyle) return '';
  const used = composerContextUsedTokens.value;
  const total = composerContextTotalTokens.value;
  if (used === null && total === null) return '';
  const usedText = formatContextTokenCount(used);
  const totalText = formatContextTokenCount(total);
  return `${usedText}/${totalText}`;
});
const composerContextUsageTooltip = computed(() => {
  const usage = composerContextUsageDisplay.value;
  if (!usage) return '';
  const ratio = composerContextUsageRatio.value;
  if (ratio !== null) {
    const percent = Math.min(999, Math.round(ratio * 100));
    return `${t('profile.stats.contextTokens')}: ${usage} (${percent}%)`;
  }
  return `${t('profile.stats.contextTokens')}: ${usage}`;
});
const composerModelWithContextTooltip = computed(() => {
  if (!composerContextUsageDisplay.value) {
    return composerModelDisplayName.value;
  }
  return `${composerModelDisplayName.value} | ${composerContextUsageDisplay.value}`;
});
const getDesktopScreenshotBridge = (): DesktopScreenshotBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopScreenshotBridge }).wunderDesktop;
  if (!candidate || typeof candidate.captureScreenshot !== 'function') {
    return null;
  }
  return candidate;
};
const desktopScreenshotSupported = computed(() => Boolean(getDesktopScreenshotBridge()));
const screenshotCaptureOptions = computed<ScreenshotCaptureOption[]>(() => [
  {
    key: 'full-keep',
    hideWindow: false,
    region: false,
    icon: 'fa-solid fa-expand',
    label: t('chat.attachments.screenshotOption.fullKeep')
  },
  {
    key: 'full-hide',
    hideWindow: true,
    region: false,
    icon: 'fa-solid fa-expand',
    label: t('chat.attachments.screenshotOption.fullHide')
  },
  {
    key: 'region-keep',
    hideWindow: false,
    region: true,
    icon: 'fa-solid fa-crop-simple',
    label: t('chat.attachments.screenshotOption.regionKeep')
  },
  {
    key: 'region-hide',
    hideWindow: true,
    region: true,
    icon: 'fa-solid fa-crop-simple',
    label: t('chat.attachments.screenshotOption.regionHide')
  }
]);
const SCREENSHOT_MENU_EDGE_MARGIN = 10;
const SCREENSHOT_MENU_GAP = 10;
const SCREENSHOT_MENU_MIN_WIDTH = 210;

const clampMenuPosition = (value: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, value));

const updateScreenshotMenuPosition = () => {
  if (!screenshotMenuVisible.value || typeof window === 'undefined') return;
  const anchor = screenshotMenuAnchorRef.value;
  if (!anchor) return;
  const rect = anchor.getBoundingClientRect();
  const panel = screenshotMenuPanelRef.value;
  const panelWidth = Math.max(
    SCREENSHOT_MENU_MIN_WIDTH,
    Number(panel?.offsetWidth || SCREENSHOT_MENU_MIN_WIDTH)
  );
  const panelHeight = Math.max(96, Number(panel?.offsetHeight || 160));
  const viewportWidth = Math.max(1, window.innerWidth);
  const viewportHeight = Math.max(1, window.innerHeight);
  const maxLeft = Math.max(SCREENSHOT_MENU_EDGE_MARGIN, viewportWidth - panelWidth - SCREENSHOT_MENU_EDGE_MARGIN);
  let left = clampMenuPosition(rect.right - panelWidth, SCREENSHOT_MENU_EDGE_MARGIN, maxLeft);
  let top = rect.top - panelHeight - SCREENSHOT_MENU_GAP;
  if (top < SCREENSHOT_MENU_EDGE_MARGIN) {
    top = rect.bottom + SCREENSHOT_MENU_GAP;
  }
  const maxTop = Math.max(SCREENSHOT_MENU_EDGE_MARGIN, viewportHeight - panelHeight - SCREENSHOT_MENU_EDGE_MARGIN);
  top = clampMenuPosition(top, SCREENSHOT_MENU_EDGE_MARGIN, maxTop);
  if (viewportWidth <= panelWidth + SCREENSHOT_MENU_EDGE_MARGIN * 2) {
    left = SCREENSHOT_MENU_EDGE_MARGIN;
  }
  screenshotMenuStyle.value = {
    left: `${Math.round(left)}px`,
    top: `${Math.round(top)}px`,
    minWidth: `${SCREENSHOT_MENU_MIN_WIDTH}px`
  };
};

const handleScreenshotMenuViewportChange = () => {
  updateScreenshotMenuPosition();
};

const syncScreenshotMenuPosition = async () => {
  await nextTick();
  updateScreenshotMenuPosition();
};
const hasInquirySelection = computed(
  () => Array.isArray(props.inquirySelection) && props.inquirySelection.length > 0
);
const sendShortcutHint = computed(() => {
  if (props.sendKey === 'ctrl_enter') return t('chat.input.sendHintCtrlEnterAlt');
  if (props.sendKey === 'enter') return t('chat.input.sendHintEnterAlt');
  return '';
});
const goalEditorVisible = computed(() => Boolean(props.goalEditorVisible));
const inputPlaceholder = computed(() => {
  const base = props.inquiryActive
    ? t('chat.input.inquiryPlaceholder')
    : props.worldStyle
      ? t('chat.input.placeholder')
      : t('chat.input.placeholderCommands');
  return sendShortcutHint.value ? `${base} | ${sendShortcutHint.value}` : base;
});
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
const approvalLabelText = computed(() => String(props.approvalLabel || '').trim());
const approvalModeOptions = computed<ApprovalModeOption[]>(() => {
  const source = Array.isArray(props.approvalModeOptions) ? props.approvalModeOptions : [];
  return source
    .map((item) => {
      const record =
        item && typeof item === 'object' && !Array.isArray(item)
          ? (item as Record<string, unknown>)
          : {};
      const value = String(record.value || '').trim();
      const label = String(record.label || value).trim();
      if (!value || !label) return null;
      return { value, label };
    })
    .filter((item): item is ApprovalModeOption => Boolean(item));
});
const approvalModeValue = computed(() => {
  const candidate = String(props.approvalMode || '').trim();
  if (!candidate) {
    return approvalModeOptions.value[0]?.value || '';
  }
  const matched = approvalModeOptions.value.find((item) => item.value === candidate);
  return matched?.value || approvalModeOptions.value[0]?.value || '';
});
const showApprovalModeSelector = computed(
  () => props.worldStyle && props.approvalModeEditable && approvalModeOptions.value.length > 0
);
const approvalModeSyncing = computed(() => props.approvalModeSyncing);
const approvalLabelTooltip = computed(() => {
  if (!approvalLabelText.value) return '';
  const hint = t('portal.agent.permission.tooltip');
  if (!hint) return approvalLabelText.value;
  return `${approvalLabelText.value} · ${hint}`;
});
const approvalModeSelectorTitle = computed(
  () => approvalLabelTooltip.value || t('portal.agent.permission.tooltip')
);
const showApprovalLabel = computed(
  () =>
    props.worldStyle &&
    props.showApprovalLabel &&
    Boolean(approvalLabelText.value) &&
    !showApprovalModeSelector.value
);
const voiceSupported = computed(() => props.worldStyle && props.voiceSupported);
const voiceRecording = computed(() => props.worldStyle && props.voiceRecording);
const voiceTranscribing = computed(() => props.worldStyle && props.voiceTranscribing);
const stopButtonActive = computed(() => Boolean(props.loading || props.goalLocked));
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
  () => !props.worldStyle && !commandMenuDismissed.value && commandSuggestions.value.length > 0
);

const quickCommandItems = computed(() =>
  slashCommandDefinitions.filter((item) => !props.goalLocked || item.command === '/stop').map((item) => ({
    command: item.command,
    description: t(item.descriptionKey)
  }))
);
const presetQuestionItems = computed(() =>
  normalizeAgentPresetQuestions(props.presetQuestions).map((question) => ({
    command: question
  }))
);
const worldComposerHeight = ref(188);
const worldComposerStyle = computed<Record<string, string>>(() => ({
  '--chat-composer-world-height': `${worldComposerHeight.value}px`
}));

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
    default:
      return '';
  }
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
  if (props.worldStyle) {
    el.style.height = '';
    el.style.overflowY = 'auto';
    return;
  }
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
  if (props.goalLocked || goalEditorVisible.value) {
    inputText.value = '';
    return;
  }
  if (worldCommandPanelVisible.value) {
    closeWorldCommandPanel();
  }
  commandMenuDismissed.value = false;
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
  persistDraftState();
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
  if (goalEditorVisible.value) {
    return;
  }
  if (isEnterKeyboardEvent(event)) {
    await handleEnterKeydown(event);
    return;
  }
  if (event.key === 'Escape' && screenshotMenuVisible.value) {
    event.preventDefault();
    closeScreenshotMenu();
    return;
  }
  if (props.worldStyle) {
    return;
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
  if (props.goalLocked || goalEditorVisible.value) {
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
  if (props.worldStyle) {
    el.style.height = '';
    el.style.overflowY = 'auto';
    return;
  }
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
  formData.append('container_id', String(containerId));
  fileList.forEach((file, index) => {
    formData.append('files', file, file.name || 'upload');
    formData.append('relative_paths', normalizeWorkspacePath(items[index]?.relativePath || file.name || 'upload'));
  });
  const response = await uploadWunderWorkspace(formData);
  const uploadedPaths = resolveUploadedWorkspacePaths(response?.data);
  const refreshDetail = {
    reason: 'composer-drop-upload',
    containerId,
    container_id: containerId,
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

const handleDragEnter = (event) => {
  if (stopButtonActive.value || goalEditorVisible.value) return;
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
  if (stopButtonActive.value || goalEditorVisible.value) return;
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
  if (stopButtonActive.value || goalEditorVisible.value) return;
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
    closeScreenshotMenu();
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
  closeScreenshotMenu();
  workspaceDropBusy.value += 1;
  try {
    const uploadedPaths = await uploadDroppedFilesToWorkspace(droppedItems);
    appendWorkspaceFileNotice(uploadedPaths, droppedItems);
    ElMessage.success(t('chat.workspaceDrop.uploaded', { count: uploadedPaths.length || droppedItems.length }));
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.workspaceDrop.failed')));
  } finally {
    workspaceDropBusy.value = Math.max(0, workspaceDropBusy.value - 1);
  }
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

const closeScreenshotMenu = () => {
  screenshotMenuVisible.value = false;
  screenshotMenuStyle.value = {};
};

const toggleScreenshotMenu = () => {
  if (stopButtonActive.value) return;
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  closeWorldCommandPanel();
  const nextVisible = !screenshotMenuVisible.value;
  screenshotMenuVisible.value = nextVisible;
  if (nextVisible) {
    void syncScreenshotMenuPosition();
  }
};

const appendFileNameSuffix = (fileName: string, suffix: string): string => {
  const normalized = String(fileName || '').trim();
  if (!normalized) return `screenshot${suffix}.png`;
  const dotIndex = normalized.lastIndexOf('.');
  if (dotIndex <= 0) return `${normalized}${suffix}`;
  return `${normalized.slice(0, dotIndex)}${suffix}${normalized.slice(dotIndex)}`;
};

const captureDesktopScreenshotAttachment = async (option: ScreenshotCaptureOption) => {
  if (stopButtonActive.value) return;
  closeScreenshotMenu();
  const bridge = getDesktopScreenshotBridge();
  if (!bridge || typeof bridge.captureScreenshot !== 'function') {
    ElMessage.warning(t('chat.attachments.screenshotUnavailable'));
    return;
  }
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  attachmentBusy.value += 1;
  try {
    const result = await bridge.captureScreenshot({
      hideWindow: option.hideWindow,
      region: option.region
    });
    if (result?.canceled) {
      return;
    }
    if (!result || result.ok === false) {
      throw new Error(
        String(result?.message || t('chat.attachments.screenshotFailed')).trim() ||
          t('chat.attachments.screenshotFailed')
      );
    }
    let dataUrl = String(result.dataUrl || '').trim();
    if (!dataUrl || !dataUrl.startsWith('data:image/')) {
      throw new Error(t('chat.attachments.screenshotFailed'));
    }
    let name = String(result.name || '').trim() || `screenshot-${Date.now()}.png`;
    if (option.region && !/[-_]region(\.[^./]+)?$/i.test(name)) {
      name = appendFileNameSuffix(name, '-region');
    }
    const mimeType = normalizeImageMimeType(String(result.mimeType || '').trim() || 'image/png') || 'image/png';
    if (!IMAGE_MIME_TYPES.has(mimeType)) {
      throw new Error(t('chat.attachments.imageInvalid'));
    }
    const byteString = atob(dataUrl.split(',', 2)[1] || '');
    const bytes = new Uint8Array(byteString.length);
    for (let index = 0; index < byteString.length; index += 1) {
      bytes[index] = byteString.charCodeAt(index);
    }
    const screenshotFile = new File([bytes], name, { type: mimeType });
    await validateImageFile(screenshotFile);
    const attachment = await processImageFile(screenshotFile);
    attachment.mime_type = mimeType;
    pushAttachment(attachment);
    ElMessage.success(t('chat.attachments.screenshotAdded', { name }));
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.attachments.screenshotFailed')));
  } finally {
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
};

const selectScreenshotCaptureOption = async (option: ScreenshotCaptureOption) => {
  await captureDesktopScreenshotAttachment(option);
};

const syncWorldComposerHeight = () => {
  if (!props.worldStyle || typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(
      WORLD_COMPOSER_HEIGHT_STORAGE_KEY,
      String(clampWorldComposerHeight(worldComposerHeight.value))
    );
  } catch {
    // ignore localStorage errors
  }
};

const stopWorldComposerResize = () => {
  worldComposerResizeRuntime = null;
  if (typeof window === 'undefined') return;
  window.removeEventListener('mousemove', handleWorldComposerResizeMove);
  window.removeEventListener('mouseup', stopWorldComposerResize);
};

const handleWorldComposerResizeMove = (event: MouseEvent) => {
  if (!props.worldStyle || !worldComposerResizeRuntime) return;
  const delta = worldComposerResizeRuntime.startY - event.clientY;
  worldComposerHeight.value = clampWorldComposerHeight(worldComposerResizeRuntime.startHeight + delta);
};

const startWorldComposerResize = (event: MouseEvent) => {
  if (!props.worldStyle || typeof window === 'undefined') return;
  worldComposerResizeRuntime = {
    startY: event.clientY,
    startHeight: worldComposerHeight.value
  };
  window.addEventListener('mousemove', handleWorldComposerResizeMove);
  window.addEventListener('mouseup', stopWorldComposerResize);
};

const clearWorldCommandPanelCloseTimer = () => {
  if (worldCommandPanelCloseTimer) {
    clearTimeout(worldCommandPanelCloseTimer);
    worldCommandPanelCloseTimer = null;
  }
};

const isWorldCommandPanelVisible = (panel: WorldCommandPanelType): boolean =>
  worldCommandPanelVisible.value === panel;

const scheduleWorldCommandPanelClose = (panel: WorldCommandPanelType) => {
  clearWorldCommandPanelCloseTimer();
  worldCommandPanelCloseTimer = setTimeout(() => {
    worldCommandPanelCloseTimer = null;
    if (worldCommandAnchorHovered.value === panel || worldCommandPanelHovered.value === panel) {
      return;
    }
    if (worldCommandPanelVisible.value === panel) {
      worldCommandPanelVisible.value = '';
    }
  }, WORLD_COMMAND_PANEL_CLOSE_DELAY_MS);
};

const openWorldCommandPanel = (panel: WorldCommandPanelType) => {
  if (stopButtonActive.value) return;
  closeScreenshotMenu();
  clearWorldCommandPanelCloseTimer();
  worldCommandPanelVisible.value = panel;
};

const closeWorldCommandPanel = () => {
  clearWorldCommandPanelCloseTimer();
  worldCommandAnchorHovered.value = '';
  worldCommandPanelHovered.value = '';
  worldCommandPanelVisible.value = '';
};

const toggleWorldCommandPanel = (panel: WorldCommandPanelType) => {
  if (isWorldCommandPanelVisible(panel)) {
    closeWorldCommandPanel();
    return;
  }
  openWorldCommandPanel(panel);
};

const handleWorldCommandAnchorMouseEnter = (panel: WorldCommandPanelType) => {
  worldCommandAnchorHovered.value = panel;
  openWorldCommandPanel(panel);
};

const handleWorldCommandAnchorMouseLeave = (panel: WorldCommandPanelType) => {
  if (worldCommandAnchorHovered.value === panel) {
    worldCommandAnchorHovered.value = '';
  }
  scheduleWorldCommandPanelClose(panel);
};

const handleWorldCommandPanelMouseEnter = (panel: WorldCommandPanelType) => {
  worldCommandPanelHovered.value = panel;
  openWorldCommandPanel(panel);
};

const handleWorldCommandPanelMouseLeave = (panel: WorldCommandPanelType) => {
  if (worldCommandPanelHovered.value === panel) {
    worldCommandPanelHovered.value = '';
  }
  scheduleWorldCommandPanelClose(panel);
};

const handleWorldCommandAnchorFocusIn = (panel: WorldCommandPanelType) => {
  worldCommandAnchorHovered.value = panel;
  openWorldCommandPanel(panel);
};

const resolveWorldCommandAnchor = (panel: WorldCommandPanelType): HTMLElement | null =>
  panel === 'preset' ? worldPresetCommandAnchorRef.value : worldSystemCommandAnchorRef.value;

const handleWorldCommandAnchorFocusOut = (panel: WorldCommandPanelType, event: FocusEvent) => {
  const anchor = resolveWorldCommandAnchor(panel);
  const nextTarget = event.relatedTarget as Node | null;
  if (anchor && nextTarget && anchor.contains(nextTarget)) {
    return;
  }
  if (worldCommandAnchorHovered.value === panel) {
    worldCommandAnchorHovered.value = '';
  }
  scheduleWorldCommandPanelClose(panel);
};

const handleApprovalModeChange = (event: Event) => {
  const target = event.target as HTMLSelectElement | null;
  const value = String(target?.value || '').trim();
  if (!value) return;
  emit('update:approval-mode', value);
};

const sendQuickCommand = async (command: string) => {
  if (goalEditorVisible.value) return;
  closeWorldCommandPanel();
  closeScreenshotMenu();
  if (!command) return;
  if (stopButtonActive.value) {
    if (command === '/stop') {
      emit('stop');
    }
    return;
  }
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  emit('send', { content: command, attachments: [] });
  inputText.value = '';
  commandMenuDismissed.value = false;
  caretPosition.value = 0;
  resetInputHeight();
  clearAttachments();
};

const applyPresetQuestion = (question: string) => {
  if (goalEditorVisible.value) return;
  closeWorldCommandPanel();
  closeScreenshotMenu();
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
  if (stopButtonActive.value || goalEditorVisible.value) return;
  if (voiceRecording.value) return;
  closeScreenshotMenu();
  if (commandSuggestionsVisible.value && applyCommandSuggestion()) {
    return;
  }
  // Prevent sending while generated attachments or workspace drop uploads are still settling.
  if (composerBusy.value > 0) {
    ElMessage.warning(chatBusyMessage.value);
    return;
  }
  const content = inputText.value.trim();
  const payloadAttachments = buildAttachmentPayload();
  if (!content && payloadAttachments.length === 0 && !hasInquirySelection.value) return;
  chatDebugLog('messenger.send', 'composer-send-emit', {
    activeSessionId: String(chatStore.activeSessionId || '').trim(),
    messageCount: Array.isArray(props.contextMessages) ? props.contextMessages.length : 0,
    contentLength: content.length,
    attachmentCount: payloadAttachments.length,
    hasInquirySelection: hasInquirySelection.value
  });
  emit('send', { content, attachments: payloadAttachments });
  inputText.value = '';
  commandMenuDismissed.value = false;
  caretPosition.value = 0;
  resetInputHeight();
  clearAttachments();
  focusComposerInputAtEnd();
};

const handleSendOrStop = async () => {
  if (goalEditorVisible.value) {
    emit('submit-goal');
    return;
  }
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
  closeScreenshotMenu();
  closeWorldCommandPanel();
  emit('toggle-voice-record');
};

const handleDocumentPointerDown = (event: PointerEvent) => {
  const target = event.target as Node | null;
  if (worldCommandPanelVisible.value) {
    const presetAnchor = worldPresetCommandAnchorRef.value;
    const systemAnchor = worldSystemCommandAnchorRef.value;
    const isInsidePreset = Boolean(presetAnchor && target && presetAnchor.contains(target));
    const isInsideSystem = Boolean(systemAnchor && target && systemAnchor.contains(target));
    if (!isInsidePreset && !isInsideSystem) {
      closeWorldCommandPanel();
    }
  }
  if (screenshotMenuVisible.value) {
    const screenshotAnchor = screenshotMenuAnchorRef.value;
    const screenshotPanel = screenshotMenuPanelRef.value;
    const isInsideAnchor = Boolean(screenshotAnchor && target && screenshotAnchor.contains(target));
    const isInsidePanel = Boolean(screenshotPanel && target && screenshotPanel.contains(target));
    if (!isInsideAnchor && !isInsidePanel) {
      closeScreenshotMenu();
    }
  }
};

onMounted(async () => {
  if (props.worldStyle && typeof window !== 'undefined') {
    worldComposerHeight.value = clampWorldComposerHeight(
      window.localStorage.getItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY)
    );
  }
  await nextTick();
  if (typeof document !== 'undefined') {
    document.addEventListener('pointerdown', handleDocumentPointerDown);
  }
});

onBeforeUnmount(() => {
  stopWorldComposerResize();
  clearWorldCommandPanelCloseTimer();
  if (typeof window !== 'undefined') {
    window.removeEventListener('resize', handleScreenshotMenuViewportChange);
    window.removeEventListener('scroll', handleScreenshotMenuViewportChange, true);
  }
  if (typeof document !== 'undefined') {
    document.removeEventListener('pointerdown', handleDocumentPointerDown);
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

watch(
  () => desktopScreenshotSupported.value,
  (supported) => {
    if (!supported) {
      closeScreenshotMenu();
    }
  }
);

watch(
  () => props.voiceRecording,
  (recording) => {
    if (recording) {
      closeScreenshotMenu();
    }
  }
);

watch(
  () => screenshotMenuVisible.value,
  (visible) => {
    if (typeof window === 'undefined') return;
    if (visible) {
      void syncScreenshotMenuPosition();
      window.addEventListener('resize', handleScreenshotMenuViewportChange);
      window.addEventListener('scroll', handleScreenshotMenuViewportChange, true);
      return;
    }
    window.removeEventListener('resize', handleScreenshotMenuViewportChange);
    window.removeEventListener('scroll', handleScreenshotMenuViewportChange, true);
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
  () => props.draftKey,
  (value, previousValue) => {
    persistDraftStateByKey(String(previousValue || ''));
    restoreDraftStateByKey(String(value || ''));
  },
  { immediate: true }
);

watch(
  () => inputText.value,
  () => {
    persistDraftState();
  }
);

watch(
  () => attachments.value,
  () => {
    syncVideoAttachmentDrafts();
    persistDraftState();
  },
  { deep: true }
);

watch(
  () => worldComposerHeight.value,
  () => {
    syncWorldComposerHeight();
  }
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

watch(
  () => Boolean(props.goalLocked),
  (locked) => {
    if (!locked) return;
    inputText.value = '';
    clearAttachments();
    closeScreenshotMenu();
    closeWorldCommandPanel();
    commandMenuDismissed.value = false;
    caretPosition.value = 0;
    void nextTick(() => {
      resizeInput();
      syncCaretPosition();
    });
  }
);

watch(
  () => goalEditorVisible.value,
  (visible) => {
    if (!visible) return;
    inputText.value = '';
    clearAttachments();
    closeScreenshotMenu();
    closeWorldCommandPanel();
    commandMenuDismissed.value = false;
    caretPosition.value = 0;
  }
);

defineExpose({
  appendTextToComposer,
  focusComposerInputAtEnd
});
</script>

<style scoped>
.chat-goal-context-usage {
  display: inline-flex;
  align-items: center;
  align-self: flex-end;
  min-height: 20px;
  margin: 2px 4px 0 0;
  font-size: 12px;
  font-weight: 600;
}
</style>
