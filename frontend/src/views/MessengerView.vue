<template>
  <div
    class="messenger-view"
    :class="{
      'messenger-view--without-right': !showRightDock,
      'messenger-view--without-middle': !showMiddlePane,
      'messenger-view--right-collapsed': showRightDock && rightDockCollapsed
    }"
  >
    <aside
      ref="leftRailRef"
      class="messenger-left-rail"
      @mouseenter="cancelMiddlePaneOverlayHide"
      @mouseleave="scheduleMiddlePaneOverlayHide"
    >
      <button class="messenger-avatar-btn" type="button" @click="openProfilePage">
        <span class="messenger-avatar-text">{{ avatarLabel(currentUsername) }}</span>
      </button>
      <div class="messenger-left-nav">
        <button
          v-for="item in primarySectionOptions"
          :key="item.key"
          class="messenger-left-nav-btn"
          :class="{ active: sessionHub.activeSection === item.key }"
          type="button"
          :title="item.label"
          :aria-label="item.label"
          @mouseenter="openMiddlePaneOverlay"
          @focus="openMiddlePaneOverlay"
          @click="switchSection(item.key)"
        >
          <i :class="item.icon" aria-hidden="true"></i>
        </button>
      </div>
      <button
        class="messenger-left-nav-btn messenger-left-nav-btn--settings"
        :class="{ active: sessionHub.activeSection === 'more' }"
        type="button"
        :title="t('messenger.section.settings')"
        :aria-label="t('messenger.section.settings')"
        @mouseenter="openMiddlePaneOverlay"
        @focus="openMiddlePaneOverlay"
        @click="openSettingsPage"
      >
        <i class="fa-solid fa-gear" aria-hidden="true"></i>
      </button>
    </aside>

    <section
      v-if="showMiddlePane"
      ref="middlePaneRef"
      class="messenger-middle-pane messenger-middle-pane--overlay"
      @mouseenter="cancelMiddlePaneOverlayHide"
      @mouseleave="scheduleMiddlePaneOverlayHide"
    >
      <header class="messenger-middle-header">
        <div class="messenger-middle-title">{{ activeSectionTitle }}</div>
        <div class="messenger-middle-subtitle">{{ activeSectionSubtitle }}</div>
      </header>

      <div v-if="sessionHub.activeSection !== 'more'" class="messenger-search-row">
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
          v-if="sessionHub.activeSection === 'agents' || sessionHub.activeSection === 'groups'"
          class="messenger-plus-btn"
          type="button"
          :title="sessionHub.activeSection === 'groups' ? t('userWorld.group.create') : t('messenger.action.newAgent')"
          :aria-label="
            sessionHub.activeSection === 'groups' ? t('userWorld.group.create') : t('messenger.action.newAgent')
          "
          @click="handleSearchCreateAction"
        >
          <i class="fa-solid fa-plus" aria-hidden="true"></i>
        </button>
      </div>

      <div class="messenger-middle-list">
        <template v-if="sessionHub.activeSection === 'messages'">
          <div
            v-for="item in filteredMixedConversations"
            :key="item.key"
            class="messenger-list-item messenger-conversation-item"
            :class="{ active: isMixedConversationActive(item) }"
            role="button"
            tabindex="0"
            @click="openMixedConversation(item)"
            @keydown.enter.prevent="openMixedConversation(item)"
            @keydown.space.prevent="openMixedConversation(item)"
          >
            <AgentAvatar
              v-if="item.kind === 'agent'"
              size="md"
              :state="resolveAgentRuntimeState(item.agentId)"
              :title="item.title"
            />
            <div v-else class="messenger-list-avatar">{{ avatarLabel(item.title) }}</div>
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
            <button
              v-if="canDeleteMixedConversation(item)"
              class="messenger-list-item-action"
              type="button"
              :title="t('chat.history.delete')"
              :aria-label="t('chat.history.delete')"
              @click.stop="deleteMixedConversation(item)"
            >
              <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
            </button>
          </div>
          <div v-if="!filteredMixedConversations.length" class="messenger-list-empty">
            {{ t('messenger.empty.list') }}
          </div>
        </template>

        <template v-else-if="sessionHub.activeSection === 'users'">
          <div class="messenger-unit-structure">
            <div class="messenger-unit-structure-head">
              <span class="messenger-unit-structure-title">{{ t('messenger.users.unitTitle') }}</span>
              <span class="messenger-unit-structure-hint">{{ t('messenger.users.unitHint') }}</span>
            </div>
            <div class="messenger-unit-structure-actions">
              <button
                class="messenger-unit-all-btn"
                :class="{ active: !selectedContactUnitId }"
                type="button"
                @click="selectedContactUnitId = ''"
              >
                <span class="messenger-unit-tree-name">{{ t('messenger.users.unitAll') }}</span>
                <span class="messenger-unit-tree-count">{{ contactTotalCount }}</span>
              </button>
            </div>
            <div class="messenger-unit-tree">
              <div
                v-for="row in contactUnitTreeRows"
                :key="`unit-tree-${row.id}`"
                class="messenger-unit-tree-row"
                :class="{
                  active: selectedContactUnitId === row.id,
                  'messenger-unit-tree-row--dir': row.hasChildren,
                  'messenger-unit-tree-row--leaf': !row.hasChildren
                }"
                :style="resolveUnitTreeRowStyle(row)"
                role="button"
                tabindex="0"
                @click="selectedContactUnitId = row.id"
                @keydown.enter.prevent="selectedContactUnitId = row.id"
                @keydown.space.prevent="selectedContactUnitId = row.id"
              >
                <button
                  v-if="row.hasChildren"
                  class="messenger-unit-tree-toggle"
                  :class="{ expanded: row.expanded }"
                  type="button"
                  :title="row.expanded ? t('common.collapse') : t('common.expand')"
                  @click.stop="toggleContactUnitExpanded(row.id)"
                >
                  <i class="fa-solid fa-chevron-right" aria-hidden="true"></i>
                </button>
                <span v-else class="messenger-unit-tree-toggle messenger-unit-tree-toggle--placeholder"></span>
                <span class="messenger-unit-tree-icon" aria-hidden="true">
                  <i
                    class="fa-solid"
                    :class="row.hasChildren ? (row.expanded ? 'fa-folder-open' : 'fa-folder') : 'fa-file-lines'"
                  ></i>
                </span>
                <span class="messenger-unit-tree-name">{{ row.label }}</span>
                <span class="messenger-unit-tree-count">{{ row.count }}</span>
              </div>
            </div>
          </div>
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
            <AgentAvatar size="md" :state="resolveAgentRuntimeState(DEFAULT_AGENT_KEY)" />
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.defaultAgent') }}</span>
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
            <AgentAvatar size="md" :state="resolveAgentRuntimeState(agent.id)" />
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ agent.name || agent.id }}</span>
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
            <AgentAvatar size="md" :state="resolveAgentRuntimeState(agent.id)" />
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
            :class="{ active: selectedToolEntryKey === 'category:builtin' }"
            type="button"
            @click="selectToolCategory('builtin')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-cubes" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('toolManager.system.builtin') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.tools.builtinDesc') }}</span>
              </div>
            </div>
          </button>
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
                <span class="messenger-list-preview">{{ t('messenger.tools.mcpDesc') }}</span>
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
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.tools.skillsDesc') }}</span>
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
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.tools.knowledgeDesc') }}</span>
              </div>
            </div>
          </button>
          <button
            class="messenger-list-item"
            :class="{ active: selectedToolEntryKey === 'category:shared' }"
            type="button"
            @click="selectToolCategory('shared')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-share-nodes" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.tools.sharedTitle') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.tools.sharedDesc') }}</span>
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
        </template>

        <template v-else-if="sessionHub.activeSection === 'files'">
          <div class="messenger-block-title messenger-block-title--tight">{{ t('messenger.files.userContainer') }}</div>
          <button
            class="messenger-list-item"
            :class="{ active: fileScope === 'user' }"
            type="button"
            @click="selectContainer('user')"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-user" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.files.userContainer') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">
                  {{ t('messenger.files.userContainerDesc', { id: USER_CONTAINER_ID }) }}
                </span>
              </div>
            </div>
          </button>
          <div class="messenger-block-title messenger-block-title--tight">
            {{ t('messenger.files.agentContainerGroup') }}
          </div>
          <div v-if="boundAgentFileContainers.length" class="messenger-block-title messenger-block-title--sub">
            {{ t('messenger.files.boundGroup') }}
          </div>
          <button
            v-for="container in boundAgentFileContainers"
            :key="`container-${container.id}`"
            class="messenger-list-item messenger-list-item--compact"
            :class="{ active: fileScope === 'agent' && selectedFileContainerId === container.id }"
            type="button"
            @click="selectContainer(container.id)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-box-archive" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.files.agentContainer', { id: container.id }) }}</span>
                <span v-if="container.agentNames.length" class="messenger-kind-tag">
                  {{ t('messenger.files.agentCount', { count: container.agentNames.length }) }}
                </span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ container.preview }}</span>
              </div>
            </div>
          </button>
          <div v-if="unboundAgentFileContainers.length" class="messenger-block-title messenger-block-title--sub">
            {{ t('messenger.files.unboundGroup') }}
          </div>
          <button
            v-for="container in unboundAgentFileContainers"
            :key="`container-unbound-${container.id}`"
            class="messenger-list-item messenger-list-item--compact"
            :class="{ active: fileScope === 'agent' && selectedFileContainerId === container.id }"
            type="button"
            @click="selectContainer(container.id)"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-box-archive" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.files.agentContainer', { id: container.id }) }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ container.preview }}</span>
              </div>
            </div>
          </button>
        </template>

        <template v-else>
          <button
            class="messenger-list-item"
            :class="{ active: settingsPanelMode === 'general' }"
            type="button"
            @click="settingsPanelMode = 'general'"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-sliders" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('messenger.section.settings') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('messenger.section.settings.desc') }}</span>
              </div>
            </div>
          </button>
          <button
            class="messenger-list-item"
            :class="{ active: settingsPanelMode === 'profile' }"
            type="button"
            @click="settingsPanelMode = 'profile'"
          >
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
          <button
            v-if="desktopMode"
            class="messenger-list-item"
            :class="{ active: settingsPanelMode === 'desktop' }"
            type="button"
            @click="settingsPanelMode = 'desktop'"
          >
            <div class="messenger-list-avatar"><i class="fa-solid fa-desktop" aria-hidden="true"></i></div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ t('desktop.settings.title') }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ t('desktop.settings.systemHint') }}</span>
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
            <div class="messenger-chat-title">{{ chatPanelTitle }}</div>
            <span v-if="chatPanelKindLabel" class="messenger-chat-kind-pill">
              {{ chatPanelKindLabel }}
            </span>
          </div>
          <div class="messenger-chat-subtitle">{{ chatPanelSubtitle }}</div>
        </div>
        <div class="messenger-chat-header-actions">
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'agents'"
            class="messenger-header-action-text"
            type="button"
            @click="enterSelectedAgentConversation"
          >
            {{ t('messenger.action.openConversation') }}
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'users' && selectedContact"
            class="messenger-header-action-text"
            type="button"
            @click="openSelectedContactConversation"
          >
            {{ t('messenger.action.openConversation') }}
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'groups' && selectedGroup"
            class="messenger-header-action-text"
            type="button"
            @click="openSelectedGroupConversation"
          >
            {{ t('messenger.action.openConversation') }}
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn"
            type="button"
            :title="t('chat.newSession')"
            :aria-label="t('chat.newSession')"
            @click="startNewDraftSession"
          >
            <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn"
            type="button"
            :title="t('common.setting')"
            :aria-label="t('common.setting')"
            @click="openActiveAgentSettings"
          >
            <i class="fa-solid fa-gear" aria-hidden="true"></i>
          </button>
        </div>
      </header>

      <div
        ref="messageListRef"
        class="messenger-chat-body"
        :class="{
          'is-settings': showChatSettingsView,
          'is-messages': !showChatSettingsView,
          'is-agent': isAgentConversationActive,
          'is-world': isWorldConversationActive
        }"
        @scroll="handleMessageListScroll"
      >
        <template v-if="showChatSettingsView">
          <div class="messenger-chat-settings">
            <template v-if="showAgentSettingsPanel">
              <div class="messenger-inline-actions">
                <button
                  class="messenger-inline-btn"
                  :class="{ active: agentSettingMode === 'agent' }"
                  type="button"
                  @click="agentSettingMode = 'agent'"
                >
                  {{ t('chat.features.agentSettings') }}
                </button>
                <button
                  v-if="canManageAgentIntegrations"
                  class="messenger-inline-btn"
                  :class="{ active: agentSettingMode === 'cron' }"
                  type="button"
                  @click="agentSettingMode = 'cron'"
                >
                  {{ t('chat.features.cron') }}
                </button>
                <button
                  v-if="canManageAgentIntegrations"
                  class="messenger-inline-btn"
                  :class="{ active: agentSettingMode === 'channel' }"
                  type="button"
                  @click="agentSettingMode = 'channel'"
                >
                  {{ t('chat.features.channels') }}
                </button>
              </div>

              <div v-if="agentSettingMode === 'agent'" class="messenger-chat-settings-block">
                <AgentSettingsPanel
                  :agent-id="settingsAgentIdForApi"
                  @saved="handleAgentSettingsSaved"
                  @deleted="handleAgentDeleted"
                />
              </div>

              <div
                v-else-if="agentSettingMode === 'cron' && canManageAgentIntegrations"
                class="messenger-chat-settings-block"
              >
                <AgentCronPanel :agent-id="settingsAgentIdForApi" />
              </div>

              <div
                v-else-if="agentSettingMode === 'channel' && canManageAgentIntegrations"
                class="messenger-chat-settings-block messenger-channel-panel-wrap"
              >
                <UserChannelSettingsPanel mode="page" :agent-id="settingsAgentIdForApi" />
              </div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'users'">
              <div v-if="selectedContact" class="messenger-entity-panel messenger-entity-panel--fill">
                <div class="messenger-entity-title">{{ selectedContact.username || selectedContact.user_id }}</div>
                <div class="messenger-entity-grid">
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.userId') }}</span>
                    <span class="messenger-entity-value">{{ selectedContact.user_id || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.conversationId') }}</span>
                    <span class="messenger-entity-value">{{ selectedContact.conversation_id || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.unread') }}</span>
                    <span class="messenger-entity-value">{{ resolveUnread(selectedContact.unread_count) }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.lastMessageAt') }}</span>
                    <span class="messenger-entity-value">{{ formatTime(selectedContact.last_message_at) || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.status') }}</span>
                    <span class="messenger-entity-value">{{ selectedContact.status || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.unitId') }}</span>
                    <span class="messenger-entity-value">{{ resolveUnitLabel(selectedContact.unit_id) }}</span>
                  </div>
                </div>
                <div class="messenger-entity-meta">{{ t('messenger.entity.lastPreview') }}</div>
                <div class="messenger-entity-meta">
                  {{ selectedContact.last_message_preview || t('messenger.preview.empty') }}
                </div>
              </div>
              <div v-else class="messenger-list-empty">{{ t('messenger.empty.users') }}</div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'groups'">
              <div v-if="selectedGroup" class="messenger-entity-panel messenger-entity-panel--fill">
                <div class="messenger-entity-title">{{ selectedGroup.group_name || selectedGroup.group_id }}</div>
                <div class="messenger-entity-grid">
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.groupId') }}</span>
                    <span class="messenger-entity-value">{{ selectedGroup.group_id || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.conversationId') }}</span>
                    <span class="messenger-entity-value">{{ selectedGroup.conversation_id || '-' }}</span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.unread') }}</span>
                    <span class="messenger-entity-value">
                      {{ resolveUnread(selectedGroup.unread_count_cache) }}
                    </span>
                  </div>
                  <div class="messenger-entity-field">
                    <span class="messenger-entity-label">{{ t('messenger.entity.lastMessageAt') }}</span>
                    <span class="messenger-entity-value">{{ formatTime(selectedGroup.last_message_at) || '-' }}</span>
                  </div>
                </div>
                <div class="messenger-entity-meta">{{ t('messenger.entity.lastPreview') }}</div>
                <div class="messenger-entity-meta">
                  {{ selectedGroup.last_message_preview || t('messenger.preview.empty') }}
                </div>
              </div>
              <div v-else class="messenger-list-empty">{{ t('messenger.empty.groups') }}</div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'tools'">
              <div v-if="toolsCatalogLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
              <template
                v-else-if="
                  selectedToolCategory === 'mcp' ||
                  selectedToolCategory === 'skills' ||
                  selectedToolCategory === 'knowledge' ||
                  selectedToolCategory === 'shared'
                "
              >
                <div class="messenger-tools-pane-host user-tools-dialog">
                  <UserMcpPane
                    v-show="selectedToolCategory === 'mcp'"
                    :visible="true"
                    :active="selectedToolCategory === 'mcp'"
                    :status="toolPaneStatus"
                    @status="toolPaneStatus = String($event || '')"
                  />
                  <UserSkillPane
                    v-show="selectedToolCategory === 'skills'"
                    :visible="true"
                    :active="selectedToolCategory === 'skills'"
                    :status="toolPaneStatus"
                    @status="toolPaneStatus = String($event || '')"
                  />
                  <UserKnowledgePane
                    v-show="selectedToolCategory === 'knowledge'"
                    :visible="true"
                    :active="selectedToolCategory === 'knowledge'"
                    :status="toolPaneStatus"
                    @status="toolPaneStatus = String($event || '')"
                  />
                  <UserSharedToolsPanel v-show="selectedToolCategory === 'shared'" />
                </div>
              </template>
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
                <div class="messenger-entity-panel messenger-entity-panel--fill">
                  <div class="messenger-entity-title">{{ selectedCustomTool.name }}</div>
                  <div class="messenger-entity-meta">{{ selectedCustomTool.description || t('common.noDescription') }}</div>
                  <div class="messenger-entity-meta">{{ t('messenger.tools.customTitle') }}</div>
                  <div class="messenger-entity-meta">
                    {{ t('messenger.tools.customManageHint') }}
                  </div>
                  <div class="messenger-inline-actions">
                    <button class="messenger-inline-btn" type="button" @click="selectToolCategory('mcp')">
                      {{ t('toolManager.system.mcp') }}
                    </button>
                    <button class="messenger-inline-btn" type="button" @click="selectToolCategory('skills')">
                      {{ t('toolManager.system.skills') }}
                    </button>
                    <button class="messenger-inline-btn" type="button" @click="selectToolCategory('knowledge')">
                      {{ t('toolManager.system.knowledge') }}
                    </button>
                  </div>
                </div>
              </template>
              <div v-else class="messenger-list-empty">{{ t('messenger.empty.selectTool') }}</div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'files'">
              <div class="messenger-files-panel">
                <div class="messenger-entity-panel">
                  <div class="messenger-entity-title">{{ t('messenger.files.title') }}</div>
                  <div class="messenger-entity-grid">
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.containerType') }}</span>
                      <span class="messenger-entity-value">
                        {{
                          fileScope === 'user'
                            ? t('messenger.files.userContainer')
                            : t('messenger.files.agentContainer', { id: selectedFileContainerId })
                        }}
                      </span>
                    </div>
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.lifecycle') }}</span>
                      <span class="messenger-entity-value">
                        {{ fileContainerLifecycleText }}
                      </span>
                    </div>
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.cloudLocation') }}</span>
                      <span class="messenger-entity-value">{{ fileContainerCloudLocation }}</span>
                    </div>
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.localLocation') }}</span>
                      <span class="messenger-entity-value">{{ fileContainerLocalLocation }}</span>
                    </div>
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.agentBinding') }}</span>
                      <span class="messenger-entity-value">
                        {{ fileScope === 'user' ? currentUsername : selectedFileContainerAgentLabel }}
                      </span>
                    </div>
                    <div class="messenger-entity-field">
                      <span class="messenger-entity-label">{{ t('messenger.files.containerId') }}</span>
                      <span class="messenger-entity-value">
                        {{ selectedFileContainerId }}
                      </span>
                    </div>
                  </div>
                </div>
                <div class="messenger-workspace-scope chat-shell messenger-files-workspace">
                  <WorkspacePanel
                    :key="workspacePanelKey"
                    :agent-id="selectedFileAgentIdForApi"
                    :container-id="selectedFileContainerId"
                    :title="fileScope === 'user' ? t('messenger.files.userContainer') : t('messenger.files.title')"
                    :empty-text="fileScope === 'user' ? t('messenger.files.userEmpty') : t('workspace.empty')"
                    @stats="handleFileWorkspaceStats"
                  />
                </div>
              </div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'more'">
              <MessengerSettingsPanel
                :mode="settingsPanelMode"
                :username="currentUsername"
                :user-id="currentUserId"
                :language-label="currentLanguageLabel"
                :send-key="messengerSendKey"
                :theme-palette="themeStore.palette"
                :ui-font-size="uiFontSize"
                :desktop-tool-call-mode="desktopToolCallMode"
                :devtools-available="debugToolsAvailable"
                @toggle-language="toggleLanguage"
                @check-update="checkClientUpdate"
                @open-tools="openDesktopTools"
                @open-system="openDesktopSystemSettings"
                @toggle-devtools="openDebugTools"
                @logout="handleSettingsLogout"
                @update:send-key="updateSendKey"
                @update:theme-palette="updateThemePalette"
                @update:ui-font-size="updateUiFontSize"
                @update:desktop-tool-call-mode="updateDesktopToolCallMode"
              />
            </template>
          </div>
        </template>

        <template v-else>
          <div v-if="bootLoading" class="messenger-chat-empty">{{ t('common.loading') }}</div>
          <div v-else-if="!sessionHub.activeConversation" class="messenger-chat-empty">
            {{ t('messenger.empty.selectConversation') }}
          </div>

          <template v-else-if="isAgentConversationActive">
            <div
              v-for="(message, index) in chatStore.messages"
              :key="resolveAgentMessageKey(message, index)"
              v-show="shouldRenderAgentMessage(message)"
              class="messenger-message"
              :class="{ mine: message.role === 'user' }"
            >
              <div v-if="message.role === 'user'" class="messenger-message-avatar">
                {{ avatarLabel(message.role === 'user' ? currentUsername : activeAgentName) }}
              </div>
              <AgentAvatar
                v-else
                size="sm"
                :state="resolveMessageAgentAvatarState(message)"
                :title="activeAgentName"
              />
              <div class="messenger-message-main">
                <div class="messenger-message-meta">
                  <span>{{ message.role === 'user' ? t('chat.message.user') : activeAgentName }}</span>
                  <span>{{ formatTime(message.created_at) }}</span>
                  <MessageThinking
                    v-if="message.role === 'assistant'"
                    :content="String(message.reasoning || '')"
                    :streaming="Boolean(message.reasoningStreaming)"
                  />
                </div>
                <div v-if="message.role === 'assistant'" class="messenger-workflow-scope chat-shell">
                  <MessageWorkflow
                    :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
                    :loading="Boolean(message.workflowStreaming)"
                    :visible="Boolean(message.workflowStreaming || message.workflowItems?.length)"
                  />
                </div>
                <div
                  v-if="message.role === 'user' || hasMessageContent(message.content)"
                  class="messenger-message-bubble messenger-markdown"
                >
                  <template v-if="isGreetingMessage(message)">
                    <div class="messenger-greeting-line">
                      <div class="messenger-greeting-text">{{ message.content }}</div>
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
                                <div class="ability-section">
                                  <div class="ability-section-title">
                                    <span>{{ t('chat.ability.tools') }}</span>
                                    <span class="ability-count">{{ agentAbilitySummary.tools.length }}</span>
                                  </div>
                                  <div v-if="agentAbilitySummary.tools.length" class="ability-item-list">
                                    <div
                                      v-for="tool in agentAbilitySummary.tools"
                                      :key="`m-tool-${tool.name}`"
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
                                    <span class="ability-count">{{ agentAbilitySummary.skills.length }}</span>
                                  </div>
                                  <div v-if="agentAbilitySummary.skills.length" class="ability-item-list">
                                    <div
                                      v-for="skill in agentAbilitySummary.skills"
                                      :key="`m-skill-${skill.name}`"
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
                  <div v-else class="markdown-body" v-html="renderAgentMarkdown(message, index)"></div>
                </div>
                <div
                  v-if="hasMessageContent(message.content) || shouldShowMessageStats(message)"
                  class="messenger-message-extra"
                >
                  <div v-if="shouldShowMessageStats(message)" class="messenger-message-stats">
                    <span
                      v-for="item in buildMessageStatsEntries(message)"
                      :key="item.label"
                      class="messenger-message-stat"
                    >
                      <span class="messenger-message-stat-label">{{ item.label }}:</span>
                      <span class="messenger-message-stat-value">{{ item.value }}</span>
                    </span>
                  </div>
                  <button
                    class="messenger-message-footer-copy"
                    type="button"
                    :title="t('chat.message.copy')"
                    :aria-label="t('chat.message.copy')"
                    @click="copyMessageContent(message.content)"
                  >
                    <i class="fa-solid fa-clone" aria-hidden="true"></i>
                  </button>
                </div>
              </div>
            </div>
          </template>

          <template v-else-if="isWorldConversationActive">
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
                </div>
                <div class="messenger-message-bubble messenger-markdown">
                  <button
                    class="messenger-bubble-copy-btn"
                    type="button"
                    :title="t('chat.message.copy')"
                    :aria-label="t('chat.message.copy')"
                    @click="copyMessageContent(message.content)"
                  >
                    <i class="fa-solid fa-clone" aria-hidden="true"></i>
                  </button>
                  <div class="markdown-body" v-html="renderWorldMarkdown(message)"></div>
                </div>
              </div>
            </div>
          </template>
          <div v-else class="messenger-chat-empty">
            {{ t('messenger.empty.selectConversation') }}
          </div>
        </template>
      </div>

      <button
        v-if="!showChatSettingsView && showScrollBottomButton"
        class="messenger-scroll-bottom-btn"
        type="button"
        :title="t('chat.toBottom')"
        :aria-label="t('chat.toBottom')"
        @click="jumpToMessageBottom"
      >
        <i class="fa-solid fa-angles-down" aria-hidden="true"></i>
      </button>

      <footer v-if="!showChatSettingsView" class="messenger-chat-footer">
        <div v-if="isAgentConversationActive" class="messenger-agent-composer messenger-composer-scope chat-shell">
          <ChatComposer
            world-style
            :loading="agentSessionLoading"
            :send-key="messengerSendKey"
            @send="sendAgentMessage"
            @stop="stopAgentMessage"
          />
        </div>
        <div
          v-else-if="isWorldConversationActive"
          ref="worldComposerRef"
          class="messenger-world-composer"
          :style="worldComposerStyle"
        >
          <button
            class="messenger-world-resize-edge"
            type="button"
            :title="t('messenger.world.resize')"
            :aria-label="t('messenger.world.resize')"
            @mousedown.prevent="startWorldComposerResize"
          >
            <span class="messenger-world-resize-grip"></span>
          </button>
          <div class="messenger-world-toolbar">
            <div
              class="messenger-world-tool-anchor messenger-world-tool-anchor--emoji"
              @mouseenter="openWorldQuickPanel('emoji')"
              @mouseleave="scheduleWorldQuickPanelClose"
            >
              <button
                class="messenger-world-tool-btn"
                type="button"
                :class="{ active: worldQuickPanelMode === 'emoji' }"
                :title="t('messenger.world.emoji')"
                :aria-label="t('messenger.world.emoji')"
                @click.prevent="toggleWorldQuickPanel('emoji')"
              >
                <svg class="messenger-world-tool-icon" aria-hidden="true">
                  <use href="#smiling-face"></use>
                </svg>
              </button>
              <div
                v-if="worldQuickPanelMode === 'emoji'"
                class="messenger-world-pop-panel messenger-world-emoji-panel"
                @mouseenter="clearWorldQuickPanelClose"
                @mouseleave="scheduleWorldQuickPanelClose"
              >
                <div v-if="worldRecentEmojis.length" class="messenger-world-emoji-section">
                  <div class="messenger-world-quick-title">{{ t('messenger.world.quick.recent') }}</div>
                  <div class="messenger-world-emoji-grid">
                    <button
                      v-for="emoji in worldRecentEmojis"
                      :key="`recent-${emoji}`"
                      class="messenger-world-emoji-item"
                      type="button"
                      @click="insertWorldEmoji(emoji)"
                    >
                      {{ emoji }}
                    </button>
                  </div>
                </div>
                <div class="messenger-world-emoji-section">
                  <div class="messenger-world-quick-title">{{ t('messenger.world.quick.all') }}</div>
                  <div class="messenger-world-emoji-grid">
                    <button
                      v-for="emoji in worldEmojiCatalog"
                      :key="`catalog-${emoji}`"
                      class="messenger-world-emoji-item"
                      type="button"
                      @click="insertWorldEmoji(emoji)"
                    >
                      {{ emoji }}
                    </button>
                  </div>
                </div>
              </div>
            </div>
            <button
              class="messenger-world-tool-btn"
              type="button"
              :disabled="worldUploading"
              :title="t('userWorld.attachments.upload')"
              :aria-label="t('userWorld.attachments.upload')"
              @click="triggerWorldUpload"
            >
              <svg class="messenger-world-tool-icon" aria-hidden="true">
                <use href="#file2"></use>
              </svg>
            </button>
            <div class="messenger-world-tool-anchor messenger-world-tool-anchor--history">
              <button
                class="messenger-world-tool-btn"
                type="button"
                :title="t('messenger.world.history')"
                :aria-label="t('messenger.world.history')"
                @click="openWorldHistoryDialog"
              >
                <svg class="messenger-world-tool-icon" aria-hidden="true">
                  <use href="#history"></use>
                </svg>
              </button>
            </div>
          </div>
          <textarea
            ref="worldTextareaRef"
            v-model.trim="worldDraft"
            class="messenger-world-input"
            :placeholder="t('userWorld.input.placeholder')"
            rows="3"
            @focus="worldQuickPanelMode = ''"
            @keydown.enter="handleWorldComposerEnterKeydown"
          ></textarea>
          <div class="messenger-world-footer">
            <div class="messenger-world-send-group">
              <button
                class="messenger-world-send-main"
                type="button"
                :disabled="!canSendWorldMessage"
                @click="sendWorldMessage"
              >
                <svg class="messenger-world-send-icon" aria-hidden="true">
                  <use href="#send"></use>
                </svg>
              </button>
              <button class="messenger-world-send-menu" type="button" :title="t('messenger.settings.sendKey')">
                <svg class="messenger-world-send-icon messenger-world-send-icon--menu" aria-hidden="true">
                  <use href="#down"></use>
                </svg>
              </button>
            </div>
          </div>
          <input
            ref="worldUploadInputRef"
            type="file"
            multiple
            hidden
            @change="handleWorldUploadInput"
          />
        </div>
        <div v-else class="messenger-chat-empty">
          {{ t('messenger.empty.input') }}
        </div>
      </footer>
    </section>

    <MessengerRightDock
      ref="rightDockRef"
      v-if="showRightDock"
      :collapsed="rightDockCollapsed"
      :show-agent-panels="showRightAgentPanels"
      :agent-id-for-api="rightPanelAgentIdForApi"
      :container-id="rightPanelContainerId"
      :active-session-id="String(chatStore.activeSessionId || '')"
      :session-history="rightPanelSessionHistory"
      @toggle-collapse="rightDockCollapsed = !rightDockCollapsed"
      @restore-session="restoreTimelineSession"
      @set-main="setTimelineSessionMain"
      @delete-session="deleteTimelineSession"
    />

    <el-dialog
      v-model="worldHistoryDialogVisible"
      class="messenger-dialog messenger-world-history-dialog"
      :title="t('messenger.world.history')"
      width="520px"
      append-to-body
    >
      <div class="messenger-world-history-dialog-list">
        <button
          v-for="entry in worldHistoryEntries"
          :key="entry.key"
          class="messenger-world-history-item"
          type="button"
          :title="entry.content"
          @click="applyWorldHistory(entry.content)"
        >
          <span class="messenger-world-history-item-text">{{ entry.content }}</span>
          <span class="messenger-world-history-item-time">
            {{ entry.time ? formatTime(entry.time) : '--' }}
          </span>
        </button>
        <div v-if="!worldHistoryEntries.length" class="messenger-world-history-empty">
          {{ t('messenger.world.historyEmpty') }}
        </div>
      </div>
    </el-dialog>

    <el-dialog
      v-model="agentPromptPreviewVisible"
      class="system-prompt-dialog"
      :title="t('chat.systemPrompt.title')"
      width="720px"
      append-to-body
    >
      <div v-if="agentPromptPreviewLoading" class="messenger-list-empty">{{ t('chat.systemPrompt.loading') }}</div>
      <pre v-else class="workflow-dialog-detail">{{ activeAgentPromptPreviewText }}</pre>
    </el-dialog>

    <el-dialog
      v-model="groupCreateVisible"
      :title="t('userWorld.group.createTitle')"
      width="440px"
      class="messenger-dialog"
      append-to-body
    >
      <div class="messenger-group-create">
        <label class="messenger-group-create-field">
          <span>{{ t('userWorld.group.nameLabel') }}</span>
          <input
            v-model.trim="groupCreateName"
            type="text"
            :placeholder="t('userWorld.group.namePlaceholder')"
            autocomplete="off"
          />
        </label>
        <label class="messenger-group-create-field">
          <span>{{ t('userWorld.group.memberLabel') }}</span>
          <input
            v-model.trim="groupCreateKeyword"
            type="text"
            :placeholder="t('userWorld.group.memberPlaceholder')"
            autocomplete="off"
          />
        </label>
        <div class="messenger-group-create-list">
          <label
            v-for="contact in filteredGroupCreateContacts"
            :key="`group-member-${contact.user_id}`"
            class="messenger-group-create-item"
          >
            <input v-model="groupCreateMemberIds" type="checkbox" :value="String(contact.user_id || '')" />
            <span class="messenger-group-create-name">{{ contact.username || contact.user_id }}</span>
            <span class="messenger-group-create-unit">{{ resolveUnitLabel(contact.unit_id) }}</span>
          </label>
          <div v-if="!filteredGroupCreateContacts.length" class="messenger-list-empty">
            {{ t('userWorld.group.memberEmpty') }}
          </div>
        </div>
      </div>
      <template #footer>
        <button class="messenger-inline-btn" type="button" :disabled="groupCreating" @click="groupCreateVisible = false">
          {{ t('common.cancel') }}
        </button>
        <button class="messenger-inline-btn primary" type="button" :disabled="groupCreating" @click="submitGroupCreate">
          {{ groupCreating ? t('common.loading') : t('userWorld.group.createSubmit') }}
        </button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { listRunningAgents } from '@/api/agents';
import { fetchOrgUnits } from '@/api/auth';
import { fetchSessionSystemPrompt, fetchRealtimeSystemPrompt } from '@/api/chat';
import { fetchCronJobs } from '@/api/cron';
import { fetchUserToolsCatalog, fetchUserToolsSummary } from '@/api/userTools';
import { uploadWunderWorkspace } from '@/api/workspace';
import UserChannelSettingsPanel from '@/components/channels/UserChannelSettingsPanel.vue';
import AgentCronPanel from '@/components/messenger/AgentCronPanel.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import MessengerRightDock from '@/components/messenger/MessengerRightDock.vue';
import MessengerSettingsPanel from '@/components/messenger/MessengerSettingsPanel.vue';
import AgentSettingsPanel from '@/components/messenger/AgentSettingsPanel.vue';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageWorkflow from '@/components/chat/MessageWorkflow.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import UserKnowledgePane from '@/components/user-tools/UserKnowledgePane.vue';
import UserMcpPane from '@/components/user-tools/UserMcpPane.vue';
import UserSharedToolsPanel from '@/components/user-tools/UserSharedToolsPanel.vue';
import UserSkillPane from '@/components/user-tools/UserSkillPane.vue';
import {
  getDesktopToolCallMode,
  isDesktopModeEnabled,
  setDesktopToolCallMode,
  type DesktopToolCallMode
} from '@/config/desktop';
import { getRuntimeConfig, resolveApiBase } from '@/config/runtime';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { useThemeStore } from '@/stores/theme';
import {
  useSessionHubStore,
  resolveSectionFromRoute,
  type MessengerSection
} from '@/stores/sessionHub';
import { useUserWorldStore } from '@/stores/userWorld';
import { renderMarkdown } from '@/utils/markdown';
import { showApiError } from '@/utils/apiError';
import { buildAssistantMessageStatsEntries } from '@/utils/messageStats';
import { collectAbilityDetails, collectAbilityNames } from '@/utils/toolSummary';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';

const DEFAULT_AGENT_KEY = '__default__';
const USER_CONTAINER_ID = 1;
const AGENT_CONTAINER_IDS = Array.from({ length: 10 }, (_, index) => index + 1);
const USER_WORLD_UPLOAD_BASE = 'user-world';
const WORLD_UPLOAD_SIZE_LIMIT = 200 * 1024 * 1024;
const WORLD_QUICK_EMOJI_STORAGE_KEY = 'wunder_world_quick_emoji';
const WORLD_MESSAGE_HISTORY_STORAGE_KEY = 'wunder_world_message_history';
const WORLD_COMPOSER_HEIGHT_STORAGE_KEY = 'wunder_world_composer_height';
const DISMISSED_AGENT_STORAGE_PREFIX = 'messenger_dismissed_agent_conversations';
const AGENT_TOOL_OVERRIDE_NONE = '__no_tools__';
const WORLD_EMOJI_CATALOG = [
  '😀',
  '😁',
  '😂',
  '🤣',
  '😊',
  '😉',
  '😍',
  '😘',
  '😎',
  '🤖',
  '🫡',
  '🤔',
  '🤩',
  '🥳',
  '😴',
  '🤯',
  '😭',
  '😤',
  '🤝',
  '👍',
  '👏',
  '🙏',
  '💪',
  '🎉',
  '🌟',
  '🔥',
  '💡',
  '📌',
  '📎',
  '✅',
  '❓',
  '❗'
];
const sectionRouteMap: Record<MessengerSection, string> = {
  messages: 'chat',
  users: 'user-world',
  groups: 'user-world',
  agents: 'home',
  tools: 'tools',
  files: 'workspace',
  more: 'settings'
};
const MESSENGER_SEND_KEY_STORAGE_KEY = 'messenger_send_key';
const MESSENGER_UI_FONT_SIZE_STORAGE_KEY = 'messenger_ui_font_size';

type AgentLocalCommand = 'new' | 'stop' | 'help' | 'compact';

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
  source: Record<string, unknown>;
};

type AgentFileContainer = {
  id: number;
  agentIds: string[];
  agentNames: string[];
  preview: string;
  primaryAgentId: string;
};

type UnitTreeNode = {
  id: string;
  label: string;
  parentId: string;
  sortOrder: number;
  children: UnitTreeNode[];
};

type UnitTreeRow = {
  id: string;
  label: string;
  depth: number;
  count: number;
  hasChildren: boolean;
  expanded: boolean;
};

type AgentRuntimeState = 'idle' | 'running' | 'done' | 'error';
type MessengerSendKeyMode = 'enter' | 'ctrl_enter';
type MessengerPerfTrace = {
  label: string;
  startedAt: number;
  marks: Array<{ name: string; at: number }>;
  meta?: Record<string, unknown>;
};

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const authStore = useAuthStore();
const agentStore = useAgentStore();
const chatStore = useChatStore();
const themeStore = useThemeStore();
const userWorldStore = useUserWorldStore();
const sessionHub = useSessionHubStore();

const bootLoading = ref(true);
const selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);
const selectedContactUserId = ref('');
const selectedGroupId = ref('');
const selectedContactUnitId = ref('');
const selectedToolCategory = ref<'builtin' | 'mcp' | 'skills' | 'knowledge' | 'shared' | ''>('');
const selectedCustomToolName = ref('');
const worldDraft = ref('');
const dismissedAgentConversationMap = ref<Record<string, number>>({});
const dismissedAgentStorageKey = ref('');
const leftRailRef = ref<HTMLElement | null>(null);
const middlePaneRef = ref<HTMLElement | null>(null);
const rightDockRef = ref<{ $el?: HTMLElement } | null>(null);
const worldComposerRef = ref<HTMLElement | null>(null);
const worldTextareaRef = ref<HTMLTextAreaElement | null>(null);
const worldUploadInputRef = ref<HTMLInputElement | null>(null);
const worldUploading = ref(false);
const worldComposerHeight = ref(188);
const worldQuickPanelMode = ref<'' | 'emoji'>('');
const worldHistoryDialogVisible = ref(false);
const agentPromptPreviewVisible = ref(false);
const agentPromptPreviewLoading = ref(false);
const agentPromptPreviewContent = ref('');
const agentPromptToolSummary = ref<Record<string, unknown> | null>(null);
const agentToolSummaryLoading = ref(false);
const agentToolSummaryError = ref('');
type TooltipLike = { updatePopper?: () => void; popperRef?: { update?: () => void } };
const agentAbilityTooltipRef = ref<TooltipLike | TooltipLike[] | null>(null);
const agentAbilityTooltipVisible = ref(false);
const agentAbilityTooltipOptions = {
  strategy: 'fixed',
  modifiers: [
    { name: 'offset', options: { offset: [0, 10] } },
    { name: 'shift', options: { padding: 8 } },
    { name: 'flip', options: { padding: 8, fallbackPlacements: ['top', 'bottom', 'right', 'left'] } },
    { name: 'preventOverflow', options: { padding: 8, altAxis: true, boundary: 'viewport' } }
  ]
};
const worldRecentEmojis = ref<string[]>([]);
const worldHistoryMap = ref<Record<string, string[]>>({});
const messageListRef = ref<HTMLElement | null>(null);
const agentRuntimeStateMap = ref<Map<string, AgentRuntimeState>>(new Map());
const runtimeStateOverrides = ref<Map<string, { state: AgentRuntimeState; expiresAt: number }>>(new Map());
const cronAgentIds = ref<Set<string>>(new Set());
const agentSettingMode = ref<'agent' | 'cron' | 'channel'>('agent');
const settingsPanelMode = ref<'general' | 'profile' | 'desktop'>('general');
const rightDockCollapsed = ref(false);
const toolsCatalogLoading = ref(false);
const customTools = ref<ToolEntry[]>([]);
const sharedTools = ref<ToolEntry[]>([]);
const builtinTools = ref<ToolEntry[]>([]);
const mcpTools = ref<ToolEntry[]>([]);
const skillTools = ref<ToolEntry[]>([]);
const knowledgeTools = ref<ToolEntry[]>([]);
const toolPaneStatus = ref('');
const fileScope = ref<'agent' | 'user'>('agent');
const selectedFileContainerId = ref(USER_CONTAINER_ID);
const fileContainerLatestUpdatedAt = ref(0);
const fileContainerEntryCount = ref(0);
const fileLifecycleNowTick = ref(Date.now());
const timelinePreviewMap = ref<Map<string, string>>(new Map());
const timelinePreviewLoadingSet = ref<Set<string>>(new Set());
const desktopToolCallMode = ref<DesktopToolCallMode>(getDesktopToolCallMode());
const messengerSendKey = ref<MessengerSendKeyMode>('enter');
const uiFontSize = ref(14);
const orgUnitPathMap = ref<Record<string, string>>({});
const orgUnitTree = ref<UnitTreeNode[]>([]);
const contactUnitExpandedIds = ref<Set<string>>(new Set());
const showScrollBottomButton = ref(false);
const autoStickToBottom = ref(true);
const groupCreateVisible = ref(false);
const groupCreateName = ref('');
const groupCreateKeyword = ref('');
const groupCreateMemberIds = ref<string[]>([]);
const groupCreating = ref(false);
const viewportWidth = ref(typeof window !== 'undefined' ? window.innerWidth : 1440);
const middlePaneOverlayVisible = ref(false);
const quickCreatingAgent = ref(false);

let statusTimer: number | null = null;
let lifecycleTimer: number | null = null;
let worldQuickPanelCloseTimer: number | null = null;
let timelinePrefetchTimer: number | null = null;
let middlePaneOverlayHideTimer: number | null = null;
let viewportResizeHandler: (() => void) | null = null;
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
const MARKDOWN_CACHE_LIMIT = 280;
const MARKDOWN_STREAM_THROTTLE_MS = 80;
const markdownCache = new Map<string, { source: string; html: string; updatedAt: number }>();
const MESSENGER_PERF_TRACE_ENABLED = (() => {
  if (typeof window === 'undefined') return false;
  const raw = String(window.localStorage.getItem('messenger_perf_trace') || '')
    .trim()
    .toLowerCase();
  if (raw === '1' || raw === 'true' || raw === 'on') return true;
  return import.meta.env.DEV;
})();

const startMessengerPerfTrace = (
  label: string,
  meta: Record<string, unknown> = {}
): MessengerPerfTrace | null => {
  if (!MESSENGER_PERF_TRACE_ENABLED) return null;
  return {
    label,
    startedAt: performance.now(),
    marks: [],
    meta
  };
};

const markMessengerPerfTrace = (trace: MessengerPerfTrace | null, name: string) => {
  if (!trace) return;
  trace.marks.push({ name, at: performance.now() });
};

const finishMessengerPerfTrace = (
  trace: MessengerPerfTrace | null,
  status: 'ok' | 'fail' | 'pending' = 'ok',
  extra: Record<string, unknown> = {}
) => {
  if (!trace) return;
  const totalMs = Number((performance.now() - trace.startedAt).toFixed(1));
  const marks = trace.marks.map((item) => ({
    name: item.name,
    sinceStartMs: Number((item.at - trace.startedAt).toFixed(1))
  }));
  console.info('[messenger-perf]', {
    label: trace.label,
    status,
    totalMs,
    ...trace.meta,
    ...extra,
    marks
  });
};

const sectionOptions = computed(() => [
  { key: 'messages' as MessengerSection, icon: 'fa-solid fa-comment-dots', label: t('messenger.section.messages') },
  { key: 'users' as MessengerSection, icon: 'fa-solid fa-user-group', label: t('messenger.section.users') },
  { key: 'groups' as MessengerSection, icon: 'fa-solid fa-comments', label: t('messenger.section.groups') },
  { key: 'agents' as MessengerSection, icon: 'fa-solid fa-robot', label: t('messenger.section.agents') },
  { key: 'tools' as MessengerSection, icon: 'fa-solid fa-wrench', label: t('messenger.section.tools') },
  { key: 'files' as MessengerSection, icon: 'fa-solid fa-folder-open', label: t('messenger.section.files') },
  { key: 'more' as MessengerSection, icon: 'fa-solid fa-gear', label: t('messenger.section.settings') }
]);

const primarySectionOptions = computed(() =>
  sectionOptions.value.filter((item) => item.key !== 'more')
);

const basePrefix = computed(() => {
  if (route.path.startsWith('/desktop')) return '/desktop';
  if (route.path.startsWith('/demo')) return '/demo';
  return '/app';
});

const desktopMode = computed(() => isDesktopModeEnabled());
const debugToolsAvailable = computed(() => typeof window !== 'undefined');

const keyword = computed({
  get: () => sessionHub.keyword,
  set: (value: string) => sessionHub.setKeyword(value)
});

const currentUsername = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.username || user?.id || t('user.guest'));
});
const currentUserId = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.id || '');
});
const canManageAgentIntegrations = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  if (!user) return false;
  const roles = Array.isArray(user.roles)
    ? user.roles.map((item) => String(item || '').trim().toLowerCase())
    : [];
  return roles.includes('admin') || roles.includes('super_admin');
});

const activeSectionTitle = computed(() =>
  sessionHub.activeSection === 'more'
    ? t('messenger.section.settings')
    : t(`messenger.section.${sessionHub.activeSection}`)
);
const activeSectionSubtitle = computed(() =>
  sessionHub.activeSection === 'more'
    ? t('messenger.section.settings.desc')
    : t(`messenger.section.${sessionHub.activeSection}.desc`)
);
const currentLanguageLabel = computed(() =>
  getCurrentLanguage() === 'zh-CN' ? t('language.zh-CN') : t('language.en-US')
);
const searchPlaceholder = computed(() => t(`messenger.search.${sessionHub.activeSection}`));
const isMiddlePaneOverlay = computed(() => viewportWidth.value <= 840);
const isRightDockOverlay = computed(() => viewportWidth.value <= 1380);
const showMiddlePane = computed(() => !isMiddlePaneOverlay.value || middlePaneOverlayVisible.value);

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
const resolvedMessageConversationKind = computed<'agent' | 'world' | ''>(() => {
  if (sessionHub.activeSection !== 'messages') {
    return '';
  }
  const identity = activeConversation.value;
  if (identity?.kind === 'agent') return 'agent';
  if (identity?.kind === 'direct' || identity?.kind === 'group') return 'world';
  const queryConversationId = String(route.query?.conversation_id || '').trim();
  if (queryConversationId) return 'world';
  const querySessionId = String(route.query?.session_id || '').trim();
  const queryAgentId = String(route.query?.agent_id || '').trim();
  const queryEntry = String(route.query?.entry || '')
    .trim()
    .toLowerCase();
  if (querySessionId || queryAgentId || queryEntry === 'default') return 'agent';
  if (String(chatStore.activeSessionId || '').trim() || String(chatStore.draftAgentId || '').trim()) {
    return 'agent';
  }
  return '';
});
const isAgentConversationActive = computed(() => resolvedMessageConversationKind.value === 'agent');
const isWorldConversationActive = computed(() => resolvedMessageConversationKind.value === 'world');

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
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (sessionId) {
    const session = chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
    return normalizeAgentId(session?.agent_id || chatStore.draftAgentId);
  }
  if (String(chatStore.draftAgentId || '').trim()) {
    return normalizeAgentId(chatStore.draftAgentId);
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
const activeAgentPromptPreviewText = computed(() =>
  String(agentPromptPreviewContent.value || '').trim() || t('chat.systemPrompt.empty')
);
const activeAgentSession = computed(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) return null;
  return (
    chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId) || null
  );
});

