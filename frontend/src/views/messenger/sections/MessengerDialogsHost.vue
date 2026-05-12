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
    :tooling-items="agentPromptPreviewToolingItems"
    @update:visible="updateAgentPromptPreviewVisible"
  />

  <MessengerResourcePreviewDialog
    :visible="resourcePreviewVisible"
    :loading="resourcePreviewLoading"
    :title="resourcePreviewTitle"
    :meta="resourcePreviewMeta"
    :hint="resourcePreviewHint"
    :src="resourcePreviewUrl"
    :content="resourcePreviewContent"
    :preview-kind="resourcePreviewKind"
    @download="handleResourcePreviewDownload"
    @close="closeResourcePreview"
  />

  <OnlyOfficeEditorDialog
    :visible="onlyOfficeVisible"
    :path="onlyOfficePath"
    :agent-id="activeAgentId"
    :container-id="currentContainerId"
    :user-id="onlyOfficeUserId"
    @update:visible="handleOnlyOfficeVisibleChange"
    @saved="handleWorkspaceEditorSaved"
    @fallback="handleWorkspaceEditorFallback"
  />

  <DrawioEditorDialog
    :visible="drawioVisible"
    :path="drawioPath"
    :agent-id="activeAgentId"
    :container-id="currentContainerId"
    :user-id="drawioUserId"
    @update:visible="handleDrawioVisibleChange"
    @saved="handleWorkspaceEditorSaved"
    @fallback="handleWorkspaceEditorFallback"
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
import DrawioEditorDialog from '@/components/chat/DrawioEditorDialog.vue';
import OnlyOfficeEditorDialog from '@/components/chat/OnlyOfficeEditorDialog.vue';
import {
  MessengerGroupCreateDialog,
  MessengerResourcePreviewDialog,
  MessengerPromptPreviewDialog,
  MessengerTimelineDetailDialog,
  MessengerWorldHistoryDialog
} from './asyncDialogs';
import type { WorldHistoryCategory, WorldHistoryRecord } from '@/views/messenger/model';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';

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
  agentPromptPreviewToolingItems,
  resourcePreviewVisible,
  resourcePreviewLoading,
  resourcePreviewUrl,
  resourcePreviewTitle,
  resourcePreviewMeta,
  resourcePreviewHint,
  resourcePreviewContent,
  resourcePreviewKind,
  handleResourcePreviewDownload,
  closeResourcePreview,
  onlyOfficeVisible,
  onlyOfficePath,
  onlyOfficeUserId,
  drawioVisible,
  drawioPath,
  drawioUserId,
  activeAgentId,
  currentContainerId,
  handleWorkspaceEditorSaved,
  handleWorkspaceEditorFallback,
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
  agentPromptPreviewToolingItems: PromptToolingPreviewItem[];
  resourcePreviewVisible: boolean;
  resourcePreviewLoading: boolean;
  resourcePreviewUrl: string;
  resourcePreviewTitle: string;
  resourcePreviewMeta: string;
  resourcePreviewHint: string;
  resourcePreviewContent: string;
  resourcePreviewKind: WorkspaceResourcePreviewKind;
  handleResourcePreviewDownload: () => void | Promise<void>;
  closeResourcePreview: () => void;
  onlyOfficeVisible: boolean;
  onlyOfficePath: string;
  onlyOfficeUserId: string;
  drawioVisible: boolean;
  drawioPath: string;
  drawioUserId: string;
  activeAgentId: string;
  currentContainerId: number;
  handleWorkspaceEditorSaved: (payload?: { path?: string }) => void | Promise<void>;
  handleWorkspaceEditorFallback: (payload?: { path?: string; message?: string }) => void | Promise<void>;
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
  (event: 'update:onlyOfficeVisible', value: boolean): void;
  (event: 'update:drawioVisible', value: boolean): void;
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

const handleOnlyOfficeVisibleChange = (value: boolean) => {
  emit('update:onlyOfficeVisible', value);
};

const handleDrawioVisibleChange = (value: boolean) => {
  emit('update:drawioVisible', value);
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
