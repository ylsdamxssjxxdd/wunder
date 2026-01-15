<template>
  <div class="tool-table">
    <div class="tool-toolbar">
      <el-input
        v-model="keyword"
        placeholder="搜索工具名称或描述"
        size="small"
        clearable
        class="tool-search"
      />
      <el-text type="info" size="small">共 {{ filteredTools.length }} 个</el-text>
    </div>
    <el-table
      :data="filteredTools"
      stripe
      :empty-text="emptyText"
      :height="tableHeight"
      :row-class-name="() => 'tool-row'"
      class="tool-grid"
    >
      <el-table-column prop="name" label="工具名称" width="240">
        <template #default="{ row }">
          <span class="tool-cell">{{ row.name }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="description" label="描述">
        <template #default="{ row }">
          <span class="tool-cell">{{ row.description || '-' }}</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="120">
        <template #default="{ row }">
          <el-button size="small" @click="openDetail(row)">查看</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="dialogVisible" title="工具详情" width="640px">
      <div class="tool-detail">
        <div class="detail-line">
          <span class="label">名称</span>
          <span>{{ selectedTool?.name || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">描述</span>
          <span>{{ selectedTool?.description || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">输入 Schema</span>
        </div>
        <pre v-if="selectedTool?.input_schema" class="tool-schema">{{
          formatSchema(selectedTool.input_schema)
        }}</pre>
        <div v-else class="tool-schema-empty">无</div>
      </div>
      <template #footer>
        <el-button @click="dialogVisible = false">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, ref } from 'vue';

const props = defineProps({
  tools: {
    type: Array,
    default: () => []
  },
  tableHeight: {
    type: [String, Number],
    default: '100%'
  },
  emptyText: {
    type: String,
    default: '暂无工具'
  }
});

const keyword = ref('');
const dialogVisible = ref(false);
const selectedTool = ref(null);

const filteredTools = computed(() => {
  const list = Array.isArray(props.tools) ? props.tools : [];
  const target = keyword.value.trim().toLowerCase();
  if (!target) return list;
  // 根据名称/描述进行模糊过滤，方便管理员快速定位工具
  return list.filter((item) => {
    const name = String(item?.name || '').toLowerCase();
    const desc = String(item?.description || '').toLowerCase();
    return name.includes(target) || desc.includes(target);
  });
});

const openDetail = (tool) => {
  // 点击查看工具详情时弹窗展示完整信息
  selectedTool.value = tool;
  dialogVisible.value = true;
};

const formatSchema = (schema) => {
  try {
    return JSON.stringify(schema, null, 2);
  } catch (error) {
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