const normalizeAbilityItemName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item.trim();
  const source = item as Record<string, unknown>;
  return String(source.name || source.tool_name || source.toolName || source.id || '').trim();
};

const buildAbilityAllowedNameSet = (summary: Record<string, unknown>): Set<string> => {
  const names = collectAbilityNames(summary);
  return new Set<string>([...(names.tools || []), ...(names.skills || [])]);
};

const filterAbilitySummaryByNames = (
  summary: Record<string, unknown>,
  selectedNames: Set<string>
): Record<string, unknown> => {
  const filterList = (list: unknown) =>
    Array.isArray(list)
      ? list.filter((item) => {
          const name = normalizeAbilityItemName(item);
          return Boolean(name) && selectedNames.has(name);
        })
      : [];
  return {
    ...summary,
    builtin_tools: filterList(summary.builtin_tools),
    mcp_tools: filterList(summary.mcp_tools),
    knowledge_tools: filterList(summary.knowledge_tools),
    user_tools: filterList(summary.user_tools),
    shared_tools: filterList(summary.shared_tools),
    skills: filterList(summary.skills)
  };
};

const effectiveAgentToolSummary = computed<Record<string, unknown> | null>(() => {
  const summary = agentPromptToolSummary.value;
  if (!summary) return null;
  const allowedSet = buildAbilityAllowedNameSet(summary);
  if (!allowedSet.size) return summary;
  const session = activeAgentSession.value as Record<string, unknown> | null;
  const sessionOverrides = Array.isArray(session?.tool_overrides)
    ? (session?.tool_overrides as unknown[])
    : [];
  const draftOverrides = Array.isArray(chatStore.draftToolOverrides)
    ? (chatStore.draftToolOverrides as unknown[])
    : [];
  const agentDefaults = Array.isArray((activeAgent.value as Record<string, unknown> | null)?.tool_names)
    ? (((activeAgent.value as Record<string, unknown> | null)?.tool_names as unknown[]) || [])
    : [];
  const sourceOverrides = sessionOverrides.length
    ? sessionOverrides
    : draftOverrides.length
      ? draftOverrides
      : agentDefaults;
  if (sourceOverrides.some((item) => String(item || '').trim() === AGENT_TOOL_OVERRIDE_NONE)) {
    return filterAbilitySummaryByNames(summary, new Set<string>());
  }
  const selectedNames = new Set<string>();
  sourceOverrides.forEach((item) => {
    const name = String(item || '').trim();
    if (name && allowedSet.has(name)) {
      selectedNames.add(name);
    }
  });
  if (!selectedNames.size && !sourceOverrides.length) {
    allowedSet.forEach((name) => selectedNames.add(name));
  }
  return filterAbilitySummaryByNames(summary, selectedNames);
});

