<template>
  <div class="messenger-settings-page">
    <template v-if="mode === 'profile'">
      <section class="messenger-settings-card">
        <div class="messenger-settings-profile-head">
          <div class="messenger-settings-profile-avatar">{{ profileInitial }}</div>
          <div class="messenger-settings-profile-meta">
            <div class="messenger-settings-profile-name">{{ username || t('user.guest') }}</div>
            <div class="messenger-settings-profile-id">{{ userId || '-' }}</div>
          </div>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.more.language') }}</div>
            <div class="messenger-settings-hint">{{ languageLabel }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('toggle-language')">
            {{ t('messenger.more.language') }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('nav.logout') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.more.logoutConfirm') }}</div>
          </div>
          <button class="messenger-settings-action danger" type="button" @click="$emit('logout')">
            {{ t('nav.logout') }}
          </button>
        </div>
      </section>
    </template>

    <template v-else>
      <section class="messenger-settings-card">
        <div class="messenger-settings-head">
          <div>
            <div class="messenger-settings-title">{{ t('messenger.settings.versionTitle') }}</div>
            <div class="messenger-settings-subtitle">{{ t('messenger.settings.versionHint') }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('check-update')">
            <i class="fa-solid fa-rotate" aria-hidden="true"></i>
            <span>{{ t('messenger.settings.checkUpdate') }}</span>
          </button>
        </div>
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('messenger.settings.sendKey') }}</div>
          <select v-model="sendKey" class="messenger-settings-select">
            <option value="enter">Enter</option>
            <option value="ctrl_enter">Ctrl + Enter</option>
          </select>
        </div>
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('messenger.settings.language') }}</div>
          <button class="messenger-settings-select-like" type="button" @click="$emit('toggle-language')">
            {{ languageLabel }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.fontSize') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.fontHint') }}</div>
          </div>
          <div class="messenger-settings-range-wrap">
            <div class="messenger-settings-stepper">
              <button type="button" @click="fontSize = Math.max(12, fontSize - 1)">-</button>
              <span>{{ fontSize }}</span>
              <button type="button" @click="fontSize = Math.min(20, fontSize + 1)">+</button>
            </div>
            <input v-model.number="fontSize" class="messenger-settings-range" type="range" min="12" max="20" />
          </div>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.autoTitle') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.autoTitleHint') }}</div>
          </div>
          <label class="messenger-settings-switch">
            <input v-model="autoTitle" type="checkbox" />
            <span></span>
          </label>
        </div>
      </section>

      <section class="messenger-settings-card">
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.cloudData') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.cloudHint') }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('cloud-config')">
            {{ t('common.setting') }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.localData') }}</div>
            <div class="messenger-settings-hint">
              {{ t('messenger.settings.localHint', statsPayload) }}
            </div>
          </div>
          <div class="messenger-settings-actions">
            <button class="messenger-settings-action ghost" type="button" @click="$emit('import-data')">
              {{ t('common.import') }}
            </button>
            <button class="messenger-settings-action ghost" type="button" @click="$emit('export-data')">
              {{ t('common.export') }}
            </button>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    mode?: 'general' | 'profile';
    username?: string;
    userId?: string;
    languageLabel?: string;
    sessionCount?: number;
    messageCount?: number;
    promptCount?: number;
    personaCount?: number;
  }>(),
  {
    mode: 'general',
    username: '',
    userId: '',
    languageLabel: '',
    sessionCount: 0,
    messageCount: 0,
    promptCount: 0,
    personaCount: 0
  }
);

defineEmits<{
  (event: 'toggle-language'): void;
  (event: 'logout'): void;
  (event: 'check-update'): void;
  (event: 'cloud-config'): void;
  (event: 'import-data'): void;
  (event: 'export-data'): void;
}>();

const { t } = useI18n();
const sendKey = ref('enter');
const fontSize = ref(14);
const autoTitle = ref(false);

const profileInitial = computed(() => {
  const source = String(props.username || '').trim();
  if (!source) return '?';
  return source.slice(0, 1).toUpperCase();
});

const statsPayload = computed(() => ({
  sessions: props.sessionCount,
  messages: props.messageCount,
  prompts: props.promptCount,
  personas: props.personaCount
}));
</script>
