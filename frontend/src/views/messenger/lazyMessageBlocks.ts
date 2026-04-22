import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const InquiryPanel = lazy(() => import('@/components/chat/InquiryPanel.vue'));
export const MessageKnowledgeCitation = lazy(() => import('@/components/chat/MessageKnowledgeCitation.vue'));
export const MessageCompactionDivider = lazy(() => import('@/components/chat/MessageCompactionDivider.vue'));
export const MessageFeedbackActions = lazy(() => import('@/components/chat/MessageFeedbackActions.vue'));
export const MessageThinking = lazy(() => import('@/components/chat/MessageThinking.vue'));
export const MessageSubagentPanel = lazy(() => import('@/components/chat/MessageSubagentPanel.vue'));
export const PlanPanel = lazy(() => import('@/components/chat/PlanPanel.vue'));
export const ToolApprovalComposer = lazy(
  () => import('@/components/chat/ToolApprovalComposer.vue')
);
export const WorkspacePanel = lazy(() => import('@/components/chat/WorkspacePanel.vue'));
