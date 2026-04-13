<template>
  <div class="input-container" :class="{ 'input-container--world': worldStyle }">
    <div v-if="showUploadArea" class="upload-preview">
      <div class="upload-preview-list">
        <div
          v-for="attachment in attachments"
          :key="attachment.id"
          class="upload-preview-item"
          :class="{
            'upload-preview-item--video': attachment.type === 'video',
            'is-active': attachment.type === 'video' && isVideoControlOpen(attachment.id),
            'is-processing': isAttachmentProcessing(attachment.id)
          }"
        >
          <div
            class="upload-preview-main"
            :class="{ 'upload-preview-main--button': attachment.type === 'video' }"
            :role="attachment.type === 'video' ? 'button' : undefined"
            :tabindex="attachment.type === 'video' ? 0 : undefined"
            @click="attachment.type === 'video' && toggleVideoControl(attachment.id)"
            @keydown.enter.prevent="attachment.type === 'video' && toggleVideoControl(attachment.id)"
            @keydown.space.prevent="attachment.type === 'video' && toggleVideoControl(attachment.id)"
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
                v-if="attachment.type === 'video' && attachment.warnings?.length"
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
            v-if="attachment.type === 'video' && isVideoControlOpen(attachment.id)"
            class="upload-preview-video-controls"
          >
            <label class="upload-preview-video-field">
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
            <button
              class="upload-preview-video-apply"
              type="button"
              :disabled="!attachment.source_public_path || isAttachmentProcessing(attachment.id)"
              @click="applyVideoFrameRate(attachment.id)"
            >
              {{ t('chat.attachments.video.reextract') }}
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
          ref="worldCommandAnchorRef"
          class="messenger-world-tool-anchor"
          :class="{ 'is-open': worldCommandPanelVisible }"
          @mouseenter="handleWorldCommandAnchorMouseEnter"
          @mouseleave="handleWorldCommandAnchorMouseLeave"
          @focusin="handleWorldCommandAnchorFocusIn"
          @focusout="handleWorldCommandAnchorFocusOut"
        >
          <button
            class="messenger-world-tool-btn"
            type="button"
            :class="{ active: worldCommandPanelVisible }"
            :title="t('chat.commandMenu.quick')"
            :aria-label="t('chat.commandMenu.quick')"
            @click.prevent="openWorldCommandPanel"
          >
            <i class="fa-solid fa-terminal chat-composer-command-btn-icon" aria-hidden="true"></i>
          </button>
          <div
            v-if="worldCommandPanelVisible"
            class="chat-composer-command-panel"
            @mouseenter="handleWorldCommandPanelMouseEnter"
            @mouseleave="handleWorldCommandPanelMouseLeave"
          >
            <template v-if="presetQuestionItems.length">
              <div class="chat-composer-command-section-label">
                {{ t('chat.commandMenu.presetQuestions') }}
              </div>
              <button
                v-for="item in presetQuestionItems"
                :key="`preset:${item.command}`"
                class="chat-composer-command-item chat-composer-command-item--question"
                type="button"
                :title="item.command"
                @click="applyPresetQuestion(item.command)"
              >
                <span class="chat-composer-command-name">{{ item.command }}</span>
              </button>
              <div class="chat-composer-command-section-label">
                {{ t('chat.commandMenu.commands') }}
              </div>
            </template>
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
          :title="t('chat.attachments.upload')"
          :aria-label="t('chat.attachments.upload')"
          :disabled="attachmentBusy > 0 || voiceRecording"
          @click="triggerUpload"
        >
          <i class="fa-solid fa-paperclip messenger-world-tool-fa-icon" aria-hidden="true"></i>
        </button>
        <button
          class="messenger-world-tool-btn"
          type="button"
          :class="{
            'messenger-world-tool-btn--recording': voiceRecording
          }"
          :title="voiceButtonTitle"
          :aria-label="voiceButtonTitle"
          :disabled="attachmentBusy > 0 || loading"
          @click="handleToggleVoiceRecord"
        >
          <i
            :class="[
              voiceRecording ? 'fa-solid fa-stop' : 'fa-solid fa-microphone',
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
            :disabled="attachmentBusy > 0 || voiceRecording"
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
      </div>
      <textarea
        v-model="inputText"
        ref="inputRef"
        :class="{ 'chat-composer-input--world': worldStyle }"
        :placeholder="inputPlaceholder"
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
              :title="loading ? t('common.stop') : t('chat.input.send')"
              :aria-label="loading ? t('common.stop') : t('chat.input.send')"
              @keydown="handleSendButtonKeydown"
              @click="handleSendOrStop"
            >
              <i
                v-if="loading"
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
        <button
          class="input-icon-btn upload-btn"
          type="button"
          :title="t('chat.attachments.upload')"
          :aria-label="t('chat.attachments.upload')"
          :disabled="attachmentBusy > 0"
          @click="triggerUpload"
        >
          <i class="fa-solid fa-paperclip input-icon" aria-hidden="true"></i>
        </button>
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
            :disabled="attachmentBusy > 0"
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
          :title="loading ? t('common.stop') : t('chat.input.send')"
          :aria-label="loading ? t('common.stop') : t('chat.input.send')"
          @keydown="handleSendButtonKeydown"
          @click="handleSendOrStop"
        >
          <i
            v-if="loading"
            class="fa-solid fa-stop input-icon input-icon-fill"
            aria-hidden="true"
          ></i>
          <i v-else class="fa-solid fa-paper-plane input-icon input-icon-fill" aria-hidden="true"></i>
        </button>
      </template>
    </div>

    <input
      ref="uploadInputRef"
      type="file"
      hidden
      multiple
      :accept="uploadAccept"
      @change="handleUploadInput"
    />

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
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { convertChatAttachment, processChatMediaAttachment } from '@/api/chat';
import {
  clearComposerDraftState,
  readComposerDraftState,
  writeComposerDraftState,
  type ComposerDraftAttachment
} from '@/components/chat/composerDraftCache';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { resolveAnyProviderModelPresetMaxContext } from '@/views/messenger/providerModelPresets';

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
  presetQuestions: {
    type: Array,
    default: () => []
  }
});

