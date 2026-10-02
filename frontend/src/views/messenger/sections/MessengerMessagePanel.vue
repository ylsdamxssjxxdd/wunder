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
            <div v-if="agentVirtualTopSpacer" class="messenger-message-virtual-spacer"
              :style="{ height: `${agentVirtualTopSpacer.height}px` }" aria-hidden="true"></div>
            <template v-for="row in agentVirtualRows" :key="row.key">
              <div v-if="row.kind === 'spacer'" class="messenger-message-virtual-spacer"
                :style="{ height: `${row.height}px` }" aria-hidden="true"></div>
              <div v-else-if="row.kind === 'greeting'" class="messenger-greeting-region"
                data-chat-measure :data-virtual-key="row.key">
                <MessengerAgentBubble :controller="controller" :item="row.assistant" />
              </div>
              <MessengerTurnRow v-else :controller="controller" :turn="row" />
            </template>
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
import MessengerTurnRow from './MessengerTurnRow.vue';
import MessengerAgentBubble from './MessengerAgentBubble.vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import MessageMarkdownBody from '@/components/chat/MessageMarkdownBody.vue';
import MessageStats from '@/components/chat/MessageStats.vue';
import { chatPerf } from '@/utils/chatPerf';

// The stable controller carries refs/actions; only this subtree tracks message render dependencies.
const props = defineProps<{ controller: MessengerControllerContext }>();
const agentVirtualRows = props.controller.agentVirtualRows;
const agentVirtualTopSpacer = props.controller.agentVirtualTopSpacer;
const avatarLabel = props.controller.avatarLabel;
const bootLoading = props.controller.bootLoading;
const copyMessageContent = props.controller.copyMessageContent;
const isMessageTtsLoading = props.controller.isMessageTtsLoading;
const isMessageTtsPlaying = props.controller.isMessageTtsPlaying;
const resolveMessageTtsActionLabel = props.controller.resolveMessageTtsActionLabel;
const toggleMessageTtsPlayback = props.controller.toggleMessageTtsPlayback;
const currentUserAvatarImageUrl = props.controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = props.controller.currentUserAvatarStyle;
const currentUsername = props.controller.currentUsername;
const formatTime = props.controller.formatTime;
const handleMessageMarkdownRendered = props.controller.handleMessageMarkdownRendered;
const hasRetainedMessageConversationContext = props.controller.hasRetainedMessageConversationContext;
const hasMessageContent = props.controller.hasMessageContent;
const isOwnMessage = props.controller.isOwnMessage;
const isWorldVoiceLoading = props.controller.isWorldVoiceLoading;
const isWorldVoiceMessage = props.controller.isWorldVoiceMessage;
const isWorldVoicePlaying = props.controller.isWorldVoicePlaying;
const MARKDOWN_STREAM_THROTTLE_MS = props.controller.MARKDOWN_STREAM_THROTTLE_MS;
const openProfilePage = props.controller.openProfilePage;
const replaceWorldAtPathTokens = props.controller.replaceWorldAtPathTokens;
const retainedMessageRenderKind = props.controller.retainedMessageRenderKind;
const resolveWorldMarkdownWorkspacePath = props.controller.resolveWorldMarkdownWorkspacePath;
const resolveWorldMessageKey = props.controller.resolveWorldMessageKey;
const resolveWorldMessageSender = props.controller.resolveWorldMessageSender;
const resolveWorldVoiceActionLabel = props.controller.resolveWorldVoiceActionLabel;
const resolveWorldVoiceDurationLabel = props.controller.resolveWorldVoiceDurationLabel;
const sessionHub = props.controller.sessionHub;
const t = props.controller.t;
const toggleWorldVoicePlayback = props.controller.toggleWorldVoicePlayback;
const worldVirtualBottomSpacer = props.controller.worldVirtualBottomSpacer;
const worldVirtualGroups = props.controller.worldVirtualGroups;
const worldVirtualTopSpacer = props.controller.worldVirtualTopSpacer;
onUpdated(() => chatPerf.count('chat_message_panel_render'));
</script>
