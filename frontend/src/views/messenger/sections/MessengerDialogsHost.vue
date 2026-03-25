<template>
  <MessengerWorldHistoryDialog
    :visible="worldHistoryDialogVisible"
    :keyword="worldHistoryKeyword"
    :active-tab="worldHistoryActiveTab"
    :date-range="worldHistoryDateRange"
    :tab-options="worldHistoryTabOptions"
    :records="filteredWorldHistoryRecords"
    :format-time="formatTime"
    @update:visible="updateWorldHistoryDialogVisible"
    @update:keyword="updateWorldHistoryKeyword"
    @update:active-tab="updateWorldHistoryActiveTab"
    @update:date-range="updateWorldHistoryDateRange"
    @locate="locateWorldHistoryMessage"
  />

  <MessengerTimelineDetailDialog
    :visible="timelineDetailDialogVisible"
    :session-id="timelineDetailSessionId"
    @update:visible="updateTimelineDetailDialogVisible"
  />

  <el-dialog
    :model-value="worldContainerPickerVisible"
    class="messenger-dialog messenger-world-file-picker-dialog"
    :title="t('userWorld.attachments.pickDialogTitle')"
    width="520px"
    destroy-on-close
    @update:model-value="updateWorldContainerPickerVisible"
  >
    <div class="messenger-world-file-picker">
      <div class="messenger-world-file-picker-toolbar">
        <button
          class="messenger-inline-btn"
          type="button"
          :disabled="worldContainerPickerLoading || !worldContainerPickerPath"
          :title="t('userWorld.attachments.pickParent')"
          :aria-label="t('userWorld.attachments.pickParent')"
          @click="openWorldContainerPickerParent"
        >
          <i class="fa-solid fa-arrow-up" aria-hidden="true"></i>
        </button>
        <button
          class="messenger-inline-btn"
          type="button"
          :disabled="worldContainerPickerLoading"
          :title="t('common.refresh')"
          :aria-label="t('common.refresh')"
          @click="refreshWorldContainerPicker"
        >
          <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
        </button>
        <div class="messenger-world-file-picker-path" :title="worldContainerPickerPathLabel">
          {{ worldContainerPickerPathLabel }}
        </div>
      </div>
      <label class="messenger-world-file-picker-search">
        <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
        <input
          :value="worldContainerPickerKeyword"
          type="text"
          :placeholder="t('userWorld.attachments.pickSearchPlaceholder')"
          @input="updateWorldContainerPickerKeyword(($event.target as HTMLInputElement).value.trim())"
        />
      </label>
      <div v-if="worldContainerPickerLoading" class="messenger-world-file-picker-empty">
        {{ t('common.loading') }}
      </div>
      <div
        v-else-if="!worldContainerPickerDisplayEntries.length"
        class="messenger-world-file-picker-empty"
      >
        {{ t('userWorld.attachments.pickEmpty') }}
      </div>
      <div v-else class="messenger-world-file-picker-list">
        <button
          v-for="entry in worldContainerPickerDisplayEntries"
          :key="entry.path"
          class="messenger-world-file-picker-item"
          type="button"
          @click="handleWorldContainerPickerEntry(entry)"
        >
          <i
            class="messenger-world-file-picker-icon"
            :class="entry.type === 'dir' ? 'fa-solid fa-folder' : 'fa-regular fa-file-lines'"
            aria-hidden="true"
          ></i>
          <span class="messenger-world-file-picker-name" :title="entry.name">
            {{ entry.name }}
          </span>
          <i
            class="messenger-world-file-picker-action"
            :class="entry.type === 'dir' ? 'fa-solid fa-chevron-right' : 'fa-solid fa-plus'"
            aria-hidden="true"
          ></i>
        </button>
      </div>
    </div>
  </el-dialog>

  <MessengerPromptPreviewDialog
    :visible="agentPromptPreviewVisible"
    :loading="agentPromptPreviewLoading"
    :html-content="activeAgentPromptPreviewHtml"
    :memory-mode="agentPromptPreviewMemoryMode"
    :tooling-mode="agentPromptPreviewToolingMode"
    :tooling-content="agentPromptPreviewToolingContent"
    @update:visible="updateAgentPromptPreviewVisible"
  />

  <MessengerImagePreviewDialog
    :visible="imagePreviewVisible"
    :image-url="imagePreviewUrl"
    :title="imagePreviewTitle"
    :workspace-path="imagePreviewWorkspacePath"
    @download="handleImagePreviewDownload"
    @close="closeImagePreview"
  />

  <MessengerGroupCreateDialog
    :visible="groupCreateVisible"
    :group-name="groupCreateName"
    :keyword="groupCreateKeyword"
    :member-ids="groupCreateMemberIds"
    :creating="groupCreating"
    :contacts="filteredGroupCreateContacts"
    :resolve-unit-label="resolveUnitLabel"
    @update:visible="updateGroupCreateVisible"
    @update:group-name="updateGroupCreateName"
    @update:keyword="updateGroupCreateKeyword"
    @update:member-ids="updateGroupCreateMemberIds"
    @submit="submitGroupCreate"
  />
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';
import {
  MessengerGroupCreateDialog,
  MessengerImagePreviewDialog,
  MessengerPromptPreviewDialog,
  MessengerTimelineDetailDialog,
  MessengerWorldHistoryDialog
} from './asyncDialogs';
import type { WorldHistoryCategory, WorldHistoryRecord } from '@/views/messenger/model';

type MessengerWorldHistoryTabOption = {
  key: WorldHistoryCategory;
  label: string;
};

const { t } = useI18n();