const emit = defineEmits([
  'send',
  'stop',
  'toggle-voice-record',
  'open-model-settings',
  'update:approval-mode'
]);

const inputText = ref('');
const inputRef = ref(null);
const uploadInputRef = ref(null);
const attachments = ref<ComposerDraftAttachment[]>([]);
const attachmentBusy = ref(0);
const dragActive = ref(false);
const dragCounter = ref(0);
const worldCommandAnchorRef = ref<HTMLElement | null>(null);
const worldCommandPanelVisible = ref(false);
const worldCommandAnchorHovered = ref(false);
const worldCommandPanelHovered = ref(false);
const screenshotMenuAnchorRef = ref<HTMLElement | null>(null);
const screenshotMenuPanelRef = ref<HTMLElement | null>(null);
const screenshotMenuVisible = ref(false);
const screenshotMenuStyle = ref<Record<string, string>>({});
const caretPosition = ref(0);
const commandMenuIndex = ref(0);
const commandMenuDismissed = ref(false);
const expandedVideoAttachmentId = ref('');
const videoFrameRateDrafts = ref<Record<string, string>>({});
const attachmentProcessingIds = ref<string[]>([]);
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
let worldCommandPanelCloseTimer: ReturnType<typeof setTimeout> | null = null;
const { t } = useI18n();
const chatStore = useChatStore();

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'wav', 'ogg', 'opus', 'aac', 'flac', 'm4a', 'webm']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'mkv', 'avi', 'webm', 'mpeg', 'mpg', 'm4v']);

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
const DOC_EXTENSIONS = [
  '.txt',
  '.md',
  '.markdown',
  '.html',
  '.htm',
  '.py',
  '.c',
  '.cpp',
  '.cc',
  '.h',
  '.hpp',
  '.json',
  '.js',
  '.ts',
  '.css',
  '.ini',
  '.cfg',
  '.log',
  '.doc',
  '.docx',
  '.odt',
  '.pptx',
  '.odp',
  '.xlsx',
  '.ods',
  '.wps',
  '.et',
  '.dps'
];
const uploadAccept = ['image/*', 'audio/*', 'video/*', ...DOC_EXTENSIONS].join(',');
const INPUT_MAX_HEIGHT = 180;
const WORLD_COMPOSER_HEIGHT_STORAGE_KEY = 'wunder_world_composer_height';
const WORLD_COMMAND_PANEL_CLOSE_DELAY_MS = 160;
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
const resolveAssistantContextTokens = (stats: Record<string, unknown> | null): number | null => {
  if (!stats) {
    return null;
  }
  const usageTotal = normalizePositiveTokenCount(
    (stats.usage as Record<string, unknown> | undefined)?.total ??
      (stats.usage as Record<string, unknown> | undefined)?.total_tokens ??
      (stats.usage as Record<string, unknown> | undefined)?.totalTokens
  );
  if (usageTotal !== null) {
    return usageTotal;
  }
  const usageInput = normalizePositiveTokenCount(
    (stats.usage as Record<string, unknown> | undefined)?.input ??
      (stats.usage as Record<string, unknown> | undefined)?.input_tokens ??
      (stats.usage as Record<string, unknown> | undefined)?.inputTokens
  );
  if (usageInput !== null) {
    return usageInput;
  }
  const roundUsageTotal = normalizePositiveTokenCount(
    (stats.roundUsage as Record<string, unknown> | undefined)?.total ??
      (stats.roundUsage as Record<string, unknown> | undefined)?.total_tokens ??
      (stats.roundUsage as Record<string, unknown> | undefined)?.totalTokens ??
      (stats.round_usage as Record<string, unknown> | undefined)?.total ??
      (stats.round_usage as Record<string, unknown> | undefined)?.total_tokens ??
      (stats.round_usage as Record<string, unknown> | undefined)?.totalTokens
  );
  if (roundUsageTotal !== null) {
    return roundUsageTotal;
  }
  const roundUsageInput = normalizePositiveTokenCount(
    (stats.roundUsage as Record<string, unknown> | undefined)?.input ??
      (stats.roundUsage as Record<string, unknown> | undefined)?.input_tokens ??
      (stats.roundUsage as Record<string, unknown> | undefined)?.inputTokens ??
      (stats.round_usage as Record<string, unknown> | undefined)?.input ??
      (stats.round_usage as Record<string, unknown> | undefined)?.input_tokens ??
      (stats.round_usage as Record<string, unknown> | undefined)?.inputTokens
  );
  if (roundUsageInput !== null) {
    return roundUsageInput;
  }
  const explicitContext = normalizePositiveTokenCount(
    stats.contextTokens ??
      stats.contextOccupancyTokens ??
      stats.context_occupancy_tokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      (stats.context_usage as Record<string, unknown> | undefined)?.context_tokens ??
      (stats.context_usage as Record<string, unknown> | undefined)?.contextTokens
  );
  if (explicitContext !== null) {
    return explicitContext;
  }
  return normalizePositiveTokenCount(
    (stats.context_usage as Record<string, unknown> | undefined)?.context_tokens ??
      (stats.context_usage as Record<string, unknown> | undefined)?.contextTokens ??
      (stats.usage as Record<string, unknown> | undefined)?.total ??
      (stats.usage as Record<string, unknown> | undefined)?.total_tokens ??
      (stats.usage as Record<string, unknown> | undefined)?.totalTokens
  );
};
const resolveAssistantContextTotalTokens = (stats: Record<string, unknown> | null): number | null => {
  if (!stats) {
    return null;
  }
  return normalizePositiveTokenCount(
    stats.contextTotalTokens ??
      stats.context_total_tokens ??
      stats.context_max_tokens ??
      stats.max_context ??
      stats.maxContext ??
      stats.context_window ??
      (stats.context_usage as Record<string, unknown> | undefined)?.max_context ??
      (stats.context_usage as Record<string, unknown> | undefined)?.context_max_tokens
  );
};
const resolveCurrentSessionContextTokens = (): number | null => {
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (!activeSessionId) {
    return null;
  }
  const session = (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).find(
    (item) => String((item as Record<string, unknown> | null)?.id || '').trim() === activeSessionId
  ) as Record<string, unknown> | undefined;
  if (!session) {
    return null;
  }
  return normalizePositiveTokenCount(
    session.contextTokens ??
      session.context_tokens ??
      session.contextOccupancyTokens ??
      session.context_occupancy_tokens ??
      (session.context_usage as Record<string, unknown> | undefined)?.context_tokens ??
      (session.context_usage as Record<string, unknown> | undefined)?.contextTokens
  );
};
const resolveCurrentSessionContextTotalTokens = (): number | null => {
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (!activeSessionId) {
    return null;
  }
  const session = (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).find(
    (item) => String((item as Record<string, unknown> | null)?.id || '').trim() === activeSessionId
  ) as Record<string, unknown> | undefined;
  if (!session) {
    return null;
  }
  return normalizePositiveTokenCount(
    session.contextTotalTokens ??
      session.context_total_tokens ??
      session.context_max_tokens ??
      session.max_context ??
      session.maxContext ??
      session.context_window ??
      (session.context_usage as Record<string, unknown> | undefined)?.max_context ??
      (session.context_usage as Record<string, unknown> | undefined)?.context_max_tokens
  );
};
const resolveLatestContextTokensFromMessages = (messages: unknown[]): number | null => {
  for (let cursor = messages.length - 1; cursor >= 0; cursor -= 1) {
    const current =
      messages[cursor] && typeof messages[cursor] === 'object'
        ? (messages[cursor] as Record<string, unknown>)
        : null;
    if (!current) continue;
    if (String(current.role || '').trim().toLowerCase() !== 'assistant') continue;
    const stats =
      current.stats && typeof current.stats === 'object'
        ? (current.stats as Record<string, unknown>)
        : null;
    if (!stats) continue;
    const normalized = resolveAssistantContextTokens(stats);
    if (normalized !== null) {
      return normalized;
    }
  }
  return null;
};
const resolveLatestContextTotalTokensFromMessages = (messages: unknown[]): number | null => {
  for (let cursor = messages.length - 1; cursor >= 0; cursor -= 1) {
    const current =
      messages[cursor] && typeof messages[cursor] === 'object'
        ? (messages[cursor] as Record<string, unknown>)
        : null;
    if (!current) continue;
    if (String(current.role || '').trim().toLowerCase() !== 'assistant') continue;
    const stats =
      current.stats && typeof current.stats === 'object'
        ? (current.stats as Record<string, unknown>)
        : null;
    if (!stats) continue;
    const normalized = resolveAssistantContextTotalTokens(stats);
    if (normalized !== null) {
      return normalized;
    }
  }
  return null;
};
const tokenNumberFormatter = (() => {
  let formatter: Intl.NumberFormat | null = null;
  return (): Intl.NumberFormat => {
    if (!formatter) {
      formatter = new Intl.NumberFormat();
    }
    return formatter;
  };
})();
const formatContextTokenCount = (value: unknown): string => {
  const normalized = normalizeTokenCount(value);
  if (normalized === null) return '--';
  return tokenNumberFormatter().format(normalized);
};

