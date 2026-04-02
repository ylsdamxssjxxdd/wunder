<template>
  <div class="chat-shell" :style="chatShellStyle">
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
            :disabled="creatingSession"
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
            :title="t('desktop.settings.system')"
            :aria-label="t('desktop.settings.system')"
            @click="handleOpenSystemSettings"
          >
            <i class="fa-solid fa-gear topbar-icon" aria-hidden="true"></i>
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
              class="user-meta-btn"
              type="button"
              :aria-label="t('user.profile.enter')"
              @click="handleOpenProfile"
            >
              <i class="fa-solid fa-user user-meta-icon" aria-hidden="true"></i>
              <div class="user-meta">
                <div class="user-name">{{ currentUser?.username || t('user.guest') }}</div>
                <div class="user-level">{{ t('user.unitLabel', { unit: currentUserUnitLabel }) }}</div>
              </div>
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
              <div v-if="historyInitialLoading" class="history-skeleton" aria-hidden="true"></div>
              <div
                v-else-if="!historySessions.length"
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
                  v-if="isSessionBusy(session.id)"
                  :class="['history-status', isSessionWaiting(session.id) ? 'is-waiting' : 'is-running']"
                  :title="resolveSessionBusyLabel(session.id)"
                  :aria-label="resolveSessionBusyLabel(session.id)"
                  role="status"
                >
                  <span :class="isSessionWaiting(session.id) ? 'agent-waiting-dot' : 'agent-running-dot'" aria-hidden="true"></span>
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
          <div
            ref="messagesContainerRef"
            class="messages-container"
            @click="handleMessageClick"
            @scroll="handleMessageScroll"
          >
            <button
              v-if="showScrollTopButton"
              class="chat-scroll-top-btn"
              type="button"
              :aria-label="t('chat.toTop')"
              @click="jumpToMessageTop"
            >
              <i class="fa-solid fa-angles-up" aria-hidden="true"></i>
            </button>
            <div v-if="messageInitialLoading" class="message-skeleton-list" aria-hidden="true">
              <div v-for="index in 4" :key="`message-skeleton-${index}`" class="message-skeleton-item"></div>
            </div>
            <div v-else-if="canLoadMoreHistory" class="message-load-more">
              <button
                class="message-load-more-btn"
                type="button"
                :disabled="historyLoading"
                @click="handleLoadOlderHistory"
              >
                {{ historyLoading ? t('chat.history.loadingMore') : t('chat.history.loadMore') }}
              </button>
            </div>
            <template
              v-for="(message, index) in chatStore.messages"
              :key="resolveMessageKey(message, index)"
            >
            <div
              v-if="!isHiddenInternalMessage(message)"
              :class="[
                'message',
                message.role === 'user' ? 'from-user' : 'from-ai',
                { 'message-compaction-marker': isCompactionMarkerMessage(message) }
              ]"
            >
              <button
                v-if="!isCompactionMarkerMessage(message)"
                class="avatar"
                :class="[
                  message.role === 'user' ? 'user-avatar' : 'ai-avatar',
                  { 'ai-avatar-working': message.role === 'assistant' && isAssistantStreaming(message) }
                ]"
                type="button"
                :title="resolveMessageAvatarActionLabel(message)"
                :aria-label="resolveMessageAvatarActionLabel(message)"
                :aria-busy="message.role === 'assistant' && isAssistantStreaming(message) ? 'true' : 'false'"
                @click="handleMessageAvatarClick(message)"
              >
                {{ message.role === 'user' ? t('chat.message.user') : t('chat.message.assistantShort') }}
              </button>
              <div class="message-content">
                <template v-if="isCompactionMarkerMessage(message)">
                  <MessageCompactionDivider
                    :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
                    :is-streaming="isAssistantStreaming(message)"
                  />
                </template>
                <template v-else>
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
                    <MessageFeedbackActions :message="message" />
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
                <MessageToolWorkflow
                  v-if="message.role === 'assistant'"
                  :items="message.workflowItems || []"
                  :loading="Boolean(message.workflowStreaming)"
                  :visible="Boolean(message.workflowStreaming || message.workflowItems?.length)"
                />
                <MessageSubagentPanel
                  v-if="message.role === 'assistant'"
                  :session-id="chatStore.activeSessionId"
                  :items="Array.isArray(message.subagents) ? message.subagents : []"
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
                              <div
                                v-for="section in abilitySections"
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
                        class="prompt-preview-btn"
                        type="button"
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
                    <div
                      v-if="hasUserMarkdownContent(message)"
                      class="markdown-body"
                      v-html="renderUserMarkdown(message)"
                    ></div>
                    <div
                      v-if="hasUserImageAttachments(message)"
                      class="message-user-image-grid"
                    >
                      <button
                        v-for="item in resolveUserImageAttachments(message)"
                        :key="item.key"
                        class="message-user-image-btn"
                        type="button"
                        :title="item.name"
                        :aria-label="item.name"
                        @click="openImagePreview(item.src, item.name)"
                      >
                        <img :src="item.src" :alt="item.name" class="message-user-image" />
                      </button>
                    </div>
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
                <MessageKnowledgeCitation
                  v-if="message.role === 'assistant'"
                  :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
                />
                <MessageCompactionDivider
                  v-if="message.role === 'assistant'"
                  :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
                  :is-streaming="isAssistantStreaming(message)"
                />
                </template>
              </div>
            </div>
            </template>
          </div>

          <InquiryPanel
            v-if="activeInquiryPanel"
            :panel="activeInquiryPanel.panel"
            @update:selected="handleInquirySelection"
          />
          <div ref="composerShellRef" class="composer-shell">
            <PlanPanel
              v-if="activePlan"
              :plan="activePlan"
              v-model:expanded="planExpanded"
              @remove="dismissActivePlan"
            />
            <ChatComposer
              :key="composerKey"
              :loading="activeSessionBusy"
              :demo-mode="demoMode"
              :inquiry-active="Boolean(activeInquiryPanel)"
              :inquiry-selection="inquirySelection"
              :preset-questions="activeAgentPresetQuestions"
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
          <button class="icon-btn" type="button" @click="closePromptPreview">脳</button>
        </div>
      </template>
      <div class="system-prompt-body">
        <div v-if="promptPreviewLoading" class="muted">{{ t('chat.systemPrompt.loading') }}</div>
        <template v-else>
          <div v-if="hasPromptPreviewTooling" class="system-prompt-view-switch">
            <button
              class="system-prompt-view-btn"
              :class="{ 'is-active': promptPreviewView === 'prompt' }"
              type="button"
              @click="promptPreviewView = 'prompt'"
            >
              {{ t('chat.systemPrompt.viewPrompt') }}
            </button>
            <button
              class="system-prompt-view-btn"
              :class="{ 'is-active': promptPreviewView === 'tooling' }"
              type="button"
              @click="promptPreviewView = 'tooling'"
            >
              {{ t('chat.systemPrompt.viewTooling') }}
            </button>
            <span class="system-prompt-tooling-mode">
              {{ t('chat.systemPrompt.toolingMode', { mode: promptPreviewToolingModeLabel }) }}
            </span>
          </div>
          <pre
            v-if="!hasPromptPreviewTooling || promptPreviewView === 'prompt'"
            class="system-prompt-content"
            v-html="promptPreviewHtml"
          ></pre>
          <div
            v-else
            class="system-prompt-tooling-content"
          >
            <PromptToolingPreviewList
              :items="promptPreviewToolingItems"
              :fallback-text="promptPreviewToolingContent"
            />
          </div>
        </template>
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
      :width="imagePreviewDialogWidth"
      top="6vh"
      :show-close="false"
      :close-on-click-modal="true"
      append-to-body
      @closed="closeImagePreview"
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">{{ imagePreviewTitle || t('chat.imagePreview') }}</div>
          <button class="icon-btn" type="button" @click="closeImagePreview">脳</button>
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
          <button class="icon-btn" type="button" @click="workspaceDialogVisible = false">脳</button>
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
          <button class="icon-btn" type="button" @click="historyDialogVisible = false">脳</button>
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
            <div v-if="historyInitialLoading" class="history-skeleton" aria-hidden="true"></div>
            <div
              v-else-if="!historySessions.length"
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
                  v-if="isSessionBusy(session.id)"
                  :class="['history-status', isSessionWaiting(session.id) ? 'is-waiting' : 'is-running']"
                  :title="resolveSessionBusyLabel(session.id)"
                  :aria-label="resolveSessionBusyLabel(session.id)"
                  role="status"
                >
                  <span :class="isSessionWaiting(session.id) ? 'agent-waiting-dot' : 'agent-running-dot'" aria-hidden="true"></span>
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
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch } from 'vue';
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
import MessageCompactionDivider from '@/components/chat/MessageCompactionDivider.vue';
import MessageFeedbackActions from '@/components/chat/MessageFeedbackActions.vue';
import MessageKnowledgeCitation from '@/components/chat/MessageKnowledgeCitation.vue';
import MessageSubagentPanel from '@/components/chat/MessageSubagentPanel.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';
import PlanPanel from '@/components/chat/PlanPanel.vue';
import AbilityTooltipListItem from '@/components/common/AbilityTooltipListItem.vue';
import PromptToolingPreviewList from '@/components/chat/PromptToolingPreviewList.vue';
import ThemeToggle from '@/components/common/ThemeToggle.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { getDesktopRuntime, isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { getRuntimeConfig } from '@/config/runtime';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { copyText } from '@/utils/clipboard';
import { hydrateExternalMarkdownImages, renderMarkdown } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import {
  buildAgentWorkspaceScopeId,
  buildWorkspacePublicPathFromScope,
  normalizeWorkspaceOwnerId,
  resolveMarkdownWorkspacePath
} from '@/utils/messageWorkspacePath';
import {
  buildWorkspaceImagePersistentCacheKey,
  readWorkspaceImagePersistentCache,
  writeWorkspaceImagePersistentCache
} from '@/utils/workspaceImagePersistentCache';
import {
  isImagePath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import {
  extractWorkspaceRefreshPaths,
  isWorkspacePathAffected,
  normalizeWorkspaceRefreshContainerId,
  normalizeWorkspaceRefreshTreeVersion
} from '@/utils/workspaceRefresh';
import {
  isCompactionOnlyWorkflowItems,
  isCompactionRunningFromWorkflowItems
} from '@/utils/chatCompactionWorkflow';
import { onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import {
  extractPromptToolingPreview,
  type PromptToolingPreviewItem
} from '@/utils/promptToolingPreview';
import { isDemoMode } from '@/utils/demo';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { collectAbilityGroupDetails, collectAbilityNames } from '@/utils/toolSummary';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';
import { redirectToLoginAfterLogout } from '@/utils/authNavigation';
import { resolveUserBasePath } from '@/utils/basePath';
import { chatPerf } from '@/utils/chatPerf';

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
// Demo mode is used for quick product walkthroughs.
const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => resolveUserBasePath(route.path));
const desktopMode = computed(() => basePath.value === '/desktop');
const desktopLocalMode = computed(() => isDesktopModeEnabled() && !isDesktopRemoteAuthMode());
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
const composerShellRef = ref(null);
const composerPaddingPx = ref(176);
const chatShellStyle = computed(() => ({
  '--chat-composer-space': `${composerPaddingPx.value}px`
}));
const inquirySelection = ref([]);
const historyListRef = ref(null);
const historyScrollTop = ref(0);
const messagesContainerRef = ref(null);
const showScrollTopButton = ref(false);
// System prompt preview dialog state.
const promptPreviewVisible = ref(false);
const promptPreviewLoading = ref(false);
const promptPreviewContent = ref('');
const promptPreviewToolingMode = ref('');
const promptPreviewToolingContent = ref('');
const promptPreviewToolingItems = ref<PromptToolingPreviewItem[]>([]);
const promptPreviewSelectedNames = ref<string[] | null>(null);
const promptPreviewView = ref<'prompt' | 'tooling'>('prompt');
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const workspaceDialogVisible = ref(false);
const historyDialogVisible = ref(false);
const manualDraftPending = ref(false);
const creatingSession = ref(false);
const bootstrappingSession = ref(true);
const featureMenuVisible = ref(false);
const featureMenuRef = ref(null);
const cronDialogVisible = ref(false);
const channelDialogVisible = ref(false);
const agentSettingsVisible = ref(false);
const isCompactLayout = ref(false);
const imagePreviewDialogWidth = computed(() => (isCompactLayout.value ? '94vw' : '92vw'));
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
let composerResizeObserver = null;

const normalizeToolItemName = (item) => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  return item.name || item.tool_name || item.toolName || item.id || '';
};

