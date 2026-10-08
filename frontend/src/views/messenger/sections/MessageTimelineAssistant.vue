<template>
  <div
    class="timeline-assistant"
    :data-turn-id="String(message.__runtime_user_turn_id || message.user_turn_id || '')"
  >
    <div
      v-if="hasBody"
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
        :cache-key="`agent:${conversationKey}:${resolveAgentMessageKey(message, item.sourceIndex)}:c${currentContainerId}`"
        :content="displayContent"
        :message="message"
        :runtime-message-id="runtimeMessageId"
        :runtime-user-turn-id="runtimeUserTurnId"
        :runtime-model-turn-id="runtimeModelTurnId"
        :session-id="activeSessionId"
        :item-id="String(message.item_id || '')"
        :content-truncated="message.content_truncated === true"
        :assistant-display="true"
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
      左侧是图标 12px + 数值 11px muted 的统计，右侧是 52×22 圆角 6 的复制/下载。
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
        <span class="timeline-body-action-label">{{ t('common.copy') }}</span>
      </button>
      <button
        class="timeline-body-action"
        type="button"
        :title="t('chat.timeline.downloadMarkdown')"
        :aria-label="t('chat.timeline.downloadMarkdown')"
        @click="handleDownload"
      >
        <i class="fa-solid fa-download" aria-hidden="true"></i>
        <span class="timeline-body-action-label">{{ t('common.save') }}</span>
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
      <MessageFeedbackActions :message="message" />
      <button
        class="messenger-message-footer-copy timeline-body-icon-action"
        :class="{ 'is-active': isMessageTtsPlaying(message, item.sourceIndex, 'agent') }"
        type="button"
        :disabled="isMessageTtsLoading(message, item.sourceIndex, 'agent')"
        :title="resolveMessageTtsActionLabel(message, item.sourceIndex, 'agent')"
        :aria-label="resolveMessageTtsActionLabel(message, item.sourceIndex, 'agent')"
        @click="toggleMessageTtsPlayback(message, item.sourceIndex, 'agent')"
      >
        <i
          v-if="isMessageTtsLoading(message, item.sourceIndex, 'agent')"
          class="fa-solid fa-spinner fa-spin"
          aria-hidden="true"
        ></i>
        <i
          v-else
          :class="isMessageTtsPlaying(message, item.sourceIndex, 'agent') ? 'fa-solid fa-pause' : 'fa-solid fa-volume-high'"
          aria-hidden="true"
        ></i>
      </button>
    </div>

    <div v-if="showEntryGroup" class="timeline-group" :class="{ 'is-open': groupOpen }">
      <button
        class="timeline-group-head"
        type="button"
        :aria-expanded="groupOpen"
        @click="toggleGroup"
      >
        <span class="timeline-group-gutter" aria-hidden="true">
          <span v-if="groupOpen" class="timeline-group-gutter-line"></span>
        </span>
        <i :class="['fa-solid', 'fa-caret-right', 'timeline-group-arrow', { 'is-open': groupOpen }]" aria-hidden="true"></i>
        <span class="timeline-group-title">{{ groupTitle }}</span>
        <span v-if="!groupOpen && latestEntrySummary" class="timeline-group-latest">{{ latestEntrySummary }}</span>
      </button>

      <div v-if="groupOpen" class="timeline-group-body">
        <MessageTimelineThinkingEntry
          v-if="reasoningEntry"
          :entry="reasoningEntry"
          :open="reasoningOpen"
          @toggle="toggleReasoning"
        />

        <MessageTimelineToolEntry
          v-for="entry in visibleToolEntries"
          :key="entry.key"
          :entry="entry"
          :open="entryOpenKeys.includes(entry.key)"
          :patch-view="patchViewFor(entry)"
          @toggle="toggleEntry(entry.key)"
        />

        <button v-if="hiddenToolCount > 0" class="timeline-group-more" type="button" @click="showMoreEntries">
          {{ t('chat.timeline.showMoreEntries', { count: hiddenToolCount }) }}
        </button>

        <div v-if="omittedRuns > 0" class="timeline-group-note" role="note">
          {{ t('chat.timeline.omittedRuns', { count: omittedRuns }) }}
        </div>
      </div>
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
import MessageTimelineThinkingEntry from '@/components/chat/MessageTimelineThinkingEntry.vue';
import MessageTimelineToolEntry from '@/components/chat/MessageTimelineToolEntry.vue';
import { resolveRuntimeMessageContentSource, resolveRuntimeMessageContentSubscriptionIds } from '@/components/chat/messageRuntimeContent';
import {
  TIMELINE_ENTRY_PAGE_SIZE,
  TIMELINE_ENTRY_RENDER_LIMIT,
  buildTimelineToolEntries,
  type TimelineReasoningEntry,
  type TimelineToolEntry
} from '@/components/chat/toolTimelineModel';
import { buildTimelinePatchView } from '@/components/chat/toolTimelinePatch';
import { createToolWorkflowRenderBatcher } from '@/components/chat/toolWorkflowRenderBatcher';
import { MAX_OPEN_ENTRIES_PER_TURN } from '@/components/chat/timelineGroupState';
import type { ToolWorkflowPatchView } from '@/components/chat/toolWorkflowTypes';
import type { WorkflowItem } from '@/components/chat/toolWorkflowRunModel';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';

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
const isMessageTtsLoading = controller.isMessageTtsLoading;
const isMessageTtsPlaying = controller.isMessageTtsPlaying;
const resolveMessageTtsActionLabel = controller.resolveMessageTtsActionLabel;
const toggleMessageTtsPlayback = controller.toggleMessageTtsPlayback;
const MessageFeedbackActions = controller.MessageFeedbackActions;
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