const showUploadArea = computed(() => attachments.value.length > 0 || attachmentBusy.value > 0);
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
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  const fromMessageStats = resolveLatestContextTokensFromMessages(messages);
  const fromSession = resolveCurrentSessionContextTokens();
  if (fromMessageStats !== null && fromSession !== null) {
    return Math.max(fromMessageStats, fromSession);
  }
  return fromMessageStats ?? fromSession;
});
const composerContextTotalTokensRaw = computed(() => {
  const fromProps = normalizeTokenCount(props.contextTotalTokens);
  if (fromProps !== null && fromProps > 0) {
    return fromProps;
  }
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  const fromStats = resolveLatestContextTotalTokensFromMessages(messages);
  const fromSession = resolveCurrentSessionContextTotalTokens();
  const fromMerged =
    fromStats !== null && fromSession !== null ? Math.max(fromStats, fromSession) : fromStats ?? fromSession;
  if (fromMerged !== null && fromMerged > 0) {
    return fromMerged;
  }
  const fromPreset = resolveAnyProviderModelPresetMaxContext(composerModelName.value);
  const normalizedPreset = normalizeTokenCount(fromPreset);
  return normalizedPreset !== null && normalizedPreset > 0 ? normalizedPreset : null;
});
const contextDisplaySessionId = computed(() => String(chatStore.activeSessionId || '').trim());
const lastContextDisplaySessionId = ref<string>(contextDisplaySessionId.value);
const composerContextUsedTokensStable = ref<number | null>(null);
const composerContextTotalTokensStable = ref<number | null>(null);
watch(
  [
    contextDisplaySessionId,
    () => Boolean(props.loading),
    composerContextUsedTokensRaw,
    composerContextTotalTokensRaw
  ],
  ([sessionId, loading, rawUsed, rawTotal]) => {
    const switchedSession = sessionId !== lastContextDisplaySessionId.value;
    lastContextDisplaySessionId.value = sessionId;
    if (switchedSession || !loading) {
      composerContextUsedTokensStable.value = rawUsed;
      composerContextTotalTokensStable.value = rawTotal;
      return;
    }
    if (rawUsed !== null) {
      const current = composerContextUsedTokensStable.value;
      composerContextUsedTokensStable.value =
        current === null ? rawUsed : Math.max(current, rawUsed);
    }
    if (rawTotal !== null) {
      const current = composerContextTotalTokensStable.value;
      composerContextTotalTokensStable.value =
        current === null ? rawTotal : Math.max(current, rawTotal);
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
  return props.voiceRecording ? t('messenger.world.voice.stop') : t('messenger.world.voice.start');
});
const voiceRecordingLabel = computed(() =>
  t('messenger.world.voice.recording', {
    duration: formatVoiceDurationLabel(props.voiceDurationMs)
  })
);
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
const canSendOrStop = computed(() => {

  if (props.loading) return true;
  if (attachmentBusy.value > 0) return false;
  return (
    Boolean(inputText.value.trim()) ||
    attachments.value.length > 0 ||
    hasInquirySelection.value
  );
});

const slashCommandDefinitions: SlashCommandDefinition[] = [
  { command: '/new', aliases: ['/reset'], descriptionKey: 'chat.commandMenu.new' },
  { command: '/stop', aliases: ['/cancel'], descriptionKey: 'chat.commandMenu.stop' },
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
  slashCommandDefinitions.map((item) => ({
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

const resolveFileExtension = (filename) => {
  const parts = String(filename || '').trim().split('.');
  if (parts.length < 2) return '';
  return parts.pop().toLowerCase();
};

const isImageFile = (file) => {
  if (file?.type && file.type.startsWith('image/')) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? IMAGE_EXTENSIONS.has(ext) : false;
};

const isAudioFile = (file) => {
  if (file?.type && file.type.startsWith('audio/')) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? AUDIO_EXTENSIONS.has(ext) : false;
};

const isVideoFile = (file) => {
  if (file?.type && file.type.startsWith('video/')) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? VIDEO_EXTENSIONS.has(ext) : false;
};

const readFileAsDataUrl = (file): Promise<string> =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ''));
    reader.onerror = () => reject(new Error(t('chat.attachments.imageReadFailed')));
    reader.readAsDataURL(file);
  });

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
  return 'fa-file-lines';
};

