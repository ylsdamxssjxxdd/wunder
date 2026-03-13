<template>
  <el-alert v-if="hasMissing" type="warning" :closable="false" show-icon>
    <template #title>
      {{ t('portal.agent.dependencies.title') }}
    </template>
    <div class="agent-dependency-notice-body">
      <div v-if="missingToolNames.length" class="agent-dependency-notice-group">
        <div class="agent-dependency-notice-label">{{ t('portal.agent.dependencies.missingTools') }}</div>
        <div class="agent-dependency-notice-tags">
          <el-tag v-for="name in missingToolNames" :key="`missing-tool-${name}`" size="small" type="warning">
            {{ name }}
          </el-tag>
        </div>
      </div>
      <div v-if="missingSkillNames.length" class="agent-dependency-notice-group">
        <div class="agent-dependency-notice-label">{{ t('portal.agent.dependencies.missingSkills') }}</div>
        <div class="agent-dependency-notice-tags">
          <el-tag v-for="name in missingSkillNames" :key="`missing-skill-${name}`" size="small" type="danger">
            {{ name }}
          </el-tag>
        </div>
      </div>
      <div class="agent-dependency-notice-hint">
        {{ t('portal.agent.dependencies.hint') }}
      </div>
    </div>
  </el-alert>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps({
  missingToolNames: {
    type: Array as () => string[],
    default: () => []
  },
  missingSkillNames: {
    type: Array as () => string[],
    default: () => []
  }
});

const { t } = useI18n();
const hasMissing = computed(() => props.missingToolNames.length > 0 || props.missingSkillNames.length > 0);
</script>

<style scoped>
.agent-dependency-notice-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.agent-dependency-notice-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.agent-dependency-notice-label {
  font-size: 12px;
  font-weight: 600;
}

.agent-dependency-notice-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.agent-dependency-notice-hint {
  font-size: 12px;
  line-height: 1.5;
  opacity: 0.85;
}
</style>