const {
  worldHistoryDialogVisible,
  worldHistoryKeyword,
  worldHistoryActiveTab,
  worldHistoryDateRange,
  worldHistoryTabOptions,
  filteredWorldHistoryRecords,
  formatTime,
  locateWorldHistoryMessage,
  timelineDetailDialogVisible,
  timelineDetailSessionId,
  worldContainerPickerVisible,
  worldContainerPickerLoading,
  worldContainerPickerPath,
  worldContainerPickerPathLabel,
  worldContainerPickerKeyword,
  worldContainerPickerDisplayEntries,
  openWorldContainerPickerParent,
  refreshWorldContainerPicker,
  handleWorldContainerPickerEntry,
  agentPromptPreviewVisible,
  agentPromptPreviewLoading,
  activeAgentPromptPreviewHtml,
  agentPromptPreviewMemoryMode,
  agentPromptPreviewToolingMode,
  agentPromptPreviewToolingContent,
  imagePreviewVisible,
  imagePreviewUrl,
  imagePreviewTitle,
  imagePreviewWorkspacePath,
  handleImagePreviewDownload,
  closeImagePreview,
  groupCreateVisible,
  groupCreateName,
  groupCreateKeyword,
  groupCreateMemberIds,
  groupCreating,
  filteredGroupCreateContacts,
  resolveUnitLabel,
  submitGroupCreate
} = defineProps<{
  worldHistoryDialogVisible: boolean;
  worldHistoryKeyword: string;
  worldHistoryActiveTab: WorldHistoryCategory;
  worldHistoryDateRange: [string, string] | [];
  worldHistoryTabOptions: MessengerWorldHistoryTabOption[];
  filteredWorldHistoryRecords: WorldHistoryRecord[];
  formatTime: (value: unknown) => string;
  locateWorldHistoryMessage: (record: WorldHistoryRecord) => void | Promise<void>;
  timelineDetailDialogVisible: boolean;
  timelineDetailSessionId: string;
  worldContainerPickerVisible: boolean;
  worldContainerPickerLoading: boolean;
  worldContainerPickerPath: string;
  worldContainerPickerPathLabel: string;
  worldContainerPickerKeyword: string;
  worldContainerPickerDisplayEntries: Array<{ path: string; name: string; type: 'dir' | 'file' }>;
  openWorldContainerPickerParent: () => void;
  refreshWorldContainerPicker: () => void;
  handleWorldContainerPickerEntry: (entry: { path: string; name: string; type: 'dir' | 'file' }) => void;
  agentPromptPreviewVisible: boolean;
  agentPromptPreviewLoading: boolean;
  activeAgentPromptPreviewHtml: string;
  agentPromptPreviewMemoryMode: 'none' | 'pending' | 'frozen';
  agentPromptPreviewToolingMode: string;
  agentPromptPreviewToolingContent: string;
  imagePreviewVisible: boolean;
  imagePreviewUrl: string;
  imagePreviewTitle: string;
  imagePreviewWorkspacePath: string;
  handleImagePreviewDownload: () => void | Promise<void>;
  closeImagePreview: () => void;
  groupCreateVisible: boolean;
  groupCreateName: string;
  groupCreateKeyword: string;
  groupCreateMemberIds: string[];
  groupCreating: boolean;
  filteredGroupCreateContacts: unknown[];
  resolveUnitLabel: (unitId: unknown) => string;
  submitGroupCreate: () => void | Promise<void>;
}>();

const emit = defineEmits<{
  (event: 'update:worldHistoryDialogVisible', value: boolean): void;
  (event: 'update:worldHistoryKeyword', value: string): void;
  (event: 'update:worldHistoryActiveTab', value: WorldHistoryCategory): void;
  (event: 'update:worldHistoryDateRange', value: [string, string] | []): void;
  (event: 'update:timelineDetailDialogVisible', value: boolean): void;
  (event: 'update:worldContainerPickerVisible', value: boolean): void;
  (event: 'update:worldContainerPickerKeyword', value: string): void;
  (event: 'update:agentPromptPreviewVisible', value: boolean): void;
  (event: 'update:groupCreateVisible', value: boolean): void;
  (event: 'update:groupCreateName', value: string): void;
  (event: 'update:groupCreateKeyword', value: string): void;
  (event: 'update:groupCreateMemberIds', value: string[]): void;
}>();

const updateWorldHistoryDialogVisible = (value: boolean) => {
  emit('update:worldHistoryDialogVisible', value);
};

const updateWorldHistoryKeyword = (value: string) => {
  emit('update:worldHistoryKeyword', value);
};

const updateWorldHistoryActiveTab = (value: WorldHistoryCategory) => {
  emit('update:worldHistoryActiveTab', value);
};

const updateWorldHistoryDateRange = (value: [string, string] | []) => {
  emit('update:worldHistoryDateRange', value);
};

const updateTimelineDetailDialogVisible = (value: boolean) => {
  emit('update:timelineDetailDialogVisible', value);
};

const updateWorldContainerPickerVisible = (value: boolean) => {
  emit('update:worldContainerPickerVisible', value);
};

const updateWorldContainerPickerKeyword = (value: string) => {
  emit('update:worldContainerPickerKeyword', value);
};

const updateAgentPromptPreviewVisible = (value: boolean) => {
  emit('update:agentPromptPreviewVisible', value);
};

const updateGroupCreateVisible = (value: boolean) => {
  emit('update:groupCreateVisible', value);
};

const updateGroupCreateName = (value: string) => {
  emit('update:groupCreateName', value);
};

const updateGroupCreateKeyword = (value: string) => {
  emit('update:groupCreateKeyword', value);
};

const updateGroupCreateMemberIds = (value: string[]) => {
  emit('update:groupCreateMemberIds', value);
};
</script>
