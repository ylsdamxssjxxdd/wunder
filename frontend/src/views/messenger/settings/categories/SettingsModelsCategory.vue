<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-models"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.models.defaultGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.models.defaultHint') }}</div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-microchip messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settingsPage.models.userDefault') }}</div>
            <div class="messenger-settings-hint">
              {{
                supportsUserDefault
                  ? t('messenger.settingsPage.models.userDefaultHint')
                  : t('chat.composer.modelSetDefaultUnsupported')
              }}
            </div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="messenger-settings-page-lock">
            <i class="fa-solid fa-lock" aria-hidden="true"></i>
            {{ t('messenger.settingsPage.lockedByAdmin') }}
          </span>
          <select
            class="messenger-settings-select"
            disabled
            :value="userDefaultModelName"
            data-testid="settings-user-default-model"
          >
            <option value="">{{ userDefaultModelName || t('chat.composer.modelUnset') }}</option>
          </select>
        </div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-sliders messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('desktop.system.reasoningEffort') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.models.effortUnsupported') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="messenger-settings-page-lock">
            <i class="fa-solid fa-lock" aria-hidden="true"></i>
            {{ t('messenger.settingsPage.serverPending') }}
          </span>
          <select class="messenger-settings-select" disabled data-testid="settings-default-effort">
            <option value="default">{{ t('desktop.system.reasoningEffort.default') }}</option>
          </select>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head messenger-settings-group-head--row">
        <div>
          <div class="messenger-settings-title">{{ t('messenger.settingsPage.models.listGroup') }}</div>
          <div class="messenger-settings-subtitle">
            {{ t('messenger.settingsPage.models.listHint', { count: catalogItems.length }) }}
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <button
            class="messenger-settings-action ghost"
            type="button"
            :disabled="loading"
            @click="reload"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ t('common.refresh') }}</span>
          </button>
        </div>
      </div>

      <div class="messenger-settings-page-toolbar">
        <div class="messenger-settings-frame-search messenger-settings-frame-search--inline">
          <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
          <input
            v-model="keyword"
            type="search"
            :placeholder="t('chat.composer.modelSearch')"
            :aria-label="t('chat.composer.modelSearch')"
          />
        </div>
        <span class="messenger-settings-hint">
          {{ t('messenger.settingsPage.models.pageIndicator', { current: safePage, total: pageCount }) }}
        </span>
      </div>

      <div v-if="loading && !catalogItems.length" class="messenger-list-empty">
        {{ t('common.loading') }}
      </div>
      <div v-else-if="failed && !catalogItems.length" class="messenger-list-empty">
        {{ t('messenger.settingsPage.models.loadFailed') }}
      </div>
      <div v-else-if="!filteredItems.length" class="messenger-list-empty">
        {{ keyword ? t('chat.composer.modelNoMatch') : t('chat.composer.modelEmpty') }}
      </div>
      <ul v-else class="messenger-settings-page-list">
        <li
          v-for="item in pagedItems"
          :key="item.id"
          class="messenger-settings-page-list-item"
        >
          <i class="fa-solid fa-cube messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div class="messenger-settings-page-list-main">
            <div class="messenger-settings-label">{{ item.name }}</div>
            <div class="messenger-settings-hint">
              {{ formatModelMeta(item) }}
            </div>
          </div>
          <span
            v-if="item.id === userDefaultModelName || item.name === userDefaultModelName"
            class="messenger-settings-page-badge"
          >
            {{ t('chat.composer.modelUserDefault', { name: item.name }) }}
          </span>
          <span v-else-if="item.isDefault" class="messenger-settings-page-badge is-muted">
            {{ t('chat.composer.modelDefault') }}
          </span>
        </li>
      </ul>
      <div v-if="pageCount > 1" class="messenger-settings-page-pager">
        <button
          class="messenger-settings-action ghost compact"
          type="button"
          :disabled="safePage <= 1"
          @click="page = safePage - 1"
        >
          {{ t('profile.avatar.pagePrev') }}
        </button>
        <button
          class="messenger-settings-action ghost compact"
          type="button"
          :disabled="safePage >= pageCount"
          @click="page = safePage + 1"
        >
          {{ t('profile.avatar.pageNext') }}
        </button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import {
  composerModelCatalog,
  ensureComposerModelCatalog,
  invalidateComposerModelCatalog,
  type ComposerModelOption
} from '@/components/chat/composerModelCatalog';
import { useI18n } from '@/i18n';

/** 单页行数上限：模型清单最多 200 条（B4 目录上限），分页避免一次性渲染。 */
const PAGE_SIZE = 8;

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const keyword = ref('');
const page = ref(1);
const loading = computed(() => composerModelCatalog.value.loading);
const failed = computed(() => composerModelCatalog.value.failed);
const catalogItems = computed<ComposerModelOption[]>(() => composerModelCatalog.value.items);
const userDefaultModelName = computed(
  () =>
    String(
      composerModelCatalog.value.userDefaultModelName ||
        composerModelCatalog.value.defaultModelName ||
        ''
    )
);
const supportsUserDefault = computed(() => composerModelCatalog.value.supportsUserDefault);

const filteredItems = computed(() => {
  const needle = keyword.value.trim().toLowerCase();
  if (!needle) return catalogItems.value;
  return catalogItems.value.filter(
    (item) =>
      item.name.toLowerCase().includes(needle) || item.id.toLowerCase().includes(needle)
  );
});

const pageCount = computed(() => Math.max(1, Math.ceil(filteredItems.value.length / PAGE_SIZE)));
const safePage = computed(() => Math.min(page.value, pageCount.value));
const pagedItems = computed(() =>
  filteredItems.value.slice((safePage.value - 1) * PAGE_SIZE, safePage.value * PAGE_SIZE)
);

watch(keyword, () => {
  page.value = 1;
});

const formatModelMeta = (item: ComposerModelOption): string => {
  const parts: string[] = [];
  if (item.context) {
    parts.push(t('messenger.settingsPage.models.context', { value: formatContext(item.context) }));
  }
  if (item.source === 'system') parts.push(t('chat.composer.modelSourceSystem'));
  else if (item.source) parts.push(t('chat.composer.modelSourceUser'));
  parts.push(item.id);
  return parts.join(' · ');
};

const formatContext = (value: number): string => {
  if (value >= 1000) {
    const scaled = value / 1000;
    return `${Number.isInteger(scaled) ? scaled : scaled.toFixed(1)}K`;
  }
  return String(value);
};

const reload = () => {
  invalidateComposerModelCatalog();
  void ensureComposerModelCatalog({ force: true });
};

onMounted(() => {
  // 复用 B4 目录缓存：命中缓存不会重复请求，按需挂载时才触发加载。
  void ensureComposerModelCatalog();
});
</script>
