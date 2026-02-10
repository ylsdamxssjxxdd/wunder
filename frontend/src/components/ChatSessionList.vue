<template>
  <div class="session-list">
    <div class="session-header">
      <span>{{ t('chat.sessions.title') }}</span>
      <el-button type="primary" size="small" @click="$emit('create')">
        {{ t('chat.sessions.new') }}
      </el-button>
    </div>
    <el-scrollbar class="session-scroll">
      <el-menu :default-active="String(activeId)" class="session-menu" @select="handleSelect">
        <el-menu-item
          v-for="session in sessions"
          :key="session.id"
          :index="String(session.id)"
        >
          <span class="session-title">{{ session.title || t('chat.sessions.unnamed') }}</span>
        </el-menu-item>
      </el-menu>
    </el-scrollbar>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

type SessionItem = {
  id?: string | number;
  title?: string;
};

const props = defineProps<{
  sessions: SessionItem[];
  activeId: string | number | null;
}>();

const emit = defineEmits<{
  select: [value: number];
  create: [];
}>();
const { t } = useI18n();

const handleSelect = (value: string): void => {
  emit('select', Number(value));
};
</script>
