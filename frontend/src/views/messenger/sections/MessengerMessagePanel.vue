<template>
        <div
          v-if="sessionHub.activeSection === 'messages'"
          class="messenger-message-panel"
        >
          <div v-if="bootLoading" class="messenger-chat-empty">{{ t('common.loading') }}</div>
          <div
            v-else-if="!hasRetainedMessageConversationContext && retainedMessageRenderKind === ''"
            class="messenger-chat-empty-state"
          >
            <div class="messenger-chat-empty-icon">
              <i class="fa-regular fa-comments" aria-hidden="true"></i>
            </div>
            <div class="messenger-chat-empty-title">{{ t('messenger.empty.selectConversation') }}</div>
          </div>
          <div
            v-else-if="retainedMessageRenderKind === ''"
            class="messenger-chat-empty-state"
          >
            <div class="messenger-chat-empty-icon">
              <i class="fa-regular fa-comments" aria-hidden="true"></i>
            </div>
            <div class="messenger-chat-empty-title">{{ t('messenger.empty.selectConversation') }}</div>
          </div>

          <div v-if="retainedMessageRenderKind === 'agent'">
            <div
              v-if="agentVirtualTopSpacer"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${agentVirtualTopSpacer.height}px` }"
              aria-hidden="true"
            ></div>
            <template v-for="(group, groupIndex) in agentVirtualGroups" :key="`agent-virtual-group:${groupIndex}`">
              <template v-for="item in group" :key="item.key">
              <div
                v-if="
                  !isHiddenInternalMessage(item.message)
                    && (!isCompactionMarkerMessage(item.message) || shouldShowCompactionDivider(item.message))
                "
                class="messenger-message"
                :data-virtual-key="item.key"
                :class="{
                  mine: item.message.role === 'user',
                  'messenger-message--compaction': isCompactionMarkerMessage(item.message)
                }"
              >
              <div v-if="!isCompactionMarkerMessage(item.message)" class="messenger-message-side">
                <button
                  v-if="item.message.role === 'user'"
                  class="messenger-message-avatar messenger-message-avatar--mine-profile messenger-message-avatar--clickable"
                  :style="currentUserAvatarStyle"
                  type="button"
                  :title="t('user.profile.enter')"
                  :aria-label="t('user.profile.enter')"
                  @click="openProfilePage"
                >
                  <img
                    v-if="currentUserAvatarImageUrl"
                    class="messenger-settings-profile-avatar-image"
                    :src="currentUserAvatarImageUrl"
                    alt=""
                  />
                  <span v-else>{{ avatarLabel(currentUsername) }}</span>
                </button>
                <AgentAvatar
                  v-else
                  class="messenger-message-avatar--clickable"
                  size="sm"
                  :state="resolveMessageAgentAvatarState(item.message)"
                  :animated="
                    latestVisibleAgentAssistantMessage === item.message &&
                    resolveMessageAgentAvatarState(item.message) === 'running'
                  "
                  :icon="activeAgentIcon"
                  :name="activeAgentName"
                  :title="activeAgentName"
                  role="button"
                  tabindex="0"
                  :aria-label="t('chat.features.agentSettings')"
                  @click="openActiveAgentSettings"
                  @keydown.enter.prevent="openActiveAgentSettings()"
                  @keydown.space.prevent="openActiveAgentSettings()"
                />
              </div>
              <div class="messenger-message-main">
                <template v-if="isCompactionMarkerMessage(item.message)">
                  <MessageCompactionDivider
                    :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                    :is-streaming="
                      Boolean(
                        item.message.workflowStreaming ||
                          item.message.reasoningStreaming ||
                          item.message.stream_incomplete
                      )
                    "
                    :manual-marker="
                      item.message.manual_compaction_marker === true
                        || item.message.manualCompactionMarker === true
                    "
                    :session-busy="activeMessengerSessionBusy"
                  />
                </template>
                <template v-else-if="isGoalMarkerMessage(item.message)">
                  <MessageGoalDivider :objective="String(item.message.content || '')" />
                </template>
                <template v-else>
                <MessageCompactionDivider
                  v-if="
                    item.message.role === 'assistant' &&
                      shouldShowCompactionDivider(item.message)
                  "
                  :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                  :is-streaming="
                    Boolean(
                      item.message.workflowStreaming ||
                        item.message.reasoningStreaming ||
                        item.message.stream_incomplete
                    )
                  "
                  :session-busy="activeMessengerSessionBusy"
                />
                <div class="messenger-message-meta">
                  <span>{{ item.message.role === 'user' ? t('chat.message.user') : activeAgentName }}</span>
                  <span>{{ formatTime(item.message.created_at) }}</span>
                  <MessageThinking
                    v-if="
                      item.message.role === 'assistant' &&
                        (Boolean(item.message.reasoningStreaming) || String(item.message.reasoning || '').trim())
                    "
                    :content="String(item.message.reasoning || '')"
                    :streaming="Boolean(item.message.reasoningStreaming)"
                    :message="item.message"
                    :runtime-message-id="String(item.message.__runtime_message_id || item.message.message_id || '')"
                    :runtime-user-turn-id="String(item.message.__runtime_user_turn_id || item.message.user_turn_id || item.message.userTurnId || '')"
                    :runtime-model-turn-id="String(item.message.__runtime_model_turn_id || item.message.model_turn_id || item.message.modelTurnId || '')"
                    :session-id="String(chatStore.activeSessionId || '')"
                  />
                </div>
                <div
                  v-if="shouldMountAgentWorkflow(item.message)"
                  class="messenger-workflow-scope chat-shell"
                >
                  <MessageToolWorkflow
                    :key="`workflow:${item.key}`"
                    :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                    :loading="Boolean(item.message.workflowStreaming)"
                    :render-version="buildMessageWorkflowRenderVersion(item.message)"
                    :runtime-message-id="String(item.message.__runtime_message_id || item.message.message_id || '')"
                    :session-id="String(chatStore.activeSessionId || '')"
                    :state-key="`${sessionHub.activeConversationKey}:workflow:${resolveMessageWorkflowStateKey(item.message, item.sourceIndex)}`"
                    :state-aliases="resolveMessageWorkflowStateAliases(item.message, item.sourceIndex, item.key)
                      .map((key) => `${sessionHub.activeConversationKey}:workflow:${key}`)"
                    :visible="
                      Boolean(
                        item.message.stream_incomplete ||
                          item.message.workflowStreaming ||
                          (Array.isArray(item.message.workflowItems) && item.message.workflowItems.length > 0)
                      )
                    "
                    :pending-placeholder="item.message.workflowPendingPlaceholder || null"
                    @layout-change="handleMessageWorkflowLayoutChange(item.key)"
                  />
                  <MessageSubagentPanel
                    v-if="Array.isArray(item.message.subagents) && item.message.subagents.length > 0"
                    :session-id="chatStore.activeSessionId"
                    :items="Array.isArray(item.message.subagents) ? item.message.subagents : []"
                  />
                </div>
                <div
                  v-if="item.message.role === 'user' || shouldMountAgentMessageBubble(item.message)"
                  class="messenger-message-bubble messenger-markdown"
                  :class="{ 'messenger-message-bubble--greeting': isGreetingMessage(item.message) }"
                >
                  <template v-if="isGreetingMessage(item.message)">
                    <div class="messenger-greeting-line">
                      <div class="messenger-greeting-text">{{ item.message.content }}</div>
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
                                      v-for="item in section.items"
                                      :key="`${section.key}-${item.name}`"
                                      :name="item.name"
                                      :display-name="item.displayName"
                                      :description="item.description"
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
                          class="messenger-greeting-preview-btn"
                          type="button"
                          :title="t('chat.promptPreview')"
                          :aria-label="t('chat.promptPreview')"
                          :disabled="agentPromptPreviewLoading"
                          @click.stop="openAgentPromptPreview"
                        >
                          <i class="fa-solid fa-eye" aria-hidden="true"></i>
                        </button>
                      </el-tooltip>
                    </div>
                  </template>
                  <MessageMarkdownBody
                    v-else
                    :cache-key="`agent:${String(sessionHub.activeConversationKey || '')}:${resolveAgentMessageKey(item.message, item.sourceIndex)}:c${currentContainerId}`"
                    :content="String(item.message.content || '')"
                    :message="item.message"
                    :runtime-message-id="String(item.message.__runtime_message_id || item.message.message_id || '')"
                    :runtime-user-turn-id="String(item.message.__runtime_user_turn_id || item.message.user_turn_id || item.message.userTurnId || '')"
                    :runtime-model-turn-id="String(item.message.__runtime_model_turn_id || item.message.model_turn_id || item.message.modelTurnId || '')"
                    :session-id="String(chatStore.activeSessionId || '')"
                    :history-id="item.message.history_id"
                    :content-truncated="
                      item.message.content_truncated === true ||
                        item.message.reasoning_truncated === true ||
                        item.message.workflowItems_truncated === true ||
                        item.message.subagents_truncated === true
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
                    v-if="item.message.role === 'user' && hasUserImageAttachments(item.message)"
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
                    v-if="item.message.role === 'user' && hasUserAudioAttachments(item.message)"
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
                    item.message.role === 'assistant' &&
                      Array.isArray(item.message.workflowItems) && item.message.workflowItems.length > 0
                  "
                  :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                />
                  <div
                    v-if="hasMessageContent(item.message.content) || item.message.role === 'assistant'"
                    class="messenger-message-extra"
                  >
                    <MessageStats
                      v-if="item.message.role === 'assistant'"
                      :message="item.message"
                      :active-session-busy="activeMessengerSessionBusy"
                      :latest-visible-assistant="latestVisibleAgentAssistantMessage === item.message"
                    />
                  <button
                    v-if="shouldShowAgentResumeButton(item.message)"
                    class="messenger-message-footer-copy"
                    type="button"
                    :title="t('chat.message.resume')"
                    :aria-label="t('chat.message.resume')"
                    @click="resumeAgentMessage(item.message)"
                  >
                    <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
                  </button>
                  <MessageFeedbackActions
                    v-if="item.message.role === 'assistant'"
                    :message="item.message"
                  />
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
                  <button
                    class="messenger-message-footer-copy"
                    type="button"
                    :title="t('chat.message.copy')"
                    :aria-label="t('chat.message.copy')"
                    @click="copyMessageContent(item.message)"
                  >
                    <i class="fa-solid fa-clone" aria-hidden="true"></i>
                  </button>
                </div>
                </template>
              </div>
              </div>
            </template>
            </template>
            <div
              v-if="agentVirtualBottomSpacer"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${agentVirtualBottomSpacer.height}px` }"
              aria-hidden="true"
            ></div>
          </div>

          <div v-if="retainedMessageRenderKind === 'world'">
            <div
              v-if="worldVirtualTopSpacer"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${worldVirtualTopSpacer.height}px` }"
              aria-hidden="true"
            ></div>
            <template v-for="(group, groupIndex) in worldVirtualGroups" :key="`world-virtual-group:${groupIndex}`">
              <div
                v-for="item in group"
                :key="item.key"
                class="messenger-message"
                :data-virtual-key="item.key"
                :id="item.domId"
                :class="{ mine: isOwnMessage(item.message) }"
              >
              <div class="messenger-message-side">
                <button
                  class="messenger-message-avatar"
                  :class="{
                    'messenger-message-avatar--mine-profile': isOwnMessage(item.message),
                    'messenger-message-avatar--clickable': true
                  }"
                  :style="isOwnMessage(item.message) ? currentUserAvatarStyle : undefined"
                  type="button"
                  :title="t('user.profile.enter')"
                  :aria-label="t('user.profile.enter')"
                  @click="openProfilePage"
                >
                  <template v-if="isOwnMessage(item.message)">
                    <img
                      v-if="currentUserAvatarImageUrl"
                      class="messenger-settings-profile-avatar-image"
                      :src="currentUserAvatarImageUrl"
                      alt=""
                    />
                    <span v-else>{{ avatarLabel(currentUsername) }}</span>
                  </template>
                  <template v-else>
                    {{ avatarLabel(resolveWorldMessageSender(item.message)) }}
                  </template>
                </button>
              </div>
              <div class="messenger-message-main">
                <div class="messenger-message-meta">
                  <span>{{ isOwnMessage(item.message) ? t('chat.message.user') : resolveWorldMessageSender(item.message) }}</span>
                  <span>{{ formatTime(item.message.created_at) }}</span>
                </div>
                <div
                  class="messenger-message-bubble"
                  :class="isWorldVoiceMessage(item.message) ? 'messenger-message-bubble--voice' : 'messenger-markdown'"
                >
                  <template v-if="isWorldVoiceMessage(item.message)">
                    <div class="messenger-world-voice-card">
                      <button
                        class="messenger-world-voice-play-btn"
                        type="button"
                        :disabled="isWorldVoiceLoading(item.message)"
                        :title="resolveWorldVoiceActionLabel(item.message)"
                        :aria-label="resolveWorldVoiceActionLabel(item.message)"
                        @click="toggleWorldVoicePlayback(item.message)"
                      >
                        <i
                          v-if="isWorldVoiceLoading(item.message)"
                          class="fa-solid fa-spinner fa-spin"
                          aria-hidden="true"
                        ></i>
                        <i
                          v-else
                          :class="isWorldVoicePlaying(item.message) ? 'fa-solid fa-pause' : 'fa-solid fa-play'"
                          aria-hidden="true"
                        ></i>
                      </button>
                      <div class="messenger-world-voice-content">
                        <div class="messenger-world-voice-title">{{ t('messenger.world.voice.title') }}</div>
                        <div
                          class="messenger-world-voice-wave"
                          :class="{ 'is-playing': isWorldVoicePlaying(item.message) }"
                          aria-hidden="true"
                        >
                          <span
                            v-for="waveIndex in 10"
                            :key="waveIndex"
                            class="messenger-world-voice-wave-bar"
                            :style="{ '--voice-wave-delay': `${waveIndex * 0.09}s` }"
                          ></span>
                        </div>
                        <div class="messenger-world-voice-duration">
                          {{ resolveWorldVoiceDurationLabel(item.message) }}
                        </div>
                      </div>
                    </div>
                  </template>
                  <MessageMarkdownBody
                    v-else
                    :cache-key="`world:${String(sessionHub.activeConversationKey || '')}:${resolveWorldMessageKey(item.message)}`"
                    :content="replaceWorldAtPathTokens(String(item.message.content || ''), String(item.message.sender_user_id || '').trim())"
                    :message="item.message"
                    :streaming="false"
                    :throttle-ms="MARKDOWN_STREAM_THROTTLE_MS"
                    :resolve-workspace-path="resolveWorldMarkdownWorkspacePath"
                    :workspace-path-context="String(item.message.sender_user_id || '').trim()"
                    @rendered="handleMessageMarkdownRendered(item.key, $event)"
                  />
                </div>
                <div
                  v-if="!isWorldVoiceMessage(item.message) && hasMessageContent(item.message.content)"
                  class="messenger-message-extra"
                >
                  <button
                    class="messenger-message-footer-copy"
                    :class="{ 'is-active': isMessageTtsPlaying(item.message, item.sourceIndex, 'world') }"
                    type="button"
                    :disabled="isMessageTtsLoading(item.message, item.sourceIndex, 'world')"
                    :title="resolveMessageTtsActionLabel(item.message, item.sourceIndex, 'world')"
                    :aria-label="resolveMessageTtsActionLabel(item.message, item.sourceIndex, 'world')"
                    @click="toggleMessageTtsPlayback(item.message, item.sourceIndex, 'world')"
                  >
                    <i
                      v-if="isMessageTtsLoading(item.message, item.sourceIndex, 'world')"
                      class="fa-solid fa-spinner fa-spin"
                      aria-hidden="true"
                    ></i>
                    <i
                      v-else
                      :class="isMessageTtsPlaying(item.message, item.sourceIndex, 'world') ? 'fa-solid fa-pause' : 'fa-solid fa-volume-high'"
                      aria-hidden="true"
                    ></i>
                  </button>
                  <button
                    class="messenger-message-footer-copy"
                    type="button"
                    :title="t('chat.message.copy')"
                    :aria-label="t('chat.message.copy')"
                    @click="copyMessageContent(item.message)"
                  >
                    <i class="fa-solid fa-clone" aria-hidden="true"></i>
                  </button>
                </div>
              </div>
              </div>
            </template>
            <div
              v-if="worldVirtualBottomSpacer"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${worldVirtualBottomSpacer.height}px` }"
              aria-hidden="true"
            ></div>
          </div>
          <div
            v-show="retainedMessageRenderKind !== 'agent' && retainedMessageRenderKind !== 'world'"
            class="messenger-chat-empty"
          >
            {{ t('messenger.empty.selectConversation') }}
          </div>
        </div>
