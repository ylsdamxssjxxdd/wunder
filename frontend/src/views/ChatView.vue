<template>
  <div class="chat-shell">
    <div class="app-shell">
      <header class="topbar">
        <div class="brand">
          <div class="brand-mark">AI</div>
          <div class="brand-meta">
            <div class="brand-title-row">
              <div class="brand-title">{{ t('chat.title') }}</div>
            </div>
            <div class="brand-sub">
              <span v-if="activeAgentSubtitle" class="brand-subtitle">{{ activeAgentSubtitle }}</span>
              <span v-if="demoMode" class="demo-badge">{{ t('user.demoMode') }}</span>
            </div>
          </div>
        </div>
        <div class="topbar-actions">
          <div v-if="isCompactLayout" class="topbar-compact-actions">
            <button
              class="topbar-panel-btn"
              type="button"
              :title="t('chat.workspacePanel')"
              :aria-label="t('chat.workspacePanel')"
              @click="openWorkspaceDialog"
            >
              <i class="fa-solid fa-folder-open topbar-icon" aria-hidden="true"></i>
              <span class="topbar-panel-text">{{ t('chat.workspacePanel') }}</span>
            </button>
            <button
              class="topbar-panel-btn"
              type="button"
              :title="t('chat.history')"
              :aria-label="t('chat.history')"
              @click="openHistoryDialog"
            >
              <i class="fa-solid fa-clock-rotate-left topbar-icon" aria-hidden="true"></i>
              <span class="topbar-panel-text">{{ t('chat.history') }}</span>
            </button>
          </div>
          <button
            class="new-chat-btn"
            type="button"
            :title="t('chat.newSession')"
            :aria-label="t('chat.newSession')"
            @click="handleCreateSession"
          >
            {{ t('chat.newSession') }}
          </button>
          <button
            class="topbar-icon-btn"
            type="button"
            :title="t('nav.world')"
            :aria-label="t('nav.world')"
            @click="handleOpenPortal"
          >
            <i class="fa-solid fa-earth-asia topbar-icon" aria-hidden="true"></i>
          </button>
          <button
            v-if="desktopMode"
            class="topbar-icon-btn"
            type="button"
            :title="t('desktop.settings.containers')"
            :aria-label="t('desktop.settings.containers')"
            @click="handleOpenContainerSettings"
          >
            <i class="fa-solid fa-box-archive topbar-icon" aria-hidden="true"></i>
          </button>
          <button
            v-if="desktopMode"
            class="topbar-icon-btn"
            type="button"
            :title="t('desktop.settings.system')"
            :aria-label="t('desktop.settings.system')"
            @click="handleOpenSystemSettings"
          >
            <i class="fa-solid fa-sliders topbar-icon" aria-hidden="true"></i>
          </button>
          <div ref="featureMenuRef" class="topbar-feature-menu-wrap">
            <button
              class="topbar-panel-btn topbar-feature-btn"
              type="button"
              :title="t('chat.features')"
              :aria-label="t('chat.features')"
              @click.stop="toggleFeatureMenu"
            >
              <i class="fa-solid fa-sliders topbar-icon" aria-hidden="true"></i>
            </button>
            <div v-if="featureMenuVisible" class="topbar-feature-menu">
              <div class="topbar-feature-transport" :class="featureTransportClass">
                <span class="topbar-feature-transport-dot" aria-hidden="true"></span>
                <span>{{ featureTransportText }}</span>
              </div>
              <button
                class="topbar-feature-item"
                type="button"
                @click="handleFeatureAction('agent-settings')"
              >
                <i class="fa-solid fa-pen-to-square topbar-icon" aria-hidden="true"></i>
                <span>{{ t('chat.features.agentSettings') }}</span>
              </button>
              <button class="topbar-feature-item" type="button" @click="handleFeatureAction('cron')">
                <i class="fa-solid fa-clock topbar-icon" aria-hidden="true"></i>
                <span>{{ t('chat.features.cron') }}</span>
              </button>
              <button class="topbar-feature-item" type="button" @click="handleFeatureAction('channels')">
                <i class="fa-solid fa-share-nodes topbar-icon" aria-hidden="true"></i>
                <span>{{ t('chat.features.channels') }}</span>
              </button>
              <button
                class="topbar-feature-item topbar-feature-item-danger"
                type="button"
                @click="handleFeatureAction('agent-delete')"
              >
                <i class="fa-solid fa-trash-can topbar-icon" aria-hidden="true"></i>
                <span>{{ t('chat.features.agentDelete') }}</span>
              </button>
            </div>
          </div>
          <ThemeToggle />
          <div class="topbar-user">
            <button
              class="user-meta user-meta-btn"
              type="button"
              :aria-label="t('user.profile.enter')"
              @click="handleOpenProfile"
            >
              <div class="user-name">{{ currentUser?.username || t('user.guest') }}</div>
              <div class="user-level">{{ t('user.unitLabel', { unit: currentUserUnitLabel }) }}</div>
            </button>
            <button class="logout-btn" type="button" @click="handleLogout">
              {{ t('nav.logout') }}
            </button>
          </div>
        </div>
      </header>

      <div class="main-grid">
        <aside v-if="!isCompactLayout" class="sidebar">
          <div class="glass-card info-panel">
            <WorkspacePanel :agent-id="activeAgentId" :container-id="activeSandboxContainerId" />
          </div>

          <div class="glass-card history-panel">
            <div class="panel-header">
              <span class="history-title">{{ t('chat.history') }}</span>
            </div>
            <div
              ref="historyListRef"
              :class="['history-container', { virtual: historyVirtual }]"
              @scroll="handleHistoryScroll"
            >
              <div
                v-if="!historySessions.length"
                class="history-empty"
                :title="t('chat.history.empty')"
                :aria-label="t('chat.history.empty')"
                role="status"
              >
                <i class="fa-solid fa-inbox history-empty-icon" aria-hidden="true"></i>
              </div>
              <div
                v-if="historyPaddingTop && historySessions.length"
                class="history-spacer"
                :style="{ height: `${historyPaddingTop}px` }"
              ></div>
              <div
                v-for="session in historySessions"
                :key="session.id"
                :class="['history-item', session.id === chatStore.activeSessionId ? 'active' : '', session.is_main ? 'is-main' : '']"
                @click="handleSelectSession(session.id)"
            >
              <div class="history-info">
                <span
                  v-if="chatStore.isSessionLoading(session.id)"
                  class="history-status"
                  :title="t('chat.session.running')"
                  :aria-label="t('chat.session.running')"
                  role="status"
                >
                  <span class="agent-running-dot" aria-hidden="true"></span>
                </span>
                <div class="history-title-text">
                  <span class="history-title-name">{{ formatTitle(session.title) }}</span>
                  <span
                    v-if="session.is_main"
                    class="history-main-badge"
                    :title="t('chat.history.main')"
                    :aria-label="t('chat.history.main')"
                    role="img"
                  ></span>
                </div>
                <span class="history-time">{{ formatTime(session.updated_at) }}</span>
              </div>
                <div class="history-actions">
                  <button
                    v-if="!session.is_main"
                    class="history-main-btn"
                    type="button"
                    :title="t('chat.history.setMain')"
                    :aria-label="t('chat.history.setMain')"
                    @click.stop="handleSetMainSession(session)"
                  >
                    <i class="fa-solid fa-star history-main-icon" aria-hidden="true"></i>
                  </button>
                  <button
                    class="history-delete-btn"
                    type="button"
                    :title="t('chat.history.delete')"
                    :aria-label="t('chat.history.delete')"
                    @click.stop="handleDeleteSession(session.id)"
                  >
                    <i class="fa-solid fa-trash-can history-delete-icon" aria-hidden="true"></i>
                  </button>
                </div>
              </div>
              <div
                v-if="historyPaddingBottom && historySessions.length"
                class="history-spacer"
                :style="{ height: `${historyPaddingBottom}px` }"
              ></div>
            </div>
          </div>

        </aside>

        <section class="chat-panel">
          <div ref="messagesContainerRef" class="messages-container" @click="handleMessageClick">
            <div
              v-for="(message, index) in chatStore.messages"
              :key="resolveMessageKey(message, index)"
              :class="['message', message.role === 'user' ? 'from-user' : 'from-ai']"
            >
              <div
                class="avatar"
                :class="[
                  message.role === 'user' ? 'user-avatar' : 'ai-avatar',
                  { 'ai-avatar-working': message.role === 'assistant' && isAssistantStreaming(message) }
                ]"
                :aria-busy="message.role === 'assistant' && isAssistantStreaming(message) ? 'true' : 'false'"
              >
                {{ message.role === 'user' ? t('chat.message.user') : t('chat.message.assistantShort') }}
              </div>
              <div class="message-content">
                <div class="message-header">
                  <div class="message-header-left">
                    <div class="message-role">
                      <span
                        v-if="message.role === 'assistant'"
                        class="message-status"
                        :class="[
                          isAssistantStreaming(message) ? 'is-running' : 'is-waiting',
                          isLatestAssistant(index) ? 'is-animated' : ''
                        ]"
                        aria-hidden="true"
                      >
                        <span
                          :class="isAssistantStreaming(message) ? 'agent-running-dot' : 'agent-waiting-dot'"
                        ></span>
                      </span>
                      {{ message.role === 'user' ? t('chat.message.user') : t('chat.message.assistant') }}
                    </div>
                    <MessageThinking
                      v-if="message.role === 'assistant'"
                      :content="message.reasoning"
                      :streaming="message.reasoningStreaming"
                    />
                  </div>
                  <div v-if="message.role === 'assistant'" class="message-actions">
                    <div class="message-time">{{ formatTime(message.created_at) }}</div>
                    <button
                      v-if="shouldShowResumeButton(message)"
                      class="message-copy-btn message-resume-btn"
                      type="button"
                      :title="t('chat.message.resume')"
                      :aria-label="t('chat.message.resume')"
                      @click="handleResumeMessage(message)"
                    >
                      <i class="fa-solid fa-rotate" aria-hidden="true"></i>
                      <span>{{ t('chat.message.resume') }}</span>
                    </button>
                    <button
                      class="message-copy-btn"
                      type="button"
                      :title="t('chat.message.copy')"
                      :aria-label="t('chat.message.copy')"
                      @click="handleCopyMessage(message)"
                    >
                      <i class="fa-solid fa-copy message-copy-icon" aria-hidden="true"></i>
                      <span>{{ t('chat.message.copy') }}</span>
                    </button>
                  </div>
                  <div v-else class="message-actions">
                    <div class="message-time">{{ formatTime(message.created_at) }}</div>
                    <button
                      class="message-copy-btn"
                      type="button"
                      :title="t('chat.message.copy')"
                      :aria-label="t('chat.message.copy')"
                      @click="handleCopyMessage(message)"
                    >
                      <i class="fa-solid fa-copy message-copy-icon" aria-hidden="true"></i>
                      <span>{{ t('chat.message.copy') }}</span>
                    </button>
                  </div>
                </div>
                <MessageWorkflow
                  v-if="message.role === 'assistant'"
                  :items="message.workflowItems || []"
                  :loading="Boolean(message.workflowStreaming)"
                  :visible="Boolean(message.workflowStreaming || message.workflowItems?.length)"
                />
                <div
                  v-if="shouldShowMessageText(message)"
                  class="message-text"
                  :class="{ greeting: message.isGreeting }"
                >
                  <template v-if="message.isGreeting">
                    <div class="greeting-text">{{ message.content }}</div>
                    <el-tooltip
                      ref="abilityTooltipRef"
                      placement="bottom-end"
                      :show-after="160"
                      :teleported="true"
                      :popper-options="abilityTooltipOptions"
                      popper-class="ability-tooltip-popper"
                      @show="handleAbilityTooltipShow"
                      @hide="handleAbilityTooltipHide"
                    >
                      <template #content>
                        <div class="ability-tooltip">
                          <div class="ability-header">
                            <span class="ability-title">{{ t('chat.ability.title') }}</span>
                            <span class="ability-sub">{{ t('chat.ability.subtitle') }}</span>
                          </div>
                          <div v-if="toolSummaryLoading && !hasAbilitySummary" class="ability-muted">
                            {{ t('chat.ability.loading') }}
                          </div>
                          <div v-else-if="toolSummaryError" class="ability-error">
                            {{ toolSummaryError }}
                          </div>
                          <template v-else>
                            <div v-if="!hasAbilitySummary" class="ability-muted">
                              {{ t('chat.ability.empty') }}
                            </div>
                            <div v-else class="ability-scroll">
                              <div class="ability-section">
                                <div class="ability-section-title">
                                  <span>{{ t('chat.ability.tools') }}</span>
                                  <span class="ability-count">{{ abilitySummary.tools.length }}</span>
                                </div>
                                <div v-if="abilitySummary.tools.length" class="ability-item-list">
                                  <div
                                    v-for="tool in abilitySummary.tools"
                                    :key="`tool-${tool.name}`"
                                    class="ability-item tool"
                                  >
                                    <div class="ability-item-name">{{ tool.name }}</div>
                                    <div
                                      class="ability-item-desc"
                                      :class="{ 'is-empty': !tool.description }"
                                    >
                                      {{ tool.description || t('chat.ability.noDesc') }}
                                    </div>
                                  </div>
                                </div>
                                <div v-else class="ability-empty">{{ t('chat.ability.emptyTools') }}</div>
                              </div>
                              <div class="ability-section">
                                <div class="ability-section-title">
                                  <span>{{ t('chat.ability.skills') }}</span>
                                  <span class="ability-count">{{ abilitySummary.skills.length }}</span>
                                </div>
                                <div v-if="abilitySummary.skills.length" class="ability-item-list">
                                  <div
                                    v-for="skill in abilitySummary.skills"
                                    :key="`skill-${skill.name}`"
                                    class="ability-item skill"
                                  >
                                    <div class="ability-item-name">{{ skill.name }}</div>
                                    <div
                                      class="ability-item-desc"
                                      :class="{ 'is-empty': !skill.description }"
                                    >
                                      {{ skill.description || t('chat.ability.noDesc') }}
                                    </div>
                                  </div>
                                </div>
                                <div v-else class="ability-empty">{{ t('chat.ability.emptySkills') }}</div>
                              </div>
                            </div>
                          </template>
                        </div>
                      </template>
                      <button
                        class="prompt-preview-btn"
                        type="button"
                        :title="t('chat.promptPreview')"
                        :aria-label="t('chat.promptPreview')"
                        :disabled="promptPreviewLoading"
                        @click="openPromptPreview"
                      >
                        <i class="fa-solid fa-eye prompt-preview-icon" aria-hidden="true"></i>
                      </button>
                    </el-tooltip>
                  </template>
                  <template v-else-if="message.role === 'assistant'">
                    <div
                      v-if="isAssistantStreaming(message)"
                      class="markdown-body streaming"
                      v-text="message.content"
                    ></div>
                    <div v-else class="markdown-body" v-html="renderAssistantMarkdown(message)"></div>
                  </template>
                  <template v-else>
                    {{ message.content }}
                  </template>
                  <div v-if="shouldShowMessageStats(message)" class="message-stats">
                    <span
                      v-for="item in buildMessageStatsEntries(message)"
                      :key="item.label"
                      class="message-stat"
                    >
                      <span class="message-stat-label">{{ item.label }}：</span>
                      <span class="message-stat-value">{{ item.value }}</span>
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <InquiryPanel
            v-if="activeInquiryPanel"
            :panel="activeInquiryPanel.panel"
            @update:selected="handleInquirySelection"
          />
          <div class="composer-shell">
            <PlanPanel
              v-if="activePlan"
              :plan="activePlan"
              v-model:expanded="planExpanded"
            />
            <ChatComposer
              :key="composerKey"
              :loading="chatStore.isSessionLoading(chatStore.activeSessionId)"
              :demo-mode="demoMode"
              :inquiry-active="Boolean(activeInquiryPanel)"
              :inquiry-selection="inquirySelection"
              @send="handleComposerSend"
              @stop="handleStop"
            />
          </div>
        </section>
      </div>
    </div>

    <el-dialog
      v-model="promptPreviewVisible"
      class="system-prompt-dialog"
      width="760px"
      top="8vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="system-prompt-header">
          <div class="system-prompt-title">{{ t('chat.systemPrompt.title') }}</div>
          <button class="icon-btn" type="button" @click="closePromptPreview">×</button>
        </div>
      </template>
      <div class="system-prompt-body">
        <div v-if="promptPreviewLoading" class="muted">{{ t('chat.systemPrompt.loading') }}</div>
        <pre v-else class="system-prompt-content" v-html="promptPreviewHtml"></pre>
      </div>
      <template #footer>
        <el-button class="system-prompt-footer-btn" @click="closePromptPreview">
          {{ t('common.close') }}
        </el-button>
      </template>
    </el-dialog>

    <FeatureCronDialog v-model="cronDialogVisible" :agent-id="activeAgentId" />
    <FeatureChannelDialog v-model="channelDialogVisible" :agent-id="activeAgentId" />
    <FeatureAgentSettingsDialog
      v-model="agentSettingsVisible"
      :agent-id="activeAgentId"
      @deleted="handleActiveAgentDeleted"
    />

    <el-dialog
      v-model="imagePreviewVisible"
      class="image-preview-dialog"
      width="auto"
      top="6vh"
      :show-close="false"
      :close-on-click-modal="true"
      append-to-body
      @closed="closeImagePreview"
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">{{ imagePreviewTitle || t('chat.imagePreview') }}</div>
          <button class="icon-btn" type="button" @click="closeImagePreview">×</button>
        </div>
      </template>
      <div class="image-preview-body">
        <img v-if="imagePreviewUrl" :src="imagePreviewUrl" class="image-preview-img" :alt="imagePreviewTitle" />
      </div>
    </el-dialog>

    <el-dialog
      v-model="workspaceDialogVisible"
      class="workspace-dialog compact-panel-dialog"
      width="90vw"
      top="6vh"
      :show-close="false"
      :append-to-body="false"
      :close-on-click-modal="true"
      destroy-on-close
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">{{ t('chat.workspacePanel') }}</div>
          <button class="icon-btn" type="button" @click="workspaceDialogVisible = false">×</button>
        </div>
      </template>
      <div class="panel-dialog-body">
        <WorkspacePanel :agent-id="activeAgentId" :container-id="activeSandboxContainerId" />
      </div>
    </el-dialog>

    <el-dialog
      v-model="historyDialogVisible"
      class="workspace-dialog compact-panel-dialog"
      width="90vw"
      top="6vh"
      :show-close="false"
      :append-to-body="false"
      :close-on-click-modal="true"
      destroy-on-close
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">{{ t('chat.history') }}</div>
          <button class="icon-btn" type="button" @click="historyDialogVisible = false">×</button>
        </div>
      </template>
      <div class="panel-dialog-body">
          <div class="history-panel">
            <div class="panel-header">
              <span class="history-title">{{ t('chat.history') }}</span>
            </div>
          <div
            ref="historyListRef"
            :class="['history-container', { virtual: historyVirtual }]"
            @scroll="handleHistoryScroll"
          >
            <div
              v-if="!historySessions.length"
              class="history-empty"
              :title="t('chat.history.empty')"
              :aria-label="t('chat.history.empty')"
              role="status"
            >
              <i class="fa-solid fa-inbox history-empty-icon" aria-hidden="true"></i>
            </div>
            <div
              v-if="historyPaddingTop && historySessions.length"
              class="history-spacer"
              :style="{ height: `${historyPaddingTop}px` }"
            ></div>
            <div
              v-for="session in historySessions"
              :key="session.id"
              :class="['history-item', session.id === chatStore.activeSessionId ? 'active' : '', session.is_main ? 'is-main' : '']"
              @click="handleSelectSession(session.id)"
            >
              <div class="history-info">
                <span
                  v-if="chatStore.isSessionLoading(session.id)"
                  class="history-status"
                  :title="t('chat.session.running')"
                  :aria-label="t('chat.session.running')"
                  role="status"
                >
                  <span class="agent-running-dot" aria-hidden="true"></span>
                </span>
                <div class="history-title-text">
                  <span class="history-title-name">{{ formatTitle(session.title) }}</span>
                  <span
                    v-if="session.is_main"
                    class="history-main-badge"
                    :title="t('chat.history.main')"
                    :aria-label="t('chat.history.main')"
                    role="img"
                  ></span>
                </div>
                <span class="history-time">{{ formatTime(session.updated_at) }}</span>
              </div>
                <div class="history-actions">
                  <button
                    v-if="!session.is_main"
                    class="history-main-btn"
                    type="button"
                    :title="t('chat.history.setMain')"
                    :aria-label="t('chat.history.setMain')"
                    @click.stop="handleSetMainSession(session)"
                  >
                    <i class="fa-solid fa-star history-main-icon" aria-hidden="true"></i>
                  </button>
                  <button
                    class="history-delete-btn"
                    type="button"
                    :title="t('chat.history.delete')"
                    :aria-label="t('chat.history.delete')"
                    @click.stop="handleDeleteSession(session.id)"
                  >
                    <i class="fa-solid fa-trash-can history-delete-icon" aria-hidden="true"></i>
                  </button>
                </div>
            </div>
            <div
              v-if="historyPaddingBottom && historySessions.length"
              class="history-spacer"
              :style="{ height: `${historyPaddingBottom}px` }"
            ></div>
          </div>
        </div>
      </div>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchRealtimeSystemPrompt, fetchSessionSystemPrompt } from '@/api/chat';