const resolveAttachmentMeta = (attachment: ComposerDraftAttachment): string => {
  if (isAttachmentProcessing(attachment.id)) {
    return t('chat.attachments.video.processingSingle');
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

const resolveVideoControlSummary = (attachment: ComposerDraftAttachment): string => {
  if (!attachment.source_public_path) {
    return t('chat.attachments.video.controlUnavailable');
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

// 发送时只保留 Wunder 需要的字段，避免 UI 状态混入请求
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
  const next: Record<string, string> = {};
  attachments.value.forEach((attachment) => {
    if (attachment.type !== 'video') return;
    const id = String(attachment.id || '').trim();
    if (!id) return;
    const existing = String(videoFrameRateDrafts.value[id] || '').trim();
    next[id] =
      existing ||
      formatFrameRate(attachment.requested_frame_rate || attachment.applied_frame_rate || 1);
  });
  videoFrameRateDrafts.value = next;
  if (expandedVideoAttachmentId.value && !next[expandedVideoAttachmentId.value]) {
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
  if (props.worldStyle) {
    el.style.height = '';
    el.style.overflowY = 'auto';
    return;
  }
  el.style.height = 'auto';
  el.style.overflowY = 'hidden';
};

const triggerUpload = () => {
  if (!uploadInputRef.value) return;
  closeScreenshotMenu();
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const hasFileDrag = (event) => Array.from(event?.dataTransfer?.types || []).includes('Files');

const handleDragEnter = (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  dragCounter.value += 1;
  dragActive.value = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'copy';
  }
};

const handleDragOver = (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'copy';
  }
};

const handleDragLeave = (event) => {
  if (!hasFileDrag(event)) return;
  dragCounter.value = Math.max(0, dragCounter.value - 1);
  if (dragCounter.value === 0) {
    dragActive.value = false;
  }
};

const handleDrop = async (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  dragCounter.value = 0;
  dragActive.value = false;
  const files = Array.from(event.dataTransfer?.files || []);
  if (!files.length) return;
  for (const file of files) {
    await handleAttachmentSelection(file);
  }
};

const requestMediaProcessing = async (formData: FormData): Promise<ProcessedMediaResponse> => {
  const response = await processChatMediaAttachment(formData);
  return (response?.data?.data || {}) as ProcessedMediaResponse;
};

const buildAudioDraftAttachment = (
  filename: string,
  payload: ProcessedMediaResponse
): ComposerDraftAttachment => {
  const attachment = Array.isArray(payload.attachments)
    ? payload.attachments
        .map((item) => normalizeProcessedMediaAttachment(item, 'audio'))
        .find(Boolean) || null
    : null;
  if (!attachment) {
    throw new Error(t('chat.attachments.emptyResult'));
  }
  attachment.id = buildAttachmentId();
  attachment.type = 'audio';
  attachment.name = attachment.name || filename;
  if (payload.warnings?.length) {
    attachment.warnings = payload.warnings;
  }
  if (Number.isFinite(payload.duration_ms)) {
    attachment.duration_ms = Number(payload.duration_ms);
  }
  return attachment;
};

const buildVideoDraftAttachment = (
  filename: string,
  payload: ProcessedMediaResponse,
  attachmentId?: string
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
    type: 'video',
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

const processAudioFile = async (file: File): Promise<ComposerDraftAttachment> => {
  const formData = new FormData();
  formData.append('file', file);
  return buildAudioDraftAttachment(file.name || 'audio', await requestMediaProcessing(formData));
};

const processVideoFile = async (
  file: File,
  requestedFrameRate?: string
): Promise<ComposerDraftAttachment> => {
  const formData = new FormData();
  formData.append('file', file);
  formData.append('frame_rate', String(requestedFrameRate || '1').trim() || '1');
  return buildVideoDraftAttachment(
    file.name || 'video',
    await requestMediaProcessing(formData)
  );
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

// 附件处理遵循 Wunder 调试面板：图片走 data URL，文件先转 Markdown
const handleAttachmentSelection = async (file) => {
  if (!file) return;
  const filename = file.name || 'upload';
  attachmentBusy.value += 1;
  try {
    if (isImageFile(file)) {
      const dataUrl = await readFileAsDataUrl(file);
      if (!dataUrl) {
        throw new Error(t('chat.attachments.imageEmpty'));
      }
      attachments.value.push({
        id: buildAttachmentId(),
        type: 'image',
        name: filename,
        content: dataUrl,
        mime_type: file.type || ''
      });
      syncVideoAttachmentDrafts();
      ElMessage.success(t('chat.attachments.imageAdded', { name: filename }));
      return;
    }

    if (isAudioFile(file)) {
      const attachment = await processAudioFile(file);
      pushAttachment(attachment);
      if (attachment.warnings?.length) {
        ElMessage.warning(attachment.warnings[0]);
      } else {
        ElMessage.success(t('chat.attachments.audioAdded', { name: filename }));
      }
      return;
    }

    if (isVideoFile(file)) {
      const attachment = await processVideoFile(file);
      pushAttachment(attachment);
      expandedVideoAttachmentId.value = attachment.id;
      if (attachment.warnings?.length) {
        ElMessage.warning(attachment.warnings[0]);
      } else {
        ElMessage.success(t('chat.attachments.videoAdded', { name: filename }));
      }
      return;
    }

    const extension = resolveFileExtension(filename);
    if (!extension || !DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(
        t('chat.attachments.unsupportedType', { ext: extension || t('common.unknown') })
      );
    }

    const response = await convertChatAttachment(file);
    const payload = response?.data?.data || {};
    const content = typeof payload.content === 'string' ? payload.content : '';
    if (!content.trim()) {
      throw new Error(t('chat.attachments.emptyResult'));
    }
    attachments.value.push({
      id: buildAttachmentId(),
      type: 'file',
      name: payload.name || filename,
      content,
      mime_type: file.type || '',
      converter: payload.converter || ''
    });
    syncVideoAttachmentDrafts();
    const warnings = Array.isArray(payload.warnings) ? payload.warnings : [];
    if (warnings.length) {
      ElMessage.warning(t('chat.attachments.convertWarning', { message: warnings[0] }));
    } else {
      ElMessage.success(t('chat.attachments.fileParsed', { name: payload.name || filename }));
    }
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.attachments.processFailed')));
  } finally {
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
};

const handleUploadInput = async (event) => {
  const files = Array.from(event.target.files || []);
  if (!files.length) return;
  for (const file of files) {
    await handleAttachmentSelection(file);
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
};

const closeScreenshotMenu = () => {
  screenshotMenuVisible.value = false;
  screenshotMenuStyle.value = {};
};

const toggleScreenshotMenu = () => {
  if (attachmentBusy.value > 0) {
    ElMessage.warning(t('chat.attachments.busy'));
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
  closeScreenshotMenu();
  const bridge = getDesktopScreenshotBridge();
  if (!bridge || typeof bridge.captureScreenshot !== 'function') {
    ElMessage.warning(t('chat.attachments.screenshotUnavailable'));
    return;
  }
  if (attachmentBusy.value > 0) {
    ElMessage.warning(t('chat.attachments.busy'));
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
    const mimeType = String(result.mimeType || '').trim() || 'image/png';
    attachments.value.push({
      id: buildAttachmentId(),
      type: 'image',
      name,
      content: dataUrl,
      mime_type: mimeType
    });
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

const scheduleWorldCommandPanelClose = () => {
  clearWorldCommandPanelCloseTimer();
  worldCommandPanelCloseTimer = setTimeout(() => {
    worldCommandPanelCloseTimer = null;
    if (worldCommandAnchorHovered.value || worldCommandPanelHovered.value) {
      return;
    }
    worldCommandPanelVisible.value = false;
  }, WORLD_COMMAND_PANEL_CLOSE_DELAY_MS);
};

const openWorldCommandPanel = () => {
  closeScreenshotMenu();
  clearWorldCommandPanelCloseTimer();
  worldCommandPanelVisible.value = true;
};

const closeWorldCommandPanel = () => {
  clearWorldCommandPanelCloseTimer();
  worldCommandAnchorHovered.value = false;
  worldCommandPanelHovered.value = false;
  worldCommandPanelVisible.value = false;
};

const handleWorldCommandAnchorMouseEnter = () => {
  worldCommandAnchorHovered.value = true;
  openWorldCommandPanel();
};

const handleWorldCommandAnchorMouseLeave = () => {
  worldCommandAnchorHovered.value = false;
  scheduleWorldCommandPanelClose();
};

const handleWorldCommandPanelMouseEnter = () => {
  worldCommandPanelHovered.value = true;
  openWorldCommandPanel();
};

const handleWorldCommandPanelMouseLeave = () => {
  worldCommandPanelHovered.value = false;
  scheduleWorldCommandPanelClose();
};

const handleWorldCommandAnchorFocusIn = () => {
  worldCommandAnchorHovered.value = true;
  openWorldCommandPanel();
};

const handleWorldCommandAnchorFocusOut = (event: FocusEvent) => {
  const anchor = worldCommandAnchorRef.value;
  const nextTarget = event.relatedTarget as Node | null;
  if (anchor && nextTarget && anchor.contains(nextTarget)) {
    return;
  }
  worldCommandAnchorHovered.value = false;
  scheduleWorldCommandPanelClose();
};

const handleApprovalModeChange = (event: Event) => {
  const target = event.target as HTMLSelectElement | null;
  const value = String(target?.value || '').trim();
  if (!value) return;
  emit('update:approval-mode', value);
};

const sendQuickCommand = async (command: string) => {
  closeWorldCommandPanel();
  closeScreenshotMenu();
  if (!command) return;
  if (props.loading) {
    if (command === '/stop') {
      emit('stop');
    }
    return;
  }
  if (attachmentBusy.value > 0) {
    ElMessage.warning(t('chat.attachments.busy'));
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
  closeWorldCommandPanel();
  closeScreenshotMenu();
  const preset = String(question || '').trim();
  if (!preset) return;
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
  if (props.loading) return;
  if (voiceRecording.value) return;
  closeScreenshotMenu();
  if (commandSuggestionsVisible.value && applyCommandSuggestion()) {
    return;
  }
  // 附件解析未完成时禁止发送，避免请求缺少必要内容
  if (attachmentBusy.value > 0) {
    ElMessage.warning(t('chat.attachments.busy'));
    return;
  }
  const content = inputText.value.trim();
  const payloadAttachments = buildAttachmentPayload();
  if (!content && payloadAttachments.length === 0 && !hasInquirySelection.value) return;
  emit('send', { content, attachments: payloadAttachments });
  inputText.value = '';
  commandMenuDismissed.value = false;
  caretPosition.value = 0;
  resetInputHeight();
  clearAttachments();
  focusComposerInputAtEnd();
};

const handleSendOrStop = async () => {
  if (props.loading) {
    emit('stop');
    return;
  }
  await handleSend();
};

const handleSendButtonKeydown = (event: KeyboardEvent) => {
  if (!props.loading) {
    return;
  }
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    event.stopPropagation();
    focusComposerInputAtEnd();
  }
};

const handleToggleVoiceRecord = () => {
  closeScreenshotMenu();
  closeWorldCommandPanel();
  emit('toggle-voice-record');
};

const handleDocumentPointerDown = (event: PointerEvent) => {
  const target = event.target as Node | null;
  if (worldCommandPanelVisible.value) {
    const commandAnchor = worldCommandAnchorRef.value;
    if (!commandAnchor || !target || !commandAnchor.contains(target)) {
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
</script>
