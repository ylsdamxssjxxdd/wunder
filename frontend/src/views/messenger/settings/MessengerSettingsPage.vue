<template>
  <div class="messenger-settings-frame">
    <nav class="messenger-settings-frame-nav" :aria-label="t('messenger.sidebar.settings')">
      <div class="messenger-settings-frame-search">
        <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
        <input
          v-model="keyword"
          type="search"
          data-testid="settings-search"
          :placeholder="t('messenger.settingsPage.search')"
          :aria-label="t('messenger.settingsPage.search')"
          @keydown.esc.prevent="keyword = ''"
        />
      </div>

      <div class="messenger-settings-frame-nav-list" role="tablist" data-testid="settings-nav">
        <button
          v-for="item in visibleCategories"
          :key="item.id"
          class="messenger-settings-frame-nav-item"
          :class="{ 'is-active': item.id === activeCategory }"
          type="button"
          role="tab"
          :aria-selected="item.id === activeCategory"
          :data-settings-category="item.id"
          @click="selectCategory(item.id)"
        >
          <i :class="item.icon" class="messenger-settings-frame-nav-icon" aria-hidden="true"></i>
          <span class="messenger-settings-frame-nav-text">
            <span class="messenger-settings-frame-nav-title">{{ t(item.titleKey) }}</span>
            <span class="messenger-settings-frame-nav-desc">{{ t(item.descKey) }}</span>
          </span>
        </button>
        <div v-if="!visibleCategories.length" class="messenger-settings-frame-nav-empty">
          {{ t('messenger.settingsPage.searchEmpty') }}
        </div>
      </div>
    </nav>

    <section class="messenger-settings-frame-content" data-testid="settings-content">
      <div class="messenger-settings-frame-content-body">
        <!-- 分类内容按需挂载：只有被选中过的分类才会创建实例，最多保留 3 个。 -->
        <KeepAlive :max="SETTINGS_KEEP_ALIVE_MAX">
          <component
            :is="activeComponent"
            :key="activeCategory"
            :controller="controller"
            :category-active="true"
            @open-agent-chat="emit('open-agent-chat')"
          />
        </KeepAlive>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';

import {
  filterSettingsCategories,
  type SettingsCategoryId
} from './settingsCategories';
import { SETTINGS_CATEGORY_COMPONENTS } from './settingsCategoryComponents';

/** 保留最近访问的 3 个分类实例；其它分类释放，避免 12 个面板同时常驻。 */
const SETTINGS_KEEP_ALIVE_MAX = 3;

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ close: []; 'open-agent-chat': [] }>();

const { t } = useI18n();

const AGENT_MODE_BY_CATEGORY: Partial<Record<SettingsCategoryId, string>> = {
  agent: 'agent',
  cron: 'cron',
  memory: 'memory',
  channels: 'channel',
  runtime: 'runtime',
  archived: 'archived'
};

const CATEGORY_BY_AGENT_MODE: Record<string, SettingsCategoryId> = {
  agent: 'agent',
  cron: 'cron',
  memory: 'memory',
  channel: 'channels',
  runtime: 'runtime',
  archived: 'archived'
};

const categoryByPanelMode = (mode: string): SettingsCategoryId | null => {
  if (mode === 'prompts') return 'prompts';
  if (mode === 'help-manual') return 'help';
  if (mode === 'profile') return 'account';
  if (mode === 'desktop-models') return 'models';
  if (mode === 'general') return 'general';
  return null;
};

const activeSection = computed(() => String(props.controller.sessionHub?.activeSection || ''));
const controllerPanelMode = computed(() => String(props.controller.settingsPanelMode?.value || ''));
const controllerAgentMode = computed(() => String(props.controller.agentSettingMode?.value || ''));

/** 由控制器状态（左栏设置入口 / 顶栏帮助 / 资料 / 聊天页动作）推导初始分类。 */
const resolveCategoryFromController = (): SettingsCategoryId => {
  if (activeSection.value === 'agents') {
    const mapped = CATEGORY_BY_AGENT_MODE[controllerAgentMode.value];
    if (mapped) return mapped;
  }
  return categoryByPanelMode(controllerPanelMode.value) || 'general';
};

const activeCategory = ref<SettingsCategoryId>(resolveCategoryFromController());

// 只在「控制器状态由外部改变」时跟随，避免把用户在当前页选的分类拉回去。
let lastPushedPanelMode = '';
let lastPushedAgentMode = '';

const pushControllerState = (category: SettingsCategoryId) => {
  const agentMode = AGENT_MODE_BY_CATEGORY[category];
  if (agentMode) {
    lastPushedAgentMode = agentMode;
    if (props.controller.agentSettingMode) {
      props.controller.agentSettingMode.value = agentMode;
    }
  }
  const panelMode =
    category === 'prompts'
      ? 'prompts'
      : category === 'help'
        ? 'help-manual'
        : category === 'account'
          ? 'profile'
          : category === 'general'
            ? 'general'
            : '';
  if (!panelMode) return;
  lastPushedPanelMode = panelMode;
  if (props.controller.settingsPanelMode) {
    props.controller.settingsPanelMode.value = panelMode;
  }
};

watch(controllerPanelMode, (mode) => {
  if (mode === lastPushedPanelMode) return;
  const mapped = categoryByPanelMode(mode);
  if (mapped) activeCategory.value = mapped;
});

watch(controllerAgentMode, (mode) => {
  if (mode === lastPushedAgentMode) return;
  if (activeSection.value !== 'agents') return;
  const mapped = CATEGORY_BY_AGENT_MODE[mode];
  if (mapped) activeCategory.value = mapped;
});

const keyword = ref('');
const visibleCategories = computed(() => filterSettingsCategories(keyword.value, t));

const activeComponent = computed(() => SETTINGS_CATEGORY_COMPONENTS[activeCategory.value]);

const selectCategory = (id: SettingsCategoryId) => {
  activeCategory.value = id;
  pushControllerState(id);
};

defineExpose({ activeCategory });
</script>