import { downloadWunderWorkspaceFile } from '@/api/workspace';
import { fetchUserToolsSummary } from '@/api/userTools';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import FeatureAgentSettingsDialog from '@/components/chat/FeatureAgentSettingsDialog.vue';
import FeatureChannelDialog from '@/components/chat/FeatureChannelDialog.vue';
import FeatureCronDialog from '@/components/chat/FeatureCronDialog.vue';
import InquiryPanel from '@/components/chat/InquiryPanel.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageWorkflow from '@/components/chat/MessageWorkflow.vue';
import PlanPanel from '@/components/chat/PlanPanel.vue';
import ThemeToggle from '@/components/common/ThemeToggle.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { copyText } from '@/utils/clipboard';
import { renderMarkdown } from '@/utils/markdown';
import { parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
import { onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import { isDemoMode } from '@/utils/demo';
import { collectAbilityDetails, collectAbilityNames } from '@/utils/toolSummary';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';
import { resolveUserBasePath } from '@/utils/basePath';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();
const agentStore = useAgentStore();
const { t } = useI18n();
const currentUser = computed(() => authStore.user);
const currentUserUnitLabel = computed(() => {
  const unit = currentUser.value?.unit;
  return unit?.path_name || unit?.pathName || unit?.name || currentUser.value?.unit_id || '-';
});
// 演示模式用于快速体验
const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => resolveUserBasePath(route.path));
const desktopMode = computed(() => basePath.value === '/desktop');
const featureTransport = computed(() => (chatStore.streamTransport === 'sse' ? 'sse' : 'ws'));
const featureTransportClass = computed(() => (featureTransport.value === 'sse' ? 'sse' : 'ws'));
const featureTransportText = computed(() =>
  t('chat.transport.current', {
    transport: t(featureTransport.value === 'sse' ? 'chat.transport.sse' : 'chat.transport.ws')
  })
);
const draftKey = ref(0);
const composerKey = computed(() =>
  chatStore.activeSessionId ? `session-${chatStore.activeSessionId}` : `draft-${draftKey.value}`
);
const inquirySelection = ref([]);
const historyListRef = ref(null);
const historyScrollTop = ref(0);
const messagesContainerRef = ref(null);
// 系统提示词预览状态
const promptPreviewVisible = ref(false);
const promptPreviewLoading = ref(false);
const promptPreviewContent = ref('');
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const workspaceDialogVisible = ref(false);
const historyDialogVisible = ref(false);
const manualDraftPending = ref(false);
const featureMenuVisible = ref(false);
const featureMenuRef = ref(null);
const cronDialogVisible = ref(false);
const channelDialogVisible = ref(false);
const agentSettingsVisible = ref(false);
const isCompactLayout = ref(false);
const promptToolSummary = ref(null);
const toolSummaryLoading = ref(false);
const toolSummaryError = ref('');
const abilityTooltipRef = ref(null);
const abilityTooltipVisible = ref(false);
const abilityTooltipOptions = {
  strategy: 'fixed',
  modifiers: [
    { name: 'offset', options: { offset: [0, 10] } },
    { name: 'shift', options: { padding: 8 } },
    { name: 'flip', options: { padding: 8, fallbackPlacements: ['top', 'bottom', 'right', 'left'] } },
    {
      name: 'preventOverflow',
      options: { padding: 8, altAxis: true, boundary: 'viewport' }
    }
  ]
};
const TOOL_OVERRIDE_NONE = '__no_tools__';
const EXTERNAL_SESSION_SYNC_VISIBLE_MS = 2500;
const EXTERNAL_SESSION_SYNC_HIDDEN_MS = 8000;
let externalSessionSyncTimer = null;
let externalSessionSyncRunning = false;
let externalSessionSyncStopped = false;

const normalizeToolItemName = (item) => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  return item.name || item.tool_name || item.toolName || item.id || '';
};

