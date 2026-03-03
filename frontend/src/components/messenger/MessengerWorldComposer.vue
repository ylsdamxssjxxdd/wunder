<template>
  <div
    ref="composerElement"
    class="messenger-world-composer"
    :style="style"
  >
    <button
      class="messenger-world-resize-edge"
      type="button"
      :title="t('messenger.world.resize')"
      :aria-label="t('messenger.world.resize')"
      @mousedown.prevent="emit('resize-mousedown', $event)"
    >
      <span class="messenger-world-resize-grip"></span>
    </button>
    <div class="messenger-world-toolbar">
      <div
        class="messenger-world-tool-anchor messenger-world-tool-anchor--emoji"
        @mouseenter="emit('open-quick-panel', 'emoji')"
        @mouseleave="emit('schedule-quick-panel-close')"
      >
        <button
          class="messenger-world-tool-btn"
          type="button"
          :class="{ active: quickPanelMode === 'emoji' }"
          :title="t('messenger.world.emoji')"
          :aria-label="t('messenger.world.emoji')"
          @click.prevent="emit('toggle-quick-panel', 'emoji')"
        >
          <i class="fa-solid fa-face-smile messenger-world-tool-fa-icon" aria-hidden="true"></i>
        </button>
        <div
          v-if="quickPanelMode === 'emoji'"
          class="messenger-world-pop-panel messenger-world-emoji-panel"
          @mouseenter="emit('clear-quick-panel-close')"
          @mouseleave="emit('schedule-quick-panel-close')"
        >
          <div v-if="recentEmojis.length" class="messenger-world-emoji-section">
            <div class="messenger-world-quick-title">{{ t('messenger.world.quick.recent') }}</div>
            <div class="messenger-world-emoji-grid">
              <button
                v-for="emoji in recentEmojis"
                :key="`recent-${emoji}`"
                class="messenger-world-emoji-item"
                type="button"
                @click="emit('insert-emoji', emoji)"
              >
                {{ emoji }}
              </button>
            </div>
          </div>
          <div class="messenger-world-emoji-section">
            <div class="messenger-world-quick-title">{{ t('messenger.world.quick.all') }}</div>
            <div class="messenger-world-emoji-grid">
              <button
                v-for="emoji in emojiCatalog"
                :key="`catalog-${emoji}`"
                class="messenger-world-emoji-item"
                type="button"
                @click="emit('insert-emoji', emoji)"
              >
                {{ emoji }}
              </button>
            </div>
          </div>
        </div>
      </div>
      <button
        class="messenger-world-tool-btn"
        type="button"
        :disabled="voiceRecording"
        :title="t('userWorld.attachments.pick')"
        :aria-label="t('userWorld.attachments.pick')"
        @click="emit('trigger-container-pick')"
      >
        <i class="fa-solid fa-folder-open messenger-world-tool-fa-icon" aria-hidden="true"></i>
      </button>
      <button
        class="messenger-world-tool-btn"
        type="button"
        :disabled="uploading || voiceRecording"
        :title="t('userWorld.attachments.uploadLocal')"
        :aria-label="t('userWorld.attachments.uploadLocal')"
        @click="emit('trigger-upload')"
      >
        <i class="fa-solid fa-paperclip messenger-world-tool-fa-icon" aria-hidden="true"></i>
      </button>
      <button
        class="messenger-world-tool-btn"
        type="button"
        :class="{
          active: voiceRecording,
          'messenger-world-tool-btn--recording': voiceRecording
        }"
        :disabled="uploading || !voiceSupported"
        :title="voiceButtonTitle"
        :aria-label="voiceButtonTitle"
        @click="emit('toggle-voice-record')"
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
        v-if="screenshotSupported"
        ref="screenshotMenuAnchorRef"
        class="messenger-world-tool-anchor messenger-world-tool-anchor--screenshot"
        :class="{ 'is-open': screenshotMenuVisible }"
      >
        <button
          class="messenger-world-tool-btn messenger-world-screenshot-toggle"
          type="button"
          :class="{ active: screenshotMenuVisible }"
          :disabled="uploading || voiceRecording"
          :title="t('chat.attachments.screenshot')"
          :aria-label="t('chat.attachments.screenshot')"
          :aria-expanded="screenshotMenuVisible"
          @click.stop.prevent="toggleScreenshotMenu"
        >
          <i class="fa-solid fa-camera messenger-world-tool-fa-icon" aria-hidden="true"></i>
          <i class="fa-solid fa-chevron-down messenger-world-screenshot-caret" aria-hidden="true"></i>
        </button>
      </div>
      <div class="messenger-world-tool-anchor messenger-world-tool-anchor--history">
        <button
          class="messenger-world-tool-btn"
          type="button"
          :title="t('messenger.world.history')"
          :aria-label="t('messenger.world.history')"
          @click="emit('open-history')"
        >
          <i class="fa-solid fa-clock-rotate-left messenger-world-tool-fa-icon" aria-hidden="true"></i>
        </button>
      </div>
      <div v-if="voiceRecording" class="messenger-world-voice-indicator">
        <i class="fa-solid fa-circle messenger-world-voice-indicator-dot" aria-hidden="true"></i>
        <span>{{ voiceRecordingLabel }}</span>
      </div>
    </div>
    <textarea
      ref="textareaElement"
      v-model="draftModel"
      class="messenger-world-input"
      :placeholder="inputPlaceholder"
      rows="3"
      @focus="emit('focus-input')"
      @keydown="handleTextareaKeydown"
    ></textarea>
    <div class="messenger-world-footer">
      <div class="messenger-world-send-group">
        <button
          class="messenger-world-send-main"
          type="button"
          :disabled="!canSend"
          @click="emit('send')"
        >
          <svg class="messenger-world-send-icon" aria-hidden="true">
            <use href="#send"></use>
          </svg>
        </button>
      </div>
    </div>
    <input
      ref="uploadInputElement"
      type="file"
      multiple
      hidden
      @change="emit('upload-change', $event)"
    />

    <Teleport to="body">
      <div
        v-if="screenshotSupported && screenshotMenuVisible"
        ref="screenshotMenuPanelRef"
        class="messenger-world-screenshot-menu messenger-world-screenshot-menu--floating"
        :style="screenshotMenuStyle"
      >
        <button
          v-for="option in screenshotCaptureOptions"
          :key="option.key"
          class="messenger-world-screenshot-menu-item"
          type="button"
          @click="selectScreenshotOption(option)"
        >
          <span class="messenger-world-screenshot-menu-item-main">
            <i :class="[option.icon, 'messenger-world-screenshot-menu-item-icon']" aria-hidden="true"></i>
            <span>{{ option.label }}</span>
          </span>
        </button>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

