<template>
  <div
    class="timeline-assistant"
    :data-turn-id="String(message.__runtime_user_turn_id || message.user_turn_id || '')"
  >
    <!--
      时间线主体（形态对齐桌面端 `frontend-slint/src/timeline.rs`）：模型每轮的正式
      输出是一段独立打印，思考与工具调用按到达顺序折成可开合的批次夹在段与段之间。
      块的顺序由投影层按 `created_seq` 定好；块渲染由 MessageTimelineBlocks 与
      子智能体详情弹窗共用，旧式单段正文（问候语、无块旧行）回退到本组件渲染。
    -->
    <MessageTimelineBlocks
      v-if="hasProjectionBlocks"
      :blocks="timelineBlocks"
      :workflow-items="workflowItems"
      :session-id="activeSessionId"
      :identity-key="item.key"
      :default-open="defaultOpen"
      :streaming="bodyStreaming"
      :content-version="runtimeContentVersion"
      :body-text-transform="transformBodyText"
      :cache-key-prefix="`agent:${conversationKey}:c${currentContainerId}:`"
      :message="message"
      :runtime-message-id="runtimeMessageId"
      :runtime-user-turn-id="runtimeUserTurnId"
      :runtime-model-turn-id="runtimeModelTurnId"
      :content-truncated="message.content_truncated === true"
      :resolve-workspace-path="resolveAgentMarkdownWorkspacePath"
      :throttle-ms="MARKDOWN_STREAM_THROTTLE_MS"
      @markdown-rendered="handleMessageMarkdownRendered(item.key, $event)"
      @history-hydrated="handleHistoryHydrated"
      @activity="(active) => emit('activity', active)"
    >
      <template #body-tail="{ isLast }">
        <template v-if="isLast">
          <div v-if="hasUserImageAttachments(message)" class="message-user-image-grid">
            <button
              v-for="imageItem in resolveUserImageAttachments(message)"
              :key="imageItem.key"
              class="message-user-image-btn"
              type="button"
              :title="imageItem.name"
              :aria-label="imageItem.name"
              @click="openResourcePreview({ src: imageItem.src, title: imageItem.name, workspacePath: imageItem.workspacePath, meta: imageItem.workspacePath || imageItem.name, kind: 'image' })"
            >
              <img :src="imageItem.src" :alt="imageItem.name" class="message-user-image" loading="lazy" decoding="async" />
            </button>
          </div>
          <div v-if="hasUserAudioAttachments(message)" class="message-user-audio-grid">
            <div v-for="audioItem in resolveUserAudioAttachments(message)" :key="audioItem.key" class="message-user-audio-card">
              <span class="message-user-audio-name" :title="audioItem.name">{{ audioItem.name }}</span>
              <audio class="message-user-audio-player" :src="audioItem.src" controls preload="metadata"></audio>
            </div>
          </div>
        </template>
      </template>
    </MessageTimelineBlocks>
    <div
      v-else-if="hasBody"
      class="timeline-body-block"
      :class="{ 'is-streaming': bodyStreaming }"
      data-turn-slot="body"
    >
      <template v-if="isGreeting">
          <div class="timeline-greeting">
            <div class="timeline-greeting-text">{{ String(message.content || '') }}</div>
            <el-tooltip
              ref="agentAbilityTooltipRef"
              placement="bottom-end"
              trigger="hover"
              :show-after="120"
              :teleported="true"
              :popper-options="agentAbilityTooltipOptions"
              popper-class="messenger-ability-tooltip-popper"
              @show="handleAgentAbilityTooltipShow"
              @hide="handleAgentAbilityTooltipHide"
            >
              <template #content>
                <div class="ability-tooltip">
                  <div class="ability-header">
                    <span class="ability-title">{{ t('chat.ability.title') }}</span>
                    <span class="ability-sub">{{ t('chat.ability.subtitle') }}</span>
                  </div>
                  <div v-if="agentToolSummaryLoading && !hasAgentAbilitySummary" class="ability-muted">
                    {{ t('chat.ability.loading') }}
                  </div>
                  <div v-else-if="agentToolSummaryError" class="ability-error">
                    {{ agentToolSummaryError }}
                  </div>
                  <template v-else>
                    <div v-if="!hasAgentAbilitySummary" class="ability-muted">
                      {{ t('chat.ability.empty') }}
                    </div>
                    <div v-else class="ability-scroll">
                      <div
                        v-for="section in agentAbilitySections"
                        :key="section.key"
                        class="ability-section"
                      >
                        <div class="ability-section-title">
                          <span>{{ section.title }}</span>
                          <span class="ability-count">{{ section.items.length }}</span>
                        </div>
                        <div v-if="section.items.length" class="ability-item-list">
                          <AbilityTooltipListItem
                            v-for="ability in section.items"
                            :key="`${section.key}-${ability.name}`"
                            :name="ability.name"
                            :display-name="ability.displayName"
                            :description="ability.description"
                            :kind="section.kind"
                            :group="section.key"
                            :source="section.key"
                            :chip="section.title"
                            :empty-text="t('chat.ability.noDesc')"
                          />
                        </div>
                        <div v-else class="ability-empty">{{ section.emptyText }}</div>
                      </div>
                    </div>
                  </template>
                </div>
              </template>
              <button
                class="timeline-greeting-preview"
                type="button"
                :title="t('chat.promptPreview')"
                :aria-label="t('chat.promptPreview')"
                :disabled="agentPromptPreviewLoading"
                @click.stop="controller.openAgentPromptPreview?.()"
              >
                <i class="fa-solid fa-eye" aria-hidden="true"></i>
              </button>
            </el-tooltip>
          </div>
        </template>
        <MessageMarkdownBody
          v-else
          :cache-key="`agent:${conversationKey}:${legacyBodyId()}:c${currentContainerId}`"
          :content="displayContent"
          :message="message"
          :runtime-message-id="runtimeMessageId"
          :runtime-user-turn-id="runtimeUserTurnId"
          :runtime-model-turn-id="runtimeModelTurnId"
          :session-id="activeSessionId"
          :item-id="legacyBodyId()"
          :content-truncated="message.content_truncated === true"
          :assistant-display="true"
          :explicit-content="false"
          :streaming="bodyStreaming"
          :throttle-ms="MARKDOWN_STREAM_THROTTLE_MS"
          :resolve-workspace-path="resolveAgentMarkdownWorkspacePath"
          @rendered="handleMessageMarkdownRendered(item.key, $event)"
          @history-message-hydrated="handleHistoryHydrated"
        />
        <div v-if="hasUserImageAttachments(message)" class="message-user-image-grid">
          <button
            v-for="imageItem in resolveUserImageAttachments(message)"
            :key="imageItem.key"
            class="message-user-image-btn"
            type="button"
            :title="imageItem.name"
            :aria-label="imageItem.name"
            @click="openResourcePreview({ src: imageItem.src, title: imageItem.name, workspacePath: imageItem.workspacePath, meta: imageItem.workspacePath || imageItem.name, kind: 'image' })"
          >
            <img :src="imageItem.src" :alt="imageItem.name" class="message-user-image" loading="lazy" decoding="async" />
          </button>
        </div>
        <div v-if="hasUserAudioAttachments(message)" class="message-user-audio-grid">
          <div v-for="audioItem in resolveUserAudioAttachments(message)" :key="audioItem.key" class="message-user-audio-card">
            <span class="message-user-audio-name" :title="audioItem.name">{{ audioItem.name }}</span>
            <audio class="message-user-audio-player" :src="audioItem.src" controls preload="metadata"></audio>
          </div>
        </div>
      </div>

    <!--
      轮次统计行（§7.3 A，桌面端 BodyBlock 的 `height: 22px` 尾部行）：
      左侧是图标 12px + 数值 11px muted 的统计，右侧是 22×22 的复制图标按钮
      （不再显示保存 / 点赞 / 踩一下 / 播放语音）。
      它刻意留在正文块**之外**：正文为空（排队 / 刚起流）时状态药丸仍要可见，
      这是云端运行时的既有契约。
    -->
    <div v-if="!isGreeting" class="timeline-body-footer">
      <MessageStats :message="statsMessage" />
      <span class="timeline-body-footer-gap" aria-hidden="true"></span>
      <button
        class="timeline-body-action"
        type="button"
        :title="t('chat.message.copy')"
        :aria-label="t('chat.message.copy')"
        @click="copyMessageContent(message)"
      >
        <i class="fa-solid fa-clone" aria-hidden="true"></i>
      </button>
      <button
        v-if="shouldShowAgentResumeButton(message)"
        class="messenger-message-footer-copy timeline-body-icon-action"
        type="button"
        :title="t('chat.message.resume')"
        :aria-label="t('chat.message.resume')"
        @click="resumeAgentMessage(message)"
      >
        <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
      </button>
    </div>

    <div
      v-if="subagents.length > 0"
      class="messenger-workflow-scope chat-shell timeline-subagent-scope"
    >
      <MessageSubagentPanel :session-id="activeSessionId" :items="subagents" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, triggerRef, watch } from 'vue';