const buildAllowedToolSet = (summary) => {
  const names = collectAbilityNames(summary || {});
  return new Set([...(names.tools || []), ...(names.skills || [])]);
};

const filterSummaryByNames = (summary, allowedSet) => {
  if (!summary) return null;
  const filterList = (list) =>
    Array.isArray(list)
      ? list.filter((item) => {
          const name = String(normalizeToolItemName(item)).trim();
          return name && allowedSet.has(name);
        })
      : [];
  return {
    ...summary,
    builtin_tools: filterList(summary.builtin_tools),
    mcp_tools: filterList(summary.mcp_tools),
    a2a_tools: filterList(summary.a2a_tools),
    skills: filterList(summary.skills),
    knowledge_tools: filterList(summary.knowledge_tools),
    user_tools: filterList(summary.user_tools),
    shared_tools: filterList(summary.shared_tools)
  };
};

function applyToolOverridesToSummary(summary, overrides = [], agentDefaults = []) {
  if (!summary) return null;
  const allowedSet = buildAllowedToolSet(summary);
  const defaultSet = new Set();
  if (Array.isArray(agentDefaults) && agentDefaults.length > 0) {
    agentDefaults.forEach((name) => {
      if (allowedSet.has(name)) {
        defaultSet.add(name);
      }
    });
  } else {
    allowedSet.forEach((name) => defaultSet.add(name));
  }
  let effectiveSet = new Set();
  if (Array.isArray(overrides) && overrides.includes(TOOL_OVERRIDE_NONE)) {
    effectiveSet = new Set();
  } else if (Array.isArray(overrides) && overrides.length > 0) {
    overrides.forEach((name) => {
      if (allowedSet.has(name)) {
        effectiveSet.add(name);
      }
    });
  } else {
    effectiveSet = defaultSet;
  }
  return filterSummaryByNames(summary, effectiveSet);
}

