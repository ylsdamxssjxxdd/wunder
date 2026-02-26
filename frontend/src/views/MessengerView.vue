<template>
  <div class="messenger-view">
    <aside class="messenger-left-rail">
      <div class="messenger-left-brand">Wunder</div>
      <button class="messenger-avatar-btn" type="button" @click="switchSection('more')">
        <span class="messenger-avatar-text">{{ avatarLabel(currentUsername) }}</span>
      </button>
      <div class="messenger-left-nav">
        <button
          v-for="item in sectionOptions"
          :key="item.key"
          class="messenger-left-nav-btn"
          :class="{ active: sessionHub.activeSection === item.key }"
          type="button"
          :title="item.label"
          :aria-label="item.label"
          @click="switchSection(item.key)"
        >
          <i :class="item.icon" aria-hidden="true"></i>
        </button>
      </div>
      <button
        class="messenger-left-refresh"
        type="button"
        :title="t('common.refresh')"
        :aria-label="t('common.refresh')"
        @click="refreshAll"
      >
        <i class="fa-solid fa-rotate" aria-hidden="true"></i>
      </button>
    </aside>

    <section class="messenger-middle-pane">
      <header class="messenger-middle-header">
        <div class="messenger-middle-title">{{ activeSectionTitle }}</div>
        <div class="messenger-middle-subtitle">{{ activeSectionSubtitle }}</div>
      </header>

      <div class="messenger-search-row">
        <label class="messenger-search">
          <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
          <input
            v-model.trim="keyword"
            type="text"
            :placeholder="searchPlaceholder"
            autocomplete="off"
            spellcheck="false"
          />
        </label>
        <button
          class="messenger-plus-btn"
          type="button"
          :title="t('messenger.action.newAgent')"
          :aria-label="t('messenger.action.newAgent')"
          @click="agentCreateVisible = true"
        >
          <i class="fa-solid fa-plus" aria-hidden="true"></i>
        </button>
      </div>

      <div class="messenger-middle-list">
        <template v-if="sessionHub.activeSection === 'messages'">
          <button
            v-for="item in filteredMixedConversations"
            :key="item.key"
            class="messenger-list-item messenger-conversation-item"
            :class="{ active: isMixedConversationActive(item) }"
            type="button"
            @click="openMixedConversation(item)"
          >
            <div class="messenger-list-avatar">{{ avatarLabel(item.title) }}</div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ item.title }}</span>
                <span class="messenger-list-time">{{ formatTime(item.lastAt) }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ item.preview || t('messenger.preview.empty') }}</span>
                <span v-if="item.unread > 0" class="messenger-list-unread">{{ item.unread }}</span>
              </div>
            </div>
            <span class="messenger-kind-tag">{{ conversationKindLabel(item.kind) }}</span>
          </button>
          <div v-if="!filteredMixedConversations.length" class="messenger-list-empty">
            {{ t('messenger.empty.list') }}
          </div>
        </template>

        <template v-else-if="sessionHub.activeSection === 'users'">
          <button
            v-for="contact in filteredContacts"
            :key="contact.user_id"
            class="messenger-list-item"
            :class="{ active: selectedContactUserId === String(contact.user_id || '') }"
            type="button"
            @click="selectContact(contact)"
          >
            <div class="messenger-list-avatar">{{ avatarLabel(contact.username || contact.user_id) }}</div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ contact.username || contact.user_id }}</span>
                <span class="messenger-list-time">{{ formatTime(contact.last_message_at) }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">
                  {{ contact.last_message_preview || t('messenger.preview.empty') }}
                </span>
                <span v-if="resolveUnread(contact.unread_count) > 0" class="messenger-list-unread">
                  {{ resolveUnread(contact.unread_count) }}
                </span>
              </div>
            </div>
          </button>
          <div v-if="!filteredContacts.length" class="messenger-list-empty">{{ t('messenger.empty.users') }}</div>
        </template>

        <template v-else-if="sessionHub.activeSection === 'groups'">
          <button
            v-for="group in filteredGroups"
            :key="group.group_id"
            class="messenger-list-item"
            :class="{ active: selectedGroupId === String(group.group_id || '') }"
            type="button"
            @click="selectGroup(group)"
          >
            <div class="messenger-list-avatar">{{ avatarLabel(group.group_name || group.group_id) }}</div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ group.group_name }}</span>
                <span class="messenger-list-time">{{ formatTime(group.last_message_at) }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">
                  {{ group.last_message_preview || t('messenger.preview.empty') }}
                </span>
                <span v-if="resolveUnread(group.unread_count_cache) > 0" class="messenger-list-unread">
                  {{ resolveUnread(group.unread_count_cache) }}
                </span>
              </div>
            </div>
          </button>
          <div v-if="!filteredGroups.length" class="messenger-list-empty">{{ t('messenger.empty.groups') }}</div>
        </template>

        <template v-else-if="sessionHub.activeSection === 'agents'">
          <div class="messenger-block-title">{{ t('messenger.agent.owned') }}</div>
          <button
            class="messenger-list-item messenger-agent-item"
            :class="{ active: selectedAgentId === DEFAULT_AGENT_KEY }"
            type="button"
            @click="selectAgentForSettings(DEFAULT_AGENT_KEY)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-robot" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.defaultAgent') }}</span>
                <span v-if="isAgentRunning(DEFAULT_AGENT_KEY)" class="messenger-running-dot"></span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.defaultAgentDesc') }}</span>
                <span v-if="hasCronTask(DEFAULT_AGENT_KEY)" class="messenger-kind-tag">
                  {{ t('messenger.agent.cron') }}
                </span>
              </div>
            </div>
          </button>
          <button
            v-for="agent in filteredOwnedAgents"
            :key="agent.id"
            class="messenger-list-item messenger-agent-item"
            :class="{ active: selectedAgentId === normalizeAgentId(agent.id) }"
            type="button"
            @click="selectAgentForSettings(agent.id)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-robot" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ agent.name || agent.id }}</span>
                <span v-if="isAgentRunning(agent.id)" class="messenger-running-dot"></span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ agent.description || t('messenger.preview.empty') }}</span>
                <span v-if="hasCronTask(agent.id)" class="messenger-kind-tag">
                  {{ t('messenger.agent.cron') }}
                </span>
              </div>
            </div>
          </button>
          <div v-if="!filteredOwnedAgents.length" class="messenger-list-empty">{{ t('messenger.empty.agents') }}</div>

          <div v-if="filteredSharedAgents.length" class="messenger-block-title">
            {{ t('messenger.agent.shared') }}
          </div>
          <button
            v-for="agent in filteredSharedAgents"
            :key="`shared-${agent.id}`"
            class="messenger-list-item messenger-agent-item"
            :class="{ active: selectedAgentId === normalizeAgentId(agent.id) }"
            type="button"
            @click="selectAgentForSettings(agent.id)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-robot" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ agent.name || agent.id }}</span>
                <span class="messenger-kind-tag">{{ t('messenger.agent.sharedTag') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ agent.description || t('messenger.preview.empty') }}</span>
              </div>
            </div>
          </button>
        </template>
        <template v-else-if="sessionHub.activeSection === 'tools'">
          <div class="messenger-block-title">{{ t('messenger.tools.customTitle') }}</div>
          <button
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === 'category:mcp' }"
            type="button"
            @click="selectToolCategory('mcp')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-plug" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('toolManager.system.mcp') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.tools.customDesc') }}</span>
              </div>
            </div>
          </button>
          <button
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === 'category:skills' }"
            type="button"
            @click="selectToolCategory('skills')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('toolManager.system.skills') }}</span>
              </div>
            </div>
          </button>
          <button
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === 'category:knowledge' }"
            type="button"
            @click="selectToolCategory('knowledge')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-database" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('toolManager.system.knowledge') }}</span>
              </div>
            </div>
          </button>

          <button
            v-for="tool in filteredCustomTools"
            :key="`user-tool-${tool.name}`"
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === `custom:${tool.name}` }"
            type="button"
            @click="selectCustomTool(tool.name)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-toolbox" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ tool.name }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ tool.description || t('common.noDescription') }}</span>
              </div>
            </div>
          </button>
          <div v-if="!filteredCustomTools.length" class="messenger-list-empty">{{ t('messenger.empty.toolsCustom') }}</div>

          <div class="messenger-block-title">{{ t('messenger.tools.sharedTitle') }}</div>
          <button
            v-for="tool in filteredSharedTools"
            :key="`shared-tool-${tool.name}`"
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === `shared:${tool.name}` }"
            type="button"
            @click="selectSharedTool(tool.name)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-share-from-square" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ tool.name }}</span>
                <span v-if="isSharedToolEnabled(tool.name)" class="messenger-kind-tag">
                  {{ t('common.enabled') }}
                </span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">
                  {{ tool.description || t('common.noDescription') }}
                </span>
              </div>
            </div>
          </button>
          <div v-if="!filteredSharedTools.length" class="messenger-list-empty">{{ t('messenger.empty.toolsShared') }}</div>
        </template>

        <template v-else-if="sessionHub.activeSection === 'files'">
          <div class="messenger-block-title">{{ t('messenger.files.title') }}</div>
          <button
            v-for="container in sandboxContainers"
            :key="`container-${container}`"
            class="messenger-list-item"
            :class="{ active: currentContainerId === container }"
            type="button"
            @click="selectContainer(container)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-box-archive" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('portal.agent.sandbox.option', { id: container }) }}</span>
                <span v-if="currentContainerId === container" class="messenger-kind-tag">
                  {{ t('messenger.files.current') }}
                </span>
              </div>
            </div>
          </button>
        </template>

        <template v-else>
          <button class="messenger-list-item" type="button" @click="toggleLanguage">
            <div class="messenger-list-avatar"><i class="fa-solid fa-language" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.more.language') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ currentLanguageLabel }}</span>
              </div>
            </div>
          </button>
          <button class="messenger-list-item" type="button" @click="openProfile">
            <div class="messenger-list-avatar"><i class="fa-solid fa-user" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('user.profile.enter') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ currentUsername }}</span>
              </div>
            </div>
          </button>
          <button class="messenger-list-item" type="button" @click="logout">
            <div class="messenger-list-avatar"><i class="fa-solid fa-right-from-bracket" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('nav.logout') }}</span>
              </div>
            </div>
          </button>
        </template>
      </div>
    </section>

    <section class="messenger-chat chat-shell">
      <header class="messenger-chat-header">
        <div class="messenger-chat-heading">
          <div class="messenger-chat-title-row">
            <div class="messenger-chat-title">{{ activeConversationTitle }}</div>
            <span v-if="activeConversationKindLabel" class="messenger-chat-kind-pill">
              {{ activeConversationKindLabel }}
            </span>
          </div>
          <div class="messenger-chat-subtitle">{{ activeConversationSubtitle }}</div>
          <div v-if="activeConversationCode" class="messenger-chat-code">{{ activeConversationCode }}</div>
        </div>
        <div class="messenger-chat-header-actions">
          <button
            class="messenger-header-btn"
            type="button"
            :title="t('chat.newSession')"
            :aria-label="t('chat.newSession')"
            @click="startNewDraftSession"
          >
            <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-header-btn"
            type="button"
            :title="t('messenger.right.sandbox')"
            :aria-label="t('messenger.right.sandbox')"
            @click="sessionHub.setRightTab('sandbox')"
          >
            <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
          </button>
        </div>
      </header>

      <div v-if="sessionHub.activeConversation" class="messenger-chat-notice">
        <i class="fa-solid fa-bullhorn" aria-hidden="true"></i>
        <span>{{ activeConversationNotice }}</span>
      </div>

      <div ref="messageListRef" class="messenger-chat-body">
        <div v-if="bootLoading" class="messenger-chat-empty">{{ t('common.loading') }}</div>
        <div v-else-if="!sessionHub.activeConversation" class="messenger-chat-empty">
          {{ t('messenger.empty.selectConversation') }}
        </div>

        <template v-else-if="isAgentConversationActive">
          <div
            v-for="(message, index) in chatStore.messages"
            :key="resolveAgentMessageKey(message, index)"
            class="messenger-message"
            :class="{ mine: message.role === 'user' }"
          >
            <div class="messenger-message-avatar">
              {{ avatarLabel(message.role === 'user' ? currentUsername : activeAgentName) }}
            </div>
            <div class="messenger-message-main">
              <div class="messenger-message-meta">
                <span>{{ message.role === 'user' ? t('chat.message.user') : activeAgentName }}</span>
                <span>{{ formatTime(message.created_at) }}</span>
                <MessageThinking
                  v-if="message.role === 'assistant'"
                  :content="String(message.reasoning || '')"
                  :streaming="Boolean(message.reasoningStreaming)"
                />
                <button
                  class="messenger-message-copy-btn"
                  type="button"
                  :title="t('chat.message.copy')"
                  :aria-label="t('chat.message.copy')"
                  @click="copyMessageContent(message.content)"
                >
                  <i class="fa-solid fa-copy" aria-hidden="true"></i>
                </button>
              </div>
              <div v-if="message.role === 'assistant'" class="messenger-workflow-scope chat-shell">
                <MessageWorkflow
                  :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
                  :loading="Boolean(message.workflowStreaming)"
                  :visible="Boolean(message.workflowStreaming || message.workflowItems?.length)"
                />
              </div>
              <div class="messenger-message-bubble messenger-markdown">
                <div class="markdown-body" v-html="renderAgentMarkdown(message, index)"></div>
              </div>
            </div>
          </div>
        </template>

        <template v-else>
          <div
            v-for="message in userWorldStore.activeMessages"
            :key="`uw-${message.message_id}`"
            class="messenger-message"
            :class="{ mine: isOwnMessage(message) }"
          >
            <div class="messenger-message-avatar">
              {{ avatarLabel(resolveWorldMessageSender(message)) }}
            </div>
            <div class="messenger-message-main">
              <div class="messenger-message-meta">
                <span>{{ resolveWorldMessageSender(message) }}</span>
                <span>{{ formatTime(message.created_at) }}</span>
                <button
                  class="messenger-message-copy-btn"
                  type="button"
                  :title="t('chat.message.copy')"
                  :aria-label="t('chat.message.copy')"
                  @click="copyMessageContent(message.content)"
                >
                  <i class="fa-solid fa-copy" aria-hidden="true"></i>
                </button>
              </div>
              <div class="messenger-message-bubble messenger-markdown">
                <div class="markdown-body" v-html="renderWorldMarkdown(message)"></div>
              </div>
            </div>
          </div>
        </template>
      </div>

      <footer class="messenger-chat-footer">
        <div v-if="isAgentConversationActive" class="messenger-agent-composer messenger-composer-scope chat-shell">
          <ChatComposer :loading="agentSessionLoading" @send="sendAgentMessage" @stop="stopAgentMessage" />
        </div>
        <div v-else-if="sessionHub.activeConversation" class="messenger-world-composer">
          <textarea
            v-model.trim="worldDraft"
            :placeholder="t('userWorld.input.placeholder')"
            rows="1"
            @keydown.enter.exact.prevent="sendWorldMessage"
          ></textarea>
          <button
            class="messenger-send-btn"
            type="button"
            :disabled="!canSendWorldMessage"
            @click="sendWorldMessage"
          >
            <i class="fa-solid fa-paper-plane" aria-hidden="true"></i>
          </button>
        </div>
        <div v-else class="messenger-chat-empty">
          {{ t('messenger.empty.input') }}
        </div>
      </footer>
    </section>

    <aside class="messenger-right-dock">
      <div class="messenger-right-tabs">
        <button
          v-for="tab in rightTabs"
          :key="tab.key"
          class="messenger-right-tab"
          :class="{ active: sessionHub.rightTab === tab.key }"
          type="button"
          @click="sessionHub.setRightTab(tab.key)"
        >
          {{ tab.label }}
        </button>
      </div>

      <div class="messenger-right-content">
        <div v-show="sessionHub.rightTab === 'sandbox'" class="messenger-right-panel">
          <div class="messenger-right-section-title">
            <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
            <span>{{ t('messenger.right.sandbox') }}</span>
          </div>
          <div class="messenger-workspace-scope chat-shell">
            <WorkspacePanel :agent-id="activeAgentIdForApi" :container-id="currentContainerId" />
          </div>
        </div>

        <div v-show="sessionHub.rightTab === 'timeline'" class="messenger-right-panel">
          <div class="messenger-right-section-title">
            <i class="fa-solid fa-timeline" aria-hidden="true"></i>
            <span>{{ t('messenger.right.timeline') }}</span>
          </div>
          <div v-if="!timelineItems.length" class="messenger-list-empty">
            {{ t('messenger.empty.timeline') }}
          </div>
          <div v-else class="messenger-timeline">
            <div v-for="item in timelineItems" :key="item.id" class="messenger-timeline-item">
              <div class="messenger-timeline-title">{{ item.title }}</div>
              <div class="messenger-timeline-detail">{{ item.detail || t('common.none') }}</div>
              <div class="messenger-timeline-meta">
                <span>{{ item.status }}</span>
                <span>{{ formatTime(item.createdAt) }}</span>
              </div>
            </div>
          </div>
        </div>

        <div v-show="sessionHub.rightTab === 'settings'" class="messenger-right-panel messenger-settings-panel">
          <div class="messenger-right-section-title">
            <i class="fa-solid fa-sliders" aria-hidden="true"></i>
            <span>{{ t('messenger.right.settings') }}</span>
          </div>
          <template v-if="showAgentSettingsPanel">
            <div class="messenger-inline-actions">
              <button class="messenger-inline-btn" type="button" @click="enterSelectedAgentConversation">
                {{ t('messenger.action.openConversation') }}
              </button>
              <button
                class="messenger-inline-btn"
                :class="{ active: agentSettingMode === 'agent' }"
                type="button"
                @click="agentSettingMode = 'agent'"
              >
                {{ t('chat.features.agentSettings') }}
              </button>
              <button
                class="messenger-inline-btn"
                :class="{ active: agentSettingMode === 'cron' }"
                type="button"
                @click="agentSettingMode = 'cron'"
              >
                {{ t('chat.features.cron') }}
              </button>
              <button
                class="messenger-inline-btn"
                :class="{ active: agentSettingMode === 'channel' }"
                type="button"
                @click="agentSettingMode = 'channel'"
              >
                {{ t('chat.features.channels') }}
              </button>
              <button
                class="messenger-inline-btn danger"
                type="button"
                :disabled="!agentSessionLoading"
                @click="stopAgentMessage"
              >
                {{ t('common.stop') }}
              </button>
            </div>

            <div v-show="agentSettingMode === 'agent'" class="messenger-right-block">
              <AgentSettingsPanel
                :agent-id="settingsAgentIdForApi"
                @saved="handleAgentSettingsSaved"
                @deleted="handleAgentDeleted"
              />
            </div>

            <div v-show="agentSettingMode === 'cron'" class="messenger-right-block">
              <AgentCronPanel :agent-id="settingsAgentIdForApi" />
            </div>

            <div v-show="agentSettingMode === 'channel'" class="messenger-right-block messenger-channel-panel-wrap">
              <UserChannelSettingsPanel mode="page" :agent-id="settingsAgentIdForApi" />
            </div>
          </template>

          <template v-else-if="sessionHub.activeSection === 'users'">
            <div v-if="selectedContact" class="messenger-entity-panel">
              <div class="messenger-entity-title">{{ selectedContact.username || selectedContact.user_id }}</div>
              <div class="messenger-entity-meta">ID: {{ selectedContact.user_id }}</div>
              <div class="messenger-entity-meta">
                {{ selectedContact.last_message_preview || t('messenger.preview.empty') }}
              </div>
              <div class="messenger-inline-actions">
                <button class="messenger-inline-btn primary" type="button" @click="openSelectedContactConversation">
                  {{ t('messenger.action.openConversation') }}
                </button>
              </div>
            </div>
            <div v-else class="messenger-list-empty">{{ t('messenger.empty.users') }}</div>
          </template>

          <template v-else-if="sessionHub.activeSection === 'groups'">
            <div v-if="selectedGroup" class="messenger-entity-panel">
              <div class="messenger-entity-title">{{ selectedGroup.group_name || selectedGroup.group_id }}</div>
              <div class="messenger-entity-meta">ID: {{ selectedGroup.group_id }}</div>
              <div class="messenger-entity-meta">
                {{ selectedGroup.last_message_preview || t('messenger.preview.empty') }}
              </div>
              <div class="messenger-inline-actions">
                <button class="messenger-inline-btn primary" type="button" @click="openSelectedGroupConversation">
                  {{ t('messenger.action.openConversation') }}
                </button>
              </div>
            </div>
            <div v-else class="messenger-list-empty">{{ t('messenger.empty.groups') }}</div>
          </template>

          <template v-else-if="sessionHub.activeSection === 'tools'">
            <div v-if="toolsCatalogLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
            <template v-else-if="selectedToolCategory">
              <div class="messenger-entity-panel">
                <div class="messenger-entity-title">{{ toolCategoryLabel(selectedToolCategory) }}</div>
                <div class="messenger-entity-meta">{{ t('messenger.tools.customDesc') }}</div>
                <div class="messenger-tool-tag-list">
                  <span
                    v-for="item in selectedToolCategoryItems"
                    :key="`tool-category-item-${selectedToolCategory}-${item.name}`"
                    class="messenger-tool-tag"
                  >
                    {{ item.name }}
                  </span>
                  <span v-if="!selectedToolCategoryItems.length" class="messenger-list-empty">
                    {{ t('common.none') }}
                  </span>
                </div>
              </div>
            </template>
            <template v-else-if="selectedCustomTool">
              <div class="messenger-entity-panel">
                <div class="messenger-entity-title">{{ selectedCustomTool.name }}</div>
                <div class="messenger-entity-meta">{{ selectedCustomTool.description || t('common.noDescription') }}</div>
                <div class="messenger-entity-meta">{{ t('messenger.tools.customTitle') }}</div>
              </div>
            </template>
            <template v-else-if="selectedSharedTool">
              <div class="messenger-entity-panel">
                <div class="messenger-entity-title">{{ selectedSharedTool.name }}</div>
                <div class="messenger-entity-meta">{{ selectedSharedTool.description || t('common.noDescription') }}</div>
                <div class="messenger-entity-meta">
                  {{ t('userTools.shared.source', { owner: selectedSharedTool.ownerId || '-' }) }}
                </div>
                <label class="messenger-switch-row">
                  <input
                    type="checkbox"
                    :checked="isSharedToolEnabled(selectedSharedTool.name)"
                    @change="
                      toggleSharedToolSelection(
                        selectedSharedTool.name,
                        ($event.target as HTMLInputElement).checked
                      )
                    "
                  />
                  <span>{{ t('common.enabled') }}</span>
                </label>
              </div>
            </template>
            <div v-else class="messenger-list-empty">{{ t('messenger.empty.selectTool') }}</div>
          </template>

          <template v-else-if="sessionHub.activeSection === 'files'">
            <div class="messenger-entity-panel">
              <div class="messenger-entity-title">{{ t('messenger.files.title') }}</div>
              <div class="messenger-entity-meta">
                {{ t('portal.agent.sandbox.option', { id: currentContainerId }) }}
              </div>
            </div>
          </template>

          <template v-else-if="sessionHub.activeSection === 'more'">
            <div class="messenger-list-empty">{{ t('messenger.section.more.desc') }}</div>
          </template>

          <div v-else class="messenger-list-empty">
            {{ t('messenger.settings.agentOnly') }}
          </div>
        </div>
      </div>
    </aside>

    <AgentCreateDialog
      v-model="agentCreateVisible"
      :copy-from-agents="agentCopyFromOptions"
      @submit="handleAgentCreateSubmit"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { listRunningAgents } from '@/api/agents';