const buildAllowedToolSet = (summary) => {
  const names = collectAbilityNames(summary || {});
  return new Set([...(names.tools || []), ...(names.skills || [])]);
};

const normalizeToolNameList = (values) => {
  const output = [];
  const seen = new Set();
  if (!Array.isArray(values)) return output;
  values.forEach((item) => {
    const name = String(item || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push(name);
  });
  return output;
};

const resolveSelectedAbilityNamesFromProfile = (agent) => {
  const source = agent && typeof agent === 'object' ? agent : {};
  const abilitySource = Array.isArray(source.ability_items)
    ? source.ability_items
    : Array.isArray(source?.abilities?.items)
      ? source.abilities.items
      : [];
  const selected = [];
  const seen = new Set();
  abilitySource.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    if (item.selected === false) return;
    const name = String(item.runtime_name || item.runtimeName || item.name || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    selected.push(name);
  });
  return selected;
};

const resolveAgentConfiguredToolNames = (agent) => {
  const source = agent && typeof agent === 'object' ? agent : {};
  const declared = normalizeToolNameList([
    ...normalizeToolNameList(source.declared_tool_names),
    ...normalizeToolNameList(source.declared_skill_names)
  ]);
  if (declared.length > 0) {
    return declared;
  }
  // tool_names is the persisted selection source; keep it ahead of legacy ability_items.
  const selectedFromToolNames = normalizeToolNameList([
    ...normalizeToolNameList(source.tool_names),
    ...normalizeToolNameList(source.toolNames)
  ]);
  if (selectedFromToolNames.length > 0) {
    return selectedFromToolNames;
  }
  const selectedFromItems = resolveSelectedAbilityNamesFromProfile(source);
  if (selectedFromItems.length > 0) {
    return selectedFromItems;
  }
  return [];
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
  const filterUnifiedItems = (list) =>
    Array.isArray(list)
      ? list.filter((item) => {
          if (!item || typeof item !== 'object') return false;
          const source = item;
          const name = String(
            source.runtime_name ||
              source.runtimeName ||
              source.name ||
              source.tool_name ||
              source.toolName ||
              source.id ||
              ''
          ).trim();
          return name && allowedSet.has(name);
        })
      : [];
  return {
    ...summary,
    builtin_tools: filterList(summary.builtin_tools),
    mcp_tools: filterList(summary.mcp_tools),
    a2a_tools: filterList(summary.a2a_tools),
    skills: filterList(summary.skills),
    skill_list: filterList(summary.skill_list),
    skillList: filterList(summary.skillList),
    knowledge_tools: filterList(summary.knowledge_tools),
    user_tools: filterList(summary.user_tools),
    shared_tools: filterList(summary.shared_tools),
    items: filterUnifiedItems(summary.items),
    itemList: filterUnifiedItems(summary.itemList)
  };
};

function applyToolOverridesToSummary(summary, overrides = [], agentDefaults = []) {
  if (!summary) return null;
  const allowedSet = buildAllowedToolSet(summary);
  if (!allowedSet.size) return summary;
  const normalizedAgentDefaults = normalizeToolNameList(agentDefaults);
  const agentDefaultSet = new Set(normalizedAgentDefaults);
  const normalizedOverrides = normalizeToolNameList(overrides);
  if (normalizedOverrides.includes(TOOL_OVERRIDE_NONE)) {
    return filterSummaryByNames(summary, new Set());
  }
  const sourceNames = normalizedOverrides.length
    ? normalizedOverrides.filter(
        (name) => !agentDefaultSet.size || agentDefaultSet.has(name)
      )
    : normalizedAgentDefaults;
  const effectiveSet = new Set();
  sourceNames.forEach((name) => {
    if (allowedSet.has(name)) {
      effectiveSet.add(name);
    }
  });
  return filterSummaryByNames(summary, effectiveSet);
}

const extractPromptPreviewSelectedToolNames = (payload) => {
  const source = payload && typeof payload === 'object' ? payload : {};
  const tooling =
    source.tooling_preview && typeof source.tooling_preview === 'object'
      ? source.tooling_preview
      : {};
  return normalizeToolNameList(tooling.selected_tool_names);
};