const agentAbilitySummary = computed(() =>
  collectAbilityDetails((effectiveAgentToolSummary.value || {}) as Record<string, unknown>)
);
const hasAgentAbilitySummary = computed(
  () =>
    Array.isArray(agentAbilitySummary.value.tools) &&
    Array.isArray(agentAbilitySummary.value.skills) &&
    (agentAbilitySummary.value.tools.length > 0 || agentAbilitySummary.value.skills.length > 0)
);
const currentContainerId = computed(() => {
  const source = activeAgent.value as Record<string, unknown> | null;
  const parsed = Number.parseInt(String(source?.sandbox_container_id ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
});

const normalizeSandboxContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? USER_CONTAINER_ID), 10);
  if (!Number.isFinite(parsed)) return USER_CONTAINER_ID;
  return Math.min(10, Math.max(1, parsed));
};

const agentFileContainers = computed<AgentFileContainer[]>(() => {
  const buckets = new Map<number, { agentIds: string[]; agentNames: string[] }>();
  const seenAgentIds = new Set<string>();
  const collect = (agent: Record<string, unknown>) => {
    const normalizedId = normalizeAgentId(agent?.id);
    if (seenAgentIds.has(normalizedId)) return;
    seenAgentIds.add(normalizedId);
    const containerId = normalizeSandboxContainerId(agent?.sandbox_container_id);
    const target = buckets.get(containerId) || { agentIds: [], agentNames: [] };
    target.agentIds.push(normalizedId);
    target.agentNames.push(String(agent?.name || normalizedId));
    buckets.set(containerId, target);
  };
  collect({
    id: DEFAULT_AGENT_KEY,
    name: t('messenger.defaultAgent'),
    sandbox_container_id: USER_CONTAINER_ID
  });
  ownedAgents.value.forEach((item) => collect(item as Record<string, unknown>));
  sharedAgents.value.forEach((item) => collect(item as Record<string, unknown>));

  return AGENT_CONTAINER_IDS.map((id) => {
    const bucket = buckets.get(id) || { agentIds: [], agentNames: [] };
    const names = bucket.agentNames.filter(Boolean);
    const preview =
      names.length === 0
        ? t('messenger.files.unboundAgentContainer')
        : names.length <= 2
          ? names.join(' / ')
          : `${names.slice(0, 2).join(' / ')} +${names.length - 2}`;
    const primaryAgentId =
      bucket.agentIds.find((agentId) => agentId !== DEFAULT_AGENT_KEY) || bucket.agentIds[0] || '';
    return {
      id,
      agentIds: bucket.agentIds,
      agentNames: names,
      preview,
      primaryAgentId
    };
  });
});