const handleDownload = (): void => {
  const content = String(displayContent.value || '');
  if (!content.trim()) return;
  const objectUrl = URL.createObjectURL(new Blob([content], { type: 'text/markdown;charset=utf-8' }));
  try {
    saveObjectUrlAsFile(objectUrl, `reply-${props.item.sourceIndex + 1}.md`);
  } finally {
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 4000);
  }
};

// ------------------------------------------------------- 工具/思考条目

// 只在真的存在工具事件时才驱动重建时钟：纯文本流式轮次不进入这条热路径。
const hasToolProjection = computed(() => {
  const items = Array.isArray(message.value.workflowItems) ? message.value.workflowItems : [];
  return items.length > 0 || Boolean(message.value.workflowStreaming);
});

const workflowItems = ref<WorkflowItem[]>([]);
const batchWorkflowItems = () => {
  const sessionId = activeSessionId.value;
  const projected = sessionId
    ? resolveRuntimeMessageContentSource({
        projection: chatStore.runtimeProjection,
        sessionId,
        runtimeMessageId: runtimeMessageId.value,
        runtimeUserTurnId: runtimeUserTurnId.value,
        runtimeModelTurnId: runtimeModelTurnId.value,
        message: message.value
      })
    : null;
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

const toolEntryModel = computed(() => buildTimelineToolEntries(workflowItems.value, t));
const allToolEntries = computed(() => toolEntryModel.value.entries);
const omittedRuns = computed(() => toolEntryModel.value.omittedRuns);

const entryRenderLimit = ref(TIMELINE_ENTRY_RENDER_LIMIT);
const visibleToolEntries = computed<TimelineToolEntry[]>(() => {
  const entries = allToolEntries.value;
  const limit = Math.max(entryRenderLimit.value, 1);
  return entries.length > limit ? entries.slice(entries.length - limit) : entries;
});
const hiddenToolCount = computed(() => Math.max(allToolEntries.value.length - visibleToolEntries.value.length, 0));
const showMoreEntries = (): void => {
  entryRenderLimit.value = Math.min(entryRenderLimit.value + TIMELINE_ENTRY_PAGE_SIZE, 400);
};

const toolCount = computed(() => allToolEntries.value.filter((entry) => entry.kind === 'tool').length);

const reasoningText = computed(() => {
  // 与正文门禁同理：投影/行对象是原地改写的，必须显式订阅内容时钟，
  // 否则思考流式增量与终态补写都不会让这个计算属性失效。
  void runtimeContentVersion.value;
  const sessionId = activeSessionId.value;
  const projected = sessionId
    ? resolveRuntimeMessageContentSource({
        projection: chatStore.runtimeProjection,
        sessionId,
        runtimeMessageId: runtimeMessageId.value,
        runtimeUserTurnId: runtimeUserTurnId.value,
        runtimeModelTurnId: runtimeModelTurnId.value,
        message: message.value
      })
    : null;
  return String(projected?.reasoning ?? message.value.reasoning ?? '');
});

const reasoningEntry = computed<TimelineReasoningEntry | null>(() => {
  const text = reasoningText.value;
  const streaming = Boolean(message.value.reasoningStreaming);
  if (!text.trim() && !streaming) return null;
  const normalized = text.replace(/\s+/g, ' ').trim();
  return {
    kind: 'reasoning',
    key: `${props.item.key}:reasoning`,
    streaming,
    summary: normalized.length > 160 ? `${normalized.slice(0, 160)}…` : normalized,
    text
  };
});

const patchViews = computed<Record<string, ToolWorkflowPatchView | null>>(() => {
  const views: Record<string, ToolWorkflowPatchView | null> = {};
  allToolEntries.value.forEach((entry) => {
    if (entry.kind !== 'tool') return;
    views[entry.key] = buildTimelinePatchView(entry.run, t);
  });
  return views;
});

const patchViewFor = (entry: TimelineToolEntry): ToolWorkflowPatchView | null =>
  patchViews.value[entry.key] || null;

const subagents = computed<Record<string, unknown>[]>(() => {
  // 子智能体进度是 `item_upsert` 原地写进行对象的，同样必须订阅内容时钟，
  // 否则卡片只在首帧渲染一次、后续 revision 不再更新（与正文门禁同一坑）。
  void runtimeContentVersion.value;
  const items = controller.resolveAgentWorkflowSubagents?.(message.value);
  return Array.isArray(items) ? (items as Record<string, unknown>[]) : [];
});

// ------------------------------------------------------------ 展开状态

const showEntryGroup = computed(() => Boolean(reasoningEntry.value) || allToolEntries.value.length > 0);
const groupOpen = ref(props.defaultOpen);
const reasoningOpen = ref(false);
const entryOpenKeys = ref<string[]>([]);
const ENTRY_OPEN_LIMIT = MAX_OPEN_ENTRIES_PER_TURN;

// 上报本轮的「有内容」状态；轮次行据此套用 MAX_OPEN_TURNS 上限。
const emit = defineEmits<{ (event: 'activity', active: boolean): void }>();
watch(
  showEntryGroup,
  (visible) => emit('activity', Boolean(visible)),
  { immediate: true }
);

const groupTitle = computed(() =>
  groupOpen.value
    ? t('chat.timeline.toolGroupOpen', { count: Math.max(allToolEntries.value.length, 1) })
    : t('chat.timeline.processed')
);

const toggleGroup = (): void => {
  groupOpen.value = !groupOpen.value;
};

const toggleReasoning = (): void => {
  reasoningOpen.value = !reasoningOpen.value;
};

const toggleEntry = (key: string): void => {
  const next = new Set(entryOpenKeys.value);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
    while (next.size > ENTRY_OPEN_LIMIT) {
      const firstKey = next.values().next().value as string | undefined;
      if (!firstKey || firstKey === key) break;
      next.delete(firstKey);
    }
  }
  entryOpenKeys.value = Array.from(next);
};

