<template>
  <div class="input-container" :class="{ 'input-container--world': worldStyle }">
    <div v-if="showUploadArea" class="upload-preview">
      <div class="upload-preview-list">
        <div v-for="attachment in attachments" :key="attachment.id" class="upload-preview-item">
          <i
            v-if="attachment.type === 'image'"
            class="fa-solid fa-image upload-preview-icon"
            aria-hidden="true"
          ></i>
          <i v-else class="fa-solid fa-file-lines upload-preview-icon" aria-hidden="true"></i>
          <span class="upload-preview-name" :title="attachment.name">{{ attachment.name }}</span>
          <button
            class="upload-preview-remove"
            type="button"
            :title="t('common.remove')"
            :aria-label="t('common.remove')"
            @click="removeAttachment(attachment.id)"
          >
            <i class="fa-solid fa-xmark upload-preview-remove-icon" aria-hidden="true"></i>
          </button>
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
            <button
              class="messenger-world-send-main"
              type="button"
              :disabled="!canSendOrStop"
              :title="loading ? t('common.stop') : t('chat.input.send')"
              :aria-label="loading ? t('common.stop') : t('chat.input.send')"
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

import { convertChatAttachment } from '@/api/chat';
import {
  clearComposerDraftState,
  readComposerDraftState,
  writeComposerDraftState,
  type ComposerDraftAttachment
} from '@/components/chat/composerDraftCache';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
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
    default: 'ctrl_enter'
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

const emit = defineEmits(['send', 'stop', 'toggle-voice-record', 'open-model-settings']);

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
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
let worldCommandPanelCloseTimer: ReturnType<typeof setTimeout> | null = null;
const { t } = useI18n();
const chatStore = useChatStore();

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);

type AttachmentPayload = {
  type: string;
  name: string;
  content: string;
  mime_type?: string;
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
const uploadAccept = ['image/*', ...DOC_EXTENSIONS].join(',');
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
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return null;
  }
  return Math.round(parsed);
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
    const normalized = normalizeTokenCount(
      stats.contextTokens ??
        stats.context_tokens ??
        stats.context_tokens_total ??
        (stats.context_usage as Record<string, unknown> | undefined)?.context_tokens ??
        (stats.context_usage as Record<string, unknown> | undefined)?.contextTokens
    );
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
const composerContextUsedTokens = computed(() => {
  const fromProps = normalizeTokenCount(props.contextUsedTokens);
  if (fromProps !== null) {
    return fromProps;
  }
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  return resolveLatestContextTokensFromMessages(messages);
});
const composerContextTotalTokens = computed(() => {
  const fromProps = normalizeTokenCount(props.contextTotalTokens);
  if (fromProps !== null && fromProps > 0) {
    return fromProps;
  }
  const fromPreset = resolveAnyProviderModelPresetMaxContext(composerModelName.value);
  const normalizedPreset = normalizeTokenCount(fromPreset);
  return normalizedPreset !== null && normalizedPreset > 0 ? normalizedPreset : null;
});
const composerContextUsageDisplay = computed(() => {
  if (!props.worldStyle) return '';
  const used = composerContextUsedTokens.value;
  const total = composerContextTotalTokens.value;
  if (used === null && total === null) return '';
  const usedText = formatContextTokenCount(used ?? 0);
  const totalText = formatContextTokenCount(total);
  return `${usedText}/${totalText}`;
});
const composerContextUsageTooltip = computed(() => {
  const usage = composerContextUsageDisplay.value;
  if (!usage) return '';
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
const approvalLabelTooltip = computed(() => {
  if (!approvalLabelText.value) return '';
  const hint = t('portal.agent.permission.tooltip');
  if (!hint) return approvalLabelText.value;
  return `${approvalLabelText.value} · ${hint}`;
});
const showApprovalLabel = computed(
  () => props.worldStyle && props.showApprovalLabel && Boolean(approvalLabelText.value)
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

const readFileAsDataUrl = (file): Promise<string> =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ''));
    reader.onerror = () => reject(new Error(t('chat.attachments.imageReadFailed')));
    reader.readAsDataURL(file);
  });

// 发送时只保留 Wunder 需要的字段，避免 UI 状态混入请求
const buildAttachmentPayload = () =>
  attachments.value
    .filter((item) => String(item?.content || '').trim())
    .map((item) => {
      const payload: AttachmentPayload = {
        type: item.type,
        name: item.name,
        content: item.content
      };
      if (item.mime_type) {
        payload.mime_type = item.mime_type;
      }
      return payload;
    });

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
  props.sendKey === 'enter' || props.sendKey === 'none' ? props.sendKey : 'ctrl_enter';

const normalizeDraftAttachment = (value: unknown): ComposerDraftAttachment | null => {
  if (!value || typeof value !== 'object') return null;
  const source = value as Record<string, unknown>;
  const id = String(source.id || '').trim();
  const type = String(source.type || '').trim();
  const name = String(source.name || '').trim();
  const content = String(source.content || '');
  if (!id || !type || !name || !content.trim()) return null;
  const attachment: ComposerDraftAttachment = {
    id,
    type,
    name,
    content
  };
  const mimeType = String(source.mime_type || '').trim();
  if (mimeType) attachment.mime_type = mimeType;
  const converter = String(source.converter || '').trim();
  if (converter) attachment.converter = converter;
  return attachment;
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
  if (hasBackupSendModifier(event)) {
    event.preventDefault();
    await handleSend();
    return;
  }
  if (hasPrimarySendModifier(event)) {
    event.preventDefault();
    await handleSend();
    return;
  }
  if (mode === 'ctrl_enter') {
    return;
  }
  if (event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) {
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
      ElMessage.success(t('chat.attachments.imageAdded', { name: filename }));
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
};

const clearAttachments = () => {
  attachments.value = [];
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
};

const handleSendOrStop = async () => {
  if (props.loading) {
    emit('stop');
    return;
  }
  await handleSend();
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
</script>