const boundAgentFileContainers = computed(() =>
  agentFileContainers.value.filter((item) => item.agentNames.length > 0)
);

const unboundAgentFileContainers = computed(() =>
  agentFileContainers.value.filter((item) => item.agentNames.length === 0)
);

const selectedAgentFileContainer = computed(
  () => agentFileContainers.value.find((item) => item.id === selectedFileContainerId.value) || null
);

const selectedFileAgentIdForApi = computed(() => {
  if (fileScope.value !== 'agent') return '';
  const target = selectedAgentFileContainer.value?.primaryAgentId || '';
  if (!target || target === DEFAULT_AGENT_KEY) return '';
  return target;
});

const selectedFileContainerAgentLabel = computed(() => {
  if (fileScope.value !== 'agent') return currentUsername.value;
  const names = selectedAgentFileContainer.value?.agentNames || [];
  if (!names.length) return t('common.none');
  if (names.length <= 3) return names.join(' / ');
  return `${names.slice(0, 3).join(' / ')} +${names.length - 3}`;
});

const fileContainerCloudLocation = computed(() => {
  const apiBase = String(resolveApiBase() || '/wunder')
    .trim()
    .replace(/\/$/, '');
  const base = apiBase || '/wunder';
  const params = new URLSearchParams({
    container_id: String(selectedFileContainerId.value || USER_CONTAINER_ID)
  });
  if (selectedFileAgentIdForApi.value) {
    params.set('agent_id', selectedFileAgentIdForApi.value);
  }
  return `${base}/workspace/tree?${params.toString()}`;
});

const fileContainerLocalLocation = computed(() => {
  const runtimeRoot = String(getRuntimeConfig().workspace_root || '').trim();
  if (!runtimeRoot) {
    return t('messenger.files.localLocationUnknown');
  }
  const normalizedRoot = runtimeRoot.replace(/[\\/]+$/, '');
  const userId = String(currentUserId.value || '').trim() || 'anonymous';
  const localScope =
    fileScope.value === 'user' || selectedFileContainerId.value === USER_CONTAINER_ID
      ? userId
      : `${userId}__c__${selectedFileContainerId.value}`;
  const separator = normalizedRoot.includes('\\') ? '\\' : '/';
  return `${normalizedRoot}${separator}${localScope}`;
});

const workspacePanelKey = computed(() =>
  `${fileScope.value}:${selectedFileContainerId.value}:${selectedFileAgentIdForApi.value || 'default'}`
);

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

const showChatSettingsView = computed(() => sessionHub.activeSection !== 'messages');

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

const UNIT_UNGROUPED_ID = '__ungrouped__';

const normalizeUnitText = (value: unknown): string => String(value || '').trim();

const normalizeUiFontSize = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 14;
  return Math.min(20, Math.max(12, Math.round(parsed)));
};

const normalizeMessengerSendKey = (value: unknown): MessengerSendKeyMode =>
  String(value || '').trim().toLowerCase() === 'ctrl_enter' ? 'ctrl_enter' : 'enter';

const applyUiFontSize = (value: number) => {
  if (typeof document === 'undefined') return;
  const normalized = normalizeUiFontSize(value);
  document.documentElement.style.setProperty('--messenger-font-size', `${normalized}px`);
  document.documentElement.style.setProperty('--messenger-font-scale', String(normalized / 14));
};

const resolveUnitIdKey = (unitId: unknown): string => {
  const cleaned = normalizeUnitText(unitId);
  return cleaned || UNIT_UNGROUPED_ID;
};

const normalizeUnitShortLabel = (value: unknown): string => {
  const text = normalizeUnitText(value);
  if (!text) return '';
  const normalized = text
    .replace(/->/g, '/')
    .replace(/>/g, '/')
    .replace(/\\/g, '/')
    .replace(/\|/g, '/');
  const parts = normalized
    .split('/')
    .map((item) => item.trim())
    .filter(Boolean);
  if (parts.length > 1) {
    return parts[parts.length - 1];
  }
  return text;
};

const contactUnitLabelMap = computed(() => {
  const map = new Map<string, string>();
  (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).forEach((item) => {
    const source = item && typeof item === 'object' ? (item as Record<string, unknown>) : {};
    const key = resolveUnitIdKey(source.unit_id);
    if (!key || key === UNIT_UNGROUPED_ID || map.has(key)) return;
    const label = normalizeUnitShortLabel(
      source.unit_name ||
        source.unitName ||
        source.unit_display_name ||
        source.unitDisplayName ||
        source.path_name ||
        source.pathName ||
        source.unit_path ||
        source.unitPath
    );
    if (label) {
      map.set(key, label);
    }
  });
  return map;
});

const resolveUnitLabel = (unitId: unknown): string => {
  const cleaned = normalizeUnitText(unitId);
  if (!cleaned) return t('userWorld.unit.ungrouped');
  const mapped = normalizeUnitShortLabel(orgUnitPathMap.value[cleaned]);
  if (mapped) return mapped;
  const contactLabel = contactUnitLabelMap.value.get(cleaned);
  if (contactLabel) return contactLabel;
  return cleaned;
};

const normalizeUnitNode = (value: unknown): UnitTreeNode | null => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const unitId = normalizeUnitText(source.unit_id || source.id);
  if (!unitId) return null;
  const parentId = normalizeUnitText(source.parent_id || source.parentId);
  const sortOrder = Number(source.sort_order ?? source.sortOrder);
  const label = normalizeUnitShortLabel(
    source.name ||
      source.unit_name ||
      source.unitName ||
      source.display_name ||
      source.displayName ||
      source.path_name ||
      source.pathName
  );
  const children = (Array.isArray(source.children) ? source.children : [])
    .map((item) => normalizeUnitNode(item))
    .filter((item): item is UnitTreeNode => Boolean(item));
  const hydratedChildren = children.map((child) => ({
    ...child,
    parentId: child.parentId || unitId
  }));
  return {
    id: unitId,
    label: label || unitId,
    parentId,
    sortOrder: Number.isFinite(sortOrder) ? sortOrder : 0,
    children: hydratedChildren
  };
};

const flattenUnitNodes = (nodes: UnitTreeNode[], sink: UnitTreeNode[] = []): UnitTreeNode[] => {
  nodes.forEach((node) => {
    sink.push({
      id: node.id,
      label: node.label,
      parentId: node.parentId,
      sortOrder: node.sortOrder,
      children: []
    });
    if (node.children.length) {
      flattenUnitNodes(node.children, sink);
    }
  });
  return sink;
};

const buildUnitTreeFromFlat = (nodes: UnitTreeNode[]): UnitTreeNode[] => {
  const nodeMap = new Map<string, UnitTreeNode>();
  nodes.forEach((node) => {
    const id = normalizeUnitText(node.id);
    if (!id) return;
    const existing = nodeMap.get(id);
    if (existing) {
      if (!existing.label || existing.label === existing.id) {
        existing.label = node.label || id;
      }
      if (!existing.parentId && node.parentId) {
        existing.parentId = node.parentId;
      }
      if ((!Number.isFinite(existing.sortOrder) || existing.sortOrder === 0) && Number.isFinite(node.sortOrder)) {
        existing.sortOrder = node.sortOrder;
      }
      return;
    }
    nodeMap.set(id, {
      id,
      label: node.label || id,
      parentId: normalizeUnitText(node.parentId),
      sortOrder: Number.isFinite(node.sortOrder) ? node.sortOrder : 0,
      children: []
    });
  });

  const hasAncestor = (node: UnitTreeNode, ancestorId: string): boolean => {
    let cursor = normalizeUnitText(node.parentId);
    let guard = 0;
    while (cursor && guard < nodeMap.size) {
      if (cursor === ancestorId) {
        return true;
      }
      const parent = nodeMap.get(cursor);
      if (!parent) {
        break;
      }
      cursor = normalizeUnitText(parent.parentId);
      guard += 1;
    }
    return false;
  };

  const roots: UnitTreeNode[] = [];
  nodeMap.forEach((node) => {
    const parentId = normalizeUnitText(node.parentId);
    const parent = parentId ? nodeMap.get(parentId) : null;
    if (!parent || parent.id === node.id || hasAncestor(parent, node.id)) {
      roots.push(node);
      return;
    }
    parent.children.push(node);
  });

  const sortNodes = (list: UnitTreeNode[]) => {
    list.sort((left, right) => {
      const leftOrder = Number.isFinite(left.sortOrder) ? left.sortOrder : 0;
      const rightOrder = Number.isFinite(right.sortOrder) ? right.sortOrder : 0;
      if (leftOrder !== rightOrder) return leftOrder - rightOrder;
      return left.label.localeCompare(right.label, 'zh-CN');
    });
    list.forEach((node) => sortNodes(node.children));
  };
  sortNodes(roots);
  return roots;
};

const collectUnitNodeIds = (nodes: UnitTreeNode[], sink: Set<string>) => {
  nodes.forEach((node) => {
    sink.add(node.id);
    if (node.children.length) {
      collectUnitNodeIds(node.children, sink);
    }
  });
};

const isContactUnitExpanded = (unitId: string): boolean => contactUnitExpandedIds.value.has(unitId);

const toggleContactUnitExpanded = (unitId: string) => {
  const cleaned = normalizeUnitText(unitId);
  if (!cleaned) return;
  const next = new Set(contactUnitExpandedIds.value);
  if (next.has(cleaned)) {
    next.delete(cleaned);
  } else {
    next.add(cleaned);
  }
  contactUnitExpandedIds.value = next;
};

const resolveUnitTreeRowStyle = (row: UnitTreeRow): Record<string, string> => ({
  '--messenger-unit-depth': String(Math.max(0, row.depth))
});

const contactTotalCount = computed(() =>
  Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts.length : 0
);

const contactUnitDirectCountMap = computed(() => {
  const map = new Map<string, number>();
  (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).forEach((item) => {
    const key = resolveUnitIdKey(item?.unit_id);
    map.set(key, (map.get(key) || 0) + 1);
  });
  return map;
});

const contactUnitKnownIdSet = computed(() => {
  const set = new Set<string>();
  collectUnitNodeIds(orgUnitTree.value, set);
  return set;
});

const contactUnitDescendantMap = computed(() => {
  const map = new Map<string, Set<string>>();
  const walk = (node: UnitTreeNode): Set<string> => {
    const ids = new Set<string>([node.id]);
    node.children.forEach((child) => {
      walk(child).forEach((value) => ids.add(value));
    });
    map.set(node.id, ids);
    return ids;
  };
  orgUnitTree.value.forEach((node) => {
    walk(node);
  });
  return map;
});

const buildUnitTreeRows = (
  nodes: UnitTreeNode[],
  depth: number,
  directCountMap: Map<string, number>
): { rows: UnitTreeRow[]; total: number } => {
  let rows: UnitTreeRow[] = [];
  let total = 0;
  nodes.forEach((node) => {
    const child = buildUnitTreeRows(node.children, depth + 1, directCountMap);
    const count = (directCountMap.get(node.id) || 0) + child.total;
    if (count <= 0) {
      return;
    }
    const hasChildren = child.rows.length > 0;
    const expanded = hasChildren && isContactUnitExpanded(node.id);
    rows.push({
      id: node.id,
      label: node.label,
      depth,
      count,
      hasChildren,
      expanded
    });
    if (expanded) {
      rows = rows.concat(child.rows);
    }
    total += count;
  });
  return { rows, total };
};

const contactUnitTreeRows = computed<UnitTreeRow[]>(() => {
  const directCountMap = contactUnitDirectCountMap.value;
  const treeRows = buildUnitTreeRows(orgUnitTree.value, 0, directCountMap).rows;
  const knownIds = contactUnitKnownIdSet.value;
  const extraRows: UnitTreeRow[] = [];
  directCountMap.forEach((count, unitId) => {
    if (!count || unitId === UNIT_UNGROUPED_ID || knownIds.has(unitId)) return;
    extraRows.push({
      id: unitId,
      label: resolveUnitLabel(unitId),
      depth: 0,
      count,
      hasChildren: false,
      expanded: false
    });
  });
  extraRows.sort((left, right) => left.label.localeCompare(right.label, 'zh-CN'));
  const ungroupedCount = directCountMap.get(UNIT_UNGROUPED_ID) || 0;
  if (ungroupedCount > 0) {
    extraRows.push({
      id: UNIT_UNGROUPED_ID,
      label: t('userWorld.unit.ungrouped'),
      depth: 0,
      count: ungroupedCount,
      hasChildren: false,
      expanded: false
    });
  }
  return treeRows.concat(extraRows);
});

const selectedContactUnitScope = computed<Set<string> | null>(() => {
  const selected = normalizeUnitText(selectedContactUnitId.value);
  if (!selected) return null;
  if (selected === UNIT_UNGROUPED_ID) {
    return new Set<string>([UNIT_UNGROUPED_ID]);
  }
  const descendants = contactUnitDescendantMap.value.get(selected);
  if (descendants?.size) {
    return descendants;
  }
  return new Set<string>([selected]);
});

