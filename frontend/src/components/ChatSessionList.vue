<template>
  <div class="session-list">
    <div class="session-header">
      <span>会话列表</span>
      <el-button type="primary" size="small" @click="$emit('create')">新会话</el-button>
    </div>
    <el-scrollbar class="session-scroll">
      <el-menu :default-active="String(activeId)" class="session-menu" @select="handleSelect">
        <el-menu-item
          v-for="session in sessions"
          :key="session.id"
          :index="String(session.id)"
        >
          <span class="session-title">{{ session.title || '未命名会话' }}</span>
        </el-menu-item>
      </el-menu>
    </el-scrollbar>
  </div>
</template>

<script setup>
defineProps({
  sessions: {
    type: Array,
    default: () => []
  },
  activeId: {
    type: [Number, String],
    default: null
  }
});

const emit = defineEmits(['select', 'create']);

const handleSelect = (value) => {
  emit('select', Number(value));
};
</script>