import { fetchCronJobs } from '@/api/cron';
import { fetchUserToolsCatalog, saveUserSharedTools } from '@/api/userTools';
import UserChannelSettingsPanel from '@/components/channels/UserChannelSettingsPanel.vue';
import AgentCronPanel from '@/components/messenger/AgentCronPanel.vue';
import AgentCreateDialog from '@/components/messenger/AgentCreateDialog.vue';
import AgentSettingsPanel from '@/components/messenger/AgentSettingsPanel.vue';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageWorkflow from '@/components/chat/MessageWorkflow.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import {
  useSessionHubStore,
  resolveSectionFromRoute,
  type MessengerSection
} from '@/stores/sessionHub';
import { useUserWorldStore } from '@/stores/userWorld';
import { renderMarkdown } from '@/utils/markdown';
import { showApiError } from '@/utils/apiError';

const DEFAULT_AGENT_KEY = '__default__';
const sandboxContainers = Array.from({ length: 10 }, (_, index) => index + 1);
const sectionRouteMap: Record<MessengerSection, string> = {
  messages: 'chat',
  users: 'user-world',
  groups: 'user-world',
  agents: 'home',
  tools: 'tools',
  files: 'workspace',
  more: 'settings'
};

type MixedConversation = {
  key: string;
  kind: 'agent' | 'direct' | 'group';
  sourceId: string;
  agentId: string;
  title: string;
  preview: string;
  unread: number;
  lastAt: number;
};

