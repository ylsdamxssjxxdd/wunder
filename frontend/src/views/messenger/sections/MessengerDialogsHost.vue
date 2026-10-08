<template>
  <MessengerTimelineDetailDialog
    :visible="timelineDetailDialogVisible"
    :session-id="timelineDetailSessionId"
    @update:visible="updateTimelineDetailDialogVisible"
  />

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
    :source-path="resourcePreviewWorkspacePath"
    :preview-kind="resourcePreviewKind"
    @download="handleResourcePreviewDownload"
    @close="closeResourcePreview"
  />

  <OnlyOfficeEditorDialog
    :visible="onlyOfficeVisible"
    :path="onlyOfficePath"
    :agent-id="onlyOfficeAgentId"
    :container-id="onlyOfficeContainerId"
    :user-id="onlyOfficeUserId"
    preserve-sidebar
    :sidebar-visible="onlyOfficeSidebarVisible"
    @update:visible="handleOnlyOfficeVisibleChange"
    @saved="handleWorkspaceEditorSaved"
    @fallback="handleWorkspaceEditorFallback"
  />

  <DrawioEditorDialog
    :visible="drawioVisible"
    :path="drawioPath"
    :agent-id="drawioAgentId"
    :container-id="drawioContainerId"
    :user-id="drawioUserId"
    preserve-sidebar
    :sidebar-visible="drawioSidebarVisible"
    @update:visible="handleDrawioVisibleChange"
    @saved="handleWorkspaceEditorSaved"
    @fallback="handleWorkspaceEditorFallback"
  />
</template>

<script setup lang="ts">
import DrawioEditorDialog from '@/components/chat/DrawioEditorDialog.vue';
import OnlyOfficeEditorDialog from '@/components/chat/OnlyOfficeEditorDialog.vue';
import {
  MessengerResourcePreviewDialog,
  MessengerPromptPreviewDialog,
  MessengerTimelineDetailDialog
} from './asyncDialogs';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';

const {
  timelineDetailDialogVisible,
  timelineDetailSessionId,
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
  resourcePreviewWorkspacePath,
  resourcePreviewKind,
  handleResourcePreviewDownload,
  closeResourcePreview,
  onlyOfficeVisible,
  onlyOfficePath,
  onlyOfficeUserId,
  onlyOfficeSidebarVisible,
  handleWorkspaceEditorSaved,
  handleWorkspaceEditorFallback,
  drawioVisible,
  drawioPath,
  drawioUserId,
  drawioSidebarVisible
} = defineProps<{
  timelineDetailDialogVisible: boolean;
  timelineDetailSessionId: string;
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
  resourcePreviewWorkspacePath: string;
  resourcePreviewKind: WorkspaceResourcePreviewKind;
  handleResourcePreviewDownload: () => void | Promise<void>;
  closeResourcePreview: () => void;
  onlyOfficeVisible: boolean;
  onlyOfficePath: string;
  onlyOfficeUserId: string;
  onlyOfficeSidebarVisible: boolean;
  onlyOfficeAgentId: string;
  onlyOfficeContainerId: number | null;
  drawioVisible: boolean;
  drawioPath: string;
  drawioUserId: string;
  drawioSidebarVisible: boolean;
  drawioAgentId: string;
  drawioContainerId: number | null;
  handleWorkspaceEditorSaved: (payload?: { path?: string }) => void | Promise<void>;
  handleWorkspaceEditorFallback: (payload?: { path?: string; message?: string }) => void | Promise<void>;
}>();

const emit = defineEmits<{
  (event: 'update:timelineDetailDialogVisible', value: boolean): void;
  (event: 'update:agentPromptPreviewVisible', value: boolean): void;
  (event: 'update:onlyOfficeVisible', value: boolean): void;
  (event: 'update:drawioVisible', value: boolean): void;
}>();

const updateTimelineDetailDialogVisible = (value: boolean) => {
  emit('update:timelineDetailDialogVisible', value);
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
</script>