import MessageMarkdownBody from '@/components/chat/MessageMarkdownBody.vue';
import MessageStats from '@/components/chat/MessageStats.vue';
import MessageSubagentPanel from '@/components/chat/MessageSubagentPanel.vue';
import MessageTimelineBlocks from '@/components/chat/MessageTimelineBlocks.vue';
import { resolveRuntimeMessageContentSource, resolveRuntimeMessageContentSubscriptionIds } from '@/components/chat/messageRuntimeContent';
import { createToolWorkflowRenderBatcher } from '@/components/chat/toolWorkflowRenderBatcher';
import type { ChatRuntimeTimelineBlock } from '@/realtime/chat/chatRuntimeTypes';
import type { WorkflowItem } from '@/components/chat/toolWorkflowRunModel';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { buildAssistantDisplayContent } from '@/utils/assistantFailureNotice';

/**
 * 助手轮次的时间线条目区（方案 §7.3 / §7.4 / §7.6，形态对齐桌面端
 * `frontend-slint/ui/timeline.slint` 的 `BodyBlock` / `FoldEntry` / `GroupBar`）。
 *
 * 结构：正文块（**全宽、无气泡**，左右内边距 24px、上下 6px，hover 底 `hover` 45%）
 * → 轮次统计行（高 22px、间距 12px、图标 12px + 数值 11px muted，右侧复制/下载
 * 52×22 圆角 6）→ 工具分组折叠条（32px，展开时沟槽画连接线；折叠态是 30px 的
 * 「已处理」）→ 分组体（每条目自带 20px 沟槽：1px 连接线 + 6px 节点圆点）
 * → 子智能体面板。
 *
 * 性能：
 * - 工具投影走 `createToolWorkflowRenderBatcher` 按帧合并（33ms），
 *   不在每个 token 上重新折叠条目；
 * - 条目窗口有界（`TIMELINE_ENTRY_RENDER_LIMIT`），超出部分按页展开；
 * - 展开状态按 key 记录且条数有界；历史轮次不参与重算。
 */
