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
          :disabled="attachmentBusy > 0"
          @click="triggerUpload"
        >
          <i class="fa-solid fa-paperclip messenger-world-tool-fa-icon" aria-hidden="true"></i>
        </button>
        <button
          v-if="desktopScreenshotSupported"
          class="messenger-world-tool-btn"
          type="button"
          :title="t('chat.attachments.screenshot')"
          :aria-label="t('chat.attachments.screenshot')"
          :disabled="attachmentBusy > 0"
          @click="captureDesktopScreenshotAttachment"
        >
          <i class="fa-solid fa-camera messenger-world-tool-fa-icon" aria-hidden="true"></i>
        </button>
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
        <button
          v-if="desktopScreenshotSupported"
          class="input-icon-btn screenshot-btn"
          type="button"
          :title="t('chat.attachments.screenshot')"
          :aria-label="t('chat.attachments.screenshot')"
          :disabled="attachmentBusy > 0"
          @click="captureDesktopScreenshotAttachment"
        >
          <i class="fa-solid fa-camera input-icon" aria-hidden="true"></i>
        </button>
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
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { convertChatAttachment } from '@/api/chat';
import {
  clearComposerDraftState,
  readComposerDraftState,
  writeComposerDraftState,
  type ComposerDraftAttachment
} from '@/components/chat/composerDraftCache';
import { useI18n } from '@/i18n';

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
  }
});

const emit = defineEmits(['send', 'stop']);

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
const caretPosition = ref(0);
const commandMenuIndex = ref(0);
const commandMenuDismissed = ref(false);
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
let worldCommandPanelCloseTimer: ReturnType<typeof setTimeout> | null = null;
const { t } = useI18n();

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);

type AttachmentPayload = {
  type: string;
  name: string;
  content: string;
  mime_type?: string;
};

type DesktopScreenshotResult = {
  ok?: boolean;
  name?: string;
  path?: string;
  mimeType?: string;
  dataUrl?: string;
  message?: string;
};

type DesktopScreenshotBridge = {
  captureScreenshot?: (
    options?: { hideWindow?: boolean }
  ) => Promise<DesktopScreenshotResult | null> | DesktopScreenshotResult | null;
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

const showUploadArea = computed(() => attachments.value.length > 0 || attachmentBusy.value > 0);
const getDesktopScreenshotBridge = (): DesktopScreenshotBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopScreenshotBridge }).wunderDesktop;
  if (!candidate || typeof candidate.captureScreenshot !== 'function') {
    return null;
  }
  return candidate;
};
const desktopScreenshotSupported = computed(() => Boolean(getDesktopScreenshotBridge()));
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

const pickScreenshotCaptureMode = async (): Promise<boolean | null> => {
  try {
    await ElMessageBox.confirm(
      t('chat.attachments.screenshotModePrompt'),
      t('chat.attachments.screenshotModeTitle'),
      {
        type: 'info',
        confirmButtonText: t('chat.attachments.screenshotModeHide'),
        cancelButtonText: t('chat.attachments.screenshotModeKeep'),
        distinguishCancelAndClose: true
      }
    );
    return true;
  } catch (action) {
    if (action === 'cancel') {
      return false;
    }
    return null;
  }
};

const captureDesktopScreenshotAttachment = async () => {
  const bridge = getDesktopScreenshotBridge();
  if (!bridge || typeof bridge.captureScreenshot !== 'function') {
    ElMessage.warning(t('chat.attachments.screenshotUnavailable'));
    return;
  }
  if (attachmentBusy.value > 0) {
    ElMessage.warning(t('chat.attachments.busy'));
    return;
  }
  const hideWindow = await pickScreenshotCaptureMode();
  if (hideWindow === null) {
    return;
  }

  attachmentBusy.value += 1;
  try {
    const result = await bridge.captureScreenshot({ hideWindow });
    if (!result || result.ok === false) {
      throw new Error(
        String(result?.message || t('chat.attachments.screenshotFailed')).trim() ||
          t('chat.attachments.screenshotFailed')
      );
    }
    const dataUrl = String(result.dataUrl || '').trim();
    if (!dataUrl || !dataUrl.startsWith('data:image/')) {
      throw new Error(t('chat.attachments.screenshotFailed'));
    }
    const name = String(result.name || '').trim() || `screenshot-${Date.now()}.png`;
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

const handleSend = async () => {
  if (props.loading) return;
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

const handleDocumentPointerDown = (event: PointerEvent) => {
  if (!worldCommandPanelVisible.value) return;
  const anchor = worldCommandAnchorRef.value;
  const target = event.target as Node | null;
  if (anchor && target && anchor.contains(target)) {
    return;
  }
  closeWorldCommandPanel();
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
