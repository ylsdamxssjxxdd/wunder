<template>
  <div class="messenger-tools-section">
    <div v-if="toolsCatalogLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
    <template v-else-if="selectedToolCategory === 'admin'">
      <div class="messenger-entity-panel messenger-entity-panel--fill">
        <div class="messenger-entity-title">{{ t('messenger.tools.adminTitle') }}</div>
        <div class="messenger-entity-meta">{{ t('messenger.tools.adminDesc') }}</div>
        <div class="messenger-tools-admin-groups">
          <section
            v-for="group in adminToolGroups"
            :key="`admin-tool-group-${group.key}`"
            class="messenger-tools-admin-group"
          >
            <div class="messenger-entity-meta messenger-tools-admin-group-title">
              {{ group.title }}
            </div>
            <div class="messenger-tool-tag-list">
              <template v-for="item in group.items" :key="`tool-admin-${group.key}-${item.name}`">
                <el-tooltip
                  placement="top-start"
                  :show-after="120"
                  popper-class="ability-card-popper"
                >
                  <template #content>
                    <AbilityTooltipCard
                      :name="resolveAdminToolDisplayName(item)"
                      :hint="resolveAdminToolDetail(item)"
                      :description="item.description"
                      :show-detail="false"
                      :kind="group.key === 'skills' ? 'skill' : 'tool'"
                      :group="group.key"
                      :chips="[group.title]"
                    />
                  </template>
                  <span class="messenger-tool-tag messenger-tool-tag--detail">
                    <AbilityIconBadge
                      :name="resolveAdminToolDisplayName(item)"
                      :description="item.description"
                      :hint="resolveAdminToolDetail(item)"
                      :kind="group.key === 'skills' ? 'skill' : 'tool'"
                      :group="group.key"
                      size="xs"
                    />
                    <span class="messenger-tool-tag-text">{{ resolveAdminToolDisplayName(item) }}</span>
                  </span>
                </el-tooltip>
              </template>
              <span v-if="!group.items.length" class="messenger-list-empty">
                {{ t('common.none') }}
              </span>
            </div>
          </section>
        </div>
      </div>
    </template>
    <template v-else-if="activeToolPaneComponent">
      <div class="messenger-tools-pane-host user-tools-dialog">
        <KeepAlive>
          <component
            :is="activeToolPaneComponent"
            :visible="true"
            :active="true"
            :status="toolPaneStatus"
            @status="handleToolPaneStatus"
            @loading-change="handleToolPaneLoadingChange"
          />
        </KeepAlive>
      </div>
    </template>
    <div v-else class="messenger-list-empty">{{ t('messenger.empty.selectTool') }}</div>

    <HoneycombWaitingOverlay
      :visible="showToolsWaitingOverlay"
      :title="t('messenger.waiting.title')"
      :target-name="toolsWaitingTargetName"
      :phase-label="toolsWaitingPhaseLabel"
      :summary-label="t('messenger.waiting.summary.tools')"
      :progress="toolsWaitingProgress"
      :teleport-to-body="false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
import AbilityTooltipCard from '@/components/common/AbilityTooltipCard.vue';
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import { useI18n } from '@/i18n';
import { UserKnowledgePane, UserMcpPane, UserSkillPane } from '@/views/messenger/lazyPanels';
import type { ToolEntry } from '@/views/messenger/model';

type ToolCategory = 'admin' | 'mcp' | 'skills' | 'knowledge' | '';

type AdminToolGroup = {
  key: string;
  title: string;
  items: ToolEntry[];
};

const props = defineProps<{
  toolsCatalogLoading: boolean;
  selectedToolCategory: ToolCategory;
  adminToolGroups: AdminToolGroup[];
  resolveAdminToolDetail: (item: ToolEntry) => string;
}>();

const { t } = useI18n();

const toolPaneStatus = ref('');
const toolPaneLoading = ref(false);

const activeToolPaneComponent = computed<Component | null>(() => {
  if (props.selectedToolCategory === 'mcp') return UserMcpPane;
  if (props.selectedToolCategory === 'skills') return UserSkillPane;
  if (props.selectedToolCategory === 'knowledge') return UserKnowledgePane;
  return null;
});

const toolsWaitingTargetName = computed(() => {
  switch (props.selectedToolCategory) {
    case 'admin':
      return t('messenger.tools.adminTitle');
    case 'mcp':
      return t('toolManager.system.mcp');
    case 'skills':
      return t('toolManager.system.skills');
    case 'knowledge':
      return t('toolManager.system.knowledge');
    default:
      return t('messenger.section.tools');
  }
});

const showToolsWaitingOverlay = computed(
  () => props.toolsCatalogLoading || (toolPaneLoading.value && Boolean(activeToolPaneComponent.value))
);

const toolsWaitingPhaseLabel = computed(() =>
  props.toolsCatalogLoading ? t('messenger.waiting.phase.preparing') : t('messenger.waiting.phase.loading')
);

const toolsWaitingProgress = computed(() => (props.toolsCatalogLoading ? 34 : 58));

const handleToolPaneStatus = (value: unknown) => {
  toolPaneStatus.value = String(value || '');
};

const handleToolPaneLoadingChange = (value: unknown) => {
  toolPaneLoading.value = value === true;
};

const resolveAdminToolDisplayName = (item: ToolEntry): string => {
  return String(item.displayName || item.name || '').trim() || item.name;
};

watch(
  () => props.selectedToolCategory,
  () => {
    toolPaneStatus.value = '';
    toolPaneLoading.value = false;
  },
  { immediate: true }
);
</script>

<style scoped>
.messenger-tools-section {
  position: relative;
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-height: 0;
  height: 100%;
  overflow: hidden;
}

.messenger-tools-section > .messenger-tools-pane-host,
.messenger-tools-section > .messenger-entity-panel--fill,
.messenger-tools-section > .messenger-list-empty {
  flex: 1 1 auto;
  min-height: 0;
}

.messenger-tools-section > .messenger-list-empty {
  display: flex;
  align-items: center;
  justify-content: center;
}

.messenger-tools-admin-groups {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
}
</style>