const props = withDefaults(defineProps<{
  controller: MessengerControllerContext;
  item: { key: string; sourceIndex: number; message: Record<string, any> };
  /** 该轮次默认展开工具分组（由时间线按 MAX_OPEN_TURNS 计算）。 */
  defaultOpen?: boolean;
}>(), {
  defaultOpen: false
});

const controller = props.controller;
const translate = controller.t;
const t = (key: string, params?: Record<string, unknown>): string => String(translate(key, params) ?? key);
const chatStore = controller.chatStore;
const message = computed<Record<string, any>>(() => props.item.message);

const activeSessionId = computed(() => String(chatStore.activeSessionId || ''));
const conversationKey = computed(() => String(controller.sessionHub?.activeConversationKey || ''));
const runtimeMessageId = computed(() => String(message.value.__runtime_message_id || message.value.message_id || ''));
const runtimeUserTurnId = computed(() =>
  String(message.value.__runtime_user_turn_id || message.value.user_turn_id || message.value.userTurnId || '')
);
const runtimeModelTurnId = computed(() =>
  String(message.value.__runtime_model_turn_id || message.value.model_turn_id || message.value.modelTurnId || '')
);

const resolveAgentMessageKey = controller.resolveAgentMessageKey;
const resolveAgentMarkdownWorkspacePath = controller.resolveAgentMarkdownWorkspacePath;
const handleMessageMarkdownRendered = controller.handleMessageMarkdownRendered;
const MARKDOWN_STREAM_THROTTLE_MS = controller.MARKDOWN_STREAM_THROTTLE_MS;
const currentContainerId = controller.currentContainerId;
const openResourcePreview = controller.openResourcePreview;
const copyMessageContent = controller.copyMessageContent;
const hasUserImageAttachments = controller.hasUserImageAttachments;
const hasUserAudioAttachments = controller.hasUserAudioAttachments;
const resolveUserImageAttachments = controller.resolveUserImageAttachments;
const resolveUserAudioAttachments = controller.resolveUserAudioAttachments;
const shouldShowAgentResumeButton = controller.shouldShowAgentResumeButton;
const resumeAgentMessage = controller.resumeAgentMessage;
// 问候语（greeting）与既有气泡共用同一套「能力预览」浮层绑定。
const AbilityTooltipListItem = controller.AbilityTooltipListItem;
const agentAbilityTooltipRef = ref<unknown>(null);
const agentAbilityTooltipOptions = controller.agentAbilityTooltipOptions;
const agentAbilitySections = controller.agentAbilitySections;
const agentToolSummaryError = controller.agentToolSummaryError;
const agentToolSummaryLoading = controller.agentToolSummaryLoading;
const hasAgentAbilitySummary = controller.hasAgentAbilitySummary;
const agentPromptPreviewLoading = controller.agentPromptPreviewLoading;
const handleAgentAbilityTooltipShow = controller.handleAgentAbilityTooltipShow;
const handleAgentAbilityTooltipHide = controller.handleAgentAbilityTooltipHide;
const isGreeting = computed(() => Boolean(controller.isGreetingMessage?.(message.value)));

