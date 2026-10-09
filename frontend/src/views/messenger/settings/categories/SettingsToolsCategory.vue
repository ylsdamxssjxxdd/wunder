<template>
  <div
    class="messenger-settings-frame-category messenger-settings-tools"
    data-testid="settings-category-tools"
  >
    <!-- 对齐桌面端：工具管理分四个选项卡（全局工具 / MCP 工具 / 技能工具 / 知识库工具）。 -->
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

    <template v-for="tab in TOOLS_TABS" :key="tab.id">
      <section
        v-if="visitedTabs.has(tab.id)"
        v-show="activeTab === tab.id"
        class="messenger-settings-card messenger-settings-tools-card"
      >
        <div class="messenger-settings-group-head messenger-settings-group-head--row">
          <div>
            <div class="messenger-settings-title">{{ t(tab.titleKey) }}</div>
            <div class="messenger-settings-subtitle">
              {{ t('messenger.settingsPage.tools.countHint', { count: toolsFor(tab).length }) }}
            </div>
          </div>
          <div v-if="tab.id === 'builtin'" class="messenger-settings-page-row-actions">
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

        <div v-if="toolsCatalogLoading && !toolsFor(tab).length" class="messenger-list-empty">
          {{ t('common.loading') }}
        </div>
        <div v-else-if="!toolsFor(tab).length" class="messenger-list-empty">
          {{ t('messenger.settingsPage.tools.empty') }}
        </div>
        <div v-else class="messenger-settings-chip-cloud">
          <span
            v-for="tool in toolsFor(tab)"
            :key="tool.name"
            class="messenger-settings-chip"
            :title="tool.description || t('common.noDescription')"
          >
            <AbilityIconBadge
              :name="tool.displayName || tool.name"
              :description="tool.description"
              :kind="tab.kind"
              size="xs"
            />
            <span class="messenger-settings-chip-label">{{ tool.displayName || tool.name }}</span>
          </span>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import type { ToolEntry } from '@/views/messenger/model';
import { useI18n } from '@/i18n';

type ToolsTabId = 'builtin' | 'mcp' | 'skill' | 'knowledge';

const TOOLS_TABS: Array<{ id: ToolsTabId; icon: string; titleKey: string; kind: string }> = [
  { id: 'builtin', icon: 'fa-solid fa-toolbox', titleKey: 'messenger.settingsPage.tools.tabBuiltin', kind: 'tool' },
  { id: 'mcp', icon: 'fa-solid fa-plug', titleKey: 'messenger.settingsPage.tools.tabMcp', kind: 'mcp' },
  { id: 'skill', icon: 'fa-solid fa-book', titleKey: 'messenger.settingsPage.tools.tabSkill', kind: 'skill' },
  { id: 'knowledge', icon: 'fa-solid fa-database', titleKey: 'messenger.settingsPage.tools.tabKnowledge', kind: 'knowledge' }
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

const toolsCatalogLoading = computed(() => Boolean(props.controller.toolsCatalogLoading?.value));

const toolsFor = (tab: { id: ToolsTabId }): ToolEntry[] => {
  switch (tab.id) {
    case 'builtin':
      return props.controller.builtinTools?.value || [];
    case 'mcp':
      return props.controller.mcpTools?.value || [];
    case 'skill':
      return props.controller.skillTools?.value || [];
    case 'knowledge':
      return props.controller.knowledgeTools?.value || [];
  }
};

const reloadCatalog = () => {
  void props.controller.loadToolsCatalog?.({ silent: true });
};

onMounted(() => {
  if (!toolsFor({ id: 'builtin' }).length) {
    reloadCatalog();
  }
});
</script>
