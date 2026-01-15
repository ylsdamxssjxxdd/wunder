<template>
  <el-dialog
    v-model="visibleProxy"
    class="user-tools-dialog"
    width="1120px"
    top="6vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="user-tools-header">
        <div class="user-tools-title">自建工具</div>
        <button class="icon-btn" type="button" @click="close">×</button>
      </div>
    </template>

    <div class="user-tools-modal">
      <div class="user-tools-sidebar">
        <div class="user-tools-sidebar-title">工具分类</div>
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
      </div>

      <div class="user-tools-content">
        <UserMcpPane
          v-show="activeTab === 'mcp'"
          :visible="visibleProxy"
          :active="activeTab === 'mcp'"
          @status="updateStatus"
        />
        <UserSkillPane
          v-show="activeTab === 'skills'"
          :visible="visibleProxy"
          :active="activeTab === 'skills'"
          @status="updateStatus"
        />
        <UserKnowledgePane
          v-show="activeTab === 'knowledge'"
          :visible="visibleProxy"
          :active="activeTab === 'knowledge'"
          @status="updateStatus"
        />
      </div>
    </div>

    <div class="user-tools-status">{{ statusMessage }}</div>

    <template #footer>
      <el-button class="user-tools-footer-btn" @click="close">关闭</el-button>
    </template>
  </el-dialog>
</template>

<script setup>
import { computed, ref, watch } from 'vue';

import UserKnowledgePane from './UserKnowledgePane.vue';
import UserMcpPane from './UserMcpPane.vue';
import UserSkillPane from './UserSkillPane.vue';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:modelValue']);

const visibleProxy = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const activeTab = ref('mcp');
const statusMessage = ref('');

const updateStatus = (message) => {
  statusMessage.value = message || '';
};

const close = () => {
  visibleProxy.value = false;
};

watch(
  () => props.modelValue,
  (value) => {
    if (value) {
      statusMessage.value = '';
      activeTab.value = activeTab.value || 'mcp';
    } else {
      statusMessage.value = '';
    }
  }
);
</script>