const activeSession = computed(
  () => chatStore.sessions.find((item) => item.id === chatStore.activeSessionId) || null
);
const routeAgentId = computed(() => String(route.query.agent_id || '').trim());
const routeEntry = computed(() => String(route.query.entry || '').trim().toLowerCase());
const activeAgentId = computed(
  () => activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || ''
);
const activeAgent = computed(() =>
  activeAgentId.value ? agentStore.agentMap[activeAgentId.value] || null : null
);
const normalizeSandboxContainerId = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};
const activeSandboxContainerId = computed(() =>
  normalizeSandboxContainerId(activeAgent.value?.sandbox_container_id)
);
const canManageActiveAgent = computed(() => Boolean(String(activeAgentId.value || '').trim()));
const activeAgentLabel = computed(
  () => activeAgent.value?.name || activeAgentId.value || ''
);
const activeAgentSubtitle = computed(() => {
  if (activeAgentLabel.value) {
    return activeAgentLabel.value;
  }
  if (!activeAgentId.value) {
    return t('portal.card.defaultTitle');
  }
  return '';
});
const greetingOverride = computed(() => {
  const desc = String(activeAgent.value?.description || '').trim();
  return desc;
});
const effectiveToolSummary = computed(() => {
  const overrides = activeSession.value?.tool_overrides;
  const draftOverrides = Array.isArray(chatStore.draftToolOverrides)
    ? chatStore.draftToolOverrides
    : [];
  return applyToolOverridesToSummary(
    promptToolSummary.value,
    overrides && overrides.length > 0 ? overrides : draftOverrides,
    activeAgent.value?.tool_names || []
  );
});
const promptPreviewHtml = computed(() => {
  const content = promptPreviewContent.value || t('chat.systemPrompt.empty');
  return renderSystemPromptHighlight(content, effectiveToolSummary.value || {});
});
// 能力悬浮提示使用的工具/技能明细
const abilitySummary = computed(() => collectAbilityDetails(effectiveToolSummary.value || {}));
const hasAbilitySummary = computed(
  () => abilitySummary.value.tools.length > 0 || abilitySummary.value.skills.length > 0
);
const HISTORY_ROW_HEIGHT = 52;
const HISTORY_OVERSCAN = 6;
const historySourceSessions = computed(() => {
  const agentId = activeAgentId.value;
  return chatStore.sessions.filter((session) => {
    const sessionAgentId = String(session.agent_id || '').trim();
    return agentId ? sessionAgentId === agentId : !sessionAgentId;
  });
});
const historyTotal = computed(() => historySourceSessions.value.length);
const historyVirtual = computed(() => historyTotal.value > 40);
const historyViewportHeight = computed(() => historyListRef.value?.clientHeight || 0);
const historyVisibleCount = computed(() =>
  Math.max(1, Math.ceil(historyViewportHeight.value / HISTORY_ROW_HEIGHT))
);
const historyStartIndex = computed(() => {
  if (!historyVirtual.value) return 0;
  const raw = Math.floor(historyScrollTop.value / HISTORY_ROW_HEIGHT) - HISTORY_OVERSCAN;
  const maxStart = Math.max(0, historyTotal.value - historyVisibleCount.value);
  return Math.max(0, Math.min(raw, maxStart));
});
const historyEndIndex = computed(() => {
  if (!historyVirtual.value) return historyTotal.value;
  return Math.min(
    historyTotal.value,
    historyStartIndex.value + historyVisibleCount.value + HISTORY_OVERSCAN * 2
  );
});
const historySessions = computed(() =>
  historyVirtual.value
    ? historySourceSessions.value.slice(historyStartIndex.value, historyEndIndex.value)
    : historySourceSessions.value
);
const historyPaddingTop = computed(() =>
  historyVirtual.value ? historyStartIndex.value * HISTORY_ROW_HEIGHT : 0
);
const historyPaddingBottom = computed(() =>
  historyVirtual.value ? Math.max(0, (historyTotal.value - historyEndIndex.value) * HISTORY_ROW_HEIGHT) : 0
);
let pendingAssistantCenter = false;
let pendingAssistantCenterCount = 0;
let pendingEnterBottomScroll = true;

const resolveInitialSessionId = (agentId) => {
  const mainSession = chatStore.sessions.find((session) => session.is_main);
  if (mainSession?.id) {
    return mainSession.id;
  }
  const persisted = chatStore.getLastSessionId?.(agentId) || '';
  if (persisted && chatStore.sessions.some((session) => session.id === persisted)) {
    return persisted;
  }
  return chatStore.sessions[0]?.id || '';
};

const openAgentSession = async (agentId) => {
  const normalizedAgentId = String(agentId || '').trim();
  const currentAgentId = String(
    activeSession.value?.agent_id || chatStore.draftAgentId || ''
  ).trim();
  const switchingAgent = currentAgentId !== normalizedAgentId;
  if (switchingAgent) {
    manualDraftPending.value = true;
    chatStore.openDraftSession({ agent_id: normalizedAgentId });
  } else {
    manualDraftPending.value = false;
  }
  await chatStore.loadSessions({ agent_id: normalizedAgentId });
  const targetId = resolveInitialSessionId(normalizedAgentId);
  if (targetId) {
    if (chatStore.activeSessionId !== targetId) {
      await chatStore.loadSessionDetail(targetId);
    }
    return;
  }
  manualDraftPending.value = true;
  chatStore.openDraftSession({ agent_id: normalizedAgentId });
};

const init = async () => {
  if (demoMode.value || !authStore.user) {
    await authStore.loadProfile();
  }
  const initialAgentId = routeEntry.value === 'default' ? '' : routeAgentId.value;
  await openAgentSession(initialAgentId);
  if (routeEntry.value === 'default') {
    router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
  }
};

const handleCreateSession = () => {
  const agentId = activeAgentId.value;
  manualDraftPending.value = true;
  chatStore.openDraftSession({ agent_id: agentId });
  draftKey.value += 1;
};

const handleOpenProfile = () => {
  router.push(`${basePath.value}/profile`);
};

const handleOpenPortal = () => {
  router.push(basePath.value + '/home');
};

const handleOpenContainerSettings = () => {
  if (!desktopMode.value) {
    return;
  }
  router.push('/desktop/containers');
};

const handleOpenSystemSettings = () => {
  if (!desktopMode.value) {
    return;
  }
  router.push('/desktop/system');
};

const closeFeatureMenu = () => {
  featureMenuVisible.value = false;
};

const toggleFeatureMenu = () => {
  featureMenuVisible.value = !featureMenuVisible.value;
};

const openCronDialog = () => {
  cronDialogVisible.value = true;
};

const openChannelDialog = () => {
  channelDialogVisible.value = true;
};

const openAgentSettingsDialog = async () => {
  if (!canManageActiveAgent.value) {
    ElMessage.warning(t('chat.features.agentMissing'));
    return;
  }
  if (activeAgentId.value && !activeAgent.value) {
    await agentStore.getAgent(activeAgentId.value).catch(() => null);
  }
  agentSettingsVisible.value = true;
};

const handleActiveAgentDeleted = (deletedAgentId) => {
  const target = String(deletedAgentId || '').trim();
  if (!target) return;
  if (target !== String(activeAgentId.value || '').trim()) return;
  router.replace(basePath.value + '/home');
};

const deleteActiveAgent = async () => {
  if (!canManageActiveAgent.value) {
    ElMessage.warning(t('chat.features.agentMissing'));
    return;
  }
  const agentId = String(activeAgentId.value || '').trim();
  if (!agentId) return;
  const agent = activeAgent.value || (await agentStore.getAgent(agentId).catch(() => null));
  const agentName = String(agent?.name || agentId).trim();
  try {
    await ElMessageBox.confirm(
      t('portal.agent.deleteConfirm', { name: agentName }),
      t('common.notice'),
      {
        confirmButtonText: t('portal.agent.delete'),
        cancelButtonText: t('portal.agent.cancel'),
        type: 'warning'
      }
    );
  } catch (error) {
    return;
  }
  try {
    await agentStore.deleteAgent(agentId);
    ElMessage.success(t('portal.agent.deleteSuccess'));
    handleActiveAgentDeleted(agentId);
  } catch (error) {
    showApiError(error, t('portal.agent.deleteFailed'));
  }
};

const handleFeatureAction = async (action) => {
  const target = String(action || '').trim();
  if (!target) return;
  closeFeatureMenu();
  if (target === 'cron') {
    openCronDialog();
    return;
  }
  if (target === 'channels') {
    openChannelDialog();
    return;
  }
  if (target === 'agent-settings') {
    await openAgentSettingsDialog();
    return;
  }
  if (target === 'agent-delete') {
    await deleteActiveAgent();
  }
};

const handleFeatureMenuClickOutside = (event) => {
  const root = featureMenuRef.value;
  if (!root) return;
  const target = event.target;
  if (target instanceof Node && !root.contains(target)) {
    closeFeatureMenu();
  }
};