</template>

<script setup lang="ts">
import { onUpdated } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import MessageMarkdownBody from '@/components/chat/MessageMarkdownBody.vue';
import MessageStats from '@/components/chat/MessageStats.vue';
import { chatPerf } from '@/utils/chatPerf';

// The stable controller carries refs/actions; only this subtree tracks message render dependencies.
const props = defineProps<{ controller: MessengerControllerContext }>();
const AbilityTooltipListItem = props.controller.AbilityTooltipListItem;
const activeAgentIcon = props.controller.activeAgentIcon;
const activeAgentName = props.controller.activeAgentName;
const activeMessengerSessionBusy = props.controller.activeMessengerSessionBusy;
const agentAbilitySections = props.controller.agentAbilitySections;
const agentAbilityTooltipOptions = props.controller.agentAbilityTooltipOptions;
const agentAbilityTooltipRef = props.controller.agentAbilityTooltipRef;
const AgentAvatar = props.controller.AgentAvatar;
const agentPromptPreviewLoading = props.controller.agentPromptPreviewLoading;
const agentVirtualBottomSpacer = props.controller.agentVirtualBottomSpacer;
const agentVirtualGroups = props.controller.agentVirtualGroups;
const agentVirtualTopSpacer = props.controller.agentVirtualTopSpacer;
const agentToolSummaryError = props.controller.agentToolSummaryError;
const agentToolSummaryLoading = props.controller.agentToolSummaryLoading;
const avatarLabel = props.controller.avatarLabel;
const bootLoading = props.controller.bootLoading;
const buildMessageWorkflowRenderVersion = props.controller.buildMessageWorkflowRenderVersion;
const chatStore = props.controller.chatStore;
const copyMessageContent = props.controller.copyMessageContent;
const isMessageTtsLoading = props.controller.isMessageTtsLoading;
const isMessageTtsPlaying = props.controller.isMessageTtsPlaying;
const resolveMessageTtsActionLabel = props.controller.resolveMessageTtsActionLabel;
const toggleMessageTtsPlayback = props.controller.toggleMessageTtsPlayback;
const currentContainerId = props.controller.currentContainerId;
const currentUserAvatarImageUrl = props.controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = props.controller.currentUserAvatarStyle;
const currentUsername = props.controller.currentUsername;
const formatTime = props.controller.formatTime;
const handleAgentAbilityTooltipHide = props.controller.handleAgentAbilityTooltipHide;
const handleAgentAbilityTooltipShow = props.controller.handleAgentAbilityTooltipShow;
const handleMessageMarkdownRendered = props.controller.handleMessageMarkdownRendered;
const handleMessageWorkflowLayoutChange = props.controller.handleMessageWorkflowLayoutChange;
const hasAgentAbilitySummary = props.controller.hasAgentAbilitySummary;
const hasRetainedMessageConversationContext = props.controller.hasRetainedMessageConversationContext;
const hasMessageContent = props.controller.hasMessageContent;
const hasUserAudioAttachments = props.controller.hasUserAudioAttachments;
const hasUserImageAttachments = props.controller.hasUserImageAttachments;
const isCompactionMarkerMessage = props.controller.isCompactionMarkerMessage;
const isGoalMarkerMessage = props.controller.isGoalMarkerMessage;
const isGreetingMessage = props.controller.isGreetingMessage;
const isHiddenInternalMessage = props.controller.isHiddenInternalMessage;
const isOwnMessage = props.controller.isOwnMessage;
const isWorldVoiceLoading = props.controller.isWorldVoiceLoading;
const isWorldVoiceMessage = props.controller.isWorldVoiceMessage;
const isWorldVoicePlaying = props.controller.isWorldVoicePlaying;
const latestVisibleAgentAssistantMessage = props.controller.latestVisibleAgentAssistantMessage;
const MARKDOWN_STREAM_THROTTLE_MS = props.controller.MARKDOWN_STREAM_THROTTLE_MS;
const MessageCompactionDivider = props.controller.MessageCompactionDivider;
const MessageGoalDivider = props.controller.MessageGoalDivider;
const MessageFeedbackActions = props.controller.MessageFeedbackActions;
const MessageKnowledgeCitation = props.controller.MessageKnowledgeCitation;
const MessageSubagentPanel = props.controller.MessageSubagentPanel;
const MessageThinking = props.controller.MessageThinking;
const MessageToolWorkflow = props.controller.MessageToolWorkflow;
const openActiveAgentSettings = props.controller.openActiveAgentSettings;
const openAgentPromptPreview = props.controller.openAgentPromptPreview;
const openResourcePreview = props.controller.openResourcePreview;
const openProfilePage = props.controller.openProfilePage;
const replaceWorldAtPathTokens = props.controller.replaceWorldAtPathTokens;
const resolveAgentMarkdownWorkspacePath = props.controller.resolveAgentMarkdownWorkspacePath;
const resolveAgentMessageKey = props.controller.resolveAgentMessageKey;
const resolveMessageWorkflowStateKey = props.controller.resolveMessageWorkflowStateKey;
const resolveMessageWorkflowStateAliases = props.controller.resolveMessageWorkflowStateAliases;
const retainedMessageRenderKind = props.controller.retainedMessageRenderKind;
const resolveMessageAgentAvatarState = props.controller.resolveMessageAgentAvatarState;
const resolveUserAudioAttachments = props.controller.resolveUserAudioAttachments;
const resolveUserImageAttachments = props.controller.resolveUserImageAttachments;
const resolveWorldMarkdownWorkspacePath = props.controller.resolveWorldMarkdownWorkspacePath;
const resolveWorldMessageKey = props.controller.resolveWorldMessageKey;
const resolveWorldMessageSender = props.controller.resolveWorldMessageSender;
const resolveWorldVoiceActionLabel = props.controller.resolveWorldVoiceActionLabel;
const resolveWorldVoiceDurationLabel = props.controller.resolveWorldVoiceDurationLabel;
const resumeAgentMessage = props.controller.resumeAgentMessage;
const sessionHub = props.controller.sessionHub;
const shouldMountAgentMessageBubble = props.controller.shouldMountAgentMessageBubble;
const shouldMountAgentWorkflow = props.controller.shouldMountAgentWorkflow;
const shouldShowAgentResumeButton = props.controller.shouldShowAgentResumeButton;
const shouldShowCompactionDivider = props.controller.shouldShowCompactionDivider;
const t = props.controller.t;
const toggleWorldVoicePlayback = props.controller.toggleWorldVoicePlayback;
const worldVirtualBottomSpacer = props.controller.worldVirtualBottomSpacer;
const worldVirtualGroups = props.controller.worldVirtualGroups;
const worldVirtualTopSpacer = props.controller.worldVirtualTopSpacer;
onUpdated(() => chatPerf.count('chat_message_panel_render'));
</script>
