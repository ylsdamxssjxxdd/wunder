<template>
  <div class="portal-shell tool-manager-shell">
    <UserTopbar
      :title="t('toolManager.title')"
      :subtitle="t('toolManager.subtitle')"
      :hide-chat="true"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section tool-manager-section">
            <div class="user-tools-modal user-tools-dialog tool-manager-page">
              <div class="user-tools-sidebar">
                <div class="user-tools-sidebar-title">{{ t('toolManager.section.title') }}</div>
                <button
                  class="user-tools-tab"
                  :class="{ active: activeTab === 'system' }"
                  type="button"
                  @click="activeTab = 'system'"
                >
                  {{ t('toolManager.section.systemTitle') }}
                </button>
                <button
                  class="user-tools-tab"
                  :class="{ active: activeTab === 'mcp' }"
                  type="button"
                  @click="activeTab = 'mcp'"
                >
                  {{ t('toolManager.system.mcp') }}
                </button>
                <button
                  class="user-tools-tab"
                  :class="{ active: activeTab === 'skills' }"
                  type="button"
                  @click="activeTab = 'skills'"
                >
                  {{ t('toolManager.system.skills') }}
                </button>
                <button
                  class="user-tools-tab"
                  :class="{ active: activeTab === 'knowledge' }"
                  type="button"
                  @click="activeTab = 'knowledge'"
                >
                  {{ t('toolManager.system.knowledge') }}
                </button>
                <button
                  class="user-tools-tab"
                  :class="{ active: activeTab === 'shared' }"
                  type="button"
                  @click="activeTab = 'shared'"
                >
                  {{ t('toolManager.section.shared') }}
                </button>
              </div>
              <div class="user-tools-content">
                <div v-show="activeTab === 'system'" class="user-tools-pane tool-catalog-pane">
                  <div class="list-header">
                    <label>{{ t('toolManager.section.systemTitle') }}</label>
                    <div class="tool-catalog-meta">
                      {{ t('toolManager.section.systemCount', { count: systemToolCount }) }}
                    </div>
                  </div>
                  <div class="muted">{{ t('toolManager.section.systemDesc') }}</div>
                  <div class="tool-catalog-grid">
                    <div
                      v-for="group in systemToolGroups"
                      :key="group.key"
                      class="tool-catalog-card"
                    >
                      <div class="tool-catalog-header">
                        <div class="tool-catalog-title">{{ group.title }}</div>
                      </div>
                      <div class="tool-catalog-meta">
                        {{ t('toolManager.section.systemCount', { count: group.items.length }) }}
                      </div>
                      <div class="tool-catalog-tags">
                        <span
                          v-for="item in group.items"
                          :key="item.name"
                          class="tool-catalog-tag"
                          :title="item.description || item.name"
                        >
                          {{ item.name }}
                        </span>
                        <span v-if="!group.items.length" class="tool-catalog-empty">
                          {{ t('common.none') }}
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
                <UserMcpPane
                  v-show="activeTab === 'mcp'"
                  :visible="activeTab === 'mcp'"
                  :active="activeTab === 'mcp'"
                  :status="statusMessage"
                  @status="updateStatus"
                />
                <UserSkillPane
                  v-show="activeTab === 'skills'"
                  :visible="activeTab === 'skills'"
                  :active="activeTab === 'skills'"
                  :status="statusMessage"
                  @status="updateStatus"
                />
                <UserKnowledgePane
                  v-show="activeTab === 'knowledge'"
                  :visible="activeTab === 'knowledge'"
                  :active="activeTab === 'knowledge'"
                  :status="statusMessage"
                  @status="updateStatus"
                />
                <UserSharedToolsPanel v-show="activeTab === 'shared'" />
              </div>
            </div>
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsCatalog } from '@/api/userTools';
import UserKnowledgePane from '@/components/user-tools/UserKnowledgePane.vue';
import UserMcpPane from '@/components/user-tools/UserMcpPane.vue';
import UserSharedToolsPanel from '@/components/user-tools/UserSharedToolsPanel.vue';
import UserSkillPane from '@/components/user-tools/UserSkillPane.vue';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const toolCatalog = ref(null);
const activeTab = ref('system');
const statusMessage = ref('');
const { t } = useI18n();

const updateStatus = (message) => {
  statusMessage.value = message || '';
};

const systemToolGroups = computed(() => {
  const payload = toolCatalog.value || {};
  const normalizeList = (list) => (Array.isArray(list) ? list : []);
  return [
    {
      key: 'builtin',
      title: t('toolManager.system.builtin'),
      items: normalizeList(payload.builtin_tools)
    },
    {
      key: 'mcp',
      title: t('toolManager.system.mcp'),
      items: normalizeList(payload.mcp_tools)
    },
    {
      key: 'skills',
      title: t('toolManager.system.skills'),
      items: normalizeList(payload.skills)
    },
    {
      key: 'knowledge',
      title: t('toolManager.system.knowledge'),
      items: normalizeList(payload.knowledge_tools)
    }
  ];
});

const systemToolCount = computed(() =>
  systemToolGroups.value.reduce((sum, group) => sum + group.items.length, 0)
);

const loadCatalog = async () => {
  try {
    const { data } = await fetchUserToolsCatalog();
    toolCatalog.value = data?.data || null;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || t('toolManager.loadFailed'));
  }
};

onMounted(() => {
  loadCatalog();
});
</script>