const handleSelectSession = async (sessionId) => {
  manualDraftPending.value = false;
  await chatStore.loadSessionDetail(sessionId);
  if (isCompactLayout.value) {
    historyDialogVisible.value = false;
  }
};

const handleDeleteSession = async (sessionId) => {
  try {
    await ElMessageBox.confirm(t('chat.history.confirmDelete'), t('chat.history.confirmTitle'), {
      confirmButtonText: t('common.delete'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  await chatStore.deleteSession(sessionId);
};

const handleSetMainSession = async (session) => {
  const sessionId = session?.id;
  if (!sessionId) return;
  await chatStore.setMainSession(sessionId);
};

const shouldShowMessageText = (message) => {
  if (!message) return false;
  if (message.role !== 'assistant') return true;
  return Boolean(String(message.content || '').trim());
};

const hasPlanSteps = (plan) =>
  Array.isArray(plan?.steps) && plan.steps.length > 0;

const activePlanMessage = computed(() => {
  for (let i = chatStore.messages.length - 1; i >= 0; i -= 1) {
    const message = chatStore.messages[i];
    if (message?.role !== 'assistant') continue;
    if (hasPlanSteps(message.plan)) {
      return message;
    }
  }
  return null;
});

const activePlan = computed(() => activePlanMessage.value?.plan || null);
const planExpanded = ref(false);

const latestAssistantIndex = computed(() => {
  for (let i = chatStore.messages.length - 1; i >= 0; i -= 1) {
    if (chatStore.messages[i]?.role === 'assistant') {
      return i;
    }
  }
  return -1;
});

const isLatestAssistant = (index) => index === latestAssistantIndex.value;

const markdownCache = new WeakMap();

const isAssistantStreaming = (message) => {
  if (!message || message.role !== 'assistant') return false;
  return Boolean(message.workflowStreaming || message.reasoningStreaming || message.stream_incomplete);
};

// AI 回复使用 Markdown 渲染，主要用于表格等富文本展示
const renderAssistantMarkdown = (message) => {
  const content = String(message?.content || '');
  if (!content) return '';
  const cached = markdownCache.get(message);
  if (cached && cached.source === content) {
    return cached.html;
  }
  const html = renderMarkdown(content);
  markdownCache.set(message, { source: content, html });
  return html;
};

const workspaceResourceCache = new Map();
let workspaceResourceHydrationFrame = null;
let stopWorkspaceRefreshListener = null;

const isAdminUser = (user) =>
  Array.isArray(user?.roles) && user.roles.some((role) => role === 'admin' || role === 'super_admin');

const normalizeWorkspaceOwnerId = (value) =>
  String(value || '')
    .trim()
    .replace(/[^a-zA-Z0-9_-]/g, '_');

const resolveWorkspaceResource = (publicPath) => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const user = authStore.user;
  if (!user) return null;
  const currentId = String(user.id || '').trim();
  const safeCurrentId = normalizeWorkspaceOwnerId(currentId);
  const workspaceId = parsed.workspaceId || parsed.userId;
  const ownerId = parsed.ownerId || workspaceId;
  const agentId = parsed.agentId || '';
  const isOwner =
    Boolean(safeCurrentId) &&
    (workspaceId === safeCurrentId || workspaceId.startsWith(`${safeCurrentId}__agent__`));
  if (isOwner) {
    return {
      ...parsed,
      requestUserId: null,
      requestAgentId: agentId || null,
      allowed: true
    };
  }
  if (isAdminUser(user)) {
    return {
      ...parsed,
      requestUserId: ownerId,
      requestAgentId: agentId || null,
      allowed: true
    };
  }
  return { ...parsed, requestUserId: null, requestAgentId: null, allowed: false };
};

const getFilenameFromHeaders = (headers, fallback) => {
  const disposition = headers?.['content-disposition'] || headers?.['Content-Disposition'];
  if (!disposition) return fallback;
  const utf8Match = /filename\\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match) {
    return decodeURIComponent(utf8Match[1]);
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match ? match[1] : fallback;
};

const getFileExtension = (filename) => {
  const base = String(filename || '').split('?')[0].split('#')[0];
  const parts = base.split('.');
  if (parts.length < 2) return '';
  return parts.pop()?.toLowerCase() || '';
};

const normalizeWorkspaceImageBlob = (blob, filename, contentType) => {
  if (!(blob instanceof Blob)) return blob;
  const extension = getFileExtension(filename);
  if (extension !== 'svg') return blob;
  const expectedType = 'image/svg+xml';
  if (blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.includes('image/svg')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob.slice(0, blob.size, expectedType);
};

const saveBlobUrl = (url, filename) => {
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

const fetchWorkspaceResource = async (resource) => {
  const cached = workspaceResourceCache.get(resource.publicPath);
  if (cached?.objectUrl) return cached;
  if (cached?.promise) return cached.promise;
  const params: Record<string, string> = { path: String(resource.relativePath || '') };
  
  if (resource.requestUserId) {
    params.user_id = resource.requestUserId;
  }
  if (resource.requestAgentId) {
    params.agent_id = resource.requestAgentId;
  }
  const promise = downloadWunderWorkspaceFile(params)
    .then((response) => {
      const filename = getFilenameFromHeaders(response.headers, resource.filename || 'download');
      const contentType = response.headers?.['content-type'] || response.headers?.['Content-Type'];
      const blob = normalizeWorkspaceImageBlob(response.data, filename, contentType);
      const objectUrl = URL.createObjectURL(blob);
      const entry = { objectUrl, filename };
      workspaceResourceCache.set(resource.publicPath, entry);
      return entry;
    })
    .catch((error) => {
      workspaceResourceCache.delete(resource.publicPath);
      throw error;
    });
  workspaceResourceCache.set(resource.publicPath, { promise });
  return promise;
};

const isWorkspaceResourceMissing = (error) => {
  const status = error?.response?.status;
  if (status === 404 || status === 410) return true;
  const raw =
    error?.response?.data?.detail ??
    error?.response?.data?.message ??
    error?.message ??
    '';
  const message = typeof raw === 'string' ? raw : String(raw || '');
  return /not found|no such|不存在|找不到|已删除|已移除|removed/i.test(message);
};

const hydrateWorkspaceResourceCard = async (card) => {
  if (!card || card.dataset.workspaceState) return;
  const kind = card.dataset.workspaceKind || 'image';
  if (kind !== 'image') {
    card.dataset.workspaceState = 'ready';
    return;
  }
  const publicPath = card.dataset.workspacePath || '';
  const status = card.querySelector('.ai-resource-status');
  const preview = card.querySelector('.ai-resource-preview');
  if (!publicPath || !preview) return;
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) {
    if (status) status.textContent = t('chat.resourceUnavailable');
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
    return;
  }
  if (!resource.allowed) {
    if (status) status.textContent = t('chat.resourceDenied');
    card.dataset.workspaceState = 'forbidden';
    card.classList.add('is-error');
    return;
  }
  card.dataset.workspaceState = 'loading';
  try {
    const entry = await fetchWorkspaceResource(resource);
    preview.src = entry.objectUrl;
    card.dataset.workspaceState = 'ready';
    card.classList.add('is-ready');
    if (status) status.textContent = '';
  } catch (error) {
    if (status) {
      status.textContent = isWorkspaceResourceMissing(error)
        ? t('chat.resourceMissing')
        : t('chat.resourceImageFailed');
    }
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
  }
};

const hydrateWorkspaceResources = () => {
  const container = messagesContainerRef.value;
  if (!container || !authStore.user) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    hydrateWorkspaceResourceCard(card);
  });
};

const resetWorkspaceResourceCards = () => {
  const container = messagesContainerRef.value;
  if (!container) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    const kind = card.dataset?.workspaceKind || 'image';
    if (kind !== 'image') return;
    const state = card.dataset?.workspaceState || '';
    if (state === 'ready') return;
    card.dataset.workspaceState = '';
    card.classList.remove('is-error');
    card.classList.remove('is-ready');
    const status = card.querySelector('.ai-resource-status');
    if (status) {
      status.textContent = t('chat.resourceImageLoading');
    }
  });
};

const handleWorkspaceRefresh = () => {
  resetWorkspaceResourceCards();
  scheduleWorkspaceResourceHydration();
};

const scheduleWorkspaceResourceHydration = () => {
  if (workspaceResourceHydrationFrame) return;
  workspaceResourceHydrationFrame = requestAnimationFrame(() => {
    workspaceResourceHydrationFrame = null;
    hydrateWorkspaceResources();
  });
};

const clearWorkspaceResourceCache = () => {
  if (workspaceResourceHydrationFrame) {
    cancelAnimationFrame(workspaceResourceHydrationFrame);
    workspaceResourceHydrationFrame = null;
  }
  workspaceResourceCache.forEach((entry) => {
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
  });
  workspaceResourceCache.clear();
};

