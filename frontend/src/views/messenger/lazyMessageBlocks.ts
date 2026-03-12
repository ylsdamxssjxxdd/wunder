import { defineAsyncComponent } from 'vue';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineAsyncComponent({
    loader,
    suspensible: false
  });

export const InquiryPanel = lazy(() => import('@/components/chat/InquiryPanel.vue'));
export const MessageKnowledgeCitation = lazy(() => import('@/components/chat/MessageKnowledgeCitation.vue'));
export const MessageThinking = lazy(() => import('@/components/chat/MessageThinking.vue'));
export const MessageToolWorkflow = lazy(() => import('@/components/chat/MessageToolWorkflow.vue'));
export const PlanPanel = lazy(() => import('@/components/chat/PlanPanel.vue'));
export const ToolApprovalComposer = lazy(
  () => import('@/components/chat/ToolApprovalComposer.vue')
);
export const WorkspacePanel = lazy(() => import('@/components/chat/WorkspacePanel.vue'));