type ToolEntry = {
  name: string;
  description: string;
  ownerId: string;
};

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const authStore = useAuthStore();
const agentStore = useAgentStore();
const chatStore = useChatStore();
const userWorldStore = useUserWorldStore();
const sessionHub = useSessionHubStore();

const bootLoading = ref(true);
const selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);
const selectedContactUserId = ref('');
const selectedGroupId = ref('');
const selectedToolCategory = ref<'mcp' | 'skills' | 'knowledge' | ''>('');
const selectedCustomToolName = ref('');
const selectedSharedToolName = ref('');
const worldDraft = ref('');
const messageListRef = ref<HTMLElement | null>(null);
const runningAgentIds = ref<Set<string>>(new Set());
const cronAgentIds = ref<Set<string>>(new Set());
const agentSettingMode = ref<'agent' | 'cron' | 'channel'>('agent');
const toolsCatalogLoading = ref(false);
const customTools = ref<ToolEntry[]>([]);
const sharedTools = ref<ToolEntry[]>([]);
const sharedToolSelectedSet = ref<Set<string>>(new Set());
const mcpTools = ref<ToolEntry[]>([]);
const skillTools = ref<ToolEntry[]>([]);
const knowledgeTools = ref<ToolEntry[]>([]);

const agentCreateVisible = ref(false);