const filteredContacts = computed(() => {
  const text = keyword.value.toLowerCase();
  const selectedScope = selectedContactUnitScope.value;
  return (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).filter((item) => {
    const username = String(item?.username || '').toLowerCase();
    const userId = String(item?.user_id || '').toLowerCase();
    const unitKey = resolveUnitIdKey(item?.unit_id);
    if (selectedScope && !selectedScope.has(unitKey)) {
      return false;
    }
    const unitLabel = resolveUnitLabel(item?.unit_id).toLowerCase();
    return !text || username.includes(text) || userId.includes(text) || unitLabel.includes(text);
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

const filteredGroupCreateContacts = computed(() => {
  const text = String(groupCreateKeyword.value || '')
    .trim()
    .toLowerCase();
  const currentUserId = String((authStore.user as Record<string, unknown> | null)?.id || '').trim();
  return (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : [])
    .filter((contact) => String(contact?.user_id || '').trim() !== currentUserId)
    .filter((contact) => {
      if (!text) return true;
      const username = String(contact?.username || '').toLowerCase();
      const userId = String(contact?.user_id || '').toLowerCase();
      const unit = resolveUnitLabel(contact?.unit_id).toLowerCase();
      return username.includes(text) || userId.includes(text) || unit.includes(text);
    });
});

const selectedToolEntryKey = computed(() => {
  if (selectedToolCategory.value) return `category:${selectedToolCategory.value}`;
  if (selectedCustomToolName.value) return `custom:${selectedCustomToolName.value}`;
  return '';
});

const selectedCustomTool = computed(
  () => customTools.value.find((item) => item.name === selectedCustomToolName.value) || null
);

const selectedToolCategoryItems = computed(() => {
  if (selectedToolCategory.value === 'builtin') return builtinTools.value;
  if (selectedToolCategory.value === 'mcp') return mcpTools.value;
  if (selectedToolCategory.value === 'skills') return skillTools.value;
  if (selectedToolCategory.value === 'knowledge') return knowledgeTools.value;
  if (selectedToolCategory.value === 'shared') return sharedTools.value;
  return [];
});

const mixedConversations = computed<MixedConversation[]>(() => {
  const dismissedMap = dismissedAgentConversationMap.value;
  const sessionsByAgent = new Map<
    string,
    Array<{ session: Record<string, unknown>; lastAt: number; isMain: boolean }>
  >();
  (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).forEach((sessionRaw) => {
    const session = (sessionRaw || {}) as Record<string, unknown>;
    const agentId = normalizeAgentId(session.agent_id);
    const list = sessionsByAgent.get(agentId) || [];
    list.push({
      session,
      lastAt: normalizeTimestamp(session.updated_at || session.last_message_at || session.created_at),
      isMain: Boolean(session.is_main)
    });
    sessionsByAgent.set(agentId, list);
  });

  const agentItems = Array.from(sessionsByAgent.entries())
    .map(([agentId, records]) => {
      const sorted = [...records].sort((left, right) => right.lastAt - left.lastAt);
      const latest = sorted[0];
      const main = sorted.find((item) => item.isMain) || latest;
      const agent = agentMap.value.get(agentId) || null;
      const title = String(
        (agent as Record<string, unknown> | null)?.name ||
          (agentId === DEFAULT_AGENT_KEY ? t('messenger.defaultAgent') : agentId)
      );
      const preview = String(
        latest?.session?.last_message_preview || latest?.session?.last_message || latest?.session?.summary || ''
      );
      return {
        key: `agent:${agentId}`,
        kind: 'agent',
        sourceId: String(main?.session?.id || ''),
        agentId,
        title,
        preview,
        unread: 0,
        lastAt: Number(latest?.lastAt || main?.lastAt || 0)
      } as MixedConversation;
    })
    .filter((item) => {
      const dismissedAt = Number(dismissedMap[item.agentId] || 0);
      if (!dismissedAt) return true;
      return item.lastAt > dismissedAt;
    });

  const draftIdentity = activeConversation.value;
  if (draftIdentity?.kind === 'agent' && draftIdentity.id.startsWith('draft:')) {
    const draftAgentId = normalizeAgentId(draftIdentity.agentId || draftIdentity.id.slice('draft:'.length));
    const draftDismissedAt = Number(dismissedMap[draftAgentId] || 0);
    if (!agentItems.some((item) => item.agentId === draftAgentId) && !draftDismissedAt) {
      const agent = agentMap.value.get(draftAgentId) || null;
      agentItems.unshift({
        key: `agent:${draftAgentId}`,
        kind: 'agent',
        sourceId: '',
        agentId: draftAgentId,
        title: String(
          (agent as Record<string, unknown> | null)?.name ||
            (draftAgentId === DEFAULT_AGENT_KEY ? t('messenger.defaultAgent') : draftAgentId)
        ),
        preview: '',
        unread: 0,
        lastAt: Date.now()
      });
    }
  }

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
    return activeAgentName.value;
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
  if (identity.kind === 'group') {
    return t('messenger.group.subtitle');
  }
  const conversation = userWorldStore.conversations.find(
    (item) => String(item?.conversation_id || '') === identity.id
  );
  const peerUserId = String(conversation?.peer_user_id || '').trim();
  if (!peerUserId) return t('messenger.direct.subtitle');
  const contact = (Array.isArray(userWorldStore.contacts) ? userWorldStore.contacts : []).find(
    (item) => String(item?.user_id || '').trim() === peerUserId
  );
  return t('userWorld.chat.userSubtitle', { unit: resolveUnitLabel(contact?.unit_id) });
});

const activeConversationKindLabel = computed(() => {
  const identity = activeConversation.value;
  if (!identity) return '';
  return t(`messenger.kind.${identity.kind}`);
});

const chatPanelTitle = computed(() => {
  if (!showChatSettingsView.value) {
    return activeConversationTitle.value;
  }
  if (showAgentSettingsPanel.value) {
    if (settingsAgentId.value === DEFAULT_AGENT_KEY) {
      return t('messenger.defaultAgent');
    }
    const target = agentMap.value.get(normalizeAgentId(settingsAgentId.value));
    return String(target?.name || settingsAgentId.value || t('messenger.section.agents'));
  }
  if (sessionHub.activeSection === 'users') {
    return String(selectedContact.value?.username || selectedContact.value?.user_id || t('messenger.section.users'));
  }
  if (sessionHub.activeSection === 'groups') {
    return String(selectedGroup.value?.group_name || selectedGroup.value?.group_id || t('messenger.section.groups'));
  }
  if (sessionHub.activeSection === 'tools') {
    if (selectedToolCategory.value) return toolCategoryLabel(selectedToolCategory.value);
    if (selectedCustomTool.value?.name) return selectedCustomTool.value.name;
  }
  if (sessionHub.activeSection === 'more') {
    if (settingsPanelMode.value === 'profile') return t('user.profile.enter');
    if (settingsPanelMode.value === 'desktop') return t('desktop.settings.title');
  }
  return activeSectionTitle.value;
});

const chatPanelSubtitle = computed(() => {
  if (!showChatSettingsView.value) {
    return activeConversationSubtitle.value;
  }
  if (showAgentSettingsPanel.value) {
    return t('messenger.agent.subtitle');
  }
  if (sessionHub.activeSection === 'users') {
    return selectedContact.value
      ? t('userWorld.chat.userSubtitle', { unit: resolveUnitLabel(selectedContact.value.unit_id) })
      : t('messenger.section.users.desc');
  }
  if (sessionHub.activeSection === 'groups') {
    return t('messenger.section.groups.desc');
  }
  if (sessionHub.activeSection === 'tools') {
    return t('messenger.section.tools.desc');
  }
  if (sessionHub.activeSection === 'more') {
    if (settingsPanelMode.value === 'profile') return currentUsername.value;
    if (settingsPanelMode.value === 'desktop') return t('desktop.settings.systemHint');
  }
  return activeSectionSubtitle.value;
});

const chatPanelKindLabel = computed(() => {
  if (!showChatSettingsView.value) return activeConversationKindLabel.value;
  return '';
});

const agentSessionLoading = computed(() => {
  if (!isAgentConversationActive.value) return false;
  const sessionId = String(chatStore.activeSessionId || '');
  if (!sessionId) return false;
  return Boolean(chatStore.isSessionLoading(sessionId));
});

const canSendWorldMessage = computed(
  () =>
    isWorldConversationActive.value &&
    Boolean(activeConversation.value?.id) &&
    !userWorldStore.sending &&
    !worldUploading.value &&
    Boolean(worldDraft.value.trim())
);

const normalizeDismissedAgentConversationMap = (value: unknown): Record<string, number> => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return {};
  }
  return Object.entries(value as Record<string, unknown>).reduce<Record<string, number>>(
    (acc, [key, raw]) => {
      const agentId = normalizeAgentId(key);
      const timestamp = Number(raw);
      if (!agentId || !Number.isFinite(timestamp) || timestamp <= 0) {
        return acc;
      }
      acc[agentId] = timestamp;
      return acc;
    },
    {}
  );
};

const resolveDismissedAgentStorageKey = (userId: unknown): string => {
  const cleaned = String(userId || '').trim() || 'anonymous';
  return `${DISMISSED_AGENT_STORAGE_PREFIX}:${cleaned}`;
};

const ensureDismissedAgentConversationState = (force = false) => {
  if (typeof window === 'undefined') {
    dismissedAgentConversationMap.value = {};
    dismissedAgentStorageKey.value = '';
    return;
  }
  const targetKey = resolveDismissedAgentStorageKey(currentUserId.value);
  if (!force && dismissedAgentStorageKey.value === targetKey) {
    return;
  }
  dismissedAgentStorageKey.value = targetKey;
  try {
    const raw = window.localStorage.getItem(targetKey);
    dismissedAgentConversationMap.value = raw ? normalizeDismissedAgentConversationMap(JSON.parse(raw)) : {};
  } catch {
    dismissedAgentConversationMap.value = {};
  }
};

const persistDismissedAgentConversationState = () => {
  if (typeof window === 'undefined') return;
  const targetKey =
    dismissedAgentStorageKey.value || resolveDismissedAgentStorageKey(currentUserId.value);
  dismissedAgentStorageKey.value = targetKey;
  try {
    window.localStorage.setItem(targetKey, JSON.stringify(dismissedAgentConversationMap.value));
  } catch {
    // ignore localStorage errors
  }
};

const markAgentConversationDismissed = (agentId: unknown) => {
  const normalized = normalizeAgentId(agentId);
  if (!normalized) return;
  dismissedAgentConversationMap.value = {
    ...dismissedAgentConversationMap.value,
    [normalized]: Date.now()
  };
  persistDismissedAgentConversationState();
};

const clearAgentConversationDismissed = (agentId: unknown) => {
  const normalized = normalizeAgentId(agentId);
  if (!normalized || !(normalized in dismissedAgentConversationMap.value)) return;
  const next = { ...dismissedAgentConversationMap.value };
  delete next[normalized];
  dismissedAgentConversationMap.value = next;
  persistDismissedAgentConversationState();
};

const loadStoredStringArray = (storageKey: string, maxCount: number): string[] => {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((item) => String(item || '').trim())
      .filter(Boolean)
      .slice(0, maxCount);
  } catch {
    return [];
  }
};

const saveStoredStringArray = (storageKey: string, items: string[]) => {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(items));
  } catch {
    // ignore localStorage errors
  }
};

const loadWorldHistoryMap = (): Record<string, string[]> => {
  if (typeof window === 'undefined') return {};
  try {
    const raw = window.localStorage.getItem(WORLD_MESSAGE_HISTORY_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return Object.entries(parsed as Record<string, unknown>).reduce<Record<string, string[]>>((acc, [key, value]) => {
      const conversationId = String(key || '').trim();
      if (!conversationId || !Array.isArray(value)) return acc;
      acc[conversationId] = value
        .map((entry) => String(entry || '').trim())
        .filter(Boolean)
        .slice(0, 30);
      return acc;
    }, {});
  } catch {
    return {};
  }
};

const saveWorldHistoryMap = (value: Record<string, string[]>) => {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(WORLD_MESSAGE_HISTORY_STORAGE_KEY, JSON.stringify(value));
  } catch {
    // ignore localStorage errors
  }
};

const activeWorldConversationId = computed(() => {
  if (!isWorldConversationActive.value) return '';
  return String(activeConversation.value?.id || '').trim();
});

type WorldHistoryEntry = {
  key: string;
  content: string;
  time: number;
};

const normalizeWorldMessageTimestamp = (value: unknown): number => {
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric < 1_000_000_000_000 ? Math.floor(numeric * 1000) : Math.floor(numeric);
  }
  const parsed = new Date(value as string | number).getTime();
  if (Number.isFinite(parsed) && parsed > 0) return parsed;
  return 0;
};

const worldHistoryEntries = computed<WorldHistoryEntry[]>(() => {
  const messages = Array.isArray(userWorldStore.activeMessages) ? userWorldStore.activeMessages : [];
  const fromMessages = messages
    .map((item, index) => {
      const content = String(item?.content || '').replace(/\s+/g, ' ').trim();
      if (!content) return null;
      const time = normalizeWorldMessageTimestamp(item?.created_at);
      return {
        key: `message:${item?.message_id || index}:${time}`,
        content: content.slice(0, 260),
        time
      } as WorldHistoryEntry;
    })
    .filter((item): item is WorldHistoryEntry => Boolean(item))
    .reverse()
    .slice(0, 30);
  if (fromMessages.length) {
    return fromMessages;
  }
  const conversationId = activeWorldConversationId.value;
  const fallback = conversationId ? worldHistoryMap.value[conversationId] || [] : [];
  return fallback.map((content, index) => ({
    key: `history:${index}:${content.slice(0, 32)}`,
    content: String(content || '').slice(0, 260),
    time: 0
  }));
});

const worldEmojiCatalog = computed(() =>
  WORLD_EMOJI_CATALOG.filter((emoji) => !worldRecentEmojis.value.includes(emoji))
);

const clampWorldComposerHeight = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 188;
  return Math.min(340, Math.max(158, Math.round(parsed)));
};

const worldComposerStyle = computed<Record<string, string>>(() => ({
  '--messenger-world-composer-height': `${worldComposerHeight.value}px`
}));

const persistWorldComposerHeight = () => {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(
      WORLD_COMPOSER_HEIGHT_STORAGE_KEY,
      String(clampWorldComposerHeight(worldComposerHeight.value))
    );
  } catch {
    // ignore localStorage errors
  }
};

const handleWorldComposerResizeMove = (event: MouseEvent) => {
  if (!worldComposerResizeRuntime) return;
  const delta = worldComposerResizeRuntime.startY - event.clientY;
  worldComposerHeight.value = clampWorldComposerHeight(worldComposerResizeRuntime.startHeight + delta);
};

const stopWorldComposerResize = () => {
  if (typeof window !== 'undefined') {
    window.removeEventListener('mousemove', handleWorldComposerResizeMove);
    window.removeEventListener('mouseup', stopWorldComposerResize);
  }
  if (!worldComposerResizeRuntime) return;
  worldComposerResizeRuntime = null;
  persistWorldComposerHeight();
};

const startWorldComposerResize = (event: MouseEvent) => {
  if (event.button !== 0) return;
  worldComposerResizeRuntime = {
    startY: event.clientY,
    startHeight: worldComposerHeight.value
  };
  if (typeof window !== 'undefined') {
    window.addEventListener('mousemove', handleWorldComposerResizeMove);
    window.addEventListener('mouseup', stopWorldComposerResize);
  }
};

const clearWorldQuickPanelClose = () => {
  if (worldQuickPanelCloseTimer) {
    window.clearTimeout(worldQuickPanelCloseTimer);
    worldQuickPanelCloseTimer = null;
  }
};

const scheduleWorldQuickPanelClose = () => {
  clearWorldQuickPanelClose();
  worldQuickPanelCloseTimer = window.setTimeout(() => {
    worldQuickPanelMode.value = '';
    worldQuickPanelCloseTimer = null;
  }, 120);
};

const openWorldQuickPanel = (mode: 'emoji') => {
  clearWorldQuickPanelClose();
  worldQuickPanelMode.value = mode;
};

const toggleWorldQuickPanel = (mode: 'emoji') => {
  clearWorldQuickPanelClose();
  worldQuickPanelMode.value = worldQuickPanelMode.value === mode ? '' : mode;
};

const openWorldHistoryDialog = () => {
  clearWorldQuickPanelClose();
  worldQuickPanelMode.value = '';
  worldHistoryDialogVisible.value = true;
};

const rememberWorldEmoji = (emoji: string) => {
  const cleaned = String(emoji || '').trim();
  if (!cleaned) return;
  worldRecentEmojis.value = [cleaned, ...worldRecentEmojis.value.filter((item) => item !== cleaned)].slice(0, 12);
  saveStoredStringArray(WORLD_QUICK_EMOJI_STORAGE_KEY, worldRecentEmojis.value);
};

const focusWorldTextareaToEnd = () => {
  nextTick(() => {
    const textarea = worldTextareaRef.value;
    if (!textarea) return;
    if (typeof textarea.focus === 'function') {
      textarea.focus();
    }
    const cursor = String(worldDraft.value || '').length;
    if (typeof textarea.setSelectionRange === 'function') {
      textarea.setSelectionRange(cursor, cursor);
    }
  });
};

const insertWorldEmoji = (emoji: string) => {
  const cleaned = String(emoji || '').trim();
  if (!cleaned) return;
  worldDraft.value = `${worldDraft.value}${cleaned}`;
  rememberWorldEmoji(cleaned);
  worldQuickPanelMode.value = '';
  focusWorldTextareaToEnd();
};

const applyWorldHistory = (content: string) => {
  const cleaned = String(content || '').trim();
  if (!cleaned) return;
  worldDraft.value = cleaned;
  worldQuickPanelMode.value = '';
  worldHistoryDialogVisible.value = false;
  focusWorldTextareaToEnd();
};

const pushWorldMessageHistory = (content: string) => {
  const cleaned = String(content || '').trim();
  const conversationId = activeWorldConversationId.value;
  if (!cleaned || !conversationId) return;
  const current = worldHistoryMap.value[conversationId] || [];
  const nextHistory = [cleaned, ...current.filter((item) => item !== cleaned)].slice(0, 30);
  worldHistoryMap.value = {
    ...worldHistoryMap.value,
    [conversationId]: nextHistory
  };
  saveWorldHistoryMap(worldHistoryMap.value);
};

const closeWorldQuickPanelWhenOutside = (event: Event) => {
  const target = event.target as Node | null;
  if (!target) {
    return;
  }
  if (worldQuickPanelMode.value) {
    if (!worldComposerRef.value || !worldComposerRef.value.contains(target)) {
      clearWorldQuickPanelClose();
      worldQuickPanelMode.value = '';
    }
  }

  if (isRightDockOverlay.value && showRightDock.value && !rightDockCollapsed.value) {
    const rightDockElement = rightDockRef.value?.$el;
    if (rightDockElement && !rightDockElement.contains(target)) {
      rightDockCollapsed.value = true;
    }
  }

  if (isMiddlePaneOverlay.value && middlePaneOverlayVisible.value) {
    const isInMiddlePane = Boolean(middlePaneRef.value?.contains(target));
    const isInLeftRail = Boolean(leftRailRef.value?.contains(target));
    if (!isInMiddlePane && !isInLeftRail) {
      clearMiddlePaneOverlayHide();
      middlePaneOverlayVisible.value = false;
    }
  }
};

const AGENT_CONTAINER_TTL_MS = 24 * 60 * 60 * 1000;

