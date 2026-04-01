<template>
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
                    :name="item.name"
                    :description="item.description"
                    :hint="resolveAdminToolDetail(item)"
                    :kind="group.key === 'skills' ? 'skill' : 'tool'"
                    :group="group.key"
                    :chips="[group.title]"
                  />
                </template>
                <span class="messenger-tool-tag messenger-tool-tag--detail">
                  <AbilityIconBadge
                    :name="item.name"
                    :description="item.description"
                    :hint="resolveAdminToolDetail(item)"
                    :kind="group.key === 'skills' ? 'skill' : 'tool'"
                    :group="group.key"
                    size="xs"
                  />
                  <span class="messenger-tool-tag-text">{{ item.name }}</span>
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
        />
      </KeepAlive>
    </div>
  </template>
  <div v-else class="messenger-list-empty">{{ t('messenger.empty.selectTool') }}</div>
</template>

<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
import AbilityTooltipCard from '@/components/common/AbilityTooltipCard.vue';
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

const activeToolPaneComponent = computed<Component | null>(() => {
  if (props.selectedToolCategory === 'mcp') return UserMcpPane;
  if (props.selectedToolCategory === 'skills') return UserSkillPane;
  if (props.selectedToolCategory === 'knowledge') return UserKnowledgePane;
  return null;
});

const handleToolPaneStatus = (value: unknown) => {
  toolPaneStatus.value = String(value || '');
};

watch(
  () => props.selectedToolCategory,
  () => {
    toolPaneStatus.value = '';
  },
  { immediate: true }
);
</script>
