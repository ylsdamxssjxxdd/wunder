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
        :title="t('userWorld.attachments.pick')"
        :aria-label="t('userWorld.attachments.pick')"
        @click="emit('trigger-container-pick')"
      >
        <i class="fa-solid fa-folder-open messenger-world-tool-fa-icon" aria-hidden="true"></i>
      </button>
      <button
        class="messenger-world-tool-btn"
        type="button"
        :disabled="uploading"
        :title="t('userWorld.attachments.uploadLocal')"
        :aria-label="t('userWorld.attachments.uploadLocal')"
        @click="emit('trigger-upload')"
      >
        <i class="fa-solid fa-paperclip messenger-world-tool-fa-icon" aria-hidden="true"></i>
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
          :disabled="uploading"
          :title="t('chat.attachments.screenshot')"
          :aria-label="t('chat.attachments.screenshot')"
          :aria-expanded="screenshotMenuVisible"
          @click.stop.prevent="toggleScreenshotMenu"
        >
          <i class="fa-solid fa-camera messenger-world-tool-fa-icon" aria-hidden="true"></i>
          <i class="fa-solid fa-chevron-down messenger-world-screenshot-caret" aria-hidden="true"></i>
        </button>
        <div v-if="screenshotMenuVisible" class="messenger-world-screenshot-menu">
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
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

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
  }>(),
  {
    sendKey: 'ctrl_enter',
    screenshotSupported: false
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
const screenshotMenuVisible = ref(false);

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
};

const toggleScreenshotMenu = () => {
  if (!props.screenshotSupported || props.uploading) return;
  screenshotMenuVisible.value = !screenshotMenuVisible.value;
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
  const target = event.target as Node | null;
  if (!anchor || !target || !anchor.contains(target)) {
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
  () => props.screenshotSupported,
  (supported) => {
    if (!supported) {
      closeScreenshotMenu();
    }
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