// -------------------------------------------------------------- 正文块

/**
 * 正文门禁必须订阅**内容时钟**（同 `MessageMarkdownBody`）。
 *
 * 渲染层按消息 id 复用同一个行对象并**原地**写入流式/终态文本
 * （`materializeChatRuntimeMessageWithCache` → `syncMaterializedMessage`），
 * 对象身份不变、也不是响应式代理；若这里只依赖行对象，`displayContent`
 * 会在首帧（内容还是空串）算完后永久缓存，助手正文就再也不进 DOM——
 * 表现为「实时回复不显示、刷新后正常」。结构时钟 + 该消息的内容时钟
 * 一起作为依赖，才能只让真正变化的行重新计算。
 */
const runtimeContentVersion = computed(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  const structureVersion = chatStore.runtimeProjectionVersionBySession?.[sessionId] || 0;
  const messageIds = resolveRuntimeMessageContentSubscriptionIds({
    projection: chatStore.runtimeProjection,
    sessionId,
    runtimeMessageId: runtimeMessageId.value,
    runtimeUserTurnId: runtimeUserTurnId.value,
    runtimeModelTurnId: runtimeModelTurnId.value,
    message: message.value
  });
  const messageScopedVersion = messageIds.reduce(
    (sum, messageId) => sum + Number(chatStore.runtimeProjectionContentVersionByMessage?.[messageId] || 0),
    0
  );
  return `${structureVersion}:${messageScopedVersion}`;
});

const displayContent = computed(() => {
  void runtimeContentVersion.value;
  return String(controller.buildAssistantDisplayContent?.(message.value, translate) || '');
});
const hasBody = computed(() => Boolean(controller.hasMessageContent?.(displayContent.value)));
const bodyStreaming = computed(() =>
  Boolean(message.value.stream_incomplete || message.value.workflowStreaming || message.value.reasoningStreaming)
);

/**
 * 统计行保留「每次行发布换一个浅拷贝」的既有口径：`MessageStats` 的计算属性
 * 以行对象身份作为失效信号，原地改写不会让它失效。这里刻意**不**再订阅内容
 * 时钟——那会把统计重算压到每个投影帧上，属于性能退化。
 */
const statsMessage = computed(() => ({ ...props.item.message }));

const handleHistoryHydrated = (detail: Record<string, unknown>): void => {
  Object.assign(message.value, detail, {
    content_truncated: false,
    reasoning_truncated: false,
    workflowItems_truncated: false,
    subagents_truncated: false
  });
};

// ------------------------------------------------------- 工具/思考条目

// 只在真的存在工具事件时才驱动重建时钟：纯文本流式轮次不进入这条热路径。
const hasToolProjection = computed(() => {
  const items = Array.isArray(message.value.workflowItems) ? message.value.workflowItems : [];
  return items.length > 0 || Boolean(message.value.workflowStreaming);
});