const activeSession = computed(
  () => chatStore.sessions.find((item) => item.id === chatStore.activeSessionId) || null
);
const routeAgentId = computed(() => String(route.query.agent_id || '').trim());
const routeEntry = computed(() => String(route.query.entry || '').trim().toLowerCase());
const routeContainerId = computed(() => normalizeSandboxContainerId(route.query.container_id));
const DEFAULT_AGENT_KEY = '__default__';
const activeAgentId = computed(
  () => activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || ''
);
const activeAgent = computed(() =>
  activeAgentId.value ? agentStore.agentMap[activeAgentId.value] || null : null
);
const defaultAgent = computed(() => agentStore.agentMap[DEFAULT_AGENT_KEY] || null);
watch([() => chatStore.activeSessionId, activeAgentId], () => {
  promptPreviewSelectedNames.value = null;
});
const activeAgentPresetQuestions = computed(() => {
  if (!activeAgentId.value) {
    return normalizeAgentPresetQuestions((defaultAgent.value as Record<string, unknown> | null)?.preset_questions);
  }
  return normalizeAgentPresetQuestions((activeAgent.value as Record<string, unknown> | null)?.preset_questions);
});
const normalizeAgentApprovalMode = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return 'full_auto';
};
const approvalModeForRequest = computed(() => {
  if (!desktopLocalMode.value) {
    return 'full_auto';
  }
  const agentId = String(activeAgentId.value || '').trim();
  if (!agentId) {
    return 'full_auto';
  }
  const agent = activeAgent.value || {};
  return normalizeAgentApprovalMode(agent.approval_mode || agent.approvalMode || 'full_auto');
});
const normalizeSandboxContainerId = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};
const activeSandboxContainerId = computed(() => {
  const agentContainer = activeAgent.value?.sandbox_container_id;
  const cleaned =
    agentContainer === null || agentContainer === undefined ? '' : String(agentContainer).trim();
  if (cleaned) {
    return normalizeSandboxContainerId(agentContainer);
  }
  return routeContainerId.value;
});
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
  if (promptPreviewSelectedNames.value !== null) {
    return filterSummaryByNames(
      promptToolSummary.value,
      new Set(promptPreviewSelectedNames.value)
    );
  }
  const activeProfile = activeAgentId.value
    ? (activeAgent.value as Record<string, unknown> | null)
    : (defaultAgent.value as Record<string, unknown> | null);
  const activeToolNames = resolveAgentConfiguredToolNames(activeProfile || {});
  return applyToolOverridesToSummary(promptToolSummary.value, [], activeToolNames);
});
const promptPreviewHtml = computed(() => {
  const content = promptPreviewContent.value || t('chat.systemPrompt.empty');
  return renderSystemPromptHighlight(content, effectiveToolSummary.value || {});
});
const promptPreviewToolingModeLabel = computed(() => {
  const mode = String(promptPreviewToolingMode.value || '').trim().toLowerCase();
  if (!mode) return '-';
  const key = `chat.systemPrompt.toolCallMode.${mode}`;
  const translated = t(key);
  return translated === key ? mode : translated;
});
const hasPromptPreviewTooling = computed(
  () =>
    promptPreviewToolingItems.value.length > 0 ||
    String(promptPreviewToolingContent.value || '').trim().length > 0
);
// Ability tooltip sections share the same tool and skill summary payload.
const abilitySections = computed(() => {
  const groups = collectAbilityGroupDetails(effectiveToolSummary.value || {});
  return [
    {
      key: 'skills',
      kind: 'skill',
      title: t('toolManager.system.skills'),
      emptyText: t('chat.ability.emptySkills'),
      items: groups.skills
    },
    {
      key: 'mcp',
      kind: 'tool',
      title: t('toolManager.system.mcp'),
      emptyText: t('chat.ability.emptyTools'),
      items: groups.mcp
    },
    {
      key: 'knowledge',
      kind: 'tool',
      title: t('toolManager.system.knowledge'),
      emptyText: t('chat.ability.emptyTools'),
      items: groups.knowledge
    },
    {
      key: 'a2a',
      kind: 'tool',
      title: t('toolManager.system.a2a'),
      emptyText: t('chat.ability.emptyTools'),
      items: groups.a2a
    },
    {
      key: 'builtin',
      kind: 'tool',
      title: t('toolManager.system.builtin'),
      emptyText: t('chat.ability.emptyTools'),
      items: groups.builtin
    }
  ].filter((section) => section.items.length > 0);
});
const hasAbilitySummary = computed(() =>
  abilitySections.value.some((section) => section.items.length > 0)
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
const historyInitialLoading = computed(
  () => bootstrappingSession.value && historySourceSessions.value.length === 0
);
const messageInitialLoading = computed(
  () =>
    bootstrappingSession.value &&
    Boolean(chatStore.activeSessionId) &&
    chatStore.messages.length <= 1
);
const historyLoading = computed(() => chatStore.historyLoading(chatStore.activeSessionId));
const canLoadMoreHistory = computed(() => chatStore.canLoadMoreHistory(chatStore.activeSessionId));
let pendingAssistantCenter = false;
let pendingAssistantCenterCount = 0;
let pendingEnterBottomScroll = true;
const MESSAGE_AUTOLOAD_TOP_PX = 32;
const MESSAGE_AUTOLOAD_COOLDOWN_MS = 1200;
const MESSAGE_AUTOLOAD_SCROLLABLE_PADDING = 8;
const MESSAGE_SCROLL_TOP_BUTTON_PX = 88;
let lastMessageScrollTop = 0;
let lastMessageAutoLoadAt = 0;
let messageAutoLoadPending = false;

const updateMessageScrollUi = (container = messagesContainerRef.value as HTMLElement | null) => {
  if (!container) {
    showScrollTopButton.value = false;
    return;
  }
  const scrollTop = Math.max(0, container.scrollTop || 0);
  const scrollableHeight = (container.scrollHeight || 0) - (container.clientHeight || 0);
  showScrollTopButton.value =
    scrollableHeight > MESSAGE_AUTOLOAD_SCROLLABLE_PADDING && scrollTop > MESSAGE_SCROLL_TOP_BUTTON_PX;
};

const scheduleMessageScrollUiRefresh = () => {
  if (typeof window === 'undefined') {
    updateMessageScrollUi();
    return;
  }
  window.requestAnimationFrame(() => {
    updateMessageScrollUi();
  });
};

const resolveInitialSessionId = (agentId, sourceSessions = chatStore.sessions) => {
  const normalizedAgentId = String(agentId || '').trim();
  const sessions = (Array.isArray(sourceSessions) ? sourceSessions : []).filter((session) => {
    const sessionAgentId = String(session?.agent_id || '').trim();
    return normalizedAgentId ? sessionAgentId === normalizedAgentId : !sessionAgentId;
  });
  const mainSession = sessions.find((session) => session.is_main);
  if (mainSession?.id) {
    return mainSession.id;
  }
  const persisted = chatStore.getLastSessionId?.(normalizedAgentId) || '';
  if (persisted && sessions.some((session) => session.id === persisted)) {
    return persisted;
  }
  return sessions[0]?.id || '';
};

const hasSessionsForAgent = (agentId, sourceSessions = chatStore.sessions) => {
  const normalizedAgentId = String(agentId || '').trim();
  return (Array.isArray(sourceSessions) ? sourceSessions : []).some((session) => {
    const sessionAgentId = String(session?.agent_id || '').trim();
    return normalizedAgentId ? sessionAgentId === normalizedAgentId : !sessionAgentId;
  });
};

const openAgentSession = async (agentId) => {
  const normalizedAgentId = String(agentId || '').trim();
  const cachedSessions = chatStore.getCachedSessions?.(normalizedAgentId) || [];
  if (cachedSessions.length) {
    chatStore.sessions = cachedSessions;
  }
  const optimisticSourceSessions = cachedSessions.length > 0 ? cachedSessions : chatStore.sessions;
  const optimisticSessionId = resolveInitialSessionId(normalizedAgentId, optimisticSourceSessions);
  const hasWarmSessionList = hasSessionsForAgent(normalizedAgentId, optimisticSourceSessions);
  const currentAgentId = String(activeSession.value?.agent_id || chatStore.draftAgentId || '').trim();
  const switchingAgent = currentAgentId !== normalizedAgentId;
  if (switchingAgent && !optimisticSessionId) {
    manualDraftPending.value = true;
    chatStore.openDraftSession({ agent_id: normalizedAgentId });
  } else {
    manualDraftPending.value = false;
  }

  let optimisticDetailPromise: Promise<unknown> | null = null;
  const hasOptimisticCache = optimisticSessionId
    ? Boolean(chatStore.hasSessionMessages?.(optimisticSessionId))
    : false;
  if (
    optimisticSessionId &&
    (chatStore.activeSessionId !== optimisticSessionId || !hasOptimisticCache)
  ) {
    optimisticDetailPromise = chatStore.loadSessionDetail(optimisticSessionId).catch(() => null);
    if (!hasOptimisticCache) {
      await optimisticDetailPromise;
    }
  }

  const reconcileSessions = async (sessions, awaitOptimistic = false) => {
    const targetId = resolveInitialSessionId(normalizedAgentId, sessions);
    if (targetId) {
      if (chatStore.activeSessionId !== targetId) {
        await chatStore.loadSessionDetail(targetId);
      } else if (awaitOptimistic && optimisticDetailPromise) {
        await optimisticDetailPromise;
      }
      return true;
    }

    manualDraftPending.value = true;
    chatStore.openDraftSession({ agent_id: normalizedAgentId });
    return false;
  };

  const sessionsPromise = chatStore.loadSessions({ agent_id: normalizedAgentId });
  if (hasWarmSessionList) {
    void sessionsPromise.then((sessions) => reconcileSessions(sessions)).catch(() => null);
    if (!optimisticSessionId && !switchingAgent) {
      manualDraftPending.value = true;
      chatStore.openDraftSession({ agent_id: normalizedAgentId });
    }
    return;
  }

  const sessions = await sessionsPromise;
  await reconcileSessions(sessions, true);
};

const init = async () => {
  bootstrappingSession.value = true;
  try {
    if (demoMode.value || !authStore.user) {
      await authStore.loadProfile();
    }
    const initialAgentId = routeEntry.value === 'default' ? '' : routeAgentId.value;
    await openAgentSession(initialAgentId);
    if (routeEntry.value === 'default') {
      router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
    }
  } finally {
    bootstrappingSession.value = false;
  }
};

const resolveReusableFreshSessionId = () =>
  chatStore.resolveReusableFreshSessionId(String(activeAgentId.value || '').trim());

const requestStopActiveSessionStream = () => {
  if (!chatStore.activeSessionId) {
    return;
  }
  // Stop the previous active thread immediately before opening a new one.
  void chatStore.stopStream().catch(() => null);
};

const openOrReuseFreshSession = async () => {
  const reusableSessionId = resolveReusableFreshSessionId();
  if (reusableSessionId) {
    if (String(chatStore.activeSessionId || '').trim() !== reusableSessionId) {
      requestStopActiveSessionStream();
    }
    await chatStore.setMainSession(reusableSessionId);
    manualDraftPending.value = false;
    return reusableSessionId;
  }

  requestStopActiveSessionStream();
  const agentId = String(activeAgentId.value || '').trim();
  const payload = agentId ? { agent_id: agentId } : {};
  const session = await chatStore.createSession(payload);
  const sessionId = String(session?.id || '').trim();
  if (sessionId) {
    await chatStore.setMainSession(sessionId);
  }
  manualDraftPending.value = false;
  return sessionId;
};

const createFreshSessionWithGuard = async () => {
  if (creatingSession.value) {
    return String(chatStore.activeSessionId || '').trim();
  }
  creatingSession.value = true;
  try {
    return await openOrReuseFreshSession();
  } finally {
    creatingSession.value = false;
  }
};

const handleCreateSession = async () => {
  try {
    await createFreshSessionWithGuard();
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
};

const handleOpenProfile = () => {
  router.push(`${basePath.value}/profile`);
};

const resolveMessageAvatarActionLabel = (message) => {
  const role = String(message?.role || '').trim().toLowerCase();
  return role === 'assistant' ? t('chat.features.agentSettings') : t('user.profile.enter');
};

const handleMessageAvatarClick = async (message) => {
  const role = String(message?.role || '').trim().toLowerCase();
  if (role === 'assistant') {
    await handleFeatureAction('agent-settings');
    return;
  }
  if (role === 'user') {
    handleOpenProfile();
  }
};

const handleOpenPortal = () => {
  router.push(basePath.value + '/home');
};

const handleOpenSystemSettings = () => {
  if (!desktopMode.value) {
    return;
  }
  router.push('/desktop/settings?section=more&panel=desktop-models');
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

const isHiddenInternalMessage = (message) => Boolean(message?.hiddenInternal);

const shouldShowMessageText = (message) => {
  if (!message) return false;
  if (message.role !== 'assistant') return true;
  return Boolean(String(message.content || '').trim());
};

const hasPlanSteps = (plan) =>
  Array.isArray(plan?.steps) && plan.steps.length > 0;

const isPlanMessageDismissed = (message) =>
  Boolean(message) && dismissedPlanMessages.value.has(message);

const markPlanMessageDismissed = (message) => {
  if (!message) return;
  dismissedPlanMessages.value.add(message);
  dismissedPlanVersion.value += 1;
};

const activePlanMessage = computed(() => {
  // Force recompute when manual dismiss state changes.
  void dismissedPlanVersion.value;
  for (let i = chatStore.messages.length - 1; i >= 0; i -= 1) {
    const message = chatStore.messages[i];
    if (message?.role !== 'assistant') continue;
    if (!hasPlanSteps(message.plan)) continue;
    if (isPlanMessageDismissed(message)) return null;
    return message;
  }
  return null;
});

const activePlan = computed(() => activePlanMessage.value?.plan || null);
const planExpanded = ref(false);
const dismissedPlanMessages = ref(new WeakSet());
const dismissedPlanVersion = ref(0);

const dismissActivePlan = () => {
  const target = activePlanMessage.value;
  if (!target) return;
  markPlanMessageDismissed(target);
  planExpanded.value = false;
};

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
const AGENT_AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;
const AGENT_AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?，。；：！？》】」』、]+)?$/;

const isAssistantStreaming = (message) => {
  if (!message || message.role !== 'assistant') return false;
  if (Boolean(message.workflowStreaming || message.reasoningStreaming || message.stream_incomplete)) {
    return true;
  }
  return isCompactionRunningFromWorkflowItems(message.workflowItems);
};

const isCompactionMarkerMessage = (message): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (!isCompactionOnlyWorkflowItems(message.workflowItems)) return false;
  if (String(message.content || '').trim()) return false;
  if (String(message.reasoning || '').trim()) return false;
  if (hasPlanSteps(message.plan)) return false;
  const panelStatus = String(message?.questionPanel?.status || '').trim().toLowerCase();
  return panelStatus !== 'pending';
};

