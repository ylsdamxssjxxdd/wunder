<template>
  <el-dialog
    v-model="dialogVisible"
    :title="t('workspace.binding.title')"
    width="720px"
    append-to-body
    destroy-on-close
  >
    <div class="workspace-binding-dialog">
      <div class="workspace-binding-field">
        <span class="workspace-binding-label">{{ t('workspace.binding.containerId') }}</span>
        <el-select v-model="selectedContainerId" class="workspace-binding-select">
          <el-option
            v-for="id in containerOptions"
            :key="`workspace-binding-container-${id}`"
            :label="t('messenger.files.agentContainer', { id })"
            :value="id"
          />
        </el-select>
      </div>

      <div v-if="desktopMode" class="workspace-binding-field">
        <span class="workspace-binding-label">{{ t('workspace.binding.localRoot') }}</span>
        <div class="workspace-binding-path-row">
          <el-input
            :model-value="selectedLocalRoot"
            readonly
            :placeholder="t('workspace.binding.localRootPlaceholder')"
          />
          <el-button type="primary" plain @click="pickDirectory">
            {{ t('workspace.binding.pickDirectory') }}
          </el-button>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="workspace-binding-footer">
        <el-button @click="dialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="handleConfirm">
          {{ t('common.confirm') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';

type DesktopBridge = {
  chooseDirectory?: (defaultPath?: string) => Promise<string | null> | string | null;
};

const props = defineProps<{
  visible: boolean;
  currentContainerId: number;
  currentPath: string;
  desktopMode: boolean;
  desktopContainerRoots: Record<number, string>;
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (
    event: 'confirm',
    payload: { containerId: number; localRoot?: string }
  ): void;
}>();

const { t } = useI18n();
const saving = ref(false);
const selectedContainerId = ref(1);
const selectedLocalRoot = ref('');

const dialogVisible = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const containerOptions = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

const normalizeAgentContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

watch(
  () => props.visible,
  (visible) => {
    if (!visible) return;
    selectedContainerId.value = normalizeAgentContainerId(props.currentContainerId);
    selectedLocalRoot.value =
      String(props.desktopContainerRoots?.[selectedContainerId.value] || '').trim();
  },
  { immediate: true }
);

watch(selectedContainerId, (value) => {
  selectedLocalRoot.value = String(props.desktopContainerRoots?.[value] || '').trim();
});

const getDesktopBridge = (): DesktopBridge | null => {
  if (typeof window === 'undefined') return null;
  const runtimeWindow = window as Window & { wunderDesktop?: DesktopBridge };
  return runtimeWindow.wunderDesktop || null;
};

const pickDirectory = async () => {
  const bridge = getDesktopBridge();
  if (!bridge?.chooseDirectory) return;
  try {
    const picked = await bridge.chooseDirectory(selectedLocalRoot.value || undefined);
    const normalized = String(picked || '').trim();
    if (normalized) {
      selectedLocalRoot.value = normalized;
    }
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.containers.pathPickerNativeFailed'));
  }
};

const handleConfirm = async () => {
  const containerId = normalizeAgentContainerId(selectedContainerId.value);
  if (!Number.isFinite(containerId) || containerId < 1) {
    ElMessage.warning(t('workspace.binding.invalidContainer'));
    return;
  }
  saving.value = true;
  try {
    emit('confirm', {
      containerId,
      ...(props.desktopMode ? { localRoot: selectedLocalRoot.value.trim() } : {})
    });
    dialogVisible.value = false;
  } finally {
    saving.value = false;
  }
};
</script>

<style scoped>
.workspace-binding-dialog {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.workspace-binding-label {
  font-size: 13px;
  font-weight: 600;
  color: #475569;
}

.workspace-binding-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.workspace-binding-select {
  width: 100%;
}

.workspace-binding-path-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  gap: 8px;
}

.workspace-binding-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