let statusTimer: number | null = null;
const MARKDOWN_CACHE_LIMIT = 280;
const MARKDOWN_STREAM_THROTTLE_MS = 80;
const markdownCache = new Map<string, { source: string; html: string; updatedAt: number }>();

const sectionOptions = computed(() => [
  { key: 'messages' as MessengerSection, icon: 'fa-solid fa-comments', label: t('messenger.section.messages') },
  { key: 'users' as MessengerSection, icon: 'fa-solid fa-user-group', label: t('messenger.section.users') },
  { key: 'groups' as MessengerSection, icon: 'fa-solid fa-users-viewfinder', label: t('messenger.section.groups') },
  { key: 'agents' as MessengerSection, icon: 'fa-solid fa-robot', label: t('messenger.section.agents') },
  { key: 'tools' as MessengerSection, icon: 'fa-solid fa-wrench', label: t('messenger.section.tools') },
  { key: 'files' as MessengerSection, icon: 'fa-solid fa-folder-open', label: t('messenger.section.files') },
  { key: 'more' as MessengerSection, icon: 'fa-solid fa-ellipsis', label: t('messenger.section.more') }
]);

const rightTabs = computed(() => [
  { key: 'sandbox', label: t('messenger.right.sandbox') },
  { key: 'timeline', label: t('messenger.right.timeline') },
  { key: 'settings', label: t('messenger.right.settings') }
]);

const basePrefix = computed(() => {
  if (route.path.startsWith('/desktop')) return '/desktop';
  if (route.path.startsWith('/demo')) return '/demo';
  return '/app';
});

const keyword = computed({
  get: () => sessionHub.keyword,
  set: (value: string) => sessionHub.setKeyword(value)
});

const currentUsername = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.username || user?.id || t('user.guest'));
});

const activeSectionTitle = computed(() => t(`messenger.section.${sessionHub.activeSection}`));
const activeSectionSubtitle = computed(() => t(`messenger.section.${sessionHub.activeSection}.desc`));
const currentLanguageLabel = computed(() =>
  getCurrentLanguage() === 'zh-CN' ? t('language.zh-CN') : t('language.en-US')
);
const searchPlaceholder = computed(() => t(`messenger.search.${sessionHub.activeSection}`));

const ownedAgents = computed(() => (Array.isArray(agentStore.agents) ? agentStore.agents : []));
const sharedAgents = computed(() => (Array.isArray(agentStore.sharedAgents) ? agentStore.sharedAgents : []));

const agentMap = computed(() => {
  const map = new Map<string, Record<string, unknown>>();
  map.set(DEFAULT_AGENT_KEY, {
    id: DEFAULT_AGENT_KEY,
    name: t('messenger.defaultAgent'),
    description: t('messenger.defaultAgentDesc'),
    sandbox_container_id: 1
  });
  ownedAgents.value.forEach((item) => {
    const id = normalizeAgentId(item?.id);
    map.set(id, item as Record<string, unknown>);
  });
  sharedAgents.value.forEach((item) => {
    const id = normalizeAgentId(item?.id);
    if (!map.has(id)) {
      map.set(id, item as Record<string, unknown>);
    }
  });
  return map;
});

const activeConversation = computed(() => sessionHub.activeConversation);
const isAgentConversationActive = computed(
  () => activeConversation.value?.kind === 'agent'
);

const activeAgentId = computed(() => {
  const identity = activeConversation.value;
  if (identity?.kind === 'agent') {
    if (identity.agentId) {
      return normalizeAgentId(identity.agentId);
    }
    if (identity.id.startsWith('draft:')) {
      return normalizeAgentId(identity.id.slice('draft:'.length));
    }
    const session = chatStore.sessions.find((item) => String(item?.id || '') === identity.id);
    return normalizeAgentId(session?.agent_id || chatStore.draftAgentId);
  }
  return normalizeAgentId(selectedAgentId.value);
});

