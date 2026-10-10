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
            <div class="messenger-settings-title">{{ t('messenger.settingsPage.tools.tabBuiltin') }}</div>
            <div class="messenger-settings-subtitle">
              {{ t('messenger.settingsPage.tools.builtinHint', { count: openToolTotal }) }}
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
        </div>

        <div v-if="toolsCatalogLoading && !openToolTotal" class="messenger-list-empty">
          {{ t('common.loading') }}
        </div>
        <div v-else-if="!openToolTotal" class="messenger-list-empty">
          {{ t('messenger.settingsPage.tools.empty') }}
        </div>
        <div v-else-if="!visibleGroups.length" class="messenger-list-empty">
          {{ t('portal.agent.tools.searchEmpty') }}
        </div>
        <!-- 对齐桌面端「管理员开放工具」：四类工具分区列成条目，说明走悬停标题。 -->
        <div v-else class="messenger-settings-tools-groups">
          <div v-for="group in visibleGroups" :key="group.id" class="messenger-settings-tools-group">
            <div class="messenger-settings-tools-group-head">
              <span class="messenger-settings-tools-group-title">{{ t(group.titleKey) }}</span>
              <span class="messenger-settings-hint">
                {{ t('messenger.settingsPage.tools.countHint', { count: group.tools.length }) }}
              </span>
            </div>
            <div v-if="group.tools.length" class="messenger-settings-chip-cloud">
              <span
                v-for="tool in group.tools"
                :key="tool.name"
                class="messenger-settings-chip"
                :title="tool.description || t('common.noDescription')"
              >
                <AbilityIconBadge
                  :name="tool.displayName || tool.name"
                  :description="tool.description"
                  :group="group.id"
                  :kind="group.kind"
                  size="xs"
                />
                <span class="messenger-settings-chip-label">{{ tool.displayName || tool.name }}</span>
              </span>
            </div>
            <div v-else class="messenger-settings-hint">{{ t('messenger.settingsPage.tools.empty') }}</div>
          </div>
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
        <UserMcpPane :visible="activeTab === 'mcp'" />
      </div>
    </section>

    <section
      v-if="visitedTabs.has('skill')"
      v-show="activeTab === 'skill'"
      class="messenger-settings-card messenger-settings-card--pane"
    >
      <div class="messenger-tools-pane-host user-tools-dialog messenger-settings-tools-host">
        <UserSkillPane :visible="activeTab === 'skill'" :active="activeTab === 'skill'" />
      </div>
    </section>

    <section
      v-if="visitedTabs.has('knowledge')"
      v-show="activeTab === 'knowledge'"
      class="messenger-settings-card messenger-settings-card--pane"
    >
      <div class="messenger-tools-pane-host user-tools-dialog messenger-settings-tools-host">
        <UserKnowledgePane :visible="activeTab === 'knowledge'" />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
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

/** 「全局工具」页签的四类分区，标题与桌面端「管理员开放工具」用同一批文案键。 */
const TOOL_GROUPS: Array<{ id: ToolsTabId; titleKey: string; kind: 'tool' | 'skill' }> = [
  { id: 'builtin', titleKey: 'toolManager.system.builtin', kind: 'tool' },
  { id: 'mcp', titleKey: 'toolManager.system.mcp', kind: 'tool' },
  { id: 'skill', titleKey: 'toolManager.system.skills', kind: 'skill' },
  { id: 'knowledge', titleKey: 'toolManager.system.knowledge', kind: 'tool' }
];

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

const toolsCatalogLoading = computed(() => Boolean(props.controller.toolsCatalogLoading?.value));

const groupTools = (id: ToolsTabId): ToolEntry[] => {
  const lists: Record<ToolsTabId, { value: ToolEntry[] } | undefined> = {
    builtin: props.controller.builtinTools,
    mcp: props.controller.mcpTools,
    skill: props.controller.skillTools,
    knowledge: props.controller.knowledgeTools
  };
  return lists[id]?.value || [];
};

const openToolTotal = computed(() =>
  TOOL_GROUPS.reduce((sum, group) => sum + groupTools(group.id).length, 0)
);

const visibleGroups = computed(() => {
  const needle = keyword.value.trim().toLowerCase();
  const groups = TOOL_GROUPS.map((group) => ({
    ...group,
    tools: needle
      ? groupTools(group.id).filter(
          (tool) =>
            String(tool.name || '').toLowerCase().includes(needle) ||
            String(tool.displayName || '').toLowerCase().includes(needle) ||
            String(tool.description || '').toLowerCase().includes(needle)
        )
      : groupTools(group.id)
  }));
  // 空搜索时四类分区常驻（0 项也说明管理员没开放这一类），带关键词时只留有命中的。
  return needle ? groups.filter((group) => group.tools.length) : groups;
});

const reloadCatalog = () => {
  void props.controller.loadToolsCatalog?.({ silent: true });
};

onMounted(() => {
  if (!openToolTotal.value) {
    reloadCatalog();
  }
});
</script>