// Assistant replies render through Markdown so tables and rich text stay readable.
const renderAssistantMarkdown = (message) => {
  const content = prepareMessageMarkdownContent(
    normalizeChatMessageContentForMarkdown(message?.content),
    message
  );
  if (!content) return '';
  const cached = markdownCache.get(message);
  if (cached && cached.source === content) {
    return cached.html;
  }
  const html = renderMarkdown(content, { resolveWorkspacePath: resolveChatMarkdownWorkspacePath });
  markdownCache.set(message, { source: content, html });
  return html;
};

const renderUserMarkdown = (message) => {
  const content = normalizeChatMessageContentForMarkdown(message?.content);
  if (!content) return '';
  const cached = markdownCache.get(message);
  if (cached && cached.source === content) {
    return cached.html;
  }
  const html = renderMarkdown(content, { resolveWorkspacePath: resolveChatMarkdownWorkspacePath });
  markdownCache.set(message, { source: content, html });
  return html;
};

const hasUserMarkdownContent = (message): boolean =>
  Boolean(String(message?.content || '').trim());

const resolveUserImageAttachments = (message) => {
  const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
  return attachments
    .map((item, index) => {
      const src = String(item?.content || '').trim();
      if (!src.startsWith('data:image/')) return null;
      const fallbackName = `image-${index + 1}`;
      const name = String(item?.name || fallbackName).trim() || fallbackName;
      return {
        key: `${name}-${index}`,
        src,
        name
      };
    })
    .filter(Boolean);
};

const hasUserImageAttachments = (message): boolean =>
  resolveUserImageAttachments(message).length > 0;

type WorkspaceResourceCachePayload = { objectUrl: string; filename: string };
type WorkspaceResourceCacheEntry = {
  objectUrl?: string;
  filename?: string;
  promise?: Promise<WorkspaceResourceCachePayload>;
};

const WORKSPACE_RESOURCE_LOADING_LABEL_DELAY_MS = 160;
const workspaceResourceCache = new Map<string, WorkspaceResourceCacheEntry>();
let workspaceResourceHydrationFrame = null;
let workspaceResourceHydrationPending = false;
let workspaceResourceHydrationForceFull = false;
let workspaceResourceHydrationContainerId: number | null = null;
let workspaceResourceHydrationTargetPaths = new Set<string>();
const workspaceRefreshVersionByScope = new Map<string, number>();
const MAX_WORKSPACE_REFRESH_SCOPE_CACHE = 64;
let stopWorkspaceRefreshListener = null;

const isAdminUser = (user) =>
  Array.isArray(user?.roles) && user.roles.some((role) => role === 'admin' || role === 'super_admin');

const normalizeUploadPath = (value: unknown): string =>
  String(value || '')
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .trim();

const buildChatWorkspaceId = (): string => {
  const ownerId = normalizeWorkspaceOwnerId(authStore.user?.id);
  if (!ownerId) return '';
  const agentScopeId = buildAgentWorkspaceScopeId(ownerId, activeAgentId.value);
  if (agentScopeId && agentScopeId !== ownerId) {
    return agentScopeId;
  }
  const containerId = Number(activeSandboxContainerId.value);
  if (Number.isFinite(containerId) && containerId > 0) {
    return `${ownerId}__c__${Math.trunc(containerId)}`;
  }
  return ownerId;
};

const resolveDesktopWorkspaceRoot = (): string => {
  const runtime = getDesktopRuntime();
  if (runtime?.workspace_root) {
    return runtime.workspace_root;
  }
  const runtimeConfig = getRuntimeConfig();
  return runtimeConfig.workspace_root || '';
};

const buildChatWorkspacePublicPath = (relativePath: string): string => {
  const workspaceId = buildChatWorkspaceId();
  return buildWorkspacePublicPathFromScope(workspaceId, relativePath);
};