const activeAgent = computed(() => agentMap.value.get(activeAgentId.value) || null);
const activeAgentIdForApi = computed(() =>
  activeAgentId.value === DEFAULT_AGENT_KEY ? '' : activeAgentId.value
);
const activeAgentName = computed(() =>
  String(
    (activeAgent.value as Record<string, unknown> | null)?.name || t('messenger.defaultAgent')
  )
);
const currentContainerId = computed(() => {
  const source = activeAgent.value as Record<string, unknown> | null;
  const parsed = Number.parseInt(String(source?.sandbox_container_id ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
});

const showAgentSettingsPanel = computed(
  () => sessionHub.activeSection === 'agents' || isAgentConversationActive.value
);

const settingsAgentId = computed(() => {
  if (sessionHub.activeSection === 'agents') {
    return normalizeAgentId(selectedAgentId.value);
  }
  if (isAgentConversationActive.value) {
    return normalizeAgentId(activeAgentId.value);
  }
  return '';
});

const settingsAgentIdForApi = computed(() => {
  const value = normalizeAgentId(settingsAgentId.value);
  return value === DEFAULT_AGENT_KEY ? '' : value;
});

const selectedContact = computed(() =>
  (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).find(
    (item) => String(item?.user_id || '') === selectedContactUserId.value
  ) || null
);

const selectedGroup = computed(() =>
  (Array.isArray(userWorldStore.groups) ? userWorldStore.groups : []).find(
    (item) => String(item?.group_id || '') === selectedGroupId.value
  ) || null
);

const agentCopyFromOptions = computed(() =>
  [...ownedAgents.value, ...sharedAgents.value]
    .filter((item) => normalizeAgentId(item?.id) !== DEFAULT_AGENT_KEY)
    .map((item) => ({
      id: String(item?.id || ''),
      name: String(item?.name || item?.id || '')
    }))
);

const filteredOwnedAgents = computed(() => {
  const text = keyword.value.toLowerCase();
  return ownedAgents.value.filter((agent) => {
    const name = String(agent?.name || '').toLowerCase();
    const desc = String(agent?.description || '').toLowerCase();
    return !text || name.includes(text) || desc.includes(text);
  });
});

const filteredSharedAgents = computed(() => {
  const text = keyword.value.toLowerCase();
  return sharedAgents.value.filter((agent) => {
    const name = String(agent?.name || '').toLowerCase();
    const desc = String(agent?.description || '').toLowerCase();
    return !text || name.includes(text) || desc.includes(text);
  });
});

const filteredContacts = computed(() => {
  const text = keyword.value.toLowerCase();
  return (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).filter((item) => {
    const username = String(item?.username || '').toLowerCase();
    const userId = String(item?.user_id || '').toLowerCase();
    return !text || username.includes(text) || userId.includes(text);
  });
});

const filteredGroups = computed(() => {
  const text = keyword.value.toLowerCase();
  return (Array.isArray(userWorldStore.groups) ? userWorldStore.groups : []).filter((item) => {
    const name = String(item?.group_name || '').toLowerCase();
    const groupId = String(item?.group_id || '').toLowerCase();
    return !text || name.includes(text) || groupId.includes(text);
  });
});

const filteredCustomTools = computed(() => {
  const text = keyword.value.toLowerCase();
  return customTools.value.filter((item) => {
    const name = String(item.name || '').toLowerCase();
    const desc = String(item.description || '').toLowerCase();
    return !text || name.includes(text) || desc.includes(text);
  });
});

const filteredSharedTools = computed(() => {
  const text = keyword.value.toLowerCase();
  return sharedTools.value.filter((item) => {
    const name = String(item.name || '').toLowerCase();
    const desc = String(item.description || '').toLowerCase();
    const owner = String(item.ownerId || '').toLowerCase();
    return !text || name.includes(text) || desc.includes(text) || owner.includes(text);
  });
});

const selectedToolEntryKey = computed(() => {
  if (selectedToolCategory.value) return `category:${selectedToolCategory.value}`;
  if (selectedCustomToolName.value) return `custom:${selectedCustomToolName.value}`;
  if (selectedSharedToolName.value) return `shared:${selectedSharedToolName.value}`;
  return '';
});

const selectedCustomTool = computed(
  () => customTools.value.find((item) => item.name === selectedCustomToolName.value) || null
);

const selectedSharedTool = computed(
  () => sharedTools.value.find((item) => item.name === selectedSharedToolName.value) || null
);

const selectedToolCategoryItems = computed(() => {
  if (selectedToolCategory.value === 'mcp') return mcpTools.value;
  if (selectedToolCategory.value === 'skills') return skillTools.value;
  if (selectedToolCategory.value === 'knowledge') return knowledgeTools.value;
  return [];
});

const mixedConversations = computed<MixedConversation[]>(() => {
  const agentItems = (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).map((session) => {
    const sessionId = String(session?.id || '');
    const agentId = normalizeAgentId(session?.agent_id);
    const agent = agentMap.value.get(agentId) || null;
    const title = String(
      session?.title ||
        (agent as Record<string, unknown> | null)?.name ||
        (agentId === DEFAULT_AGENT_KEY ? t('messenger.defaultAgent') : agentId)
    );
    const preview = String(
      session?.last_message_preview || session?.last_message || session?.summary || ''
    );
    return {
      key: `agent:${sessionId}`,
      kind: 'agent',
      sourceId: sessionId,
      agentId,
      title,
      preview,
      unread: 0,
      lastAt: normalizeTimestamp(session?.updated_at || session?.last_message_at || session?.created_at)
    } as MixedConversation;
  });

  const worldItems = (Array.isArray(userWorldStore.conversations) ? userWorldStore.conversations : []).map(
    (conversation) => {
      const conversationId = String(conversation?.conversation_id || '');
      const isGroup = String(conversation?.conversation_type || '').toLowerCase() === 'group';
      const title = userWorldStore.resolveConversationTitle(conversation) || conversationId;
      return {
        key: `${isGroup ? 'group' : 'direct'}:${conversationId}`,
        kind: isGroup ? 'group' : 'direct',
        sourceId: conversationId,
        agentId: '',
        title,
        preview: String(conversation?.last_message_preview || ''),
        unread: resolveUnread(userWorldStore.resolveConversationUnread(conversationId)),
        lastAt: normalizeTimestamp(conversation?.last_message_at || conversation?.updated_at)
      } as MixedConversation;
    }
  );

  return [...agentItems, ...worldItems].sort((left, right) => right.lastAt - left.lastAt);
});

const filteredMixedConversations = computed(() => {
  const text = keyword.value.toLowerCase();
  return mixedConversations.value.filter((item) => {
    if (!text) return true;
    return item.title.toLowerCase().includes(text) || item.preview.toLowerCase().includes(text);
  });
});

const activeConversationTitle = computed(() => {
  const identity = activeConversation.value;
  if (!identity) return t('messenger.empty.noConversation');
  if (identity.kind === 'agent') {
    if (identity.id.startsWith('draft:')) return activeAgentName.value;
    const session = chatStore.sessions.find((item) => String(item?.id || '') === identity.id);
    return String(session?.title || activeAgentName.value);
  }
  const conversation = userWorldStore.conversations.find(
    (item) => String(item?.conversation_id || '') === identity.id
  );
  return userWorldStore.resolveConversationTitle(conversation) || t('messenger.empty.noConversation');
});

const activeConversationSubtitle = computed(() => {
  const identity = activeConversation.value;
  if (!identity) return t('messenger.empty.subtitle');
  if (identity.kind === 'agent') {
    const info = activeAgent.value as Record<string, unknown> | null;
    return String(info?.description || t('messenger.agent.subtitle'));
  }
  return identity.kind === 'group' ? t('messenger.group.subtitle') : t('messenger.direct.subtitle');
});

const activeConversationKindLabel = computed(() => {
  const identity = activeConversation.value;
  if (!identity) return '';
  return t(`messenger.kind.${identity.kind}`);
});

const activeConversationCode = computed(() => {
  const identity = activeConversation.value;
  if (!identity) return '';
  if (identity.kind === 'agent' && identity.id.startsWith('draft:')) {
    return t('chat.newSession');
  }
  return `ID: ${identity.id}`;
});

const activeConversationNotice = computed(() => {
  const subtitle = String(activeConversationSubtitle.value || '').trim();
  if (!subtitle) return t('messenger.empty.subtitle');
  return subtitle;
});

const agentSessionLoading = computed(() => {
  if (!isAgentConversationActive.value) return false;
  const sessionId = String(chatStore.activeSessionId || '');
  if (!sessionId) return false;
  return Boolean(chatStore.isSessionLoading(sessionId));
});

const canSendWorldMessage = computed(
  () =>
    !isAgentConversationActive.value &&
    Boolean(activeConversation.value?.id) &&
    !userWorldStore.sending &&
    Boolean(worldDraft.value.trim())
);

const timelineItems = computed(() => {
  if (!isAgentConversationActive.value) return [];
  const output: Array<{ id: string; title: string; detail: string; status: string; createdAt: unknown }> = [];
  chatStore.messages.forEach((message, messageIndex) => {
    if (message?.role !== 'assistant') return;
    const workflowItems = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
    workflowItems.forEach((item: Record<string, unknown>, index: number) => {
      output.push({
        id: `${messageIndex}-${item?.id ?? index}`,
        title: String(item?.title || t('chat.workflow.title')),
        detail: String(item?.detail || ''),
        status: String(item?.status || 'pending'),
        createdAt: message?.created_at
      });
    });
  });
  return output.reverse();
});

const hasCronTask = (agentId: unknown): boolean =>
  cronAgentIds.value.has(normalizeAgentId(agentId));

const isAgentRunning = (agentId: unknown): boolean =>
  runningAgentIds.value.has(normalizeAgentId(agentId));

const conversationKindLabel = (kind: 'agent' | 'direct' | 'group') =>
  t(`messenger.kind.${kind}`);

const avatarLabel = (value: unknown): string => {
  const source = String(value || '').trim();
  if (!source) return '?';
  return source.slice(0, 1).toUpperCase();
};

const resolveUnread = (value: unknown): number => {
  const parsed = Number.parseInt(String(value || ''), 10);
  if (!Number.isFinite(parsed)) return 0;
  return Math.max(0, parsed);
};

const normalizeTimestamp = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  const date = new Date(value as string | number);
  if (!Number.isNaN(date.getTime())) return date.getTime();
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return 0;
  return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
};

const formatTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) return '';
  const date = new Date(ts);
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  return `${month}-${day} ${hour}:${minute}`;
};

