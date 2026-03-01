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
          <svg class="messenger-world-tool-icon" aria-hidden="true">
            <use href="#smiling-face"></use>
          </svg>
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
        <svg class="messenger-world-tool-icon" aria-hidden="true">
          <use href="#file2"></use>
        </svg>
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
      <div class="messenger-world-tool-anchor messenger-world-tool-anchor--history">
        <button
          class="messenger-world-tool-btn"
          type="button"
          :title="t('messenger.world.history')"
          :aria-label="t('messenger.world.history')"
          @click="emit('open-history')"
        >
          <svg class="messenger-world-tool-icon" aria-hidden="true">
            <use href="#history"></use>
          </svg>
        </button>
      </div>
    </div>
    <textarea
      ref="textareaElement"
      v-model.trim="draftModel"
      class="messenger-world-input"
      :placeholder="t('userWorld.input.placeholder')"
      rows="3"
      @focus="emit('focus-input')"
      @keydown.enter="emit('enter', $event)"
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
        <button class="messenger-world-send-menu" type="button" :title="t('messenger.settings.sendKey')">
          <svg class="messenger-world-send-icon messenger-world-send-icon--menu" aria-hidden="true">
            <use href="#down"></use>
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
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps<{
  style: Record<string, string>;
  quickPanelMode: '' | 'emoji';
  recentEmojis: string[];
  emojiCatalog: string[];
  draft: string;
  canSend: boolean;
  uploading: boolean;
}>();

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

const draftModel = computed({
  get: () => props.draft,
  set: (value: string) => emit('update:draft', String(value || '').trim())
});

const getComposerElement = (): HTMLElement | null => composerElement.value;
const getTextareaElement = (): HTMLTextAreaElement | null => textareaElement.value;
const getUploadInputElement = (): HTMLInputElement | null => uploadInputElement.value;

defineExpose({
  getComposerElement,
  getTextareaElement,
  getUploadInputElement
});
</script>