const workflowItems = ref<WorkflowItem[]>([]);
/**
 * 实时投影里的这一行。渲染层按 id 复用行对象并原地改写，因此**行对象本身**不
 * 一定是最新值；正文、时间线、工具条目都必须走这张解析表取投影，否则流式增量
 * 只落到投影、DOM 停在首帧（表现为「实时不更新、刷新后正常」）。
 */
const resolveProjectedMessage = () => {
  const sessionId = activeSessionId.value;
  if (!sessionId) return null;
  return resolveRuntimeMessageContentSource({
    projection: chatStore.runtimeProjection,
    sessionId,
    runtimeMessageId: runtimeMessageId.value,
    runtimeUserTurnId: runtimeUserTurnId.value,
    runtimeModelTurnId: runtimeModelTurnId.value,
    message: message.value
  });
};

const batchWorkflowItems = () => {
  const projected = resolveProjectedMessage();
  const projectedItems = Array.isArray(projected?.workflowItems) ? (projected.workflowItems as WorkflowItem[]) : null;
  const messageItems = Array.isArray(message.value.workflowItems) ? (message.value.workflowItems as WorkflowItem[]) : [];
  // 投影短暂为空时不要抹掉已实体化的结构快照。
  const source = projectedItems && (projectedItems.length > 0 || messageItems.length === 0)
    ? projectedItems
    : messageItems;
  workflowItems.value = source;
  triggerRef(workflowItems);
};
const workflowBatcher = createToolWorkflowRenderBatcher(batchWorkflowItems, { intervalMs: 33 });

watch(
  () => {
    if (!hasToolProjection.value) return 'idle';
    const sessionId = activeSessionId.value;
    const messageId = runtimeMessageId.value;
    const contentVersion = messageId
      ? Number(chatStore.runtimeProjectionContentVersionByMessage?.[messageId] || 0)
      : 0;
    const sessionVersion = sessionId
      ? Number(chatStore.runtimeProjectionVersionBySession?.[sessionId] || 0)
      : 0;
    return [
      sessionId,
      messageId,
      contentVersion,
      sessionVersion,
      Array.isArray(message.value.workflowItems) ? message.value.workflowItems.length : 0,
      controller.buildMessageWorkflowRenderVersion?.(message.value) || 0
    ].join('\u0001');
  },
  (signature, previousSignature) => {
    if (signature === 'idle') return;
    const identity = (value: unknown) => String(value || '').split('\u0001').slice(0, 3).join('\u0001');
    // 会话/消息身份切换与终态立即刷新，高频 output delta 交给按帧合并。
    workflowBatcher.request(identity(signature) !== identity(previousSignature));
  },
  { immediate: true }
);

onBeforeUnmount(() => workflowBatcher.dispose());

// ------------------------------------------------- 时间线块（正文/批次交错）

const timelineBlocks = computed<ChatRuntimeTimelineBlock[]>(() => {
  // 与正文门禁同一张内容时钟：块列表随投影原地换引用，不订阅就收不到新段。
  void runtimeContentVersion.value;
  // 块里的正文是投影层组合好的最新文本，行对象上的副本可能落后一帧。
  const blocks = resolveProjectedMessage()?.timeline ?? message.value.timeline;
  return Array.isArray(blocks) ? (blocks as ChatRuntimeTimelineBlock[]) : [];
});

/** 有投影块列表时交给共享的 MessageTimelineBlocks 渲染（与子智能体详情共用）。 */
const hasProjectionBlocks = computed(() => timelineBlocks.value.length > 0);

/** 最后一段正文套失败提示（与旧单段形态同源）；块内文本其余保持投影原样。 */
const transformBodyText = (text: string, isLast: boolean): string =>
  isLast ? buildAssistantDisplayContent(message.value, translate, text) : text;

const legacyBodyId = (): string =>
  String(resolveAgentMessageKey?.(message.value, props.item.sourceIndex) || props.item.key);

const subagents = computed<Record<string, unknown>[]>(() => {
  // 子智能体进度是 `item_upsert` 原地写进行对象的，同样必须订阅内容时钟，
  // 否则卡片只在首帧渲染一次、后续 revision 不再更新（与正文门禁同一坑）。
  void runtimeContentVersion.value;
  const items = controller.resolveAgentWorkflowSubagents?.(message.value);
  return Array.isArray(items) ? (items as Record<string, unknown>[]) : [];
});

