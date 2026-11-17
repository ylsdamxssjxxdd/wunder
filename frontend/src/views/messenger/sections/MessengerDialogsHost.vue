<template>
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

  <WorkspaceDocumentDialog
    :visible="resourcePreviewVisible"
    :loading="resourcePreviewLoading"
    :title="resourcePreviewTitle"
    :path="resourcePreviewWorkspacePath"
    :meta="resourcePreviewMeta"
    :hint="resourcePreviewHint"
    :error="''"
    :too-large="resourcePreviewTooLarge"
    :editable="resourcePreviewEditable"
    :saving="resourcePreviewSaving"
    :preview-kind="resourcePreviewKind"
    :preview-url="resourcePreviewUrl"
    :content="resourcePreviewContent"
    @download="handleResourcePreviewDownload"
    @close="closeResourcePreview"
    @save="handleResourcePreviewSave"
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
import { MessengerPromptPreviewDialog, WorkspaceDocumentDialog } from './asyncDialogs';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';

const {
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
  resourcePreviewTooLarge,
  resourcePreviewEditable,
  resourcePreviewSaving,
  handleResourcePreviewDownload,
  handleResourcePreviewSave,
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
  resourcePreviewTooLarge: boolean;
  resourcePreviewEditable: boolean;
  resourcePreviewSaving: boolean;
  handleResourcePreviewDownload: () => void | Promise<void>;
  handleResourcePreviewSave: (content: string) => void | Promise<void>;
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
  (event: 'update:agentPromptPreviewVisible', value: boolean): void;
  (event: 'update:onlyOfficeVisible', value: boolean): void;
  (event: 'update:drawioVisible', value: boolean): void;
}>();

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