const trimMarkdownCache = () => {
  while (markdownCache.size > MARKDOWN_CACHE_LIMIT) {
    const oldestKey = markdownCache.keys().next().value;
    if (!oldestKey) break;
    markdownCache.delete(oldestKey);
  }
};

const renderMessageMarkdown = (
  cacheKey: string,
  content: unknown,
  options: { streaming?: boolean } = {}
): string => {
  const source = String(content || '');
  const normalizedKey = String(cacheKey || '').trim();
  if (!source) {
    if (normalizedKey) {
      markdownCache.delete(normalizedKey);
    }
    return '';
  }
  if (!normalizedKey) return renderMarkdown(source);
  const cached = markdownCache.get(normalizedKey);
  if (cached && cached.source === source) {
    return cached.html;
  }
  const now = Date.now();
  if (options.streaming && cached && now - cached.updatedAt < MARKDOWN_STREAM_THROTTLE_MS) {
    return cached.html;
  }
  const html = renderMarkdown(source);
  markdownCache.set(normalizedKey, { source, html, updatedAt: now });
  trimMarkdownCache();
  return html;
};

const renderAgentMarkdown = (message: Record<string, unknown>, index: number): string => {
  const cacheKey = `agent:${resolveAgentMessageKey(message, index)}`;
  const streaming =
    Boolean(message?.stream_incomplete) ||
    Boolean(message?.workflowStreaming) ||
    Boolean(message?.reasoningStreaming);
  return renderMessageMarkdown(cacheKey, message?.content, { streaming });
};

const renderWorldMarkdown = (message: Record<string, unknown>): string => {
  const cacheKey = `world:${resolveWorldMessageKey(message)}`;
  return renderMessageMarkdown(cacheKey, message?.content);
};

const copyMessageContent = async (content: unknown) => {
  const text = String(content || '').trim();
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    ElMessage.success(t('chat.message.copySuccess'));
  } catch {
    ElMessage.warning(t('chat.message.copyFailed'));
  }
};

const isOwnMessage = (message: Record<string, unknown>): boolean => {
  const sender = String(message?.sender_user_id || '').trim();
  const user = authStore.user as Record<string, unknown> | null;
  const current = String(user?.id || '').trim();
  return Boolean(sender && current && sender === current);
};

const resolveWorldMessageSender = (message: Record<string, unknown>): string => {
  const sender = String(message?.sender_user_id || '').trim();
  if (!sender) return t('user.guest');
  const contact = userWorldStore.contacts.find((item) => String(item?.user_id || '') === sender);
  if (contact?.username) return contact.username;
  const user = authStore.user as Record<string, unknown> | null;
  if (String(user?.id || '') === sender) {
    return String(user?.username || sender);
  }
  return sender;
};

const resolveWorldMessageKey = (message: Record<string, unknown>): string =>
  String(
    message?.message_id ||
      message?.id ||
      `${message?.sender_user_id || 'peer'}-${message?.created_at || ''}`
  );

const resolveAgentMessageKey = (message: Record<string, unknown>, index: number): string =>
  String(message?.id || message?.message_id || `${message?.role || 'm'}-${index}`);

const isMixedConversationActive = (item: MixedConversation): boolean => {
  const identity = activeConversation.value;
  if (!identity) return false;
  if (item.kind === 'agent') {
    if (identity.kind !== 'agent') return false;
    if (identity.id === item.sourceId) return true;
    return identity.id.startsWith('draft:') && normalizeAgentId(identity.agentId) === item.agentId;
  }
  return identity.kind === item.kind && identity.id === item.sourceId;
};

const switchSection = (section: MessengerSection) => {
  sessionHub.setSection(section);
  sessionHub.setKeyword('');
  if (section !== 'tools') {
    selectedToolCategory.value = '';
    selectedCustomToolName.value = '';
    selectedSharedToolName.value = '';
  }
  if (section !== 'users') {
    selectedContactUserId.value = '';
  }
  if (section !== 'groups') {
    selectedGroupId.value = '';
  }
  if (section === 'agents') {
    agentSettingMode.value = 'agent';
  }
  const targetPath = `${basePrefix.value}/${sectionRouteMap[section]}`;
  const nextQuery = { ...route.query, section } as Record<string, any>;
  if (section !== 'messages') {
    delete nextQuery.session_id;
    delete nextQuery.agent_id;
    delete nextQuery.entry;
  }
  if (section !== 'users' && section !== 'groups') {
    delete nextQuery.conversation_id;
  }
  router.push({ path: targetPath, query: nextQuery }).catch(() => undefined);
  if (section === 'tools') {
    loadToolsCatalog();
  }
  ensureSectionSelection();
};

const openMixedConversation = async (item: MixedConversation) => {
  if (item.kind === 'agent') {
    await openAgentSession(item.sourceId, item.agentId);
    return;
  }
  try {
    await userWorldStore.setActiveConversation(item.sourceId);
    sessionHub.setActiveConversation({ kind: item.kind, id: item.sourceId });
    const nextQuery = {
      ...route.query,
      section: 'messages',
      conversation_id: item.sourceId
    } as Record<string, any>;
    delete nextQuery.session_id;
    delete nextQuery.agent_id;
    delete nextQuery.entry;
    router.replace({
      path: `${basePrefix.value}/chat`,
      query: nextQuery
    }).catch(() => undefined);
    await scrollMessagesToBottom();
  } catch (error) {
    showApiError(error, t('messenger.error.openConversation'));
  }
};

const selectContact = (contact: Record<string, unknown>) => {
  selectedContactUserId.value = String(contact?.user_id || '').trim();
  selectedGroupId.value = '';
  sessionHub.setRightTab('settings');
};

const selectGroup = (group: Record<string, unknown>) => {
  selectedGroupId.value = String(group?.group_id || '').trim();
  selectedContactUserId.value = '';
  sessionHub.setRightTab('settings');
};

const openWorldConversation = async (
  conversationId: string,
  kind: 'direct' | 'group'
) => {
  if (!conversationId) return;
  try {
    await userWorldStore.setActiveConversation(conversationId);
    sessionHub.setActiveConversation({ kind, id: conversationId });
    const section = kind === 'group' ? 'groups' : 'users';
    const nextQuery = { ...route.query, section, conversation_id: conversationId } as Record<string, any>;
    delete nextQuery.session_id;
    delete nextQuery.agent_id;
    delete nextQuery.entry;
    router.replace({
      path: `${basePrefix.value}/user-world`,
      query: nextQuery
    }).catch(() => undefined);
    await scrollMessagesToBottom();
  } catch (error) {
    showApiError(error, t('messenger.error.openConversation'));
  }
};

const openAgentById = async (agentId: unknown) => {
  const normalized = normalizeAgentId(agentId);
  selectedAgentId.value = normalized;
  const session = chatStore.sessions.find(
    (item) => normalizeAgentId(item?.agent_id) === normalized
  );
  if (session?.id) {
    await openAgentSession(String(session.id), normalized);
    return;
  }
  chatStore.openDraftSession({ agent_id: normalized === DEFAULT_AGENT_KEY ? '' : normalized });
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: `draft:${normalized}`,
    agentId: normalized
  });
  sessionHub.setSection('messages');
  const nextQuery = {
    ...route.query,
    section: 'messages',
    agent_id: normalized === DEFAULT_AGENT_KEY ? '' : normalized,
    entry: normalized === DEFAULT_AGENT_KEY ? 'default' : undefined
  } as Record<string, any>;
  delete nextQuery.conversation_id;
  delete nextQuery.session_id;
  router.replace({
    path: `${basePrefix.value}/chat`,
    query: nextQuery
  }).catch(() => undefined);
  await scrollMessagesToBottom();
};

const selectAgentForSettings = (agentId: unknown) => {
  selectedAgentId.value = normalizeAgentId(agentId);
  agentSettingMode.value = 'agent';
  sessionHub.setRightTab('settings');
};

const enterSelectedAgentConversation = async () => {
  const target = settingsAgentId.value || DEFAULT_AGENT_KEY;
  await openAgentById(target);
};

