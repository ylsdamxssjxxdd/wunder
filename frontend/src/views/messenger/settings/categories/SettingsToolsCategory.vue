<template>
  <div
    class="messenger-settings-frame-category messenger-settings-tools"
    data-testid="settings-category-tools"
  >
    <!-- 对齐桌面端：工具管理恢复选项卡形式（内置工具 / MCP / 知识与资源）。 -->
    <div class="messenger-settings-tools-tabs" role="tablist">
      <button
        v-for="tab in TOOLS_TABS"
        :key="tab.id"
        class="messenger-settings-tools-tab"
        :class="{ 'is-active': activeTab === tab.id }"
        type="button"
        role="tab"
        :aria-selected="activeTab === tab.id"
        :data-tools-tab="tab.id"
        @click="selectTab(tab.id)"
      >
        <i :class="tab.icon" aria-hidden="true"></i>
        <span>{{ t(tab.titleKey) }}</span>
      </button>
    </div>

    <template v-if="activeTab === 'builtin'">
      <section class="messenger-settings-card">
        <div class="messenger-settings-group-head messenger-settings-group-head--row">
          <div>
            <div class="messenger-settings-title">{{ t('toolManager.system.builtin') }}</div>
            <div class="messenger-settings-subtitle">
              {{ t('messenger.settingsPage.tools.builtinHint', { count: builtinTools.length }) }}
            </div>
          </div>
          <div class="messenger-settings-page-row-actions">
            <span class="messenger-settings-page-lock">
              <i class="fa-solid fa-lock" aria-hidden="true"></i>
              {{ t('messenger.settingsPage.lockedByAdmin') }}
            </span>
            <button
              class="messenger-settings-action ghost"
              type="button"
              :disabled="toolsCatalogLoading"
              @click="reloadCatalog"
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
              :placeholder="t('messenger.settingsPage.tools.searchPlaceholder')"
              :aria-label="t('messenger.settingsPage.tools.searchPlaceholder')"
            />
          </div>
          <span class="messenger-settings-hint">
            {{ t('messenger.settingsPage.models.pageIndicator', { current: safePage, total: pageCount }) }}
          </span>
        </div>

        <div v-if="toolsCatalogLoading && !builtinTools.length" class="messenger-list-empty">
          {{ t('common.loading') }}
        </div>
        <div v-else-if="!builtinTools.length" class="messenger-list-empty">
          {{ t('messenger.settingsPage.tools.empty') }}
        </div>
        <div v-else-if="!filteredTools.length" class="messenger-list-empty">
          {{ t('portal.agent.tools.searchEmpty') }}
        </div>
        <ul v-else class="messenger-settings-page-list">
          <li v-for="tool in pagedTools" :key="tool.name" class="messenger-settings-page-list-item">
            <i class="fa-solid fa-wrench messenger-settings-page-row-icon" aria-hidden="true"></i>
            <div class="messenger-settings-page-list-main">
              <div class="messenger-settings-label">{{ tool.displayName || tool.name }}</div>
              <div class="messenger-settings-hint">
                {{ tool.description || t('common.noDescription') }}
              </div>
            </div>
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

      <section class="messenger-settings-card">
        <div class="messenger-settings-group-head">
          <div class="messenger-settings-title">{{ t('messenger.tools.sharedTitle') }}</div>
          <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.tools.sharedHint') }}</div>
        </div>
        <div class="messenger-chat-settings-block">
          <UserSharedToolsPanel />
        </div>
      </section>
    </template>

    <section
      v-if="visitedTabs.has('mcp')"
      v-show="activeTab === 'mcp'"
      class="messenger-settings-card messenger-settings-card--pane"
    >
      <div class="messenger-tools-pane-host user-tools-dialog messenger-settings-tools-host">
        <UserMcpPane />
      </div>
    </section>

    <section
      v-if="visitedTabs.has('skill')"
      v-show="activeTab === 'skill'"
      class="messenger-settings-card messenger-settings-card--pane"
    >
      <div class="messenger-tools-pane-host user-tools-dialog messenger-settings-tools-host">
        <UserSkillPane />
      </div>
    </section>

    <section
      v-if="visitedTabs.has('knowledge')"
      v-show="activeTab === 'knowledge'"
      class="messenger-settings-card messenger-settings-card--pane"
    >
      <div class="messenger-tools-pane-host user-tools-dialog messenger-settings-tools-host">
        <UserKnowledgePane />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import type { ToolEntry } from '@/views/messenger/model';
import { useI18n } from '@/i18n';
import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

/** 用户级工具开关沿用既有面板（真实接口 `/user_tools/shared_tools`）。 */
const UserSharedToolsPanel = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserSharedToolsPanel.vue')
);

/** 选项卡面板：复用旧版自建工具三面板（MCP / 知识库 / 技能），样式见 dialogs/user-tools.css。 */
const UserMcpPane = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserMcpPane.vue')
);
const UserKnowledgePane = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserKnowledgePane.vue')
);
const UserSkillPane = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserSkillPane.vue')
);

type ToolsTabId = 'builtin' | 'mcp' | 'skill' | 'knowledge';

const TOOLS_TABS: Array<{ id: ToolsTabId; icon: string; titleKey: string }> = [
  { id: 'builtin', icon: 'fa-solid fa-toolbox', titleKey: 'messenger.settingsPage.tools.tabBuiltin' },
  { id: 'mcp', icon: 'fa-solid fa-plug', titleKey: 'messenger.settingsPage.tools.tabMcp' },
  { id: 'skill', icon: 'fa-solid fa-book', titleKey: 'messenger.settingsPage.tools.tabSkill' },
  { id: 'knowledge', icon: 'fa-solid fa-database', titleKey: 'messenger.settingsPage.tools.tabKnowledge' }
];

/** 内置工具单页行数：清单由管理员开放，分页 + 搜索避免一次性渲染。 */
const PAGE_SIZE = 8;

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const activeTab = ref<ToolsTabId>('builtin');
/** 面板按需挂载：首次进入的选项卡才创建实例，之后用 v-show 保活避免重复拉取。 */
const visitedTabs = reactive(new Set<ToolsTabId>(['builtin']));

const selectTab = (id: ToolsTabId) => {
  activeTab.value = id;
  visitedTabs.add(id);
};

const keyword = ref('');
const page = ref(1);

const builtinTools = computed<ToolEntry[]>(() => props.controller.builtinTools?.value || []);
const toolsCatalogLoading = computed(() => Boolean(props.controller.toolsCatalogLoading?.value));

const filteredTools = computed(() => {
  const needle = keyword.value.trim().toLowerCase();
  if (!needle) return builtinTools.value;
  return builtinTools.value.filter(
    (tool) =>
      String(tool.name || '').toLowerCase().includes(needle) ||
      String(tool.displayName || '').toLowerCase().includes(needle) ||
      String(tool.description || '').toLowerCase().includes(needle)
  );
});

const pageCount = computed(() => Math.max(1, Math.ceil(filteredTools.value.length / PAGE_SIZE)));
const safePage = computed(() => Math.min(page.value, pageCount.value));
const pagedTools = computed(() =>
  filteredTools.value.slice((safePage.value - 1) * PAGE_SIZE, safePage.value * PAGE_SIZE)
);

watch(keyword, () => {
  page.value = 1;
});

const reloadCatalog = () => {
  void props.controller.loadToolsCatalog?.({ silent: true });
};

onMounted(() => {
  if (!builtinTools.value.length) {
    reloadCatalog();
  }
});
</script>