const downloadWorkspaceResource = async (publicPath) => {
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) return;
  if (!resource.allowed) {
    ElMessage.warning(t('chat.resourceDenied'));
    return;
  }
  try {
    const entry = await fetchWorkspaceResource(resource);
    saveBlobUrl(entry.objectUrl, entry.filename || resource.filename || 'download');
  } catch (error) {
    ElMessage.error(
      isWorkspaceResourceMissing(error)
        ? t('chat.resourceMissing')
        : t('chat.resourceDownloadFailed')
    );
  }
};

type ChatLocalCommand = 'new' | 'stop' | 'help' | 'compact';

const parseLocalCommand = (value): ChatLocalCommand | '' => {
  const raw = String(value || '').trim();
  if (!raw.startsWith('/')) return '';
  const token = raw.split(/\s+/, 1)[0].replace(/^\/+/, '').toLowerCase();
  if (!token) return '';
  if (token === 'new' || token === 'reset') return 'new';
  if (token === 'stop' || token === 'cancel') return 'stop';
  if (token === 'help' || token === '?') return 'help';
  if (token === 'compact') return 'compact';
  return '';
};

const resolveCommandErrorMessage = (error) =>
  String(error?.response?.data?.detail || error?.message || t('common.requestFailed')).trim();

const appendLocalCommandMessages = (commandText, replyText) => {
  chatStore.appendLocalMessage('user', commandText);
  chatStore.appendLocalMessage('assistant', replyText);
};

const handleLocalCommand = async (command: ChatLocalCommand, rawText) => {
  if (command === 'help') {
    appendLocalCommandMessages(rawText, t('chat.command.help'));
    await scrollMessagesToBottom();
    return;
  }

  if (command === 'new') {
    try {
      const agentId = String(activeAgentId.value || '').trim();
      await chatStore.createSession(agentId ? { agent_id: agentId } : {});
      appendLocalCommandMessages(rawText, t('chat.command.newSuccess'));
    } catch (error) {
      appendLocalCommandMessages(
        rawText,
        t('chat.command.newFailed', { message: resolveCommandErrorMessage(error) })
      );
    }
    await scrollMessagesToBottom();
    return;
  }

  if (command === 'stop') {
    const activeId = String(chatStore.activeSessionId || '').trim();
    if (!activeId) {
      appendLocalCommandMessages(rawText, t('chat.command.stopNoSession'));
      await scrollMessagesToBottom();
      return;
    }
    const cancelled = await chatStore.stopStream();
    appendLocalCommandMessages(
      rawText,
      cancelled ? t('chat.command.stopRequested') : t('chat.command.stopNoRunning')
    );
    await scrollMessagesToBottom();
    return;
  }

  if (command === 'compact') {
    const activeId = String(chatStore.activeSessionId || '').trim();
    if (!activeId) {
      appendLocalCommandMessages(rawText, t('chat.command.compactMissingSession'));
      await scrollMessagesToBottom();
      return;
    }
    try {
      await chatStore.compactSession(activeId);
      appendLocalCommandMessages(rawText, t('chat.command.compactSuccess'));
      scheduleExternalSessionSync(true);
    } catch (error) {
      appendLocalCommandMessages(
        rawText,
        t('chat.command.compactFailed', { message: resolveCommandErrorMessage(error) })
      );
    }
    await scrollMessagesToBottom();
  }
};

const handleComposerSend = async ({ content, attachments }) => {
  const payloadAttachments = Array.isArray(attachments) ? attachments : [];
  const trimmedContent = String(content || '').trim();
  const active = activeInquiryPanel.value;
  const selectedRoutes = resolveInquirySelectionRoutes(active?.panel, inquirySelection.value);
  const hasSelection = selectedRoutes.length > 0;
  if (!trimmedContent && payloadAttachments.length === 0 && !hasSelection) return;

  const localCommand = parseLocalCommand(trimmedContent);
  if (localCommand && !hasSelection) {
    if (active) {
      chatStore.resolveInquiryPanel(active.message, { status: 'dismissed' });
    }
    if (payloadAttachments.length > 0) {
      appendLocalCommandMessages(trimmedContent, t('chat.command.attachmentsUnsupported'));
      inquirySelection.value = [];
      await scrollMessagesToBottom();
      return;
    }
    await handleLocalCommand(localCommand, trimmedContent);
    inquirySelection.value = [];
    return;
  }

  let finalContent = trimmedContent;
  if (active) {
    if (hasSelection) {
      chatStore.resolveInquiryPanel(active.message, {
        status: 'answered',
        selected: selectedRoutes.map((route) => route.label)
      });
      const selectionText = buildInquiryReply(active.panel, selectedRoutes);
      if (trimmedContent) {
        const appended = t('chat.askPanelUserAppend', { content: trimmedContent });
        finalContent = `${selectionText}\n\n${appended}`;
      } else {
        finalContent = selectionText;
      }
    } else {
      chatStore.resolveInquiryPanel(active.message, { status: 'dismissed' });
    }
  }

  pendingAssistantCenter = true;
  pendingAssistantCenterCount = chatStore.messages.length;
  try {
    await chatStore.sendMessage(finalContent, { attachments: payloadAttachments });
  } catch (error) {
    pendingAssistantCenter = false;
    pendingAssistantCenterCount = 0;
    throw error;
  } finally {
    inquirySelection.value = [];
  }
};

const handleStop = async () => {
  await chatStore.stopStream();
};

const buildInquiryReply = (panel, routes) => {
  const header = t('chat.askPanelPrefix');
  const question = panel?.question ? t('chat.askPanelQuestion', { question: panel.question }) : '';
  const lines = routes.map((route) => {
    const detail = route.description ? `：${route.description}` : '';
    return `- ${route.label}${detail}`;
  });
  return [header, question, ...lines].filter(Boolean).join('\n');
};

const resolveInquirySelectionRoutes = (panel, selected) => {
  if (!panel || !Array.isArray(selected) || selected.length === 0) {
    return [];
  }
  return selected
    .map((index) => panel.routes?.[index])
    .filter((route) => route && route.label);
};

const activeInquiryPanel = computed(() => {
  for (let i = chatStore.messages.length - 1; i >= 0; i -= 1) {
    const message = chatStore.messages[i];
    if (message?.role !== 'assistant') continue;
    const panel = message?.questionPanel;
    if (panel?.status === 'pending') {
      return { message, panel };
    }
  }
  return null;
});

const handleInquirySelection = (selected) => {
  inquirySelection.value = Array.isArray(selected) ? selected : [];
};

const openImagePreview = (src, title = '') => {
  if (!src) return;
  imagePreviewUrl.value = src;
  const trimmedTitle = String(title || '').trim();
  imagePreviewTitle.value = trimmedTitle || t('chat.imagePreview');
  imagePreviewVisible.value = true;
};

const closeImagePreview = () => {
  imagePreviewVisible.value = false;
  imagePreviewUrl.value = '';
  imagePreviewTitle.value = '';
};

const handleCopyMessage = async (message) => {
  const content = String(message?.content || '').trim();
  if (!content) {
    ElMessage.warning(t('chat.message.copyEmpty'));
    return;
  }
  const ok = await copyText(content);
  if (ok) {
    ElMessage.success(t('chat.message.copySuccess'));
  } else {
    ElMessage.error(t('chat.message.copyFailed'));
  }
};

const shouldShowResumeButton = (message) => {
  if (!message || message.role !== 'assistant') return false;
  if (message.workflowStreaming) return false;
  return Boolean(message.slow_client);
};

const handleResumeMessage = async (message) => {
  if (!message) return;
  const sessionId = chatStore.activeSessionId;
  if (!sessionId) return;
  message.slow_client = false;
  await chatStore.resumeStream(sessionId, message, { force: true });
};

const handleMessageClick = async (event) => {
  const target = event?.target;
  if (!(target instanceof Element)) return;
  const resourceButton = target.closest('[data-workspace-action]');
  if (resourceButton) {
    const action = resourceButton.getAttribute('data-workspace-action');
    const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
    const publicPath = container?.dataset?.workspacePath || '';
    if (action === 'download' && publicPath) {
      event.preventDefault();
      await downloadWorkspaceResource(publicPath);
    }
    return;
  }
  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
  if (resourceLink) {
    const publicPath = resourceLink.dataset?.workspacePath || '';
    if (publicPath) {
      event.preventDefault();
      await downloadWorkspaceResource(publicPath);
    }
    return;
  }
  const previewImage = target.closest('img.ai-resource-preview');
  if (previewImage) {
    const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
    if (card?.dataset?.workspaceState !== 'ready') return;
    const src = previewImage.getAttribute('src') || '';
    if (!src) return;
    const title = card?.querySelector('.ai-resource-name')?.textContent || '';
    openImagePreview(src, title);
    return;
  }
  const copyButton = target.closest('.ai-code-copy');
  if (!copyButton) return;
  event.preventDefault();
  const codeBlock = copyButton.closest('.ai-code-block');
  const codeElement = codeBlock?.querySelector('code');
  const codeText = codeElement?.textContent || '';
  if (!codeText.trim()) {
    ElMessage.warning(t('chat.message.copyEmpty'));
    return;
  }
  const ok = await copyText(codeText);
  if (ok) {
    ElMessage.success(t('chat.message.copySuccess'));
  } else {
    ElMessage.error(t('chat.message.copyFailed'));
  }
};