const resolveChatMarkdownWorkspacePath = (rawPath: string): string => {
  const workspaceId = buildChatWorkspaceId();
  if (!workspaceId) return '';
  return resolveMarkdownWorkspacePath({
    rawPath,
    ownerId: authStore.user?.id,
    workspaceScopeId: workspaceId,
    desktopLocalMode: desktopLocalMode.value,
    workspaceRoot: resolveDesktopWorkspaceRoot()
  });
};

const decodeAgentAtPathToken = (value: string): string => {
  if (!/%[0-9a-fA-F]{2}/.test(value)) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const replaceAgentAtPathTokens = (content: string): string => {
  if (!content) return '';
  return content.replace(AGENT_AT_PATH_RE, (match, prefix, token, doubleQuoted, singleQuoted) => {
    const raw = doubleQuoted ?? singleQuoted ?? token ?? '';
    if (!raw) return match;
    let value = raw;
    let suffix = '';
    if (!doubleQuoted && !singleQuoted) {
      const split = AGENT_AT_PATH_SUFFIX_RE.exec(value);
      if (split) {
        value = split[1] ?? value;
        suffix = split[2] ?? '';
      }
    }
    const decoded = decodeAgentAtPathToken(String(value || '').trim());
    const normalized = normalizeUploadPath(decoded);
    if (!normalized) return match;
    const pathLike =
      decoded.startsWith('/') ||
      decoded.startsWith('./') ||
      decoded.startsWith('../') ||
      normalized.includes('/') ||
      normalized.includes('.');
    if (!pathLike) return match;
    const publicPath = buildChatWorkspacePublicPath(normalized);
    if (!publicPath) return match;
    const replacement = isImagePath(normalized)
      ? `![${decoded}](${publicPath})`
      : `[${decoded}](${publicPath})`;
    return `${prefix}${replacement}${suffix}`;
  });
};

const normalizeChatMessageContentForMarkdown = (value: unknown): string => {
  const content = String(value || '');
  if (!content) return '';
  return replaceAgentAtPathTokens(content);
};

const workspaceBelongsToCurrentUser = (workspaceId: string, currentUserId: string) => {
  if (!workspaceId || !currentUserId) return false;
  return (
    workspaceId === currentUserId ||
    workspaceId.startsWith(`${currentUserId}__c__`) ||
    workspaceId.startsWith(`${currentUserId}__a__`) ||
    workspaceId.startsWith(`${currentUserId}__agent__`)
  );
};

const resolveWorkspaceResource = (publicPath) => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const user = authStore.user;
  if (!user) return null;
  const currentId = String(user.id || '').trim();
  const safeCurrentId = normalizeWorkspaceOwnerId(currentId);
  const workspaceId = normalizeWorkspaceOwnerId(parsed.workspaceId || parsed.userId || '');
  const ownerId = normalizeWorkspaceOwnerId(parsed.ownerId || workspaceId);
  const agentId = String(parsed.agentId || '').trim();
  const containerId =
    typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
      ? parsed.containerId
      : null;
  if (!workspaceId || !ownerId) return null;
  const isOwner = workspaceBelongsToCurrentUser(workspaceId, safeCurrentId);
  if (isOwner) {
    const requestUserId = ownerId === safeCurrentId ? null : ownerId;
    return {
      ...parsed,
      requestUserId,
      requestAgentId: agentId || null,
      requestContainerId: containerId,
      allowed: true
    };
  }
  if (isAdminUser(user)) {
    return {
      ...parsed,
      requestUserId: ownerId,
      requestAgentId: agentId || null,
      requestContainerId: containerId,
      allowed: true
    };
  }
  return {
    ...parsed,
    requestUserId: null,
    requestAgentId: agentId || null,
    requestContainerId: containerId,
    allowed: false
  };
};

const buildWorkspaceResourcePersistentCacheKey = (resource) => {
  const currentUserId = normalizeWorkspaceOwnerId(authStore.user?.id);
  return buildWorkspaceImagePersistentCacheKey({
    scope: currentUserId || 'anonymous',
    requestUserId: resource.requestUserId,
    requestAgentId: resource.requestAgentId,
    requestContainerId: resource.requestContainerId,
    publicPath: resource.publicPath
  });
};

const resolveWorkspaceLoadingLabel = (status: HTMLElement | null): string => {
  const raw = status?.dataset?.loadingLabel;
  const normalized = String(raw || '').trim();
  return normalized || t('chat.resourceImageLoading');
};

const scheduleWorkspaceLoadingLabel = (
  card: HTMLElement,
  status: HTMLElement | null
): number | null => {
  if (!status || typeof window === 'undefined') return null;
  status.textContent = '';
  const label = resolveWorkspaceLoadingLabel(status);
  return window.setTimeout(() => {
    if (!card.isConnected || card.dataset.workspaceState !== 'loading') return;
    status.textContent = label;
  }, WORKSPACE_RESOURCE_LOADING_LABEL_DELAY_MS);
};

const clearWorkspaceLoadingLabelTimer = (timerId: number | null) => {
  if (timerId === null || typeof window === 'undefined') return;
  window.clearTimeout(timerId);
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
  if (cached?.objectUrl) {
    return {
      objectUrl: cached.objectUrl,
      filename: cached.filename || resource.filename || 'download'
    };
  }
  if (cached?.promise) return cached.promise;
  const allowPersistentCache = isImagePath(resource.filename || resource.relativePath || '');
  const persistentCacheKey = allowPersistentCache
    ? buildWorkspaceResourcePersistentCacheKey(resource)
    : '';
  const promise = (async () => {
    if (allowPersistentCache && persistentCacheKey) {
      const cachedPayload = await readWorkspaceImagePersistentCache(persistentCacheKey);
      if (cachedPayload?.blob) {
        const filename = cachedPayload.filename || resource.filename || 'download';
        const cachedBlob = normalizeWorkspaceImageBlob(
          cachedPayload.blob,
          filename,
          cachedPayload.blob.type
        );
        const objectUrl = URL.createObjectURL(cachedBlob);
        const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
        workspaceResourceCache.set(resource.publicPath, entry);
        return entry;
      }
    }
    const params: Record<string, string> = { path: String(resource.relativePath || '') };
    if (resource.requestUserId) {
      params.user_id = resource.requestUserId;
    }
    if (resource.requestAgentId) {
      params.agent_id = resource.requestAgentId;
    }
    if (resource.requestContainerId !== null && Number.isFinite(resource.requestContainerId)) {
      params.container_id = String(resource.requestContainerId);
    }
    const response = await downloadWunderWorkspaceFile(params);
    try {
      const filename = getFilenameFromHeaders(response.headers, resource.filename || 'download');
      const contentType = response.headers?.['content-type'] || response.headers?.['Content-Type'];
      const blob = normalizeWorkspaceImageBlob(response.data, filename, contentType);
      const objectUrl = URL.createObjectURL(blob);
      const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
      workspaceResourceCache.set(resource.publicPath, entry);
      if (allowPersistentCache && persistentCacheKey) {
        void writeWorkspaceImagePersistentCache(persistentCacheKey, {
          blob,
          filename
        });
      }
      return entry;
    } catch (error) {
      workspaceResourceCache.delete(resource.publicPath);
      throw error;
    }
  })()
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
  card.classList.remove('is-error');
  card.classList.remove('is-ready');
  const loadingTimerId = scheduleWorkspaceLoadingLabel(card as HTMLElement, status as HTMLElement | null);
  try {
    const entry = await fetchWorkspaceResource(resource);
    preview.src = entry.objectUrl;
    card.dataset.workspaceState = 'ready';
    card.classList.add('is-ready');
    if (status) status.textContent = '';
  } catch (error) {
    const fallbackText = card.dataset?.workspaceFallback || '';
    if (fallbackText && isWorkspaceResourceMissing(error)) {
      const fallbackNode = document.createElement('span');
      fallbackNode.className = 'ai-resource-fallback';
      fallbackNode.textContent = fallbackText;
      card.replaceWith(fallbackNode);
      return;
    }
    if (status) {
      status.textContent = isWorkspaceResourceMissing(error)
        ? t('chat.resourceMissing')
        : t('chat.resourceImageFailed');
    }
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
  } finally {
    clearWorkspaceLoadingLabelTimer(loadingTimerId);
  }
};

const shouldHydrateWorkspaceCard = (publicPath, changedPaths = [], eventContainerId = null) => {
  if (!publicPath) return false;
  if (!changedPaths.length && !Number.isFinite(eventContainerId)) {
    return true;
  }
  const { relativePath, containerId } = resolveWorkspaceCardMeta(publicPath);
  if (Number.isFinite(eventContainerId) && Number.isFinite(containerId) && containerId !== eventContainerId) {
    return false;
  }
  return isWorkspacePathAffected(relativePath, changedPaths);
};

const hydrateWorkspaceResources = (options: { changedPaths?: string[]; eventContainerId?: number | null } = {}) => {
  const container = messagesContainerRef.value;
  if (!container || !authStore.user) return;
  const changedPaths = Array.isArray(options.changedPaths) ? options.changedPaths : [];
  const eventContainerId = normalizeWorkspaceRefreshContainerId(options.eventContainerId);
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    const publicPath = card.getAttribute('data-workspace-path') || '';
    if (!shouldHydrateWorkspaceCard(publicPath, changedPaths, eventContainerId)) return;
    hydrateWorkspaceResourceCard(card);
  });
  hydrateExternalMarkdownImages(container);
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
      status.textContent = '';
    }
  });
};

