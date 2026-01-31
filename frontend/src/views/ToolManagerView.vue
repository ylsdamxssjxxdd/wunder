<template>
  <div class="portal-shell tool-manager-shell">
    <UserTopbar
      title="工具管理"
      subtitle="查看管理员开放工具，并配置你的 MCP、技能与知识库"
      :hide-chat="true"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section tool-manager-section">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">工具分类</div>
                <div class="portal-section-desc">查看管理员开放工具，并配置你的工具分类</div>
              </div>
            </div>
            <div class="user-tools-dialog user-tools-page tool-manager-page">
              <div class="user-tools-modal">
                <div class="user-tools-sidebar">
                  <div class="user-tools-sidebar-title">工具分类</div>
                  <button
                    class="user-tools-tab"
                    :class="{ active: activeTab === 'system' }"
                    type="button"
                    @click="activeTab = 'system'"
                  >
                    管理员开放工具（只读）
                  </button>
                  <button
                    class="user-tools-tab"
                    :class="{ active: activeTab === 'mcp' }"
                    type="button"
                    @click="activeTab = 'mcp'"
                  >
                    MCP 工具
                  </button>
                  <button
                    class="user-tools-tab"
                    :class="{ active: activeTab === 'skills' }"
                    type="button"
                    @click="activeTab = 'skills'"
                  >
                    技能工具
                  </button>
                  <button
                    class="user-tools-tab"
                    :class="{ active: activeTab === 'knowledge' }"
                    type="button"
                    @click="activeTab = 'knowledge'"
                  >
                    知识库工具
                  </button>
                  <button
                    class="user-tools-tab"
                    :class="{ active: activeTab === 'shared' }"
                    type="button"
                    @click="activeTab = 'shared'"
                  >
                    共享工具
                  </button>
                </div>
                <div class="user-tools-content">
                  <div v-show="activeTab === 'system'" class="user-tools-pane tool-catalog-pane">
                    <div class="list-header">
                      <label>管理员开放工具</label>
                      <div class="tool-catalog-meta">共 {{ systemToolCount }} 项</div>
                    </div>
                    <div class="muted">管理员开放工具仅可查看。</div>
                    <div class="tool-catalog-grid">
                      <div v-if="!systemToolGroups.length" class="tool-catalog-empty">
                        暂无可用工具
                      </div>
                      <div
                        v-for="group in systemToolGroups"
                        :key="group.key"
                        class="tool-catalog-card"
                      >
                        <div class="tool-catalog-header">
                          <div class="tool-catalog-title">{{ group.title }}</div>
                        </div>
                        <div class="tool-catalog-meta">{{ group.items.length }} 项</div>
                        <div class="tool-catalog-tags">
                          <span
                            v-for="item in group.items"
                            :key="item.name"
                            class="tool-catalog-tag"
                            :title="item.description || item.name"
                          >
                            {{ item.name }}
                          </span>
                          <span v-if="!group.items.length" class="tool-catalog-empty">暂无</span>
                        </div>
                      </div>
                    </div>
                  </div>
                  <UserMcpPane
                    v-show="activeTab === 'mcp'"
                    :visible="activeTab === 'mcp'"
                    :active="activeTab === 'mcp'"
                    @status="updateStatus"
                  />
                  <UserSkillPane
                    v-show="activeTab === 'skills'"
                    :visible="activeTab === 'skills'"
                    :active="activeTab === 'skills'"
                    @status="updateStatus"
                  />
                  <UserKnowledgePane
                    v-show="activeTab === 'knowledge'"
                    :visible="activeTab === 'knowledge'"
                    :active="activeTab === 'knowledge'"
                    @status="updateStatus"
                  />
                  <UserSharedToolsPanel v-show="activeTab === 'shared'" />
                </div>
              </div>
              <div v-if="activeTab !== 'shared' && activeTab !== 'system'" class="user-tools-status">
                {{ statusMessage }}
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

const toolCatalog = ref(null);
const activeTab = ref('system');
const statusMessage = ref('');

const updateStatus = (message) => {
  statusMessage.value = message || '';
};

const systemToolGroups = computed(() => {
  const payload = toolCatalog.value || {};
  const normalizeList = (list) => (Array.isArray(list) ? list : []);
  return [
    {
      key: 'builtin',
      title: '内置工具',
      items: normalizeList(payload.builtin_tools)
    },
    {
      key: 'mcp',
      title: 'MCP 工具',
      items: normalizeList(payload.mcp_tools)
    },
    {
      key: 'a2a',
      title: 'A2A 工具',
      items: normalizeList(payload.a2a_tools)
    },
    {
      key: 'skills',
      title: '技能工具',
      items: normalizeList(payload.skills)
    },
    {
      key: 'knowledge',
      title: '知识库工具',
      items: normalizeList(payload.knowledge_tools)
    }
  ].filter((group) => group.items.length > 0);
});

const systemToolCount = computed(() =>
  systemToolGroups.value.reduce((sum, group) => sum + group.items.length, 0)
);

const loadCatalog = async () => {
  try {
    const { data } = await fetchUserToolsCatalog();
    toolCatalog.value = data?.data || null;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工具清单加载失败');
  }
};

onMounted(() => {
  loadCatalog();
});
</script>