const handleLogout = () => {
  if (basePath.value === '/desktop') {
    router.push('/desktop/home');
    return;
  }
  if (demoMode.value) {
    router.push('/login');
    return;
  }
  authStore.logout();
  router.push('/login');
};

const handleHistoryScroll = (event) => {
  historyScrollTop.value = event.target.scrollTop || 0;
};

const flushChatSnapshot = () => {
  chatStore.scheduleSnapshot(true);
};

const resolveSessionActivityTimestampMs = (session) => {
  const parsed = parseTimeValue(session?.updated_at ?? session?.last_message_at ?? session?.created_at);
  return parsed ? parsed.getTime() : 0;
};

const shouldSkipExternalSessionSync = () => {
  const id = String(chatStore.activeSessionId || '').trim();
  if (!id) return false;
  if (chatStore.isSessionLoading?.(id)) return true;
  // Avoid disrupting a locally-initiated stream; wait until it completes.
  const last = chatStore.messages?.[chatStore.messages.length - 1];
  if (last?.role === 'assistant' && (last.workflowStreaming || last.stream_incomplete)) {
    return true;
  }
  return false;
};

const canAutoOpenIncomingSession = () => {
  if (manualDraftPending.value) {
    return false;
  }
  return !chatStore.messages.some((message) => {
    if (!message || message.isGreeting) return false;
    return String(message.content || '').trim().length > 0;
  });
};

const clearExternalSessionSyncTimer = () => {
  if (externalSessionSyncTimer) {
    clearTimeout(externalSessionSyncTimer);
    externalSessionSyncTimer = null;
  }
};

const scheduleExternalSessionSync = (immediate = false) => {
  if (externalSessionSyncStopped) return;
  clearExternalSessionSyncTimer();
  const hidden = typeof document !== 'undefined' && document.hidden;
  const delay = immediate
    ? 0
    : hidden
      ? EXTERNAL_SESSION_SYNC_HIDDEN_MS
      : EXTERNAL_SESSION_SYNC_VISIBLE_MS;
  externalSessionSyncTimer = setTimeout(() => {
    void runExternalSessionSync();
  }, delay);
};

const runExternalSessionSync = async () => {
  if (externalSessionSyncStopped) return;
  if (externalSessionSyncRunning) {
    scheduleExternalSessionSync(false);
    return;
  }
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (activeSessionId && shouldSkipExternalSessionSync()) {
    scheduleExternalSessionSync(false);
    return;
  }
  externalSessionSyncRunning = true;
  try {
    const previousActivity = new Map(
      chatStore.sessions.map((session) => [
        String(session?.id || '').trim(),
        resolveSessionActivityTimestampMs(session)
      ])
    );
    const agentId = String(activeAgentId.value || '').trim();
    const sessions = await chatStore.loadSessions({ agent_id: agentId, skipTransportRefresh: true });
    if (!Array.isArray(sessions)) return;

    const latestActive = String(chatStore.activeSessionId || '').trim();
    if (!latestActive) {
      if (canAutoOpenIncomingSession()) {
        const targetId = resolveInitialSessionId(agentId);
        if (targetId) {
          await chatStore.loadSessionDetail(targetId);
        }
      }
      return;
    }

    if (shouldSkipExternalSessionSync()) return;

    const activeSession = sessions.find((item) => String(item?.id || '').trim() === latestActive);
    if (!activeSession) return;
    const previousTimestamp = previousActivity.get(latestActive) || 0;
    const nextTimestamp = resolveSessionActivityTimestampMs(activeSession);
    if (nextTimestamp <= previousTimestamp) return;
    await chatStore.loadSessionDetail(latestActive);
  } catch (error) {
    // ignore background sync failures
  } finally {
    externalSessionSyncRunning = false;
    scheduleExternalSessionSync(false);
  }
};

const handleVisibilityChange = () => {
  flushChatSnapshot();
  scheduleExternalSessionSync(true);
};

const handleBeforeUnload = () => {
  chatStore.markPageUnloading();
  flushChatSnapshot();
};

// 触发 Popper 重新计算，避免首帧内容变化导致溢出
const updateAbilityTooltip = async () => {
  await nextTick();
  const tooltip = abilityTooltipRef.value;
  if (tooltip?.updatePopper) {
    tooltip.updatePopper();
  } else if (tooltip?.popperRef?.update) {
    tooltip.popperRef.update();
  }
  requestAnimationFrame(() => {
    if (tooltip?.updatePopper) {
      tooltip.updatePopper();
    } else if (tooltip?.popperRef?.update) {
      tooltip.popperRef.update();
    }
  });
};

// 读取工具与技能汇总信息，供提示词预览与悬浮提示使用
const loadToolSummary = async () => {
  if (toolSummaryLoading.value || promptToolSummary.value) {
    return promptToolSummary.value;
  }
  toolSummaryLoading.value = true;
  toolSummaryError.value = '';
  try {
    const toolsResult = await fetchUserToolsSummary();
    const payload = toolsResult?.data?.data || null;
    if (payload) {
      promptToolSummary.value = payload;
    }
    return payload;
  } catch (error) {
    toolSummaryError.value = error.response?.data?.detail || t('chat.toolSummaryFailed');
    return null;
  } finally {
    toolSummaryLoading.value = false;
    if (abilityTooltipVisible.value) {
      await updateAbilityTooltip();
    }
  }
};

const handleAbilityTooltipShow = () => {
  abilityTooltipVisible.value = true;
  loadToolSummary();
  updateAbilityTooltip();
};

const handleAbilityTooltipHide = () => {
  abilityTooltipVisible.value = false;
};

// 打开系统提示词预览，优先读取会话快照
const openPromptPreview = async () => {
  promptPreviewVisible.value = true;
  promptPreviewLoading.value = true;
  promptPreviewContent.value = '';
  const toolSummaryPromise = loadToolSummary();
  try {
    const overrides =
      activeSession.value?.tool_overrides && activeSession.value.tool_overrides.length > 0
        ? activeSession.value.tool_overrides
        : Array.isArray(chatStore.draftToolOverrides) && chatStore.draftToolOverrides.length > 0
          ? chatStore.draftToolOverrides
          : undefined;
    const agentId =
      activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || undefined;
    const requestPayload = {
      ...(agentId ? { agent_id: agentId } : {}),
      ...(overrides ? { tool_overrides: overrides } : {})
    };
    const promptRequest = chatStore.activeSessionId
      ? fetchSessionSystemPrompt(chatStore.activeSessionId, requestPayload)
      : fetchRealtimeSystemPrompt(requestPayload);
    const promptResult = await promptRequest;
    await toolSummaryPromise;
    const responsePayload = promptResult?.data?.data || {};
    promptPreviewContent.value = responsePayload.prompt || '';
  } catch (error) {
    showApiError(error, t('chat.systemPromptFailed'));
    promptPreviewContent.value = '';
  } finally {
    promptPreviewLoading.value = false;
  }
};

const closePromptPreview = () => {
  promptPreviewVisible.value = false;
};