const openSelectedContactConversation = async () => {
  if (!selectedContact.value) return;
  const conversationId = String(selectedContact.value.conversation_id || '').trim();
  if (conversationId) {
    await openWorldConversation(conversationId, 'direct');
    return;
  }
  const peerUserId = String(selectedContact.value.user_id || '').trim();
  if (!peerUserId) return;
  try {
    await userWorldStore.openConversationByPeer(peerUserId);
    if (userWorldStore.activeConversationId) {
      await openWorldConversation(userWorldStore.activeConversationId, 'direct');
    }
  } catch (error) {
    showApiError(error, t('userWorld.contact.openFailed'));
  }
};

const openSelectedGroupConversation = async () => {
  if (!selectedGroup.value) return;
  const conversationId = String(selectedGroup.value.conversation_id || '').trim();
  if (!conversationId) return;
  await openWorldConversation(conversationId, 'group');
};

const openAgentSession = async (sessionId: string, agentId = '') => {
  if (!sessionId) return;
  try {
    await chatStore.loadSessionDetail(sessionId);
    const session = chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
    const targetAgentId = agentId
      ? normalizeAgentId(agentId)
      : normalizeAgentId(session?.agent_id ?? chatStore.draftAgentId);
    selectedAgentId.value = targetAgentId || DEFAULT_AGENT_KEY;
    sessionHub.setActiveConversation({
      kind: 'agent',
      id: sessionId,
      agentId: targetAgentId || DEFAULT_AGENT_KEY
    });
    const nextQuery = {
      ...route.query,
      section: 'messages',
      session_id: sessionId,
      agent_id: targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId
    } as Record<string, any>;
    delete nextQuery.conversation_id;
    router.replace({
      path: `${basePrefix.value}/chat`,
      query: nextQuery
    }).catch(() => undefined);
    await scrollMessagesToBottom();
  } catch (error) {
    showApiError(error, t('messenger.error.openConversation'));
  }
};

const selectContainer = (containerId: number) => {
  const parsed = Math.min(10, Math.max(1, Number(containerId) || 1));
  const source = activeAgent.value as Record<string, unknown> | null;
  if (source) {
    source.sandbox_container_id = parsed;
  }
  sessionHub.setRightTab('sandbox');
  sessionHub.setSection('files');
};

const normalizeToolEntry = (item: unknown): ToolEntry | null => {
  if (!item) return null;
  if (typeof item === 'string') {
    const name = item.trim();
    if (!name) return null;
    return { name, description: '', ownerId: '' };
  }
  const source = item as Record<string, unknown>;
  const name = String(source.name || source.tool_name || source.id || '').trim();
  if (!name) return null;
  return {
    name,
    description: String(source.description || '').trim(),
    ownerId: String(source.owner_id || source.ownerId || '').trim()
  };
};