// 上报本轮的「有内容」状态；轮次行据此套用 MAX_OPEN_TURNS 上限。
const emit = defineEmits<{ (event: 'activity', active: boolean): void }>();

// 无投影块的轮次（纯文本、问候语、旧行）不挂共享块渲染组件，这里补上
// 「无批次」上报；块从有到无（卸载子组件）时同样补报，保持既有清理语义。
watch(
  () => [props.item.key, hasProjectionBlocks.value] as const,
  ([, hasBlocks]) => {
    if (!hasBlocks) emit('activity', false);
  },
  { immediate: true }
);

watch(
  () => props.item.key,
  (key, previousKey) => {
    if (!key || key === previousKey) return;
    // 虚拟列表会复用行组件：轮次身份变化时立即刷新工具投影批次；
    // 批次/条目的展开状态由 MessageTimelineBlocks 随 identityKey 自行复位。
    batchWorkflowItems();
  },
  { immediate: true }
);
</script>

<style scoped>
.timeline-assistant {
  display: flex;
  flex-direction: column;
  gap: 0;
  width: 100%;
  min-width: 0;
}

/* 正文块（.timeline-body-block）与工具分组折叠条（.timeline-group*）的样式
   随块渲染一起移入 MessageTimelineBlocks（与子智能体详情共用）。 */

/* 轮次统计行：高 22px、间距 12px、图标 12px + 数值 11px muted（见 MessageStats 的
   `.messenger-message-stats`），右侧仅保留「复制」图标按钮 22×22 圆角 6。 */
.timeline-body-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  box-sizing: border-box;
  width: 100%;
  min-width: 0;
  height: 22px;
  padding: 0 24px;
}

.timeline-body-footer-gap {
  flex: 1 1 auto;
  min-width: 0;
}

.timeline-body-footer :deep(.messenger-message-stats) {
  flex: 0 1 auto;
  min-width: 0;
  margin: 0;
  gap: 0 12px;
  flex-wrap: nowrap;
  overflow: hidden;
  font-size: 11px;
  color: var(--mz-text-muted, #8a8f99);
}

.timeline-body-footer :deep(.messenger-message-stat) {
  min-width: 0;
  gap: 5px;
  white-space: nowrap;
}

.timeline-body-footer :deep(.messenger-message-stat-icon) {
  width: 12px;
  min-width: 12px;
  font-size: 11px;
}

.timeline-body-footer :deep(.messenger-message-stat-value) {
  color: var(--mz-text-muted, #8a8f99);
}

.timeline-body-footer :deep(.messenger-message-stat.is-status) {
  padding: 0 8px;
  line-height: 18px;
  color: var(--mz-text-muted, #8a8f99);
}

.timeline-body-footer :deep(.messenger-message-stat.is-status .messenger-message-stat-value) {
  color: currentColor;
}

/* 复制按钮：仅图标 22×22（去掉文字标签），hover 才显形。 */
.timeline-body-action {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  line-height: 1;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.16s ease, background-color 0.16s ease;
}

.timeline-assistant:hover .timeline-body-action,
.timeline-body-footer:focus-within .timeline-body-action {
  opacity: 1;
}

.timeline-body-action:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
  color: var(--mz-text-secondary, #3d3d3d);
}

.timeline-body-footer :deep(.messenger-message-footer-copy.timeline-body-icon-action) {
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
}

.timeline-body-footer :deep(.messenger-message-footer-copy.timeline-body-icon-action:hover) {
  background: var(--mz-timeline-hover, #f6f5f3);
  color: var(--mz-text-secondary, #3d3d3d);
}

/* 问候语：桌面端 BodyBlock 的等价物——全宽文本、无气泡、无头像。 */
.timeline-greeting {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.timeline-greeting-text {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-text, #1f2329);
  font-size: calc(14px * var(--messenger-font-scale, 1));
  line-height: 1.7;
}

.timeline-greeting-preview {
  flex: 0 0 auto;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  cursor: pointer;
}

.timeline-greeting-preview:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
  color: var(--mz-text-secondary, #3d3d3d);
}

.timeline-subagent-scope {
  width: 100%;
}
</style>