const formatRemainingDuration = (ms: number): string => {
  const safe = Math.max(0, Math.floor(ms / 1000));
  const days = Math.floor(safe / 86400);
  const hours = Math.floor((safe % 86400) / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  if (days > 0) {
    return t('messenger.files.lifecycleDaysHours', { days, hours });
  }
  if (hours > 0) {
    return t('messenger.files.lifecycleHoursMinutes', { hours, minutes });
  }
  return t('messenger.files.lifecycleMinutes', { minutes: Math.max(1, minutes) });
};

const fileContainerLifecycleText = computed(() => {
  if (fileScope.value === 'user') {
    return t('messenger.files.lifecyclePermanentValue');
  }
  if (!fileContainerEntryCount.value || fileContainerLatestUpdatedAt.value <= 0) {
    return t('messenger.files.lifecycleEmptyValue');
  }
  const remaining = fileContainerLatestUpdatedAt.value + AGENT_CONTAINER_TTL_MS - fileLifecycleNowTick.value;
  if (remaining <= 0) {
    return t('messenger.files.lifecycleExpiredValue');
  }
  return t('messenger.files.lifecycleRemainingValue', {
    remaining: formatRemainingDuration(remaining)
  });
});

const showRightDock = computed(() => {
  if (sessionHub.activeSection === 'agents') return true;
  return sessionHub.activeSection === 'messages' && isAgentConversationActive.value;
});

const showRightAgentPanels = computed(() => showRightDock.value);

const rightPanelAgentId = computed(() => {
  if (!showRightAgentPanels.value) return '';
  return normalizeAgentId(settingsAgentId.value || activeAgentId.value);
});

const rightPanelAgentIdForApi = computed(() => {
  const value = normalizeAgentId(rightPanelAgentId.value);
  return value === DEFAULT_AGENT_KEY ? '' : value;
});

const rightPanelContainerId = computed(() => {
  const value = normalizeAgentId(rightPanelAgentId.value);
  const source = agentMap.value.get(value) || null;
  const parsed = Number.parseInt(String((source as Record<string, unknown> | null)?.sandbox_container_id ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
});

const extractLatestUserPreview = (messages: unknown[]): string => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const item = (messages[index] || {}) as Record<string, unknown>;
    if (String(item.role || '').trim() !== 'user') continue;
    const content = String(item.content || '').trim();
    if (content) {
      return content.replace(/\s+/g, ' ').slice(0, 120);
    }
  }
  return '';
};

const resolveSessionTimelinePreview = (session: Record<string, unknown>): string => {
  const sessionId = String(session?.id || '').trim();
  if (sessionId) {
    const cached = String(timelinePreviewMap.value.get(sessionId) || '').trim();
    if (cached) return cached;
  }
  return String(
    session?.last_user_message_preview ||
      session?.last_user_message ||
      session?.last_message_preview ||
      session?.last_message ||
      session?.summary ||
      ''
  )
    .replace(/\s+/g, ' ')
    .slice(0, 120);
};

const rightPanelSessionHistory = computed(() => {
  if (!showRightDock.value) return [];
  const targetAgentId = normalizeAgentId(rightPanelAgentId.value);
  const result = (Array.isArray(chatStore.sessions) ? chatStore.sessions : [])
    .filter((session) => normalizeAgentId(session?.agent_id) === targetAgentId)
    .map((session) => ({
      id: String(session?.id || ''),
      title: String(session?.title || t('chat.newSession')),
      preview: resolveSessionTimelinePreview(session as Record<string, unknown>),
      lastAt: session?.updated_at || session?.last_message_at || session?.created_at,
      isMain: Boolean(session?.is_main)
    }))
    .filter((item) => item.id)
    .sort((left, right) => normalizeTimestamp(right.lastAt) - normalizeTimestamp(left.lastAt));
  return result;
});

const preloadTimelinePreview = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  if (timelinePreviewMap.value.has(targetId) || timelinePreviewLoadingSet.value.has(targetId)) {
    return;
  }
  timelinePreviewLoadingSet.value.add(targetId);
  try {
    await chatStore.preloadSessionDetail(targetId);
    const messages = chatStore.getCachedSessionMessages(targetId);
    if (!Array.isArray(messages) || !messages.length) {
      timelinePreviewMap.value.set(targetId, '');
      return;
    }
    const preview = extractLatestUserPreview(messages as unknown[]);
    timelinePreviewMap.value.set(targetId, preview);
  } catch {
    // Ignore timeline prefetch errors to keep the dock lightweight.
  } finally {
    timelinePreviewLoadingSet.value.delete(targetId);
  }
};

const hasCronTask = (agentId: unknown): boolean =>
  cronAgentIds.value.has(normalizeAgentId(agentId));

const normalizeRuntimeState = (state: unknown, pendingQuestion = false): AgentRuntimeState => {
  const raw = String(state || '')
    .trim()
    .toLowerCase();
  if (pendingQuestion || raw === 'running' || raw === 'waiting' || raw === 'cancelling') return 'running';
  if (raw === 'done' || raw === 'completed' || raw === 'finish' || raw === 'finished') return 'done';
  if (raw === 'error' || raw === 'failed' || raw === 'timeout') return 'error';
  return 'idle';
};

const setRuntimeStateOverride = (agentId: unknown, state: AgentRuntimeState, ttlMs = 0) => {
  const key = normalizeAgentId(agentId);
  if (ttlMs <= 0) {
    runtimeStateOverrides.value.delete(key);
    return;
  }
  runtimeStateOverrides.value.set(key, {
    state,
    expiresAt: Date.now() + ttlMs
  });
};

const resolveAgentRuntimeState = (agentId: unknown): AgentRuntimeState => {
  const key = normalizeAgentId(agentId);
  const now = Date.now();
  const override = runtimeStateOverrides.value.get(key);
  if (override && override.expiresAt > now) {
    return override.state;
  }
  if (override && override.expiresAt <= now) {
    runtimeStateOverrides.value.delete(key);
  }
  return agentRuntimeStateMap.value.get(key) || 'idle';
};

const hasMessageContent = (value: unknown): boolean => Boolean(String(value || '').trim());

const hasWorkflowOrThinking = (message: Record<string, unknown>): boolean =>
  Boolean(message?.workflowStreaming) ||
  Boolean(message?.reasoningStreaming) ||
  Boolean((message?.workflowItems as unknown[])?.length) ||
  hasMessageContent(message?.reasoning);

const shouldRenderAgentMessage = (message: Record<string, unknown>): boolean => {
  if (String(message?.role || '') === 'user') return true;
  return hasMessageContent(message?.content) || hasWorkflowOrThinking(message);
};

const isGreetingMessage = (message: Record<string, unknown>): boolean =>
  String(message?.role || '') === 'assistant' && Boolean(message?.isGreeting);

const resolveMessageAgentAvatarState = (message: Record<string, unknown>): AgentRuntimeState => {
  if (String(message?.role || '') !== 'assistant') return 'idle';
  if (
    Boolean(message?.stream_incomplete) ||
    Boolean(message?.workflowStreaming) ||
    Boolean(message?.reasoningStreaming)
  ) {
    return 'running';
  }
  const current = resolveAgentRuntimeState(activeAgentId.value);
  return current === 'idle' ? 'done' : current;
};

const buildMessageStatsEntries = (message: Record<string, unknown>) =>
  buildAssistantMessageStatsEntries(message as Record<string, any>, t);

const shouldShowMessageStats = (message: Record<string, unknown>): boolean =>
  buildMessageStatsEntries(message).length > 0;

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
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? 0 : value.getTime();
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const date = new Date(text);
  return Number.isNaN(date.getTime()) ? 0 : date.getTime();
};

const formatTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) return '';
  const date = new Date(ts);
  const now = new Date();
  const sameYear = date.getFullYear() === now.getFullYear();
  const sameDay =
    sameYear && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  if (sameDay) {
    return `${hour}:${minute}`;
  }
  if (sameYear) {
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${month}-${day}`;
  }
  return String(date.getFullYear());
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
    const currentAgentId = normalizeAgentId(
      identity.agentId ||
        (identity.id.startsWith('draft:')
          ? identity.id.slice('draft:'.length)
          : chatStore.sessions.find((session) => String(session?.id || '') === identity.id)?.agent_id)
    );
    return currentAgentId === item.agentId;
  }
  return identity.kind === item.kind && identity.id === item.sourceId;
};

const canDeleteMixedConversation = (item: MixedConversation): boolean =>
  item?.kind === 'agent' || Boolean(item?.sourceId);

const deleteMixedConversation = async (item: MixedConversation) => {
  const sourceId = String(item?.sourceId || '').trim();
  if (!sourceId) return;
  try {
    await ElMessageBox.confirm(t('chat.history.confirmDelete'), t('chat.history.confirmTitle'), {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
  } catch {
    return;
  }
  try {
    if (item.kind === 'agent') {
      const agentId = normalizeAgentId(item.agentId);
      markAgentConversationDismissed(agentId);
      if (sourceId) {
        timelinePreviewMap.value.delete(sourceId);
      }
      if (isMixedConversationActive(item)) {
        const fallback = mixedConversations.value.find((entry) => entry.key !== item.key);
        if (fallback) {
          await openMixedConversation(fallback);
        } else {
          sessionHub.clearActiveConversation();
          const nextQuery = {
            ...route.query,
            section: 'messages'
          } as Record<string, any>;
          delete nextQuery.conversation_id;
          delete nextQuery.session_id;
          delete nextQuery.agent_id;
          delete nextQuery.entry;
          router.replace({ path: `${basePrefix.value}/chat`, query: nextQuery }).catch(() => undefined);
        }
      }
    } else {
      await userWorldStore.dismissConversation(sourceId);
    }
    ElMessage.success(t('chat.history.delete'));
  } catch (error) {
    showApiError(error, t('chat.sessions.deleteFailed'));
  }
};

const switchSection = (section: MessengerSection) => {
  openMiddlePaneOverlay();
  sessionHub.setSection(section);
  sessionHub.setKeyword('');
  worldHistoryDialogVisible.value = false;
  agentPromptPreviewVisible.value = false;
  toolPaneStatus.value = '';
  if (section === 'more') {
    settingsPanelMode.value = 'general';
  }
  if (section !== 'tools') {
    selectedToolCategory.value = '';
    selectedCustomToolName.value = '';
  }
  if (section !== 'users') {
    selectedContactUserId.value = '';
    selectedContactUnitId.value = '';
  }
  if (section !== 'groups') {
    selectedGroupId.value = '';
  }
  if (section === 'agents') {
    agentSettingMode.value = 'agent';
  }
  if (section === 'files') {
    if (fileScope.value === 'user') {
      selectedFileContainerId.value = USER_CONTAINER_ID;
    } else if (!agentFileContainers.value.some((item) => item.id === selectedFileContainerId.value)) {
      selectedFileContainerId.value = agentFileContainers.value[0]?.id ?? USER_CONTAINER_ID;
    }
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

const openSettingsPage = () => {
  settingsPanelMode.value = 'general';
  switchSection('more');
};

const openProfilePage = () => {
  settingsPanelMode.value = 'profile';
  sessionHub.setSection('more');
  sessionHub.setKeyword('');
  const nextQuery = { ...route.query, section: 'more' } as Record<string, any>;
  delete nextQuery.session_id;
  delete nextQuery.agent_id;
  delete nextQuery.entry;
  delete nextQuery.conversation_id;
  router.push({ path: `${basePrefix.value}/profile`, query: nextQuery }).catch(() => undefined);
};

const handleSettingsLogout = () => {
  if (desktopMode.value) {
    router.push('/desktop/home').catch(() => undefined);
    return;
  }
  authStore.logout();
  router.push('/login').catch(() => undefined);
};

const clearMiddlePaneOverlayHide = () => {
  if (typeof window !== 'undefined' && middlePaneOverlayHideTimer) {
    window.clearTimeout(middlePaneOverlayHideTimer);
    middlePaneOverlayHideTimer = null;
  }
};

const openMiddlePaneOverlay = () => {
  if (!isMiddlePaneOverlay.value) return;
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = true;
};

const cancelMiddlePaneOverlayHide = () => {
  clearMiddlePaneOverlayHide();
};

const scheduleMiddlePaneOverlayHide = () => {
  if (!isMiddlePaneOverlay.value) return;
  clearMiddlePaneOverlayHide();
  if (typeof window === 'undefined') {
    middlePaneOverlayVisible.value = false;
    return;
  }
  middlePaneOverlayHideTimer = window.setTimeout(() => {
    middlePaneOverlayHideTimer = null;
    middlePaneOverlayVisible.value = false;
  }, 140);
};

const openCreatedAgentSettings = (agentId: unknown) => {
  const normalizedId = normalizeAgentId(agentId);
  if (!normalizedId) {
    return;
  }
  sessionHub.setSection('agents');
  selectedAgentId.value = normalizedId;
  agentSettingMode.value = 'agent';
  router
    .replace({ path: `${basePrefix.value}/home`, query: { ...route.query, section: 'agents' } })
    .catch(() => undefined);
};

const buildQuickAgentName = () => {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, '0');
  const suffix = `${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(
    now.getMinutes()
  )}${pad(now.getSeconds())}`;
  return `${t('messenger.action.newAgent')} ${suffix}`;
};

const createAgentQuickly = async () => {
  if (quickCreatingAgent.value) {
    return;
  }
  quickCreatingAgent.value = true;
  try {
    const created = await agentStore.createAgent({
      name: buildQuickAgentName(),
      description: '',
      system_prompt: '',
      tool_names: []
    });
    ElMessage.success(t('portal.agent.createSuccess'));
    const tasks: Promise<unknown>[] = [loadRunningAgents()];
    if (canManageAgentIntegrations.value) {
      tasks.push(loadCronAgentIds());
    } else {
      cronAgentIds.value = new Set<string>();
    }
    await Promise.all(tasks);
    openCreatedAgentSettings(created?.id);
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    quickCreatingAgent.value = false;
  }
};

const handleSearchCreateAction = async () => {
  if (sessionHub.activeSection === 'groups') {
    groupCreateName.value = '';
    groupCreateKeyword.value = '';
    groupCreateMemberIds.value = [];
    groupCreateVisible.value = true;
    return;
  }
  if (sessionHub.activeSection === 'agents') {
    await createAgentQuickly();
  }
};

const openMixedConversation = async (item: MixedConversation) => {
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = false;
  if (item.kind === 'agent') {
    if (item.sourceId) {
      await openAgentSession(item.sourceId, item.agentId);
      return;
    }
    await openAgentById(item.agentId);
    return;
  }
  await openWorldConversation(item.sourceId, item.kind, 'messages');
};

const selectContact = (contact: Record<string, unknown>) => {
  selectedContactUserId.value = String(contact?.user_id || '').trim();
  selectedGroupId.value = '';
};

const selectGroup = (group: Record<string, unknown>) => {
  selectedGroupId.value = String(group?.group_id || '').trim();
  selectedContactUserId.value = '';
};

const openWorldConversation = async (
  conversationId: string,
  kind: 'direct' | 'group',
  mode: 'detail' | 'messages' = 'detail'
) => {
  if (!conversationId) return;
  const perfTrace = startMessengerPerfTrace('openWorldConversation', {
    conversationId,
    kind,
    mode
  });
  try {
    if (mode === 'messages') {
      clearMiddlePaneOverlayHide();
      middlePaneOverlayVisible.value = false;
    }
    markMessengerPerfTrace(perfTrace, 'beforeActivate');
    const activateTask = userWorldStore.setActiveConversation(conversationId, { waitForLoad: false });
    markMessengerPerfTrace(perfTrace, 'afterActivateScheduled');
    sessionHub.setActiveConversation({ kind, id: conversationId });
    const section = mode === 'messages' ? 'messages' : kind === 'group' ? 'groups' : 'users';
    const nextQuery = { ...route.query, section, conversation_id: conversationId } as Record<string, any>;
    delete nextQuery.session_id;
    delete nextQuery.agent_id;
    delete nextQuery.entry;
    markMessengerPerfTrace(perfTrace, 'beforeRouteReplace');
    router.replace({
      path: mode === 'messages' ? `${basePrefix.value}/chat` : `${basePrefix.value}/user-world`,
      query: nextQuery
    }).catch(() => undefined);
    markMessengerPerfTrace(perfTrace, 'afterRouteReplace');
    await scrollMessagesToBottom(true);
    markMessengerPerfTrace(perfTrace, 'afterScrollBottom');
    finishMessengerPerfTrace(perfTrace, 'pending');
    void activateTask.then(
      () => {
        finishMessengerPerfTrace(perfTrace, 'ok', { phase: 'activateTask' });
      },
      (error) => {
        finishMessengerPerfTrace(perfTrace, 'fail', {
          phase: 'activateTask',
          error: (error as { message?: string })?.message || String(error)
        });
        showApiError(error, t('messenger.error.openConversation'));
      }
    );
  } catch (error) {
    finishMessengerPerfTrace(perfTrace, 'fail', {
      phase: 'openWorldConversation',
      error: (error as { message?: string })?.message || String(error)
    });
    showApiError(error, t('messenger.error.openConversation'));
  }
};

const openAgentById = async (agentId: unknown) => {
  const normalized = normalizeAgentId(agentId);
  clearAgentConversationDismissed(normalized);
  selectedAgentId.value = normalized;
  const sessions = (Array.isArray(chatStore.sessions) ? chatStore.sessions : [])
    .filter((item) => normalizeAgentId(item?.agent_id) === normalized)
    .sort(
      (left, right) =>
        normalizeTimestamp(right?.updated_at || right?.last_message_at || right?.created_at) -
        normalizeTimestamp(left?.updated_at || left?.last_message_at || left?.created_at)
    );
  const mainSession = sessions.find((item) => Boolean(item?.is_main));
  const targetSession = mainSession || sessions[0];
  if (targetSession?.id) {
    await openAgentSession(String(targetSession.id), normalized);
    return;
  }
  chatStore.openDraftSession({ agent_id: normalized === DEFAULT_AGENT_KEY ? '' : normalized });
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = false;
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
  await scrollMessagesToBottom(true);
};

const selectAgentForSettings = (agentId: unknown) => {
  selectedAgentId.value = normalizeAgentId(agentId);
  agentSettingMode.value = 'agent';
};

const enterSelectedAgentConversation = async () => {
  const target = settingsAgentId.value || DEFAULT_AGENT_KEY;
  await openAgentById(target);
};

const openActiveAgentSettings = () => {
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  selectedAgentId.value = targetAgentId;
  agentSettingMode.value = 'agent';
  sessionHub.setSection('agents');
  const nextQuery = {
    ...route.query,
    section: 'agents',
    agent_id: targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId
  } as Record<string, any>;
  delete nextQuery.session_id;
  delete nextQuery.entry;
  delete nextQuery.conversation_id;
  router
    .push({
      path: `${basePrefix.value}/home`,
      query: nextQuery
    })
    .catch(() => undefined);
};

const updateAgentAbilityTooltip = async () => {
  await nextTick();
  const raw = agentAbilityTooltipRef.value;
  const tooltipRefs = Array.isArray(raw) ? raw : raw ? [raw] : [];
  tooltipRefs.forEach((tooltip) => {
    if (tooltip?.updatePopper) {
      tooltip.updatePopper();
    } else if (tooltip?.popperRef?.update) {
      tooltip.popperRef.update();
    }
  });
  requestAnimationFrame(() => {
    tooltipRefs.forEach((tooltip) => {
      if (tooltip?.updatePopper) {
        tooltip.updatePopper();
      } else if (tooltip?.popperRef?.update) {
        tooltip.popperRef.update();
      }
    });
  });
};

const loadAgentToolSummary = async () => {
  if (agentToolSummaryLoading.value || agentPromptToolSummary.value) {
    return agentPromptToolSummary.value;
  }
  agentToolSummaryLoading.value = true;
  agentToolSummaryError.value = '';
  try {
    const result = await fetchUserToolsSummary();
    const summary = (result?.data?.data as Record<string, unknown> | null) || null;
    agentPromptToolSummary.value = summary;
    return summary;
  } catch (error) {
    agentToolSummaryError.value =
      (error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail ||
      t('chat.toolSummaryFailed');
    return null;
  } finally {
    agentToolSummaryLoading.value = false;
    if (agentAbilityTooltipVisible.value) {
      await updateAgentAbilityTooltip();
    }
  }
};

const handleAgentAbilityTooltipShow = () => {
  agentAbilityTooltipVisible.value = true;
  void loadAgentToolSummary();
  void updateAgentAbilityTooltip();
};

const handleAgentAbilityTooltipHide = () => {
  agentAbilityTooltipVisible.value = false;
};

const openAgentPromptPreview = async () => {
  agentPromptPreviewVisible.value = true;
  agentPromptPreviewLoading.value = true;
  agentPromptPreviewContent.value = '';
  const summaryPromise = loadAgentToolSummary();
  try {
    const session = activeAgentSession.value as Record<string, unknown> | null;
    const sessionId = String(chatStore.activeSessionId || '').trim();
    const sessionOverrides = Array.isArray(session?.tool_overrides)
      ? (session?.tool_overrides as unknown[])
      : [];
    const draftOverrides = Array.isArray(chatStore.draftToolOverrides)
      ? (chatStore.draftToolOverrides as unknown[])
      : [];
    const overrides = sessionOverrides.length ? sessionOverrides : draftOverrides.length ? draftOverrides : undefined;
    const sourceAgentId = normalizeAgentId(
      session?.agent_id || chatStore.draftAgentId || activeAgentId.value
    );
    const agentId = sourceAgentId === DEFAULT_AGENT_KEY ? '' : sourceAgentId;
    const payload = {
      ...(agentId ? { agent_id: agentId } : {}),
      ...(overrides ? { tool_overrides: overrides } : {})
    };
    const promptRequest = sessionId
      ? fetchSessionSystemPrompt(sessionId, payload)
      : fetchRealtimeSystemPrompt(payload);
    const promptResult = await promptRequest;
    await summaryPromise;
    const promptPayload = (promptResult?.data?.data || {}) as Record<string, unknown>;
    agentPromptPreviewContent.value = String(promptPayload.prompt || '');
  } catch (error) {
    showApiError(error, t('chat.systemPromptFailed'));
    agentPromptPreviewContent.value = '';
  } finally {
    agentPromptPreviewLoading.value = false;
  }
};

const openSelectedContactConversation = async () => {
  if (!selectedContact.value) return;
  const perfTrace = startMessengerPerfTrace('openSelectedContactConversation', {
    selectedContactUserId: String(selectedContact.value?.user_id || '').trim()
  });
  const peerUserId = String(selectedContact.value.user_id || '').trim();
  const listMatchedConversationId = (Array.isArray(userWorldStore.conversations) ? userWorldStore.conversations : [])
    .find((item) => {
      const kind = String(item?.conversation_type || '').trim().toLowerCase();
      return kind !== 'group' && String(item?.peer_user_id || '').trim() === peerUserId;
    })
    ?.conversation_id;
  const conversationId = String(selectedContact.value.conversation_id || listMatchedConversationId || '').trim();
  if (conversationId) {
    markMessengerPerfTrace(perfTrace, 'hitExistingConversation');
    await openWorldConversation(conversationId, 'direct', 'messages');
    finishMessengerPerfTrace(perfTrace, 'ok', { reusedConversation: true });
    return;
  }
  if (!peerUserId) return;
  try {
    markMessengerPerfTrace(perfTrace, 'callOpenConversationByPeer');
    const conversation = await userWorldStore.openConversationByPeer(peerUserId, {
      waitForLoad: false,
      activate: false
    });
    markMessengerPerfTrace(perfTrace, 'returnedOpenConversationByPeer');
    const targetConversationId = String(
      (conversation as Record<string, unknown> | null)?.conversation_id || userWorldStore.activeConversationId || ''
    ).trim();
    if (targetConversationId) {
      await openWorldConversation(targetConversationId, 'direct', 'messages');
      finishMessengerPerfTrace(perfTrace, 'ok', { reusedConversation: false });
      return;
    }
    finishMessengerPerfTrace(perfTrace, 'fail', { phase: 'missingConversationId' });
  } catch (error) {
    finishMessengerPerfTrace(perfTrace, 'fail', {
      phase: 'openConversationByPeer',
      error: (error as { message?: string })?.message || String(error)
    });
    showApiError(error, t('userWorld.contact.openFailed'));
  }
};

const openSelectedGroupConversation = async () => {
  if (!selectedGroup.value) return;
  const conversationId = String(selectedGroup.value.conversation_id || '').trim();
  if (!conversationId) return;
  await openWorldConversation(conversationId, 'group', 'messages');
};

const submitGroupCreate = async () => {
  const groupName = String(groupCreateName.value || '').trim();
  const members = groupCreateMemberIds.value
    .map((item) => String(item || '').trim())
    .filter((item) => Boolean(item));
  if (!groupName) {
    ElMessage.warning(t('userWorld.group.namePlaceholder'));
    return;
  }
  if (!members.length) {
    ElMessage.warning(t('userWorld.group.memberEmpty'));
    return;
  }
  groupCreating.value = true;
  try {
    const created = await userWorldStore.createGroupConversation(groupName, members);
    groupCreateVisible.value = false;
    groupCreateName.value = '';
    groupCreateKeyword.value = '';
    groupCreateMemberIds.value = [];
    ElMessage.success(t('userWorld.group.createSuccess'));
    const conversationId = String(created?.conversation_id || '').trim();
    if (conversationId) {
      await openWorldConversation(conversationId, 'group', 'messages');
    } else {
      await userWorldStore.refreshGroups();
    }
  } catch (error) {
    showApiError(error, t('userWorld.group.createFailed'));
  } finally {
    groupCreating.value = false;
  }
};

const openAgentSession = async (sessionId: string, agentId = '') => {
  if (!sessionId) return;
  const perfTrace = startMessengerPerfTrace('openAgentSession', { sessionId, agentId });
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = false;
  const knownSession = chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
  const fallbackAgentId = agentId
    ? normalizeAgentId(agentId)
    : normalizeAgentId(knownSession?.agent_id ?? chatStore.draftAgentId);
  clearAgentConversationDismissed(fallbackAgentId);
  selectedAgentId.value = fallbackAgentId || DEFAULT_AGENT_KEY;
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: sessionId,
    agentId: fallbackAgentId || DEFAULT_AGENT_KEY
  });
  const nextQuery = {
    ...route.query,
    section: 'messages',
    session_id: sessionId,
    agent_id: fallbackAgentId === DEFAULT_AGENT_KEY ? '' : fallbackAgentId
  } as Record<string, any>;
  delete nextQuery.conversation_id;
  router.replace({
    path: `${basePrefix.value}/chat`,
    query: nextQuery
  }).catch(() => undefined);
  await scrollMessagesToBottom(true);
  markMessengerPerfTrace(perfTrace, 'uiReady');
  try {
    markMessengerPerfTrace(perfTrace, 'beforeLoadSessionDetail');
    await chatStore.loadSessionDetail(sessionId);
    markMessengerPerfTrace(perfTrace, 'afterLoadSessionDetail');
    const session = chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
    const targetAgentId = normalizeAgentId(session?.agent_id ?? fallbackAgentId);
    selectedAgentId.value = targetAgentId || DEFAULT_AGENT_KEY;
    sessionHub.setActiveConversation({
      kind: 'agent',
      id: sessionId,
      agentId: targetAgentId || DEFAULT_AGENT_KEY
    });
    finishMessengerPerfTrace(perfTrace, 'ok');
  } catch (error) {
    finishMessengerPerfTrace(perfTrace, 'fail', {
      error: (error as { message?: string })?.message || String(error)
    });
    showApiError(error, t('messenger.error.openConversation'));
  }
};

const restoreTimelineSession = async (sessionId: string) => {
  if (!sessionId) return;
  await openAgentSession(sessionId);
};

const setTimelineSessionMain = async (sessionId: string) => {
  if (!sessionId) return;
  try {
    await chatStore.setMainSession(sessionId);
    ElMessage.success(t('chat.history.setMainSuccess'));
  } catch (error) {
    showApiError(error, t('chat.history.setMainFailed'));
  }
};

const deleteTimelineSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  try {
    await ElMessageBox.confirm(t('chat.history.confirmDelete'), t('chat.history.confirmTitle'), {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
  } catch {
    return;
  }
  try {
    await chatStore.deleteSession(targetId);
    timelinePreviewMap.value.delete(targetId);
    ElMessage.success(t('chat.history.delete'));
  } catch (error) {
    showApiError(error, t('chat.sessions.deleteFailed'));
  }
};

const selectContainer = (containerId: number | 'user') => {
  if (containerId === 'user') {
    fileScope.value = 'user';
    selectedFileContainerId.value = USER_CONTAINER_ID;
    fileContainerLatestUpdatedAt.value = 0;
    fileContainerEntryCount.value = 0;
    sessionHub.setSection('files');
    return;
  }
  const parsed = Math.min(10, Math.max(1, Number(containerId) || 1));
  const target = agentFileContainers.value.find((item) => item.id === parsed);
  if (!target) {
    ElMessage.warning(t('messenger.files.agentContainerEmpty'));
    return;
  }
  fileScope.value = 'agent';
  selectedFileContainerId.value = parsed;
  fileContainerLatestUpdatedAt.value = 0;
  fileContainerEntryCount.value = 0;
  sessionHub.setSection('files');
};

const handleFileWorkspaceStats = (payload: unknown) => {
  const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  fileContainerEntryCount.value = Math.max(0, Number(source.entryCount || 0));
  fileContainerLatestUpdatedAt.value = normalizeTimestamp(source.latestUpdatedAt);
  fileLifecycleNowTick.value = Date.now();
};

const normalizeToolEntry = (item: unknown): ToolEntry | null => {
  if (!item) return null;
  if (typeof item === 'string') {
    const name = item.trim();
    if (!name) return null;
    return { name, description: '', ownerId: '', source: {} };
  }
  const source = item as Record<string, unknown>;
  const name = String(source.name || source.tool_name || source.id || '').trim();
  if (!name) return null;
  return {
    name,
    description: String(source.description || '').trim(),
    ownerId: String(source.owner_id || source.ownerId || '').trim(),
    source
  };
};

const loadToolsCatalog = async () => {
  toolsCatalogLoading.value = true;
  try {
    const { data } = await fetchUserToolsCatalog();
    const payload = (data?.data || {}) as Record<string, unknown>;
    builtinTools.value = (Array.isArray(payload.builtin_tools) ? payload.builtin_tools : [])
      .map((item) => normalizeToolEntry(item))
      .filter(Boolean) as ToolEntry[];
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
    if (!selectedToolCategory.value && !selectedCustomToolName.value) {
      if (customTools.value.length > 0) {
        selectedCustomToolName.value = customTools.value[0].name;
      } else if (sharedTools.value.length > 0) {
        selectedToolCategory.value = 'shared';
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

const loadOrgUnits = async () => {
  try {
    const { data } = await fetchOrgUnits();
    const sourceTree = Array.isArray(data?.data?.tree) ? data.data.tree : [];
    const sourceItems = Array.isArray(data?.data?.items)
      ? data.data.items
      : Array.isArray(data?.data)
        ? data.data
        : sourceTree;
    const normalized = sourceItems
      .map((item) => normalizeUnitNode(item))
      .filter((item): item is UnitTreeNode => Boolean(item));
    const flatNodes = flattenUnitNodes(normalized);
    const tree = buildUnitTreeFromFlat(flatNodes);
    const nextMap: Record<string, string> = {};
    const allNodeIds = new Set<string>();
    const rootIds = new Set<string>();
    const walk = (nodes: UnitTreeNode[]) => {
      nodes.forEach((node) => {
        nextMap[node.id] = node.label;
        allNodeIds.add(node.id);
        if (node.children.length) {
          walk(node.children);
        }
      });
    };
    tree.forEach((node) => {
      rootIds.add(node.id);
    });
    walk(tree);
    const retainedExpanded = new Set<string>();
    contactUnitExpandedIds.value.forEach((unitId) => {
      if (allNodeIds.has(unitId)) {
        retainedExpanded.add(unitId);
      }
    });
    orgUnitPathMap.value = nextMap;
    orgUnitTree.value = tree;
    contactUnitExpandedIds.value = retainedExpanded.size > 0 ? retainedExpanded : rootIds;
  } catch {
    orgUnitPathMap.value = {};
    orgUnitTree.value = [];
    contactUnitExpandedIds.value = new Set();
  }
};

const selectToolCategory = (category: 'builtin' | 'mcp' | 'skills' | 'knowledge' | 'shared') => {
  toolPaneStatus.value = '';
  selectedToolCategory.value = category;
  selectedCustomToolName.value = '';
};

const selectCustomTool = (toolName: string) => {
  toolPaneStatus.value = '';
  selectedCustomToolName.value = String(toolName || '').trim();
  selectedToolCategory.value = '';
};

const toolCategoryLabel = (category: string) => {
  if (category === 'builtin') return t('toolManager.system.builtin');
  if (category === 'mcp') return t('toolManager.system.mcp');
  if (category === 'skills') return t('toolManager.system.skills');
  if (category === 'knowledge') return t('toolManager.system.knowledge');
  if (category === 'shared') return t('messenger.tools.sharedTitle');
  return category;
};

const handleAgentSettingsSaved = async () => {
  const tasks: Promise<unknown>[] = [agentStore.loadAgents(), loadRunningAgents()];
  if (canManageAgentIntegrations.value) {
    tasks.push(loadCronAgentIds());
  } else {
    cronAgentIds.value = new Set<string>();
  }
  await Promise.allSettled(tasks);
};

const handleAgentDeleted = async () => {
  selectedAgentId.value = DEFAULT_AGENT_KEY;
  const tasks: Promise<unknown>[] = [chatStore.loadSessions(), loadRunningAgents()];
  if (canManageAgentIntegrations.value) {
    tasks.push(loadCronAgentIds());
  } else {
    cronAgentIds.value = new Set<string>();
  }
  await Promise.allSettled(tasks);
};

const ensureSectionSelection = () => {
  if (sessionHub.activeSection === 'agents') {
    if (!selectedAgentId.value) {
      selectedAgentId.value = DEFAULT_AGENT_KEY;
    }
    return;
  }

  if (sessionHub.activeSection === 'users') {
    const exists = filteredContacts.value.some(
      (item) => String(item?.user_id || '') === selectedContactUserId.value
    );
    if (!exists) {
      selectedContactUserId.value = String(filteredContacts.value[0]?.user_id || '');
    }
    if (!selectedContactUserId.value && filteredContacts.value.length > 0) {
      selectedContactUserId.value = String(filteredContacts.value[0]?.user_id || '');
    }
    return;
  }

  if (sessionHub.activeSection === 'groups') {
    if (!selectedGroupId.value && filteredGroups.value.length > 0) {
      selectedGroupId.value = String(filteredGroups.value[0]?.group_id || '');
    }
    return;
  }

  if (sessionHub.activeSection === 'tools') {
    if (!selectedToolEntryKey.value) {
      if (customTools.value.length > 0) {
        selectedCustomToolName.value = customTools.value[0].name;
      } else if (sharedTools.value.length > 0) {
        selectedToolCategory.value = 'shared';
      } else {
        selectedToolCategory.value = 'mcp';
      }
    }
    return;
  }

  if (sessionHub.activeSection === 'files') {
    if (fileScope.value === 'user') {
      selectedFileContainerId.value = USER_CONTAINER_ID;
      return;
    }
    const exists = agentFileContainers.value.some((item) => item.id === selectedFileContainerId.value);
    if (!exists) {
      const fallbackId = agentFileContainers.value[0]?.id ?? USER_CONTAINER_ID;
      selectedFileContainerId.value = fallbackId;
      if (fallbackId === USER_CONTAINER_ID && !agentFileContainers.value.length) {
        fileScope.value = 'user';
      }
    }
    return;
  }
};

const syncAgentConversationFallback = () => {
  if (sessionHub.activeSection !== 'messages') return;
  if (sessionHub.activeConversation) return;
  const routeConversationId = String(route.query?.conversation_id || '').trim();
  if (routeConversationId || String(userWorldStore.activeConversationId || '').trim()) return;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (sessionId) {
    const session = chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
    sessionHub.setActiveConversation({
      kind: 'agent',
      id: sessionId,
      agentId: normalizeAgentId(session?.agent_id ?? chatStore.draftAgentId)
    });
    return;
  }
  if (!String(chatStore.draftAgentId || '').trim() && !chatStore.messages.length) {
    return;
  }
  const draftAgent = normalizeAgentId(chatStore.draftAgentId || selectedAgentId.value);
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: `draft:${draftAgent}`,
    agentId: draftAgent
  });
};

const parseAgentLocalCommand = (value: unknown): AgentLocalCommand | '' => {
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

const resolveCommandErrorMessage = (error: unknown): string =>
  String((error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail || (error as { message?: string })?.message || t('common.requestFailed')).trim();

const appendAgentLocalCommandMessages = (commandText: string, replyText: string) => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  chatStore.appendLocalMessage('user', commandText, { sessionId });
  chatStore.appendLocalMessage('assistant', replyText, { sessionId });
};

const handleAgentLocalCommand = async (command: AgentLocalCommand, rawText: string) => {
  if (command === 'help') {
    appendAgentLocalCommandMessages(rawText, t('chat.command.help'));
    await scrollMessagesToBottom();
    return;
  }

  if (command === 'new') {
    try {
      const payloadAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
      const created = await chatStore.createSession(
        payloadAgentId === DEFAULT_AGENT_KEY ? {} : { agent_id: payloadAgentId }
      );
      sessionHub.setActiveConversation({
        kind: 'agent',
        id: String(created?.id || chatStore.activeSessionId || ''),
        agentId: normalizeAgentId(created?.agent_id || payloadAgentId)
      });
      appendAgentLocalCommandMessages(rawText, t('chat.command.newSuccess'));
    } catch (error) {
      appendAgentLocalCommandMessages(
        rawText,
        t('chat.command.newFailed', { message: resolveCommandErrorMessage(error) })
      );
    }
    await scrollMessagesToBottom();
    return;
  }

  if (command === 'stop') {
    const sessionId = String(chatStore.activeSessionId || '').trim();
    if (!sessionId) {
      appendAgentLocalCommandMessages(rawText, t('chat.command.stopNoSession'));
      await scrollMessagesToBottom();
      return;
    }
    const cancelled = await chatStore.stopStream();
    appendAgentLocalCommandMessages(
      rawText,
      cancelled ? t('chat.command.stopRequested') : t('chat.command.stopNoRunning')
    );
    await scrollMessagesToBottom();
    return;
  }

  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) {
    appendAgentLocalCommandMessages(rawText, t('chat.command.compactMissingSession'));
    await scrollMessagesToBottom();
    return;
  }
  try {
    await chatStore.compactSession(sessionId);
    appendAgentLocalCommandMessages(rawText, t('chat.command.compactSuccess'));
  } catch (error) {
    appendAgentLocalCommandMessages(
      rawText,
      t('chat.command.compactFailed', { message: resolveCommandErrorMessage(error) })
    );
  }
  await scrollMessagesToBottom();
};

const sendAgentMessage = async (payload: { content?: string; attachments?: unknown[] }) => {
  const content = String(payload?.content || '').trim();
  const attachments = Array.isArray(payload?.attachments) ? payload.attachments : [];
  if (!content && attachments.length === 0) return;
  const localCommand = parseAgentLocalCommand(content);
  if (localCommand) {
    if (attachments.length > 0) {
      appendAgentLocalCommandMessages(content, t('chat.command.attachmentsUnsupported'));
      await scrollMessagesToBottom();
      return;
    }
    await handleAgentLocalCommand(localCommand, content);
    return;
  }
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  setRuntimeStateOverride(targetAgentId, 'running', 30_000);
  try {
    await chatStore.sendMessage(content, { attachments });
    setRuntimeStateOverride(targetAgentId, 'done', 8_000);
    if (chatStore.activeSessionId) {
      sessionHub.setActiveConversation({
        kind: 'agent',
        id: String(chatStore.activeSessionId),
        agentId: normalizeAgentId(chatStore.draftAgentId || activeAgentId.value)
      });
    }
    await scrollMessagesToBottom();
  } catch (error) {
    setRuntimeStateOverride(targetAgentId, 'error', 8_000);
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

const normalizeUploadPath = (value: unknown): string =>
  String(value || '')
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .trim();

const buildWorldAttachmentToken = (rawPath: unknown): string => {
  const normalized = normalizeUploadPath(rawPath);
  if (!normalized) return '';
  if (/\s/.test(normalized)) {
    if (!normalized.includes('"')) {
      return `@"${normalized}"`;
    }
    if (!normalized.includes("'")) {
      return `@'${normalized}'`;
    }
    return `@${encodeURIComponent(normalized)}`;
  }
  return `@${normalized}`;
};

const appendWorldAttachmentTokens = (paths: string[]) => {
  const tokens = paths.map((path) => buildWorldAttachmentToken(path)).filter(Boolean);
  if (!tokens.length) return;
  const prefix = worldDraft.value.trim() ? '\n' : '';
  worldDraft.value = `${worldDraft.value}${prefix}${tokens.join(' ')}`;
};

const triggerWorldUpload = () => {
  if (!isWorldConversationActive.value || worldUploading.value || !worldUploadInputRef.value) return;
  worldQuickPanelMode.value = '';
  worldUploadInputRef.value.value = '';
  worldUploadInputRef.value.click();
};

const handleWorldUploadInput = async (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  const files = target?.files ? Array.from(target.files) : [];
  if (!files.length) return;
  const oversized = files.find((file) => Number(file.size || 0) > WORLD_UPLOAD_SIZE_LIMIT);
  if (oversized) {
    ElMessage.warning(t('workspace.upload.tooLarge', { limit: '200 MB' }));
    if (target) target.value = '';
    return;
  }
  worldUploading.value = true;
  try {
    const formData = new FormData();
    formData.append('path', USER_WORLD_UPLOAD_BASE);
    formData.append('container_id', String(USER_CONTAINER_ID));
    files.forEach((file) => {
      formData.append('files', file as Blob);
    });
    const { data } = await uploadWunderWorkspace(formData);
    const uploaded = (Array.isArray(data?.files) ? data.files : [])
      .map((item) => normalizeUploadPath(item))
      .filter(Boolean);
    if (uploaded.length) {
      appendWorldAttachmentTokens(uploaded);
      emitWorkspaceRefresh({
        reason: 'messenger-world-upload',
        containerId: USER_CONTAINER_ID
      });
    }
    ElMessage.success(
      t('userWorld.attachments.uploadSuccess', { count: uploaded.length || files.length })
    );
  } catch (error) {
    showApiError(error, t('workspace.upload.failed'));
  } finally {
    worldUploading.value = false;
    if (target) {
      target.value = '';
    }
  }
};

const sendWorldMessage = async () => {
  if (!canSendWorldMessage.value) return;
  const text = worldDraft.value.trim();
  if (!text) return;
  worldQuickPanelMode.value = '';
  worldDraft.value = '';
  try {
    await userWorldStore.sendToActiveConversation(text);
    pushWorldMessageHistory(text);
    await scrollMessagesToBottom();
  } catch (error) {
    worldDraft.value = text;
    showApiError(error, t('userWorld.input.sendFailed'));
  }
};

const handleWorldComposerEnterKeydown = async (event: KeyboardEvent) => {
  if (messengerSendKey.value === 'ctrl_enter') {
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      await sendWorldMessage();
    }
    return;
  }
  if (event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) {
    return;
  }
  event.preventDefault();
  await sendWorldMessage();
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
  await scrollMessagesToBottom(true);
};

const toggleLanguage = () => {
  const next = getCurrentLanguage() === 'zh-CN' ? 'en-US' : 'zh-CN';
  setLanguage(next);
  ElMessage.success(t('messenger.more.languageChanged'));
};

const checkClientUpdate = () => {
  ElMessage.success(t('common.refreshSuccess'));
};

const updateDesktopToolCallMode = (value: DesktopToolCallMode) => {
  desktopToolCallMode.value = value === 'function_call' ? 'function_call' : 'tool_call';
};

const updateSendKey = (value: MessengerSendKeyMode) => {
  const normalized = normalizeMessengerSendKey(value);
  messengerSendKey.value = normalized;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(MESSENGER_SEND_KEY_STORAGE_KEY, normalized);
  }
};

const updateThemePalette = (value: 'hula-green' | 'eva-orange' | 'minimal') => {
  themeStore.setPalette(value);
};

const updateUiFontSize = (value: number) => {
  const normalized = normalizeUiFontSize(value);
  uiFontSize.value = normalized;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY, String(normalized));
  }
  applyUiFontSize(normalized);
};

const openDesktopTools = () => {
  if (!desktopMode.value) return;
  router.push('/desktop/tools').catch(() => undefined);
};

const openDesktopSystemSettings = () => {
  if (!desktopMode.value) return;
  router.push('/desktop/system').catch(() => undefined);
};

const openDebugTools = async () => {
  if (typeof window === 'undefined') return;
  try {
    const desktopApi = (window as any).wunderDesktop;
    if (typeof desktopApi?.toggleDevTools === 'function') {
      await desktopApi.toggleDevTools();
      return;
    }
  } catch {
    ElMessage.warning(t('desktop.common.saveFailed'));
    return;
  }
  ElMessage.info(t('messenger.settings.debugHint'));
};

const loadRunningAgents = async () => {
  try {
    const response = await listRunningAgents();
    const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
    const stateMap = new Map<string, AgentRuntimeState>();
    items.forEach((item: Record<string, unknown>) => {
      const key =
        normalizeAgentId(
          item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')
        ) || DEFAULT_AGENT_KEY;
      const state = normalizeRuntimeState(item?.state, item?.pending_question === true);
      stateMap.set(key, state);
    });
    agentRuntimeStateMap.value = stateMap;
  } catch {
    agentRuntimeStateMap.value = new Map<string, AgentRuntimeState>();
  }
};

const resolveHttpStatus = (error: unknown): number => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

const loadCronAgentIds = async () => {
  if (!canManageAgentIntegrations.value) {
    cronAgentIds.value = new Set<string>();
    return;
  }
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
  } catch (error) {
    const status = resolveHttpStatus(error);
    if (status === 401 || status === 403) {
      cronAgentIds.value = new Set<string>();
      return;
    }
    cronAgentIds.value = new Set<string>();
  }
};

const refreshAll = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(true),
    loadOrgUnits(),
    loadRunningAgents(),
    loadToolsCatalog()
  ];
  if (canManageAgentIntegrations.value) {
    tasks.push(loadCronAgentIds());
  } else {
    cronAgentIds.value = new Set<string>();
  }
  await Promise.allSettled(tasks);
  ensureSectionSelection();
  ElMessage.success(t('common.refreshSuccess'));
};

const updateMessageScrollState = () => {
  const container = messageListRef.value;
  if (!container || showChatSettingsView.value) {
    showScrollBottomButton.value = false;
    autoStickToBottom.value = true;
    return;
  }
  const remaining = container.scrollHeight - container.clientHeight - container.scrollTop;
  const shouldStick = remaining <= 72;
  autoStickToBottom.value = shouldStick;
  showScrollBottomButton.value =
    !shouldStick && (isAgentConversationActive.value || isWorldConversationActive.value);
};

const handleMessageListScroll = () => {
  updateMessageScrollState();
};

const scrollMessagesToBottom = async (force = false) => {
  await nextTick();
  const container = messageListRef.value;
  if (!container) return;
  if (!force && !autoStickToBottom.value) {
    updateMessageScrollState();
    return;
  }
  container.scrollTop = container.scrollHeight;
  updateMessageScrollState();
};

const jumpToMessageBottom = async () => {
  autoStickToBottom.value = true;
  await scrollMessagesToBottom(true);
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
    if (conversation) {
      const kind =
        String(conversation?.conversation_type || '').toLowerCase() === 'group' ? 'group' : 'direct';
      if (route.path.includes('/chat')) {
        await userWorldStore.setActiveConversation(queryConversationId);
        sessionHub.setActiveConversation({ kind, id: queryConversationId });
        await scrollMessagesToBottom(true);
      } else {
        await openWorldConversation(queryConversationId, kind);
      }
      return;
    }
    const nextQuery = { ...route.query } as Record<string, any>;
    delete nextQuery.conversation_id;
    router.replace({ path: route.path, query: nextQuery }).catch(() => undefined);
  }

  const querySessionId = String(query?.session_id || '').trim();
  if (querySessionId) {
    const session = chatStore.sessions.find((item) => String(item?.id || '') === querySessionId);
    if (session) {
      await openAgentSession(querySessionId, normalizeAgentId(session?.agent_id));
      return;
    }
    const nextQuery = { ...route.query } as Record<string, any>;
    delete nextQuery.session_id;
    router.replace({ path: route.path, query: nextQuery }).catch(() => undefined);
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
  try {
    await authStore.loadProfile();
  } catch {
    // keep bootstrap resilient when profile endpoint is temporarily unavailable
  }
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(),
    loadOrgUnits(),
    loadRunningAgents(),
    loadToolsCatalog()
  ];
  if (canManageAgentIntegrations.value) {
    tasks.push(loadCronAgentIds());
  } else {
    cronAgentIds.value = new Set<string>();
  }
  await Promise.allSettled(tasks);
  await restoreConversationFromRoute();
  ensureSectionSelection();
  bootLoading.value = false;
};

watch(
  () => isMiddlePaneOverlay.value,
  (overlay) => {
    if (!overlay) {
      clearMiddlePaneOverlayHide();
      middlePaneOverlayVisible.value = false;
    }
  },
  { immediate: true }
);

watch(
  () => showRightDock.value,
  (visible) => {
    if (!visible) {
      rightDockCollapsed.value = false;
    }
  }
);

watch(
  () => [route.path, route.query.section],
  () => {
    settingsPanelMode.value = route.path.includes('/profile') ? 'profile' : 'general';
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
  () => currentUserId.value,
  () => {
    ensureDismissedAgentConversationState(true);
  },
  { immediate: true }
);

watch(
  () => canManageAgentIntegrations.value,
  (enabled) => {
    if (!enabled) {
      cronAgentIds.value = new Set<string>();
      if (agentSettingMode.value !== 'agent') {
        agentSettingMode.value = 'agent';
      }
    }
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
  () => [
    sessionHub.activeSection,
    sessionHub.activeConversationKey,
    chatStore.activeSessionId,
    chatStore.draftAgentId,
    route.query?.conversation_id
  ],
  () => {
    syncAgentConversationFallback();
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
    if (!value || sessionHub.activeSection !== 'messages') return;
    if (activeConversation.value?.kind === 'direct' || activeConversation.value?.kind === 'group') return;
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
  () => currentContainerId.value,
  (value) => {
    if (fileScope.value !== 'agent') return;
    if (sessionHub.activeSection === 'files') return;
    selectedFileContainerId.value = value;
  },
  { immediate: true }
);

watch(
  () => rightPanelSessionHistory.value.map((item) => item.id).join('|'),
  (value) => {
    if (!value) return;
    if (typeof window !== 'undefined' && timelinePrefetchTimer) {
      window.clearTimeout(timelinePrefetchTimer);
      timelinePrefetchTimer = null;
    }
    const prefetchTargets = rightPanelSessionHistory.value.slice(0, 4).map((item) => item.id);
    const runPrefetch = () => {
      prefetchTargets.forEach((sessionId) => {
        void preloadTimelinePreview(sessionId);
      });
    };
    if (typeof window !== 'undefined') {
      timelinePrefetchTimer = window.setTimeout(() => {
        timelinePrefetchTimer = null;
        runPrefetch();
      }, 80);
      return;
    }
    runPrefetch();
  },
  { immediate: true }
);

watch(
  () => [chatStore.activeSessionId, chatStore.messages.length],
  () => {
    const sessionId = String(chatStore.activeSessionId || '').trim();
    if (!sessionId || !Array.isArray(chatStore.messages) || !chatStore.messages.length) return;
    const preview = extractLatestUserPreview(chatStore.messages as unknown[]);
    if (preview) {
      timelinePreviewMap.value.set(sessionId, preview);
    }
  }
);

watch(
  () => desktopToolCallMode.value,
  (value) => {
    if (!desktopMode.value) return;
    setDesktopToolCallMode(value);
  }
);

watch(
  () => showChatSettingsView.value,
  () => {
    updateMessageScrollState();
  }
);

watch(
  () => [chatStore.messages.length, userWorldStore.activeMessages.length, sessionHub.activeConversationKey],
  () => {
    if (autoStickToBottom.value) {
      scrollMessagesToBottom();
    } else {
      updateMessageScrollState();
    }
  }
);

watch(
  () => [fileScope.value, selectedFileContainerId.value, selectedFileAgentIdForApi.value],
  () => {
    fileContainerLatestUpdatedAt.value = 0;
    fileContainerEntryCount.value = 0;
    fileLifecycleNowTick.value = Date.now();
  }
);

watch(
  () => isWorldConversationActive.value,
  (active) => {
    if (!active) {
      clearWorldQuickPanelClose();
      worldQuickPanelMode.value = '';
    }
  }
);

watch(
  () => activeWorldConversationId.value,
  () => {
    clearWorldQuickPanelClose();
    worldQuickPanelMode.value = '';
  }
);

onMounted(async () => {
  if (typeof window !== 'undefined') {
    viewportResizeHandler = () => {
      viewportWidth.value = window.innerWidth;
    };
    viewportResizeHandler();
    window.addEventListener('resize', viewportResizeHandler);
    messengerSendKey.value = normalizeMessengerSendKey(
      window.localStorage.getItem(MESSENGER_SEND_KEY_STORAGE_KEY)
    );
    uiFontSize.value = normalizeUiFontSize(window.localStorage.getItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY));
    worldComposerHeight.value = clampWorldComposerHeight(
      window.localStorage.getItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY)
    );
    worldRecentEmojis.value = loadStoredStringArray(WORLD_QUICK_EMOJI_STORAGE_KEY, 12);
    worldHistoryMap.value = loadWorldHistoryMap();
    window.addEventListener('pointerdown', closeWorldQuickPanelWhenOutside);
  }
  applyUiFontSize(uiFontSize.value);
  await bootstrap();
  updateMessageScrollState();
  lifecycleTimer = window.setInterval(() => {
    fileLifecycleNowTick.value = Date.now();
  }, 60_000);
  statusTimer = window.setInterval(() => {
    loadRunningAgents();
    if (canManageAgentIntegrations.value) {
      loadCronAgentIds();
    }
  }, 12000);
});

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') {
    if (viewportResizeHandler) {
      window.removeEventListener('resize', viewportResizeHandler);
      viewportResizeHandler = null;
    }
    window.removeEventListener('pointerdown', closeWorldQuickPanelWhenOutside);
  }
  clearWorldQuickPanelClose();
  clearMiddlePaneOverlayHide();
  stopWorldComposerResize();
  if (statusTimer) {
    window.clearInterval(statusTimer);
    statusTimer = null;
  }
  if (lifecycleTimer) {
    window.clearInterval(lifecycleTimer);
    lifecycleTimer = null;
  }
  if (typeof window !== 'undefined' && timelinePrefetchTimer) {
    window.clearTimeout(timelinePrefetchTimer);
    timelinePrefetchTimer = null;
  }
  markdownCache.clear();
  timelinePreviewMap.value.clear();
  timelinePreviewLoadingSet.value.clear();
  userWorldStore.stopAllWatchers();
});
</script>
