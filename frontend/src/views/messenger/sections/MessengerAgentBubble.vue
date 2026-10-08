<template>
  <div
    class="messenger-message"
    :class="{ mine: isUser, 'messenger-message--timeline': true }"
    :data-virtual-key="item.key"
    :data-turn-id="String(item.message.__runtime_user_turn_id || item.message.user_turn_id || '')"
    :data-message-status="String(item.message.status || '')"
  >
    <div class="messenger-message-main">
      <MessageTimelineAssistant
        v-if="!isUser"
        :controller="controller"
        :item="item"
        :default-open="defaultOpen"
        @activity="emit('activity', $event)"
      />

      <div
        v-if="isUser"
        class="messenger-message-bubble messenger-markdown"
      >
        <MessageMarkdownBody
          :cache-key="`agent:${String(sessionHub.activeConversationKey || '')}:${resolveAgentMessageKey(item.message, item.sourceIndex)}:c${currentContainerId}`"
          :content="String(item.message.content || '')"
          :message="item.message"
          :runtime-message-id="String(item.message.__runtime_message_id || item.message.message_id || '')"
          :runtime-user-turn-id="String(item.message.__runtime_user_turn_id || item.message.user_turn_id || item.message.userTurnId || '')"
          :runtime-model-turn-id="String(item.message.__runtime_model_turn_id || item.message.model_turn_id || item.message.modelTurnId || '')"
          :session-id="String(chatStore.activeSessionId || '')"
          :item-id="String(item.message.item_id || '')"
          :content-truncated="
            item.message.content_truncated === true
          "
          :assistant-display="true"
          :streaming="
            Boolean(
              item.message.stream_incomplete ||
                item.message.workflowStreaming ||
                item.message.reasoningStreaming
            )
          "
          :throttle-ms="MARKDOWN_STREAM_THROTTLE_MS"
          :resolve-workspace-path="resolveAgentMarkdownWorkspacePath"
          @rendered="handleMessageMarkdownRendered(item.key, $event)"
          @history-message-hydrated="Object.assign(item.message, $event, {
            content_truncated: false,
            reasoning_truncated: false,
            workflowItems_truncated: false,
            subagents_truncated: false
          })"
        />
        <div
          v-if="hasUserImageAttachments(item.message)"
          class="message-user-image-grid"
        >
          <button
            v-for="imageItem in resolveUserImageAttachments(item.message)"
            :key="imageItem.key"
            class="message-user-image-btn"
            type="button"
            :title="imageItem.name"
            :aria-label="imageItem.name"
            @click="openResourcePreview({ src: imageItem.src, title: imageItem.name, workspacePath: imageItem.workspacePath, meta: imageItem.workspacePath || imageItem.name, kind: 'image' })"
          >
            <img
              :src="imageItem.src"
              :alt="imageItem.name"
              class="message-user-image"
              loading="lazy"
              decoding="async"
            />
          </button>
        </div>
        <div
          v-if="hasUserAudioAttachments(item.message)"
          class="message-user-audio-grid"
        >
          <div
            v-for="audioItem in resolveUserAudioAttachments(item.message)"
            :key="audioItem.key"
            class="message-user-audio-card"
          >
            <span class="message-user-audio-name" :title="audioItem.name">
              {{ audioItem.name }}
            </span>
            <audio
              class="message-user-audio-player"
              :src="audioItem.src"
              controls
              preload="metadata"
            ></audio>
          </div>
        </div>
      </div>

      <MessageKnowledgeCitation
        v-if="
          !isUser &&
            Array.isArray(item.message.workflowItems) && item.message.workflowItems.length > 0
        "
        :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
      />

      <div
        v-if="isUser && hasMessageContent(item.message.content)"
        class="messenger-message-extra"
      >
        <button
          class="messenger-message-footer-copy"
          :class="{ 'is-active': isMessageTtsPlaying(item.message, item.sourceIndex, 'agent') }"
          type="button"
          :disabled="isMessageTtsLoading(item.message, item.sourceIndex, 'agent')"
          :title="resolveMessageTtsActionLabel(item.message, item.sourceIndex, 'agent')"
          :aria-label="resolveMessageTtsActionLabel(item.message, item.sourceIndex, 'agent')"
          @click="toggleMessageTtsPlayback(item.message, item.sourceIndex, 'agent')"
        >
          <i
            v-if="isMessageTtsLoading(item.message, item.sourceIndex, 'agent')"
            class="fa-solid fa-spinner fa-spin"
            aria-hidden="true"
          ></i>
          <i
            v-else
            :class="isMessageTtsPlaying(item.message, item.sourceIndex, 'agent') ? 'fa-solid fa-pause' : 'fa-solid fa-volume-high'"
            aria-hidden="true"
          ></i>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import MessageMarkdownBody from '@/components/chat/MessageMarkdownBody.vue';
import MessageTimelineAssistant from './MessageTimelineAssistant.vue';

/**
 * 聊天区消息行（§7 时间线形态）。
 *
 * 形态契约（桌面端 `frontend-slint/ui/timeline.slint`）：
 * - **任何消息都不再渲染头像**，也没有「名字 / 时间」标签行；
 * - 用户消息是右对齐的浅灰气泡（无头像、无气泡尾巴），助手轮次由
 *   `MessageTimelineAssistant` 负责全宽正文 + 工具分组时间线；
 * - 行外壳本身保持 `.messenger-message` / `.mine` / `data-turn-id` /
 *   `data-message-status` 这组钩子不变（真机 DOM 审计依赖它们）。
 */
const props = withDefaults(defineProps<{
  controller: MessengerControllerContext;
  item: { key: string; sourceIndex: number; message: Record<string, any> };
  /** 工具分组默认展开（时间线按 MAX_OPEN_TURNS 计算后下发）。 */
  defaultOpen?: boolean;
}>(), {
  defaultOpen: false
});

const emit = defineEmits<{ (event: 'activity', active: boolean): void }>();
const defaultOpen = computed(() => props.defaultOpen === true);
const isUser = computed(() => String(props.item.message?.role || '') === 'user');

const chatStore = props.controller.chatStore;
const currentContainerId = props.controller.currentContainerId;
const handleMessageMarkdownRendered = props.controller.handleMessageMarkdownRendered;
const hasMessageContent = props.controller.hasMessageContent;
const hasUserAudioAttachments = props.controller.hasUserAudioAttachments;
const hasUserImageAttachments = props.controller.hasUserImageAttachments;
const isMessageTtsLoading = props.controller.isMessageTtsLoading;
const isMessageTtsPlaying = props.controller.isMessageTtsPlaying;
const resolveMessageTtsActionLabel = props.controller.resolveMessageTtsActionLabel;
const toggleMessageTtsPlayback = props.controller.toggleMessageTtsPlayback;
const MARKDOWN_STREAM_THROTTLE_MS = props.controller.MARKDOWN_STREAM_THROTTLE_MS;
const MessageKnowledgeCitation = props.controller.MessageKnowledgeCitation;
const openResourcePreview = props.controller.openResourcePreview;
const resolveAgentMarkdownWorkspacePath = props.controller.resolveAgentMarkdownWorkspacePath;
const resolveAgentMessageKey = props.controller.resolveAgentMessageKey;
const resolveUserAudioAttachments = props.controller.resolveUserAudioAttachments;
const resolveUserImageAttachments = props.controller.resolveUserImageAttachments;
const sessionHub = props.controller.sessionHub;
</script>