const parseTimeValue = (value) => {
  if (value === null || value === undefined) return null;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? null : value;
  }
  if (typeof value === 'number') {
    const millis = value < 1e12 ? value * 1000 : value;
    const date = new Date(millis);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  const text = String(value).trim();
  if (!text) return null;
  if (/^\d+$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return null;
    const millis = numeric < 1e12 ? numeric * 1000 : numeric;
    const date = new Date(millis);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  const parsed = new Date(text);
  if (!Number.isNaN(parsed.getTime())) {
    return parsed;
  }
  const match = text.match(
    /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?/
  );
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const hour = Number(match[4] || 0);
  const minute = Number(match[5] || 0);
  const second = Number(match[6] || 0);
  const date = new Date(year, month, day, hour, minute, second);
  return Number.isNaN(date.getTime()) ? null : date;
};

const formatTime = (value) => {
  const parsed = parseTimeValue(value);
  if (!parsed) {
    if (value === null || value === undefined) return '';
    return String(value).trim();
  }
  const pad = (part) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}:${pad(parsed.getSeconds())}`;
};

const messageRenderKeys = new WeakMap();
let messageRenderKeySeq = 0;

const resolveMessageKey = (message, index) => {
  if (!message || typeof message !== 'object') {
    return `message-${index}`;
  }
  const cached = messageRenderKeys.get(message);
  if (cached) {
    return cached;
  }
  messageRenderKeySeq += 1;
  const role = String(message.role || 'unknown');
  const createdAt = String(message.created_at || '');
  const streamEventId = String(message.stream_event_id || '');
  const streamRound = String(message.stream_round || '');
  const key = `${role}:${createdAt}:${streamRound}:${streamEventId}:${messageRenderKeySeq}`;
  messageRenderKeys.set(message, key);
  return key;
};

const formatTitle = (title) => {
  const text = String(title || '').trim();
  if (!text) return t('chat.session.unnamed');
  return text.length > 20 ? `${text.slice(0, 20)}...` : text;
};

const updateCompactLayout = () => {
  if (typeof window === 'undefined') return;
  if (window.matchMedia) {
    isCompactLayout.value = window.matchMedia('(max-width: 960px)').matches;
    return;
  }
  isCompactLayout.value = window.innerWidth <= 960;
};


const openWorkspaceDialog = () => {
  workspaceDialogVisible.value = true;
};

const openHistoryDialog = async () => {
  historyDialogVisible.value = true;
  historyScrollTop.value = 0;
  await nextTick();
  const container = historyListRef.value;
  if (container) {
    container.scrollTop = 0;
  }
};

const formatDuration = (seconds) => {
  if (seconds === null || seconds === undefined || Number.isNaN(seconds)) return '-';
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return '-';
  if (value < 1) {
    return `${Math.max(1, Math.round(value * 1000))} ms`;
  }
  return `${value.toFixed(2)} s`;
};

const formatCount = (value) => {
  if (value === null || value === undefined) return '-';
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) return '-';
  return String(parsed);
};

const formatSpeed = (value) => {
  if (value === null || value === undefined) return '-';
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '-';
  return `${parsed.toFixed(2)} token/s`;
};

const normalizeDurationSeconds = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const resolveDurationSeconds = (stats) => {
  const interaction = normalizeDurationSeconds(
    stats?.interaction_duration_s ??
      stats?.interactionDurationS ??
      stats?.interactionDuration ??
      stats?.duration_s ??
      stats?.elapsed_s
  );
  if (interaction !== null) return interaction;
  const prefill = normalizeDurationSeconds(stats?.prefill_duration_s);
  const decode = normalizeDurationSeconds(stats?.decode_duration_s);
  if (prefill === null && decode === null) return null;
  return (prefill ?? 0) + (decode ?? 0);
};

const resolveTokenSpeed = (stats, durationSeconds) => {
  const outputTokens = Number(stats?.usage?.output);
  const decode = normalizeDurationSeconds(stats?.decode_duration_s);
  if (Number.isFinite(outputTokens) && outputTokens > 0 && decode !== null && decode > 0) {
    return outputTokens / decode;
  }
  const totalTokens = Number(stats?.usage?.total);
  if (Number.isFinite(totalTokens) && totalTokens > 0 && durationSeconds && durationSeconds > 0) {
    return totalTokens / durationSeconds;
  }
  return null;
};

const buildMessageStatsEntries = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return [];
  if (isAssistantStreaming(message)) return [];
  const stats = message.stats || null;
  if (!stats) return [];
  const durationSeconds = resolveDurationSeconds(stats);
  const speed = resolveTokenSpeed(stats, durationSeconds);
  const hasUsage = Number.isFinite(Number(stats?.usage?.total)) && Number(stats.usage.total) > 0;
  const hasDuration = Number.isFinite(Number(durationSeconds)) && Number(durationSeconds) > 0;
  const hasToolCalls = Number.isFinite(Number(stats?.toolCalls)) && Number(stats.toolCalls) > 0;
  const hasQuota = Number.isFinite(Number(stats?.quotaConsumed)) && Number(stats.quotaConsumed) > 0;
  if (!hasUsage && !hasDuration && !hasToolCalls && !hasQuota) {
    return [];
  }
  const entries = [
    { label: t('chat.stats.duration'), value: formatDuration(durationSeconds) },
    { label: t('chat.stats.speed'), value: formatSpeed(speed) },
    { label: t('chat.stats.contextTokens'), value: formatCount(stats?.usage?.total) },
    { label: t('chat.stats.toolCalls'), value: formatCount(stats?.toolCalls) },
    { label: t('chat.stats.quota'), value: formatCount(stats?.quotaConsumed) }
  ];
  return entries;
};

const shouldShowMessageStats = (message) => buildMessageStatsEntries(message).length > 0;

const scrollMessagesToBottom = async () => {
  await nextTick();
  const applyBottomScroll = () => {
    const container = messagesContainerRef.value;
    if (!container) return false;
    container.scrollTop = container.scrollHeight;
    return true;
  };
  const applied = applyBottomScroll();
  requestAnimationFrame(() => {
    applyBottomScroll();
  });
  return applied;
};

const ensureEnterScrollToBottom = async () => {
  if (!pendingEnterBottomScroll) return;
  if (!chatStore.activeSessionId && chatStore.messages.length === 0) return;
  const applied = await scrollMessagesToBottom();
  if (applied) {
    pendingEnterBottomScroll = false;
  }
};

const scrollLatestAssistantToCenter = async () => {
    await nextTick();
    const container = messagesContainerRef.value;
    if (!container) return;
    const items = container.querySelectorAll('.message.from-ai');
    if (!items.length) return;
    const target = items[items.length - 1];
    requestAnimationFrame(() => {
      const containerRect = container.getBoundingClientRect();
      const targetRect = target.getBoundingClientRect();
      const targetCenter = targetRect.top - containerRect.top + targetRect.height / 2;
      const nextTop = container.scrollTop + targetCenter - container.clientHeight / 2;
      const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
      container.scrollTop = Math.max(0, Math.min(nextTop, maxTop));
    });
  };

onMounted(async () => {
  await init();
  await ensureEnterScrollToBottom();
  loadToolSummary();
  scheduleWorkspaceResourceHydration();
  stopWorkspaceRefreshListener = onWorkspaceRefresh(handleWorkspaceRefresh);
  updateCompactLayout();
  externalSessionSyncStopped = false;
  scheduleExternalSessionSync(true);
  window.addEventListener('resize', updateCompactLayout);
  window.addEventListener('beforeunload', handleBeforeUnload);
  document.addEventListener('visibilitychange', handleVisibilityChange);
  document.addEventListener('click', handleFeatureMenuClickOutside);
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', updateCompactLayout);
  window.removeEventListener('beforeunload', handleBeforeUnload);
  document.removeEventListener('visibilitychange', handleVisibilityChange);
  document.removeEventListener('click', handleFeatureMenuClickOutside);
  externalSessionSyncStopped = true;
  externalSessionSyncRunning = false;
  clearExternalSessionSyncTimer();
  if (stopWorkspaceRefreshListener) {
    stopWorkspaceRefreshListener();
    stopWorkspaceRefreshListener = null;
  }
  clearWorkspaceResourceCache();
});

watch(
  () => chatStore.activeSessionId,
  (value, oldValue) => {
    if (!value && oldValue) {
      draftKey.value += 1;
    }
    if (value) {
      manualDraftPending.value = false;
    }
    if (value !== oldValue) {
      clearWorkspaceResourceCache();
      scheduleWorkspaceResourceHydration();
      planExpanded.value = false;
      scheduleExternalSessionSync(true);
    }
  }
);

watch(
  () => activePlan.value,
  (value) => {
    if (!value) {
      planExpanded.value = false;
    }
  }
);

watch(
  () => chatStore.messages.length,
  (value) => {
    if (!pendingAssistantCenter) return;
    if (value <= pendingAssistantCenterCount) return;
    const lastMessage = chatStore.messages[value - 1];
    if (lastMessage?.role !== 'assistant') return;
    pendingAssistantCenter = false;
    pendingAssistantCenterCount = value;
    scrollLatestAssistantToCenter();
  }
);

watch(
  () => chatStore.messages.length,
  (value) => {
    scheduleWorkspaceResourceHydration();
    if (value > 0) {
      void ensureEnterScrollToBottom();
    }
  }
);

watch(
  () => activeInquiryPanel.value,
  (value) => {
    if (!value) {
      inquirySelection.value = [];
    }
  }
);

watch(
  () => chatStore.messages[chatStore.messages.length - 1]?.content,
  () => {
    scheduleWorkspaceResourceHydration();
  }
);

watch(
  () => authStore.user?.id,
  () => {
    scheduleWorkspaceResourceHydration();
  }
);

watch(
  () => demoMode.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    await init();
    loadToolSummary();
    scheduleExternalSessionSync(true);
  }
);

watch(
  () => activeAgentId.value,
  (value) => {
    if (!value) return;
    agentStore.getAgent(value).catch(() => null);
  },
  { immediate: true }
);

watch(
  () => greetingOverride.value,
  (value, oldValue) => {
    if (value === oldValue) return;
    chatStore.setGreetingOverride(value);
  },
  { immediate: true }
);

watch(
  () => routeEntry.value,
  async (value, oldValue) => {
    if (!value || value === oldValue || value !== 'default') return;
    await openAgentSession('');
    scheduleExternalSessionSync(true);
    router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
  }
);

watch(
  () => routeAgentId.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    if (routeEntry.value === 'default') return;
    await openAgentSession(value);
    scheduleExternalSessionSync(true);
  }
);

watch(
  () => route.fullPath,
  () => {
    closeFeatureMenu();
  }
);

watch(
  () => isCompactLayout.value,
  (value) => {
    if (!value) {
      workspaceDialogVisible.value = false;
      historyDialogVisible.value = false;
    }
  }
);

watch(
  () => abilitySummary.value,
  () => {
    if (abilityTooltipVisible.value) {
      updateAbilityTooltip();
    }
  },
  { deep: true }
);
</script>