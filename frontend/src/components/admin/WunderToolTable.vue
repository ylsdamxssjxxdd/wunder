<template>
  <div class="tool-table">
    <div class="tool-toolbar">
      <el-input
        v-model="keyword"
        :placeholder="t('admin.tools.search')"
        size="small"
        clearable
        class="tool-search"
      />
      <el-text type="info" size="small">
        {{ t('admin.tools.count', { count: filteredTools.length }) }}
      </el-text>
    </div>
    <el-table
      :data="filteredTools"
      stripe
      :empty-text="resolvedEmptyText"
      :height="tableHeight"
      :row-class-name="() => 'tool-row'"
      class="tool-grid"
    >
      <el-table-column prop="name" :label="t('admin.tools.column.name')" width="240">
        <template #default="{ row }">
          <span class="tool-cell">{{ row.name }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="description" :label="t('admin.tools.column.description')">
        <template #default="{ row }">
          <span class="tool-cell">{{ row.description || '-' }}</span>
        </template>
      </el-table-column>
      <el-table-column :label="t('admin.tools.column.action')" width="120">
        <template #default="{ row }">
          <el-button size="small" @click="openDetail(row)">{{ t('common.view') }}</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="dialogVisible" :title="t('admin.tools.detail.title')" width="640px">
      <div class="tool-detail">
        <div class="detail-line">
          <span class="label">{{ t('admin.tools.detail.name') }}</span>
          <span>{{ selectedTool?.name || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">{{ t('admin.tools.detail.description') }}</span>
          <span>{{ selectedTool?.description || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">{{ t('admin.tools.detail.schema') }}</span>
        </div>
        <pre v-if="selectedTool?.input_schema" class="tool-schema">{{
          formatSchema(selectedTool.input_schema)
        }}</pre>
        <div v-else class="tool-schema-empty">{{ t('common.none') }}</div>
      </div>
      <template #footer>
        <el-button @click="dialogVisible = false">{{ t('common.close') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import type { PropType } from 'vue';

import { useI18n } from '@/i18n';

type ToolItem = {
  name?: string;
  description?: string;
  input_schema?: unknown;
};

const props = defineProps({
  tools: {
    type: Array as PropType<ToolItem[]>,
    default: () => []
  },
  tableHeight: {
    type: [String, Number],
    default: '100%'
  },
  emptyText: {
    type: String,
    default: ''
  }
});

const { t } = useI18n();
const keyword = ref('');
const dialogVisible = ref(false);
const selectedTool = ref<ToolItem | null>(null);
const resolvedEmptyText = computed(() => props.emptyText || t('admin.tools.empty'));

const filteredTools = computed(() => {
  const list = Array.isArray(props.tools) ? props.tools : [];
  const target = keyword.value.trim().toLowerCase();
  if (!target) return list;
  return list.filter((item) => {
    const name = String(item?.name || '').toLowerCase();
    const desc = String(item?.description || '').toLowerCase();
    return name.includes(target) || desc.includes(target);
  });
});

const openDetail = (tool: ToolItem) => {
  selectedTool.value = tool;
  dialogVisible.value = true;
};

const formatSchema = (schema: unknown) => {
  try {
    return JSON.stringify(schema, null, 2);
  } catch {
    return String(schema || '');
  }
};
</script>

<style scoped>
.tool-table {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  overflow: hidden;
}

.tool-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 0 0 auto;
}

.tool-search {
  width: 240px;
}

.tool-detail {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 8px;
}

.detail-line {
  display: flex;
  gap: 12px;
  align-items: baseline;
}

.detail-line .label {
  min-width: 90px;
  color: var(--light-text);
}

.tool-cell {
  display: inline-block;
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tool-schema {
  margin: 0;
  padding: 10px;
  background: #0f172a;
  color: #e2e8f0;
  border-radius: 6px;
  font-size: 12px;
  white-space: pre-wrap;
}

.tool-schema-empty {
  color: var(--light-text);
  font-size: 12px;
}

:deep(.tool-grid) {
  flex: 1;
  min-height: 0;
}

:deep(.tool-grid .el-table__header-wrapper) {
  flex: 0 0 auto;
}

:deep(.tool-grid .el-table__body-wrapper) {
  overflow-y: auto;
}

:deep(.tool-row td) {
  height: 48px;
}

:deep(.tool-row .cell) {
  padding: 0 12px;
  line-height: 20px;
}
</style>