const resolveWorkspaceCardMeta = (publicPath) => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) {
    return {
      relativePath: '',
      containerId: null
    };
  }
  return {
    relativePath: normalizeUploadPath(parsed.relativePath || ''),
    containerId:
      typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
        ? parsed.containerId
        : null
  };
};

const shouldApplyWorkspaceContainerFilter = (containerId, currentContainerId) => {
  if (!Number.isFinite(containerId)) return false;
  return containerId !== currentContainerId;
};

const clearWorkspaceResourceCacheByPaths = (changedPaths = [], eventContainerId = null) => {
  const currentContainerId = normalizeWorkspaceRefreshContainerId(activeSandboxContainerId.value) ?? 0;
  Array.from(workspaceResourceCache.entries()).forEach(([publicPath, entry]) => {
    const { relativePath, containerId } = resolveWorkspaceCardMeta(publicPath);
    const resourceContainerId = Number.isFinite(containerId) ? containerId : 0;
    if (shouldApplyWorkspaceContainerFilter(eventContainerId, currentContainerId)) {
      if (resourceContainerId !== eventContainerId) {
        return;
      }
    }
    if (!isWorkspacePathAffected(relativePath, changedPaths)) {
      return;
    }
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
    workspaceResourceCache.delete(publicPath);
  });
};

const resetWorkspaceResourceCardsByPaths = (changedPaths = [], eventContainerId = null) => {
  const container = messagesContainerRef.value;
  if (!container) return;
  const currentContainerId = normalizeWorkspaceRefreshContainerId(activeSandboxContainerId.value) ?? 0;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    const publicPath = card.getAttribute('data-workspace-path') || '';
    const kind = card.getAttribute('data-workspace-kind') || 'image';
    const { relativePath, containerId } = resolveWorkspaceCardMeta(publicPath);
    const resourceContainerId = Number.isFinite(containerId) ? containerId : 0;
    if (shouldApplyWorkspaceContainerFilter(eventContainerId, currentContainerId)) {
      if (resourceContainerId !== eventContainerId) {
        return;
      }
    }
    if (!isWorkspacePathAffected(relativePath, changedPaths)) {
      return;
    }
    card.setAttribute('data-workspace-state', '');
    card.classList.remove('is-error');
    card.classList.remove('is-ready');
    if (kind === 'image') {
      const preview = card.querySelector('.ai-resource-preview');
      if (preview && preview instanceof HTMLImageElement) {
        preview.removeAttribute('src');
      }
      const status = card.querySelector('.ai-resource-status');
      if (status) {
        status.textContent = '';
      }
    }
  });
};

const pruneWorkspaceRefreshVersionCache = () => {
  const overflow = workspaceRefreshVersionByScope.size - MAX_WORKSPACE_REFRESH_SCOPE_CACHE;
  if (overflow <= 0) return;
  let remaining = overflow;
  for (const key of workspaceRefreshVersionByScope.keys()) {
    workspaceRefreshVersionByScope.delete(key);
    remaining -= 1;
    if (remaining <= 0) break;
  }
};

const resolveWorkspaceRefreshScopeKey = (
  detail: Record<string, unknown>,
  currentAgentId: string,
  currentContainerId: number
) => {
  const workspaceId = String(detail.workspaceId ?? detail.workspace_id ?? '').trim();
  if (workspaceId) return workspaceId;
  return `${currentAgentId || '__default__'}|${currentContainerId}`;
};

const shouldSkipWorkspaceRefreshByVersion = (
  detail: Record<string, unknown>,
  scopeKey: string
) => {
  const nextVersion = normalizeWorkspaceRefreshTreeVersion(
    detail.treeVersion ?? detail.tree_version ?? detail.version
  );
  if (nextVersion === null) return false;
  const previous = workspaceRefreshVersionByScope.get(scopeKey);
  if (Number.isFinite(previous) && nextVersion <= (previous as number)) {
    return true;
  }
  workspaceRefreshVersionByScope.set(scopeKey, nextVersion);
  pruneWorkspaceRefreshVersionCache();
  return false;
};

const handleWorkspaceRefresh = (event?: Event) => {
  const detail =
    (event as CustomEvent<Record<string, unknown>> | undefined)?.detail &&
    typeof (event as CustomEvent<Record<string, unknown>>).detail === 'object'
      ? ((event as CustomEvent<Record<string, unknown>>).detail as Record<string, unknown>)
      : {};
  const eventAgentId = String(detail.agentId ?? detail.agent_id ?? '').trim();
  const currentAgentId = String(activeAgentId.value || '').trim();
  if (eventAgentId && eventAgentId !== currentAgentId) return;
  const eventContainerId = normalizeWorkspaceRefreshContainerId(
    detail.containerId ?? detail.container_id
  );
  const currentContainerId = normalizeWorkspaceRefreshContainerId(activeSandboxContainerId.value) ?? 0;
  if (Number.isFinite(eventContainerId) && eventContainerId !== currentContainerId) return;
  const scopeKey = resolveWorkspaceRefreshScopeKey(detail, currentAgentId, currentContainerId);
  if (shouldSkipWorkspaceRefreshByVersion(detail, scopeKey)) {
    chatPerf.count('workspace.resource.refresh', 1, {
      scope: 'chat',
      mode: 'skip-version'
    });
    return;
  }
  const changedPaths = extractWorkspaceRefreshPaths(detail);
  if (!changedPaths.length) {
    clearWorkspaceResourceCache();
    resetWorkspaceResourceCards();
    chatPerf.count('workspace.resource.refresh', 1, {
      scope: 'chat',
      mode: 'full'
    });
    scheduleWorkspaceResourceHydration();
    return;
  }
  clearWorkspaceResourceCacheByPaths(changedPaths, eventContainerId);
  resetWorkspaceResourceCardsByPaths(changedPaths, eventContainerId);
  chatPerf.count('workspace.resource.refresh', 1, {
    scope: 'chat',
    mode: 'incremental',
    pathCount: changedPaths.length
  });
  scheduleWorkspaceResourceHydration({
    changedPaths,
    eventContainerId
  });
};

const scheduleWorkspaceResourceHydration = (
  options: { changedPaths?: string[]; eventContainerId?: number | null } = {}
) => {
  const changedPaths = Array.isArray(options.changedPaths)
    ? options.changedPaths.filter((item): item is string => typeof item === 'string' && item.length > 0)
    : [];
  const eventContainerId = normalizeWorkspaceRefreshContainerId(options.eventContainerId);
  if (!changedPaths.length) {
    workspaceResourceHydrationForceFull = true;
    workspaceResourceHydrationTargetPaths = new Set<string>();
    workspaceResourceHydrationContainerId = null;
  } else if (!workspaceResourceHydrationForceFull) {
    changedPaths.forEach((path) => workspaceResourceHydrationTargetPaths.add(path));
    if (Number.isFinite(eventContainerId)) {
      workspaceResourceHydrationContainerId = eventContainerId;
    }
  }
  if (workspaceResourceHydrationFrame || workspaceResourceHydrationPending) return;
  workspaceResourceHydrationPending = true;
  void nextTick(() => {
    workspaceResourceHydrationPending = false;
    if (workspaceResourceHydrationFrame) return;
    workspaceResourceHydrationFrame = requestAnimationFrame(() => {
      workspaceResourceHydrationFrame = null;
      const useFullHydration =
        workspaceResourceHydrationForceFull || workspaceResourceHydrationTargetPaths.size === 0;
      const pendingPaths = useFullHydration
        ? []
        : Array.from(workspaceResourceHydrationTargetPaths);
      const pendingContainerId = useFullHydration ? null : workspaceResourceHydrationContainerId;
      workspaceResourceHydrationForceFull = false;
      workspaceResourceHydrationTargetPaths = new Set<string>();
      workspaceResourceHydrationContainerId = null;
      hydrateWorkspaceResources({
        changedPaths: pendingPaths,
        eventContainerId: pendingContainerId
      });
    });
  });
};