const loadToolsCatalog = async () => {
  toolsCatalogLoading.value = true;
  try {
    const { data } = await fetchUserToolsCatalog();
    const payload = (data?.data || {}) as Record<string, unknown>;
    customTools.value = (Array.isArray(payload.user_tools) ? payload.user_tools : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
    sharedTools.value = (Array.isArray(payload.shared_tools) ? payload.shared_tools : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
    mcpTools.value = (Array.isArray(payload.mcp_tools) ? payload.mcp_tools : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
    skillTools.value = (Array.isArray(payload.skills) ? payload.skills : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
    knowledgeTools.value = (Array.isArray(payload.knowledge_tools) ? payload.knowledge_tools : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
    const selected = Array.isArray(payload.shared_tools_selected) ? payload.shared_tools_selected : [];
    sharedToolSelectedSet.value = new Set(selected.map((item) => String(item || '').trim()).filter(Boolean));

    if (!selectedToolCategory.value && !selectedCustomToolName.value && !selectedSharedToolName.value) {
      if (customTools.value.length > 0) {
        selectedCustomToolName.value = customTools.value[0].name;
      } else if (sharedTools.value.length > 0) {
        selectedSharedToolName.value = sharedTools.value[0].name;
      } else {
        selectedToolCategory.value = 'mcp';
      }
    }
  } catch (error) {
    showApiError(error, t('toolManager.loadFailed'));
  } finally {
    toolsCatalogLoading.value = false;
  }
};

const selectToolCategory = (category: 'mcp' | 'skills' | 'knowledge') => {
  selectedToolCategory.value = category;
  selectedCustomToolName.value = '';
  selectedSharedToolName.value = '';
  sessionHub.setRightTab('settings');
};

const selectCustomTool = (toolName: string) => {
  selectedCustomToolName.value = String(toolName || '').trim();
  selectedToolCategory.value = '';
  selectedSharedToolName.value = '';
  sessionHub.setRightTab('settings');
};

const selectSharedTool = (toolName: string) => {
  selectedSharedToolName.value = String(toolName || '').trim();
  selectedToolCategory.value = '';
  selectedCustomToolName.value = '';
  sessionHub.setRightTab('settings');
};

const isSharedToolEnabled = (toolName: string): boolean =>
  sharedToolSelectedSet.value.has(String(toolName || '').trim());

const toggleSharedToolSelection = async (toolName: string, checked: boolean) => {
  const target = String(toolName || '').trim();
  if (!target) return;
  const next = new Set(sharedToolSelectedSet.value);
  if (checked) {
    next.add(target);
  } else {
    next.delete(target);
  }
  sharedToolSelectedSet.value = next;
  try {
    await saveUserSharedTools({ shared_tools: Array.from(next) });
  } catch (error) {
    showApiError(error, t('userTools.shared.saveFailed'));
  }
};

const toolCategoryLabel = (category: string) => {
  if (category === 'mcp') return t('toolManager.system.mcp');
  if (category === 'skills') return t('toolManager.system.skills');
  if (category === 'knowledge') return t('toolManager.system.knowledge');
  return category;
};

const handleAgentSettingsSaved = async () => {
  await Promise.allSettled([agentStore.loadAgents(), loadRunningAgents(), loadCronAgentIds()]);
};

const handleAgentDeleted = async () => {
  selectedAgentId.value = DEFAULT_AGENT_KEY;
  await Promise.allSettled([chatStore.loadSessions(), loadRunningAgents(), loadCronAgentIds()]);
};

const ensureSectionSelection = () => {
  if (sessionHub.activeSection === 'agents') {
    if (!selectedAgentId.value) {
      selectedAgentId.value = DEFAULT_AGENT_KEY;
    }
    sessionHub.setRightTab('settings');
    return;
  }

  if (sessionHub.activeSection === 'users') {
    if (!selectedContactUserId.value && filteredContacts.value.length > 0) {
      selectedContactUserId.value = String(filteredContacts.value[0]?.user_id || '');
    }
    sessionHub.setRightTab('settings');
    return;
  }

  if (sessionHub.activeSection === 'groups') {
    if (!selectedGroupId.value && filteredGroups.value.length > 0) {
      selectedGroupId.value = String(filteredGroups.value[0]?.group_id || '');
    }
    sessionHub.setRightTab('settings');
    return;
  }

  if (sessionHub.activeSection === 'tools') {
    if (!selectedToolEntryKey.value) {
      if (customTools.value.length > 0) {
        selectedCustomToolName.value = customTools.value[0].name;
      } else if (sharedTools.value.length > 0) {
        selectedSharedToolName.value = sharedTools.value[0].name;
      } else {
        selectedToolCategory.value = 'mcp';
      }
    }
    sessionHub.setRightTab('settings');
    return;
  }

  if (sessionHub.activeSection === 'files') {
    sessionHub.setRightTab('sandbox');
  }
};

const sendAgentMessage = async (payload: { content?: string; attachments?: unknown[] }) => {
  const content = String(payload?.content || '').trim();
  const attachments = Array.isArray(payload?.attachments) ? payload.attachments : [];
  if (!content && attachments.length === 0) return;
  try {
    await chatStore.sendMessage(content, { attachments });
    if (chatStore.activeSessionId) {
      sessionHub.setActiveConversation({
        kind: 'agent',
        id: String(chatStore.activeSessionId),
        agentId: normalizeAgentId(chatStore.draftAgentId || activeAgentId.value)
      });
    }
    await scrollMessagesToBottom();
  } catch (error) {
    showApiError(error, t('chat.error.requestFailed'));
  }
};

const stopAgentMessage = async () => {
  try {
    await chatStore.stopStream();
  } catch {
    // ignore stop errors to keep UI responsive
  }
};

const sendWorldMessage = async () => {
  if (!canSendWorldMessage.value) return;
  const text = worldDraft.value.trim();
  if (!text) return;
  worldDraft.value = '';
  try {
    await userWorldStore.sendToActiveConversation(text);
    await scrollMessagesToBottom();
  } catch (error) {
    worldDraft.value = text;
    showApiError(error, t('userWorld.input.sendFailed'));
  }
};

const startNewDraftSession = async () => {
  if (!isAgentConversationActive.value) return;
  const targetAgent = activeAgentId.value;
  chatStore.openDraftSession({ agent_id: targetAgent === DEFAULT_AGENT_KEY ? '' : targetAgent });
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: `draft:${targetAgent}`,
    agentId: targetAgent
  });
  await scrollMessagesToBottom();
};

const handleAgentCreateSubmit = async (payload: Record<string, unknown>) => {
  try {
    const created = await agentStore.createAgent(payload);
    agentCreateVisible.value = false;
    ElMessage.success(t('portal.agent.createSuccess'));
    await Promise.all([loadRunningAgents(), loadCronAgentIds()]);
    if (created?.id) {
      sessionHub.setSection('agents');
      selectedAgentId.value = normalizeAgentId(created.id);
      agentSettingMode.value = 'agent';
      sessionHub.setRightTab('settings');
      router
        .replace({ path: `${basePrefix.value}/home`, query: { ...route.query, section: 'agents' } })
        .catch(() => undefined);
    }
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  }
};

const openProfile = () => {
  router.push(`${basePrefix.value}/profile`);
};

const toggleLanguage = () => {
  const next = getCurrentLanguage() === 'zh-CN' ? 'en-US' : 'zh-CN';
  setLanguage(next);
  ElMessage.success(t('messenger.more.languageChanged'));
};

const logout = async () => {
  try {
    await ElMessageBox.confirm(t('messenger.more.logoutConfirm'), t('nav.logout'), {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
  } catch {
    return;
  }
  authStore.logout();
  router.replace('/login');
};

const loadRunningAgents = async () => {
  try {
    const response = await listRunningAgents();
    const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
    const result = new Set<string>();
    items.forEach((item: Record<string, unknown>) => {
      const state = String(item?.state || '').trim().toLowerCase();
      const active =
        state === 'running' ||
        state === 'waiting' ||
        state === 'cancelling' ||
        item?.pending_question === true;
      if (!active) return;
      const key =
        normalizeAgentId(
          item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')
        ) || DEFAULT_AGENT_KEY;
      result.add(key);
    });
    runningAgentIds.value = result;
  } catch {
    runningAgentIds.value = new Set<string>();
  }
};

const loadCronAgentIds = async () => {
  try {
    const response = await fetchCronJobs();
    const jobs = Array.isArray(response?.data?.data?.jobs) ? response.data.data.jobs : [];
    const result = new Set<string>();
    jobs.forEach((job: Record<string, unknown>) => {
      const rawAgentId = String(job?.agent_id || '').trim();
      const target = String(job?.session_target || '').trim().toLowerCase();
      const resolved =
        rawAgentId ||
        (target === 'default' || target === 'system' || target === '__default__' || job?.is_default
          ? DEFAULT_AGENT_KEY
          : '');
      if (!resolved) return;
      result.add(normalizeAgentId(resolved));
    });
    cronAgentIds.value = result;
  } catch {
    cronAgentIds.value = new Set<string>();
  }
};

const refreshAll = async () => {
  await Promise.allSettled([
    agentStore.loadAgents(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(true),
    loadRunningAgents(),
    loadCronAgentIds(),
    loadToolsCatalog()
  ]);
  ensureSectionSelection();
  ElMessage.success(t('common.refreshSuccess'));
};

const scrollMessagesToBottom = async () => {
  await nextTick();
  const container = messageListRef.value;
  if (!container) return;
  container.scrollTop = container.scrollHeight;
};

const normalizeAgentId = (value: unknown): string => {
  const text = String(value || '').trim();
  return text || DEFAULT_AGENT_KEY;
};

const restoreConversationFromRoute = async () => {
  const query = route.query;
  const queryConversationId = String(query?.conversation_id || '').trim();
  if (queryConversationId) {
    const conversation = userWorldStore.conversations.find(
      (item) => String(item?.conversation_id || '') === queryConversationId
    );
    const kind = String(conversation?.conversation_type || '').toLowerCase() === 'group' ? 'group' : 'direct';
    if (route.path.includes('/chat')) {
      await userWorldStore.setActiveConversation(queryConversationId);
      sessionHub.setActiveConversation({ kind, id: queryConversationId });
      await scrollMessagesToBottom();
    } else {
      await openWorldConversation(queryConversationId, kind);
    }
    return;
  }

  const querySessionId = String(query?.session_id || '').trim();
  if (querySessionId) {
    const session = chatStore.sessions.find((item) => String(item?.id || '') === querySessionId);
    await openAgentSession(querySessionId, normalizeAgentId(session?.agent_id));
    return;
  }

  const queryAgentId = String(query?.agent_id || '').trim();
  const queryEntry = String(query?.entry || '').trim().toLowerCase();
  if (queryAgentId || queryEntry === 'default') {
    await openAgentById(queryAgentId || DEFAULT_AGENT_KEY);
    return;
  }

  const preferredSection = resolveSectionFromRoute(route.path, query.section);
  if (preferredSection === 'messages') {
    const first = mixedConversations.value[0];
    if (first) {
      await openMixedConversation(first);
      return;
    }
  }

  chatStore.openDraftSession({ agent_id: '' });
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: `draft:${DEFAULT_AGENT_KEY}`,
    agentId: DEFAULT_AGENT_KEY
  });
};

const bootstrap = async () => {
  bootLoading.value = true;
  await Promise.allSettled([
    authStore.loadProfile(),
    agentStore.loadAgents(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(),
    loadRunningAgents(),
    loadCronAgentIds(),
    loadToolsCatalog()
  ]);
  await restoreConversationFromRoute();
  ensureSectionSelection();
  bootLoading.value = false;
};

watch(
  () => [route.path, route.query.section],
  () => {
    const sectionHint = String(route.query.section || '').trim().toLowerCase();
    if (route.path.includes('/user-world') && sectionHint === 'groups') {
      sessionHub.setSection('groups');
      return;
    }
    sessionHub.setSection(resolveSectionFromRoute(route.path, route.query.section));
  },
  { immediate: true }
);

watch(
  () => sessionHub.activeSection,
  (section) => {
    if (section === 'tools' && !customTools.value.length && !sharedTools.value.length) {
      loadToolsCatalog();
    }
    ensureSectionSelection();
  },
  { immediate: true }
);

watch(
  () => [filteredContacts.value.length, filteredGroups.value.length, filteredOwnedAgents.value.length, filteredSharedAgents.value.length],
  () => {
    ensureSectionSelection();
  }
);

watch(
  () => sessionHub.activeConversationKey,
  () => {
    markdownCache.clear();
  }
);

watch(
  () => chatStore.activeSessionId,
  (value) => {
    if (!value || !isAgentConversationActive.value) return;
    const session = chatStore.sessions.find((item) => String(item?.id || '') === String(value));
    selectedAgentId.value = normalizeAgentId(session?.agent_id ?? activeAgentId.value);
    sessionHub.setActiveConversation({
      kind: 'agent',
      id: String(value),
      agentId: normalizeAgentId(session?.agent_id ?? activeAgentId.value)
    });
  }
);

watch(
  () => [chatStore.messages.length, userWorldStore.activeMessages.length, sessionHub.activeConversationKey],
  () => {
    scrollMessagesToBottom();
  }
);

onMounted(async () => {
  await bootstrap();
  statusTimer = window.setInterval(() => {
    loadRunningAgents();
    loadCronAgentIds();
  }, 12000);
});

onBeforeUnmount(() => {
  if (statusTimer) {
    window.clearInterval(statusTimer);
    statusTimer = null;
  }
  markdownCache.clear();
  userWorldStore.stopAllWatchers();
});
</script>