type SendKeyMode = 'enter' | 'ctrl_enter' | 'none';
type ScreenshotCapturePayload = {
  hideWindow: boolean;
  region: boolean;
};

type ScreenshotCaptureOption = ScreenshotCapturePayload & {
  key: string;
  icon: string;
  label: string;
};

const props = withDefaults(
  defineProps<{
    style: Record<string, string>;
    quickPanelMode: '' | 'emoji';
    recentEmojis: string[];
    emojiCatalog: string[];
    draft: string;
    canSend: boolean;
    uploading: boolean;
    screenshotSupported?: boolean;
    sendKey?: SendKeyMode;
    voiceRecording?: boolean;
    voiceDurationMs?: number;
    voiceSupported?: boolean;
  }>(),
  {
    sendKey: 'ctrl_enter',
    screenshotSupported: false,
    voiceRecording: false,
    voiceDurationMs: 0,
    voiceSupported: true
  }
);

const emit = defineEmits<{
  'update:draft': [value: string];
  'resize-mousedown': [event: MouseEvent];
  'open-quick-panel': [mode: 'emoji'];
  'toggle-quick-panel': [mode: 'emoji'];
  'clear-quick-panel-close': [];
  'schedule-quick-panel-close': [];
  'insert-emoji': [emoji: string];
  'trigger-container-pick': [];
  'trigger-upload': [];
  'toggle-voice-record': [];
  'trigger-screenshot': [payload: ScreenshotCapturePayload];
  'open-history': [];
  'focus-input': [];
  enter: [event: KeyboardEvent];
  send: [];
  'upload-change': [event: Event];
}>();