const clearWorkspaceResourceCache = () => {
  if (workspaceResourceHydrationFrame) {
    cancelAnimationFrame(workspaceResourceHydrationFrame);
    workspaceResourceHydrationFrame = null;
  }
  workspaceResourceHydrationPending = false;
  workspaceResourceHydrationForceFull = false;
  workspaceResourceHydrationTargetPaths = new Set<string>();
  workspaceResourceHydrationContainerId = null;
  workspaceRefreshVersionByScope.clear();
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
      await createFreshSessionWithGuard();
      chatStore.appendLocalMessage('assistant', t('chat.command.newSuccess'));
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
    chatStore.appendLocalMessage('user', rawText, { sessionId: activeId });
    try {
      await chatStore.compactSession(activeId);
      scheduleExternalSessionSync(false);
    } catch (error) {
      chatStore.appendLocalMessage(
        'assistant',
        t('chat.command.compactFailed', { message: resolveCommandErrorMessage(error) }),
        { sessionId: activeId }
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
    const normalizedContent = normalizeChatMessageContentForMarkdown(finalContent);
    await chatStore.sendMessage(normalizedContent, {
      attachments: payloadAttachments,
      suppressQueuedNotice: hasSelection,
      approvalMode: approvalModeForRequest.value
    });
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

const handleLoadOlderHistory = async (autoOrEvent: boolean | Event = false) => {
  const auto = typeof autoOrEvent === 'boolean' ? autoOrEvent : false;
  const sessionId = chatStore.activeSessionId;
  if (!sessionId || historyLoading.value || !canLoadMoreHistory.value) return;
  if (chatPerf.enabled()) {
    chatPerf.count('chat_history_load_trigger', 1, {
      reason: auto ? 'auto' : 'manual',
      sessionId
    });
  }
  const container = messagesContainerRef.value as HTMLElement | null;
  const previousHeight = container?.scrollHeight || 0;
  const previousScrollTop = container?.scrollTop || 0;
  await chatStore.loadOlderHistory(sessionId);
  await nextTick();
  if (container) {
    const nextHeight = container.scrollHeight || 0;
    const delta = nextHeight - previousHeight;
    container.scrollTop = previousScrollTop + delta;
    updateMessageScrollUi(container);
  }
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
  return Boolean(message.slow_client || message.resume_available);
};

const handleResumeMessage = async (message) => {
  if (!message) return;
  const sessionId = chatStore.activeSessionId;
  if (!sessionId) return;
  message.resume_available = false;
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
  if (demoMode.value) {
    router.replace('/login');
    return;
  }
  authStore.logout();
  redirectToLoginAfterLogout((to) => router.replace(to));
};

const handleMessageScroll = async (event) => {
  const container = event?.target as HTMLElement | null;
  if (!container) return;
  const scrollTop = container.scrollTop || 0;
  const previousTop = lastMessageScrollTop;
  lastMessageScrollTop = scrollTop;
  updateMessageScrollUi(container);
  if (messageInitialLoading.value || historyLoading.value || !canLoadMoreHistory.value) {
    return;
  }
  if (scrollTop > MESSAGE_AUTOLOAD_TOP_PX) {
    return;
  }
  if (container.scrollHeight - container.clientHeight <= MESSAGE_AUTOLOAD_SCROLLABLE_PADDING) {
    return;
  }
  const now = Date.now();
  if (messageAutoLoadPending) {
    return;
  }
  if (previousTop <= MESSAGE_AUTOLOAD_TOP_PX && now - lastMessageAutoLoadAt < MESSAGE_AUTOLOAD_COOLDOWN_MS) {
    return;
  }
  messageAutoLoadPending = true;
  lastMessageAutoLoadAt = now;
  try {
    await handleLoadOlderHistory(true);
  } finally {
    messageAutoLoadPending = false;
  }
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

const isSessionBusy = (sessionId) =>
  Boolean(chatStore.isSessionBusy?.(sessionId) || chatStore.isSessionLoading?.(sessionId));

const resolveSessionRuntimeStatus = (sessionId) =>
  String(chatStore.sessionRuntimeStatus?.(sessionId) || '').trim().toLowerCase();

const isSessionWaiting = (sessionId) => {
  const status = resolveSessionRuntimeStatus(sessionId);
  return status === 'waiting_approval' || status === 'waiting_user_input';
};

const resolveSessionBusyLabel = (sessionId) => {
  const status = resolveSessionRuntimeStatus(sessionId);
  if (status === 'waiting_approval') {
    return t('chat.session.waitingApproval');
  }
  if (status === 'waiting_user_input') {
    return t('chat.session.waitingUserInput');
  }
  return t('chat.session.running');
};

const activeSessionBusy = computed(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) return false;
  return isSessionBusy(sessionId);
});

const shouldSkipExternalSessionSync = () => {
  const id = String(chatStore.activeSessionId || '').trim();
  if (!id) return false;
  if (isSessionBusy(id)) return true;
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

// Force Popper to recalculate after content changes so the first frame does not overflow.
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

// Load the shared tool summary for prompt preview and ability tooltip rendering.
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

const fetchPromptPreviewPayload = async () => {
  const normalizedAgentId = String(
    activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || DEFAULT_AGENT_KEY
  ).trim();
  await agentStore.getAgent(normalizedAgentId || DEFAULT_AGENT_KEY, { force: true }).catch(() => null);
  const previewAgentProfile = activeAgentId.value
    ? (activeAgent.value as Record<string, unknown> | null)
    : (defaultAgent.value as Record<string, unknown> | null);
  const previewAgentDefaults = resolveAgentConfiguredToolNames(previewAgentProfile || {});
  const overrides = previewAgentDefaults.length > 0 ? previewAgentDefaults : [TOOL_OVERRIDE_NONE];
  const agentId =
    activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || undefined;
  const requestPayload = chatStore.activeSessionId
    ? {
        ...(agentId ? { agent_id: agentId } : {})
      }
    : {
        ...(agentId ? { agent_id: agentId } : {}),
        ...(overrides ? { tool_overrides: overrides } : {})
      };
  const promptRequest = chatStore.activeSessionId
    ? fetchSessionSystemPrompt(chatStore.activeSessionId, requestPayload)
    : fetchRealtimeSystemPrompt(requestPayload);
  const promptResult = await promptRequest;
  return (promptResult?.data?.data || {}) as Record<string, unknown>;
};

const syncPromptPreviewSelectedNames = async (options: { force?: boolean } = {}) => {
  if (promptPreviewSelectedNames.value !== null && options.force !== true) {
    return promptPreviewSelectedNames.value;
  }
  try {
    const payload = await fetchPromptPreviewPayload();
    promptPreviewSelectedNames.value = extractPromptPreviewSelectedToolNames(payload);
    return promptPreviewSelectedNames.value;
  } catch {
    promptPreviewSelectedNames.value = null;
    return null;
  } finally {
    if (abilityTooltipVisible.value) {
      await updateAbilityTooltip();
    }
  }
};

const handleAbilityTooltipShow = () => {
  abilityTooltipVisible.value = true;
  void loadToolSummary();
  void syncPromptPreviewSelectedNames({ force: true });
  void updateAbilityTooltip();
};

const handleAbilityTooltipHide = () => {
  abilityTooltipVisible.value = false;
};

// Open the system prompt preview and prefer the current session snapshot.
const openPromptPreview = async () => {
  promptPreviewVisible.value = true;
  promptPreviewLoading.value = true;
  promptPreviewContent.value = '';
  promptPreviewToolingMode.value = '';
  promptPreviewToolingContent.value = '';
  promptPreviewToolingItems.value = [];
  promptPreviewView.value = 'prompt';
  const toolSummaryPromise = loadToolSummary();
  try {
    const responsePayload = await fetchPromptPreviewPayload();
    await toolSummaryPromise;
    promptPreviewSelectedNames.value = extractPromptPreviewSelectedToolNames(responsePayload);
    promptPreviewContent.value =
      typeof responsePayload.prompt === 'string' ? responsePayload.prompt : '';
    const toolingPreview = extractPromptToolingPreview(responsePayload);
    promptPreviewToolingMode.value = toolingPreview.mode;
    promptPreviewToolingContent.value = toolingPreview.text;
    promptPreviewToolingItems.value = toolingPreview.items;
    promptPreviewView.value = 'prompt';
  } catch (error) {
    showApiError(error, t('chat.systemPromptFailed'));
    promptPreviewSelectedNames.value = null;
    promptPreviewContent.value = '';
    promptPreviewToolingMode.value = '';
    promptPreviewToolingContent.value = '';
    promptPreviewToolingItems.value = [];
    promptPreviewView.value = 'prompt';
  } finally {
    promptPreviewLoading.value = false;
  }
};

const closePromptPreview = () => {
  promptPreviewVisible.value = false;
  promptPreviewView.value = 'prompt';
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
  } else {
    isCompactLayout.value = window.innerWidth <= 960;
  }
  updateComposerPadding();
};

const updateComposerPadding = () => {
  if (typeof window === 'undefined') return;
  const composerHeight = composerShellRef.value?.offsetHeight || 0;
  const viewport = window.innerHeight || 900;
  const minPadding = 132;
  const maxPadding = Math.max(180, Math.floor(viewport * 0.34));
  const nextPadding = Math.max(minPadding, Math.min(maxPadding, Math.round(composerHeight + 44)));
  if (composerPaddingPx.value !== nextPadding) {
    composerPaddingPx.value = nextPadding;
  }
};

const initComposerResizeObserver = () => {
  if (typeof window === 'undefined' || typeof ResizeObserver === 'undefined') return;
  if (composerResizeObserver) {
    composerResizeObserver.disconnect();
  }
  composerResizeObserver = new ResizeObserver(() => {
    updateComposerPadding();
  });
  if (composerShellRef.value) {
    composerResizeObserver.observe(composerShellRef.value);
  }
};

const destroyComposerResizeObserver = () => {
  if (!composerResizeObserver) return;
  composerResizeObserver.disconnect();
  composerResizeObserver = null;
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

const MIN_SPEED_DURATION_S = 0.2;
const MAX_REASONABLE_SPEED = 10000;
const DIRECT_SPEED_OUTLIER_RATIO = 2.5;

const normalizeSpeed = (speed, durationSeconds) => {
  if (!Number.isFinite(speed) || speed <= 0) return null;
  if (durationSeconds !== null && durationSeconds < MIN_SPEED_DURATION_S) return null;
  if (speed > MAX_REASONABLE_SPEED) return null;
  return speed;
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

const resolveTokenSpeed = (stats) => {
  const usageOutputTokens = Number(
    stats?.usage?.output ?? stats?.usage?.output_tokens ?? stats?.usage?.outputTokens
  );
  const usageTotalTokens = Number(
    stats?.usage?.total ?? stats?.usage?.total_tokens ?? stats?.usage?.totalTokens
  );
  const usageInputTokens = Number(
    stats?.usage?.input ?? stats?.usage?.input_tokens ?? stats?.usage?.inputTokens
  );
  const derivedOutputTokens =
    Number.isFinite(usageTotalTokens) &&
    usageTotalTokens > 0 &&
    Number.isFinite(usageInputTokens) &&
    usageInputTokens >= 0
      ? Math.max(0, usageTotalTokens - usageInputTokens)
      : NaN;
  const outputTokens =
    Number.isFinite(usageOutputTokens) && usageOutputTokens > 0
      ? usageOutputTokens
      : derivedOutputTokens;
  const averageSpeedRaw = Number(
    stats?.avg_model_round_speed_tps ??
      stats?.avgModelRoundSpeedTps ??
      stats?.average_speed_tps ??
      stats?.averageSpeedTps
  );
  const averageRoundsRaw = Number(
    stats?.avg_model_round_speed_rounds ??
      stats?.avgModelRoundSpeedRounds ??
      stats?.average_speed_rounds ??
      stats?.averageSpeedRounds
  );
  const averageSpeed = normalizeSpeed(averageSpeedRaw, null);
  const averageRounds = Number.isFinite(averageRoundsRaw) ? averageRoundsRaw : 0;
  const hasMultiRoundAverage = averageSpeed !== null && averageRounds >= 2;
  const decode = normalizeDurationSeconds(
    stats?.decode_duration_s ??
      stats?.decodeDurationS ??
      stats?.decodeDuration ??
      stats?.decode_duration_total_s ??
      stats?.decodeDurationTotalS
  );
  if (Number.isFinite(outputTokens) && outputTokens > 0 && decode !== null && decode > 0) {
    const speed = normalizeSpeed(outputTokens / decode, decode);
    if (speed !== null) {
      if (hasMultiRoundAverage && speed > averageSpeed * DIRECT_SPEED_OUTLIER_RATIO) {
        return averageSpeed;
      }
      return speed;
    }
  }
  if (Number.isFinite(outputTokens) && outputTokens > 0 && hasMultiRoundAverage) {
    return averageSpeed;
  }
  const hasSingleAverageRound =
    !Number.isFinite(averageRounds) || averageRounds <= 0 || averageRounds === 1;
  if (averageSpeed !== null && hasSingleAverageRound) {
    return averageSpeed;
  }
  return null;
};

const buildMessageStatsEntries = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return [];
  if (isAssistantStreaming(message)) return [];
  const stats = message.stats || null;
  if (!stats) return [];
  const durationSeconds = resolveDurationSeconds(stats);
  const speed = resolveTokenSpeed(stats);
  const usageInputTokens = Number(
    stats?.usage?.input ?? stats?.usage?.input_tokens ?? stats?.usage?.inputTokens
  );
  const usageTotalTokens = Number(
    stats?.usage?.total ?? stats?.usage?.total_tokens ?? stats?.usage?.totalTokens
  );
  const roundUsageInputTokens = Number(
    stats?.roundUsage?.input ??
    stats?.roundUsage?.input_tokens ??
    stats?.roundUsage?.inputTokens ??
    stats?.round_usage?.input ??
    stats?.round_usage?.input_tokens ??
    stats?.round_usage?.inputTokens
  );
  const roundUsageTotalTokens = Number(
    stats?.roundUsage?.total ??
    stats?.roundUsage?.total_tokens ??
    stats?.roundUsage?.totalTokens ??
    stats?.round_usage?.total ??
    stats?.round_usage?.total_tokens ??
    stats?.round_usage?.totalTokens
  );
  const explicitContextTokens = Number(
    stats?.contextTokens ??
    stats?.context_tokens ??
    stats?.context_tokens_total ??
    stats?.context_usage?.context_tokens ??
    stats?.context_usage?.contextTokens
  );
  const contextTokens =
    (Number.isFinite(roundUsageTotalTokens) && roundUsageTotalTokens > 0
      ? roundUsageTotalTokens
      : null) ??
    (Number.isFinite(roundUsageInputTokens) && roundUsageInputTokens > 0
      ? roundUsageInputTokens
      : null) ??
    (Number.isFinite(usageTotalTokens) && usageTotalTokens > 0 ? usageTotalTokens : null) ??
    (Number.isFinite(usageInputTokens) && usageInputTokens > 0 ? usageInputTokens : null) ??
    (Number.isFinite(explicitContextTokens) && explicitContextTokens > 0
      ? explicitContextTokens
      : null) ??
    null;
  const hasUsage = Number.isFinite(Number(contextTokens)) && Number(contextTokens) > 0;
  const hasDuration = Number.isFinite(Number(durationSeconds)) && Number(durationSeconds) > 0;
  const hasSpeed = Number.isFinite(Number(speed)) && Number(speed) > 0;
  const hasToolCalls = Number.isFinite(Number(stats?.toolCalls)) && Number(stats.toolCalls) > 0;
  const hasQuota = Number.isFinite(Number(stats?.quotaConsumed)) && Number(stats.quotaConsumed) > 0;
  if (!hasUsage && !hasDuration && !hasToolCalls && !hasQuota && !hasSpeed) {
    return [];
  }
  const entries = [
    { label: t('chat.stats.duration'), value: formatDuration(durationSeconds) },
    { label: t('chat.stats.speed'), value: formatSpeed(speed) },
    { label: t('chat.stats.contextTokens'), value: formatCount(contextTokens) },
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
    updateMessageScrollUi(container);
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
      updateMessageScrollUi(container);
    });
  };

const jumpToMessageTop = async () => {
  await nextTick();
  const container = messagesContainerRef.value;
  if (!container) return;
  if (typeof container.scrollTo === 'function') {
    container.scrollTo({ top: 0, behavior: 'smooth' });
  } else {
    container.scrollTop = 0;
  }
  scheduleMessageScrollUiRefresh();
};

onUpdated(() => {
  scheduleWorkspaceResourceHydration();
  scheduleMessageScrollUiRefresh();
});

onMounted(async () => {
  await init();
  await ensureEnterScrollToBottom();
  loadToolSummary();
  scheduleWorkspaceResourceHydration();
  scheduleMessageScrollUiRefresh();
  stopWorkspaceRefreshListener = onWorkspaceRefresh(handleWorkspaceRefresh);
  updateCompactLayout();
  await nextTick();
  initComposerResizeObserver();
  updateComposerPadding();
  externalSessionSyncStopped = false;
  scheduleExternalSessionSync(false);
  window.addEventListener('resize', updateCompactLayout);
  window.addEventListener('beforeunload', handleBeforeUnload);
  document.addEventListener('visibilitychange', handleVisibilityChange);
  document.addEventListener('click', handleFeatureMenuClickOutside);
});

onBeforeUnmount(() => {
  destroyComposerResizeObserver();
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
      dismissedPlanMessages.value = new WeakSet();
      dismissedPlanVersion.value += 1;
      showScrollTopButton.value = false;
      scheduleMessageScrollUiRefresh();
      scheduleExternalSessionSync(false);
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
    scheduleMessageScrollUiRefresh();
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
    scheduleMessageScrollUiRefresh();
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
    scheduleExternalSessionSync(false);
  }
);

watch(
  () => activeAgentId.value,
  (value) => {
    agentStore.getAgent(value || DEFAULT_AGENT_KEY, { force: true }).catch(() => null);
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
    scheduleExternalSessionSync(false);
    router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
  }
);

watch(
  () => routeAgentId.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    if (routeEntry.value === 'default') return;
    await openAgentSession(value);
    scheduleExternalSessionSync(false);
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
  () => abilitySections.value,
  () => {
    if (abilityTooltipVisible.value) {
      updateAbilityTooltip();
    }
  },
  { deep: true }
);
</script>