const latestEntrySummary = computed(() => {
  const entries = allToolEntries.value;
  const last = entries.length > 0 ? entries[entries.length - 1] : null;
  if (last) return last.summary || last.toolLabel;
  return reasoningEntry.value?.summary || '';
});

watch(
  () => props.item.key,
  (key, previousKey) => {
    if (!key || key === previousKey) return;
    // 虚拟列表会复用行组件：轮次身份变化时复位局部展开状态。
    reasoningOpen.value = false;
    entryOpenKeys.value = [];
    entryRenderLimit.value = TIMELINE_ENTRY_RENDER_LIMIT;
    batchWorkflowItems();
  },
  { immediate: true }
);

// 默认开合由时间线按 MAX_OPEN_TURNS 计算后下发（只跟随下发的值）。
watch(
  () => props.defaultOpen,
  (open) => {
    groupOpen.value = Boolean(open);
  }
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

/* §7.3 A BodyBlock：全宽、无气泡；左右内边距 24px、上下 6px；
   hover 底 `hover` 45% 透明；正文 14px `text`。 */
.timeline-body-block {
  position: relative;
  width: 100%;
  min-width: 0;
  padding: 6px 24px;
  color: var(--mz-text, #1f2329);
  font-size: calc(14px * var(--messenger-font-scale, 1));
  line-height: 1.7;
  transition: background-color 0.12s ease;
}

.timeline-body-block:hover {
  background: rgba(246, 245, 243, 0.45);
}

/* 轮次统计行：高 22px、间距 12px、图标 12px + 数值 11px muted（见 MessageStats 的
   `.messenger-message-stats`），右侧复制/下载按钮 52×22 圆角 6。 */
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

.timeline-body-action {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  box-sizing: border-box;
  width: 52px;
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

.timeline-body-action-label {
  font-size: 11px;
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

.timeline-body-footer :deep(.messenger-message-feedback-actions) {
  gap: 4px;
}

.timeline-body-footer :deep(.messenger-message-feedback-actions .messenger-message-footer-copy) {
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
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

/* ------------------------------------------------------- 工具分组折叠条 */

.timeline-group {
  width: 100%;
  min-width: 0;
}

/* 折叠态是桌面端的「已处理」分隔条（30px、12px muted）；展开态是工具分组条
   （32px、13px text-secondary），展开时沟槽画连接线。 */
.timeline-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 30px;
  padding: 0 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.timeline-group.is-open .timeline-group-head {
  height: 32px;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 13px;
}

.timeline-group-head:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
}

.timeline-group-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 20px;
}

.timeline-group-gutter-line {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 9.5px;
  width: 1px;
  background: var(--mz-timeline-line, #e2dfda);
}

.timeline-group-arrow {
  flex: 0 0 auto;
  width: 12px;
  font-size: 11px;
  color: var(--mz-text-muted, #8a8f99);
  transition: transform 0.16s ease;
}

.timeline-group-arrow.is-open {
  transform: rotate(90deg);
}

.timeline-group-title {
  flex: 0 0 auto;
}

.timeline-group-latest {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-text-muted, #8a8f99);
  font-weight: 400;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 条目行距 0：每条目自带 20px 沟槽，连接线在相邻行之间连续。 */
.timeline-group-body {
  display: flex;
  flex-direction: column;
  gap: 0;
  min-width: 0;
}

.timeline-group-more {
  align-self: flex-start;
  margin: 4px 0 4px 44px;
  padding: 3px 8px;
  border: 1px dashed var(--mz-border-strong, #d8d5d0);
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  cursor: pointer;
}

.timeline-group-note {
  margin: 4px 0 4px 44px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}

.timeline-subagent-scope {
  width: 100%;
}
</style>