const { t } = useI18n();
const composerElement = ref<HTMLElement | null>(null);
const textareaElement = ref<HTMLTextAreaElement | null>(null);
const uploadInputElement = ref<HTMLInputElement | null>(null);
const screenshotMenuAnchorRef = ref<HTMLElement | null>(null);
const screenshotMenuPanelRef = ref<HTMLElement | null>(null);
const screenshotMenuVisible = ref(false);
const screenshotMenuStyle = ref<Record<string, string>>({});

const draftModel = computed({
  get: () => props.draft,
  set: (value: string) => emit('update:draft', String(value || ''))
});
const sendShortcutHint = computed(() => {
  if (props.sendKey === 'ctrl_enter') return t('chat.input.sendHintCtrlEnterAlt');
  if (props.sendKey === 'enter') return t('chat.input.sendHintEnterAlt');
  return '';
});
const inputPlaceholder = computed(
  () =>
    sendShortcutHint.value
      ? `${t('userWorld.input.placeholder')} | ${sendShortcutHint.value}`
      : t('userWorld.input.placeholder')
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
  if (!props.voiceSupported) {
    return t('messenger.world.voice.unsupported');
  }
  return props.voiceRecording ? t('messenger.world.voice.stop') : t('messenger.world.voice.start');
});
const voiceRecordingLabel = computed(() =>
  t('messenger.world.voice.recording', {
    duration: formatVoiceDurationLabel(props.voiceDurationMs)
  })
);
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
const handleTextareaKeydown = (event: KeyboardEvent) => {
  if (event.isComposing) {
    return;
  }
  if (isEnterKeyboardEvent(event)) {
    emit('enter', event);
  }
};

const closeScreenshotMenu = () => {
  screenshotMenuVisible.value = false;
  screenshotMenuStyle.value = {};
};

const toggleScreenshotMenu = () => {
  if (!props.screenshotSupported || props.uploading || props.voiceRecording) return;
  const nextVisible = !screenshotMenuVisible.value;
  screenshotMenuVisible.value = nextVisible;
  if (nextVisible) {
    void syncScreenshotMenuPosition();
  }
};

const selectScreenshotOption = (option: ScreenshotCaptureOption) => {
  closeScreenshotMenu();
  emit('trigger-screenshot', {
    hideWindow: option.hideWindow,
    region: option.region
  });
};

const handleDocumentPointerDown = (event: PointerEvent) => {
  if (!screenshotMenuVisible.value) return;
  const anchor = screenshotMenuAnchorRef.value;
  const panel = screenshotMenuPanelRef.value;
  const target = event.target as Node | null;
  const isInsideAnchor = Boolean(anchor && target && anchor.contains(target));
  const isInsidePanel = Boolean(panel && target && panel.contains(target));
  if (!isInsideAnchor && !isInsidePanel) {
    closeScreenshotMenu();
  }
};

const handleDocumentKeydown = (event: KeyboardEvent) => {
  if (!screenshotMenuVisible.value || event.key !== 'Escape') return;
  event.preventDefault();
  closeScreenshotMenu();
};

onMounted(() => {
  if (typeof document === 'undefined') return;
  document.addEventListener('pointerdown', handleDocumentPointerDown);
  document.addEventListener('keydown', handleDocumentKeydown, true);
});

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') {
    window.removeEventListener('resize', handleScreenshotMenuViewportChange);
    window.removeEventListener('scroll', handleScreenshotMenuViewportChange, true);
  }
  if (typeof document === 'undefined') return;
  document.removeEventListener('pointerdown', handleDocumentPointerDown);
  document.removeEventListener('keydown', handleDocumentKeydown, true);
});

watch(
  () => props.uploading,
  (uploading) => {
    if (uploading) {
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
  () => props.screenshotSupported,
  (supported) => {
    if (!supported) {
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

const getComposerElement = (): HTMLElement | null => composerElement.value;
const getTextareaElement = (): HTMLTextAreaElement | null => textareaElement.value;
const getUploadInputElement = (): HTMLInputElement | null => uploadInputElement.value;

defineExpose({
  getComposerElement,
  getTextareaElement,
  getUploadInputElement
});
</script>
