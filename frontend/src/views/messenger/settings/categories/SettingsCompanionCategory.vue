<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-companion"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.companion.behaviorGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.companion.behaviorHint') }}</div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-paw messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('companions.setting.enabled') }}</div>
            <div class="messenger-settings-hint">{{ t('companions.setting.enabledHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <label class="messenger-settings-switch">
            <input
              type="checkbox"
              data-testid="settings-companion-enabled"
              :checked="settings.enabled === true"
              @change="handleEnabledChange"
            />
            <span></span>
          </label>
        </div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-comment-dots messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('companions.setting.messageHints') }}</div>
            <div class="messenger-settings-hint">{{ t('companions.setting.messageHintsHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <label class="messenger-settings-switch">
            <input
              type="checkbox"
              :checked="settings.messageHintsEnabled !== false"
              @change="handleMessageHintsChange"
            />
            <span></span>
          </label>
        </div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-up-right-and-down-left-from-center messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('companions.scale') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.companion.scaleHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="messenger-settings-page-value">{{ scaleText }}</span>
          <input
            class="messenger-settings-range messenger-settings-page-range"
            type="range"
            min="0.5"
            max="1.6"
            step="0.1"
            :value="settings.scale"
            data-testid="settings-companion-scale"
            @change="handleScaleChange"
          />
        </div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-location-crosshairs messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settingsPage.companion.position') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.companion.positionHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <label class="messenger-settings-page-field">
            <span>X</span>
            <input
              class="messenger-settings-page-number"
              type="number"
              min="0"
              :value="settings.position.x"
              @change="handlePositionChange('x', $event)"
            />
          </label>
          <label class="messenger-settings-page-field">
            <span>Y</span>
            <input
              class="messenger-settings-page-number"
              type="number"
              min="0"
              :value="settings.position.y"
              @change="handlePositionChange('y', $event)"
            />
          </label>
          <button
            class="messenger-settings-action ghost"
            type="button"
            data-testid="settings-companion-reset-position"
            @click="handleResetPosition"
          >
            {{ t('common.reset') }}
          </button>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head messenger-settings-group-head--row">
        <div>
          <div class="messenger-settings-title">{{ t('companions.library') }}</div>
          <div class="messenger-settings-subtitle">
            {{ t('messenger.settingsPage.companion.libraryHint', { count: libraryItems.length }) }}
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <button
            class="messenger-settings-action ghost"
            type="button"
            :disabled="globalLoading"
            @click="reloadLibrary"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ t('common.refresh') }}</span>
          </button>
          <button class="messenger-settings-action ghost" type="button" :disabled="importing" @click="triggerImport">
            <i class="fa-solid fa-file-import" aria-hidden="true"></i>
            <span>{{ t('companions.import') }}</span>
          </button>
          <input
            ref="fileInputRef"
            class="messenger-settings-page-file-input"
            type="file"
            accept=".zip,.wunder-companion,application/zip"
            @change="handleImportFile"
          />
        </div>
      </div>

      <div v-if="loading && !libraryItems.length" class="messenger-list-empty">
        {{ t('common.loading') }}
      </div>
      <div v-else-if="!libraryItems.length" class="messenger-list-empty">
        {{ t('companions.emptyDetail') }}
      </div>
      <template v-else>
        <ul class="messenger-settings-page-grid">
          <li
            v-for="item in pagedLibraryItems"
            :key="item.id"
            class="messenger-settings-page-grid-item"
            :class="{ 'is-active': item.id === settings.selectedId }"
          >
            <span class="messenger-settings-page-grid-preview" aria-hidden="true">
              <CompanionSprite
                :source="item.spritesheetDataUrl || item.spritesheetUrl || ''"
                :state="PREVIEW_SPRITE_STATE"
                fit
                paused
              />
            </span>
            <div class="messenger-settings-page-grid-meta">
              <div class="messenger-settings-label" :title="item.displayName">{{ item.displayName }}</div>
              <div class="messenger-settings-hint">
                {{ item.scope === 'global' ? t('portal.agent.companion.sourceGlobal') : t('portal.agent.companion.sourcePrivate') }}
              </div>
            </div>
            <button
              class="messenger-settings-action ghost compact"
              type="button"
              :disabled="item.scope === 'global' || item.id === settings.selectedId"
              :title="item.scope === 'global' ? t('messenger.settingsPage.companion.globalBindHint') : ''"
              @click="selectCompanion(item.id)"
            >
              {{ item.id === settings.selectedId ? t('chat.composer.modelCurrent') : t('companions.use') }}
            </button>
          </li>
        </ul>
        <div v-if="libraryPageCount > 1" class="messenger-settings-page-pager">
          <button
            class="messenger-settings-action ghost compact"
            type="button"
            :disabled="libraryPage <= 1"
            @click="libraryPage = libraryPage - 1"
          >
            {{ t('profile.avatar.pagePrev') }}
          </button>
          <span class="messenger-settings-hint">
            {{ t('profile.avatar.pageIndicator', { current: libraryPage, total: libraryPageCount }) }}
          </span>
          <button
            class="messenger-settings-action ghost compact"
            type="button"
            :disabled="libraryPage >= libraryPageCount"
            @click="libraryPage = libraryPage + 1"
          >
            {{ t('profile.avatar.pageNext') }}
          </button>
        </div>
      </template>

      <div class="messenger-settings-hint messenger-settings-page-note">
        {{ t('messenger.settingsPage.companion.bindHint') }}
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import {
  COMPANION_LAYOUT_RESET_EVENT,
  useCompanionStore,
  type CompanionPackageRecord
} from '@/stores/companions';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { parseAgentAvatarIconConfig } from '@/utils/agentAvatar';
import { showApiError } from '@/utils/apiError';

/** 形象库单页数量：避免一次性渲染大图帧。 */
const LIBRARY_PAGE_SIZE = 6;
const PREVIEW_SPRITE_STATE = 'idle' as const;
const DEFAULT_POSITION = { x: 28, y: 28 };

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();
const companionStore = useCompanionStore();
const agentStore = useAgentStore();

const settings = computed(() => companionStore.settings);
const loading = computed(() => companionStore.loading || companionStore.globalCompanionsLoading);
const globalLoading = computed(() => companionStore.globalCompanionsLoading);
const importing = ref(false);
const libraryPage = ref(1);
const fileInputRef = ref<HTMLInputElement | null>(null);

const libraryItems = computed<CompanionPackageRecord[]>(() => [
  ...(companionStore.companions || []),
  ...(companionStore.globalCompanions || []).filter(
    (item) => !(companionStore.companions || []).some((local) => local.id === item.id)
  )
]);

const libraryPageCount = computed(() =>
  Math.max(1, Math.ceil(libraryItems.value.length / LIBRARY_PAGE_SIZE))
);
const pagedLibraryItems = computed(() =>
  libraryItems.value.slice(
    (libraryPage.value - 1) * LIBRARY_PAGE_SIZE,
    libraryPage.value * LIBRARY_PAGE_SIZE
  )
);

const scaleText = computed(() => `${Number(settings.value.scale || 1).toFixed(1)}x`);

const loadMissingPreviews = () => {
  pagedLibraryItems.value.forEach((item) => {
    if (item.scope !== 'global' || item.spritesheetDataUrl) return;
    void companionStore.ensureGlobalCompanion(item.id).catch(() => undefined);
  });
};

watch(libraryPage, () => {
  loadMissingPreviews();
});

const handleEnabledChange = (event: Event) => {
  companionStore.setEnabled((event.target as HTMLInputElement).checked === true);
};

const handleMessageHintsChange = (event: Event) => {
  companionStore.setMessageHintsEnabled((event.target as HTMLInputElement).checked === true);
};

const handleScaleChange = (event: Event) => {
  const next = Math.min(1.6, Math.max(0.5, Number((event.target as HTMLInputElement).value) || 1));
  companionStore.setScale(next);
  // 已绑定桌宠的智能体同步到同一缩放，避免设置页数值与实际显示不一致。
  const agentIds = new Set<string>(Object.keys(companionStore.agentOverrides || {}));
  agentStore.agents.forEach((agent) => {
    const agentId = String(agent?.id || '').trim();
    if (!agentId) return;
    if (parseAgentAvatarIconConfig(agent.icon).kind === 'companion') {
      agentIds.add(agentId);
    }
  });
  agentIds.forEach((agentId) => {
    companionStore.setAgentOverride(agentId, { scale: next });
  });
};

const handlePositionChange = (axis: 'x' | 'y', event: Event) => {
  const raw = Number((event.target as HTMLInputElement).value);
  const value = Number.isFinite(raw) && raw >= 0 ? Math.round(raw) : DEFAULT_POSITION[axis];
  companionStore.setPosition({
    x: axis === 'x' ? value : settings.value.position.x,
    y: axis === 'y' ? value : settings.value.position.y
  });
};

const handleResetPosition = () => {
  companionStore.setPosition({ ...DEFAULT_POSITION });
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent(COMPANION_LAYOUT_RESET_EVENT));
  }
  ElMessage.success(t('messenger.settingsPage.companion.positionReset'));
};

const selectCompanion = (id: string) => {
  companionStore.selectCompanion(id);
  ElMessage.success(t('companions.enabledMessage', { name: id }));
};

const reloadLibrary = () => {
  libraryPage.value = 1;
  void companionStore.loadGlobalCompanions({ force: true }).catch(() => undefined);
};

const triggerImport = () => {
  fileInputRef.value?.click();
};

const handleImportFile = async (event: Event) => {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = '';
  if (!file || importing.value) return;
  importing.value = true;
  try {
    const record = await companionStore.importPackage(file);
    libraryPage.value = 1;
    ElMessage.success(t('companions.importSuccess', { name: record.displayName }));
  } catch (error) {
    showApiError(error, t('companions.importFailed', { message: t('common.requestFailed') }));
  } finally {
    importing.value = false;
  }
};

onMounted(() => {
  void companionStore.hydrate().catch(() => undefined);
  void companionStore.loadGlobalCompanions().catch(() => undefined);
  loadMissingPreviews();
});
</script>
