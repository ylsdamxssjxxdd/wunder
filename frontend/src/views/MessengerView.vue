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
      <button class="messenger-avatar-btn messenger-avatar-btn--profile" :style="currentUserAvatarStyle" type="button" @click="openProfilePage">
        <img
          v-if="currentUserAvatarImageUrl"
          class="messenger-settings-profile-avatar-image"
          :src="currentUserAvatarImageUrl"
          alt=""
        />
        <span v-else class="messenger-avatar-text">{{ avatarLabel(currentUsername) }}</span>
      </button>
      <div class="messenger-left-rail-divider messenger-left-rail-divider--profile" aria-hidden="true"></div>
      <div class="messenger-left-nav">
        <div class="messenger-left-nav-group">
          <button
            v-for="item in leftRailMainSectionOptions"
            :key="item.key"
            class="messenger-left-nav-btn"
            :class="{ active: isLeftNavSectionActive(item.key) }"
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
        <div
          v-if="leftRailSocialSectionOptions.length"
          class="messenger-left-rail-divider messenger-left-rail-divider--section"
          aria-hidden="true"
        ></div>
        <div v-if="leftRailSocialSectionOptions.length" class="messenger-left-nav-group">
          <button
            v-for="item in leftRailSocialSectionOptions"
            :key="item.key"
            class="messenger-left-nav-btn"
            :class="{ active: isLeftNavSectionActive(item.key) }"
            type="button"
            :title="item.label"
            :aria-label="item.label"
            @mouseenter="openMiddlePaneOverlay"
            @focus="openMiddlePaneOverlay"
            @click="switchSection(item.key)"
          >
            <i :class="item.icon" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-left-nav-btn messenger-left-nav-btn--helper"
            :class="{ active: showHelperAppsWorkspace }"
            type="button"
            :title="t('userWorld.helperApps.title')"
            :aria-label="t('userWorld.helperApps.title')"
            @mouseenter="openMiddlePaneOverlay"
            @focus="openMiddlePaneOverlay"
            @click="openHelperAppsDialog"
          >
            <i class="fa-solid fa-toolbox" aria-hidden="true"></i>
          </button>
        </div>
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

    <Transition :name="middlePaneTransitionName">
      <section
        v-show="showMiddlePane"
        ref="middlePaneRef"
        class="messenger-middle-pane messenger-middle-pane--overlay"
        @mouseenter="cancelMiddlePaneOverlayHide"
        @mouseleave="scheduleMiddlePaneOverlayHide"
      >
        <MessengerMiddlePane
          v-model:keyword="keywordInput"
          v-model:selected-contact-unit-id="selectedContactUnitId"
          v-model:settings-panel-mode="settingsPanelMode"
          :active-section="sessionHub.activeSection"
          :active-section-title="activeSectionTitle"
          :active-section-subtitle="activeSectionSubtitle"
          :show-helper-apps-workspace="showHelperAppsWorkspace"
          :search-placeholder="searchPlaceholder"
          :agent-overview-mode="agentOverviewMode"
          :user-world-permission-denied="userWorldPermissionDenied"
          :handle-search-create-action="handleSearchCreateAction"
          :toggle-agent-overview-mode="toggleAgentOverviewMode"
          :helper-apps-offline-items="helperAppsOfflineItems"
          :helper-apps-online-items="helperAppsOnlineItems"
          :helper-apps-online-loading="helperAppsOnlineLoading"
          :is-helper-app-active="isHelperAppActive"
          :select-helper-app="selectHelperApp"
          :resolve-external-icon="resolveExternalIcon"
          :resolve-external-icon-style="resolveExternalIconStyle"
          :resolve-external-host="resolveExternalHost"
          :filtered-mixed-conversations="filteredMixedConversations"
          :is-mixed-conversation-active="isMixedConversationActive"
          :open-mixed-conversation="openMixedConversation"
          :resolve-agent-runtime-state="resolveAgentRuntimeState"
          :avatar-label="avatarLabel"
          :format-time="formatTime"
          :can-delete-mixed-conversation="canDeleteMixedConversation"
          :delete-mixed-conversation="deleteMixedConversation"
          :contact-total-count="contactTotalCount"
          :contact-unit-tree-rows="contactUnitTreeRows"
          :resolve-unit-tree-row-style="resolveUnitTreeRowStyle"
          :toggle-contact-unit-expanded="toggleContactUnitExpanded"
          :filtered-contacts="filteredContacts"
          :set-contact-virtual-list-ref="setContactVirtualListRef"
          :handle-contact-virtual-scroll="handleContactVirtualScroll"
          :contact-virtual-top-padding="contactVirtualTopPadding"
          :contact-virtual-bottom-padding="contactVirtualBottomPadding"
          :visible-filtered-contacts="visibleFilteredContacts"
          :selected-contact-user-id="selectedContactUserId"
          :select-contact="selectContact"
          :open-contact-conversation-from-list="openContactConversationFromList"
          :is-contact-online="isContactOnline"
          :format-contact-presence="formatContactPresence"
          :resolve-unread="resolveUnread"
          :filtered-beeroom-groups="filteredBeeroomGroups"
          :selected-beeroom-group-id="beeroomStore.activeGroupId"
          :select-beeroom-group="selectBeeroomGroup"
          :filtered-groups="filteredGroups"
          :selected-group-id="selectedGroupId"
          :select-group="selectGroup"
          :filtered-owned-agents="filteredOwnedAgents"
          :filtered-shared-agents="filteredSharedAgents"
          :selected-agent-id="selectedAgentId"
          :default-agent-key="DEFAULT_AGENT_KEY"
          :select-agent-for-settings="selectAgentForSettings"
          :open-agent-by-id="openAgentById"
          :normalize-agent-id="normalizeAgentId"
          :selected-tool-entry-key="selectedToolEntryKey"
          :select-tool-category="selectToolCategory"
          :desktop-local-mode="desktopLocalMode"
          :file-scope="fileScope"
          :selected-file-container-id="selectedFileContainerId"
          :user-container-id="USER_CONTAINER_ID"
          :select-container="selectContainer"
          :open-file-container-menu="openFileContainerMenu"
          :bound-agent-file-containers="boundAgentFileContainers"
          :unbound-agent-file-containers="unboundAgentFileContainers"
          :desktop-mode="desktopMode"
          :current-username="currentUsername"
          :settings-logout-disabled="settingsLogoutDisabled"
          :handle-settings-logout="handleSettingsLogout"
        />
      </section>
    </Transition>

    <section class="messenger-chat chat-shell">
      <header class="messenger-chat-header">
        <div class="messenger-chat-heading">
          <div class="messenger-chat-title-row">
            <div class="messenger-chat-title">{{ chatPanelTitle }}</div>
          </div>
          <div class="messenger-chat-subtitle">{{ chatPanelSubtitle }}</div>
        </div>
        <div class="messenger-chat-header-actions">
          <div v-if="sessionHub.activeSection === 'swarms'" class="messenger-inline-actions messenger-chat-header-toggle">
            <button
              class="messenger-inline-btn"
              :class="{ active: beeroomWorkbenchMode === 'text' }"
              type="button"
              @click="beeroomWorkbenchMode = 'text'"
            >
              {{ t('beeroom.view.text') }}
            </button>
            <button
              class="messenger-inline-btn"
              :class="{ active: beeroomWorkbenchMode === 'canvas' }"
              type="button"
              @click="beeroomWorkbenchMode = 'canvas'"
            >
              {{ t('beeroom.view.canvas') }}
            </button>
          </div>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'agents' && !showAgentGridOverview"
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
            v-if="showChatSettingsView && sessionHub.activeSection === 'groups' && selectedGroup && !showHelperAppsWorkspace"
            class="messenger-header-action-text"
            type="button"
            @click="openSelectedGroupConversation"
          >
            {{ t('messenger.action.openConversation') }}
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn messenger-header-btn--text"
            type="button"
            :title="t('chat.newSession')"
            :aria-label="t('chat.newSession')"
            @click="startNewDraftSession"
          >
            <i class="fa-solid fa-plus" aria-hidden="true"></i>
            {{ t('chat.newSession') }}
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
          'is-settings': showChatSettingsView && !showHelperAppsWorkspace,
          'is-messages': !showChatSettingsView && !showHelperAppsWorkspace,
          'is-helper-workspace': showHelperAppsWorkspace,
          'is-beeroom': sessionHub.activeSection === 'swarms',
          'is-agent': isAgentConversationActive,
          'is-world': isWorldConversationActive
        }"
        @scroll="handleMessageListScroll"
        @click="handleMessageContentClick"
      >
        <template v-if="showHelperAppsWorkspace">
          <div class="messenger-helper-workspace">
            <div class="messenger-helper-body">
              <MessengerLocalFileSearchPanel
                v-if="helperAppsActiveKind === 'offline' && helperAppsActiveKey === 'local-file-search'"
              />
              <div
                v-else-if="helperAppsActiveKind === 'online' && helperAppsActiveExternalItem"
                class="messenger-helper-external-panel"
              >
                <iframe
                  :src="helperAppsActiveExternalItem.url"
                  class="messenger-helper-external-frame"
                  referrerpolicy="no-referrer"
                ></iframe>
              </div>
              <div
                v-else-if="helperAppsActiveKind === 'online' && helperAppsOnlineLoading"
                class="messenger-helper-empty"
              >
                {{ t('common.loading') }}
              </div>
              <div
                v-else-if="helperAppsActiveKind === 'online' && !helperAppsOnlineItems.length"
                class="messenger-helper-empty"
              >
                {{ t('userWorld.helperApps.onlineEmpty') }}
              </div>
              <div v-else class="messenger-helper-empty">
                {{ t('userWorld.helperApps.selectHint') }}
              </div>
            </div>
          </div>
        </template>

        <template v-else-if="showChatSettingsView">
          <div
            class="messenger-chat-settings"
            :class="{ 'messenger-chat-settings--beeroom': sessionHub.activeSection === 'swarms' }"
          >
            <template v-if="showAgentSettingsPanel">
              <template v-if="showAgentGridOverview">
                <div class="messenger-chat-settings-block messenger-agent-grid-panel">
                  <div class="messenger-agent-grid-header">
                  </div>
                  <div v-if="!agentOverviewCards.length" class="messenger-list-empty">
                    {{ t('messenger.agent.overviewEmpty') }}
                  </div>
                  <div v-else class="messenger-agent-grid">
                    <article
                      v-for="card in agentOverviewCards"
                      :key="`agent-overview-${card.id}`"
                      class="messenger-agent-grid-card"
                      :class="{ active: selectedAgentId === card.id }"
                      role="button"
                      tabindex="0"
                      @click="selectAgentForSettings(card.id)"
                      @keydown.enter.prevent="selectAgentForSettings(card.id)"
                      @keydown.space.prevent="selectAgentForSettings(card.id)"
                    >
                      <div class="messenger-agent-grid-card-head">
                        <AgentAvatar size="md" :state="card.runtimeState" />
                        <div class="messenger-agent-grid-main">
                          <div class="messenger-agent-grid-name">{{ card.name }}</div>
                        </div>
                      </div>
                      <p class="messenger-agent-grid-desc">
                        {{ card.description || t('messenger.preview.empty') }}
                      </p>
                      <div class="messenger-agent-grid-actions">
                        <button
                          class="messenger-inline-btn"
                          type="button"
                          @click.stop="selectAgentForSettings(card.id)"
                        >
                          {{ t('messenger.agent.openSettings') }}
                        </button>
                        <button
                          class="messenger-inline-btn"
                          type="button"
                          @click.stop="openAgentById(card.id)"
                        >
                          {{ t('messenger.agent.openChat') }}
                        </button>
                      </div>
                      <div class="messenger-agent-grid-foot">
                        <span
                          class="messenger-agent-grid-rounds"
                          :title="t('messenger.agent.userRoundsLabel')"
                        >
                          <i class="fa-solid fa-user" aria-hidden="true"></i>
                          <span>{{ formatUserRounds(card.userRounds) }}</span>
                        </span>
                        <span class="messenger-agent-grid-foot-icons">
                          <i
                            v-if="card.hasCron"
                            class="fa-solid fa-clock"
                            :title="t('messenger.agent.cron')"
                            :aria-label="t('messenger.agent.cron')"
                          ></i>
                          <i
                            v-if="card.hasChannelBinding"
                            class="fa-solid fa-comments"
                            :title="t('messenger.agent.channelTag')"
                            :aria-label="t('messenger.agent.channelTag')"
                          ></i>
                        </span>
                        <span class="messenger-agent-grid-container-id">
                          <i class="fa-solid fa-box" aria-hidden="true"></i>
                          #{{ card.containerId }}
                        </span>
                      </div>
                    </article>
                  </div>
                </div>
              </template>
              <template v-else>
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
                    class="messenger-inline-btn"
                    :class="{ active: agentSettingMode === 'runtime' }"
                    type="button"
                    @click="agentSettingMode = 'runtime'"
                  >
                    {{ t('chat.features.runtimeRecords') }}
                  </button>
                </div>

                <div v-if="agentSettingMode === 'agent'" class="messenger-chat-settings-block">
                  <AgentSettingsPanel
                    :agent-id="settingsAgentIdForPanel"
                    :readonly="isSettingsDefaultAgentReadonly"
                    @saved="handleAgentSettingsSaved"
                    @deleted="handleAgentDeleted"
                  />
                </div>

                <div v-else-if="agentSettingMode === 'cron'" class="messenger-chat-settings-block">
                  <AgentCronPanel :agent-id="settingsAgentIdForApi" @changed="handleCronPanelChanged" />
                </div>

                <div
                  v-else-if="agentSettingMode === 'channel'"
                  class="messenger-chat-settings-block messenger-channel-panel-wrap"
                >
                  <UserChannelSettingsPanel
                    mode="page"
                    :agent-id="settingsAgentIdForApi"
                    @changed="loadChannelBoundAgentIds"
                  />
                </div>
                <div v-else-if="agentSettingMode === 'runtime'" class="messenger-chat-settings-block">
                  <AgentRuntimeRecordsPanel :agent-id="settingsRuntimeAgentIdForApi" />
                </div>

              </template>
            </template>

            <template v-else-if="sessionHub.activeSection === 'swarms'">
              <BeeroomWorkbench
                :group="selectedBeeroomGroup"
                :agents="beeroomStore.activeAgents"
                :missions="beeroomStore.activeMissions"
                :available-agents="beeroomCandidateAgents"
                :loading="beeroomStore.detailLoading || beeroomStore.loading"
                :refreshing="beeroomStore.refreshing"
                :error="beeroomStore.error"
                :view-mode="beeroomWorkbenchMode"
                @refresh="refreshActiveBeeroom"
                @move-agents="handleBeeroomMoveAgents"
                @open-agent="openAgentById"
              />
            </template>

            <template v-else-if="sessionHub.activeSection === 'users'">
              <div v-if="selectedContact" class="messenger-entity-panel messenger-entity-panel--fill">
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
                    <span class="messenger-entity-value">
                      {{ formatContactPresence(selectedContact) }}
                      <template v-if="selectedContact.status">
                        · {{ selectedContact.status }}
                      </template>
                    </span>
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
              <template v-else-if="selectedToolCategory === 'admin'">
                <div class="messenger-entity-panel messenger-entity-panel--fill">
                  <div class="messenger-entity-title">{{ t('messenger.tools.adminTitle') }}</div>
                  <div class="messenger-entity-meta">{{ t('messenger.tools.adminDesc') }}</div>
                  <div class="messenger-tools-admin-groups">
                    <section
                      v-for="group in adminToolGroups"
                      :key="`admin-tool-group-${group.key}`"
                      class="messenger-tools-admin-group"
                    >
                      <div class="messenger-entity-meta messenger-tools-admin-group-title">
                        {{ group.title }}
                      </div>
                      <div class="messenger-tool-tag-list">
                        <template v-for="item in group.items" :key="`tool-admin-${group.key}-${item.name}`">
                          <el-tooltip
                            placement="top-start"
                            :show-after="120"
                            :content="resolveAdminToolDetail(item)"
                            popper-class="messenger-tool-detail-popper"
                          >
                            <span class="messenger-tool-tag messenger-tool-tag--detail">
                              {{ item.name }}
                            </span>
                          </el-tooltip>
                        </template>
                        <span v-if="!group.items.length" class="messenger-list-empty">
                          {{ t('common.none') }}
                        </span>
                      </div>
                    </section>
                  </div>
                </div>
              </template>
              <template
                v-else-if="
                  selectedToolCategory === 'mcp' ||
                  selectedToolCategory === 'skills' ||
                  selectedToolCategory === 'knowledge' ||
                  (selectedToolCategory === 'shared' && !desktopLocalMode)
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
                  <UserSharedToolsPanel v-if="!desktopLocalMode" v-show="selectedToolCategory === 'shared'" />
                </div>
              </template>
              <div v-else class="messenger-list-empty">{{ t('messenger.empty.selectTool') }}</div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'files'">
              <div class="messenger-files-panel">
                <div class="messenger-entity-panel">
                  <div class="messenger-entity-head">
                    <div class="messenger-entity-title">{{ t('messenger.files.title') }}</div>
                    <button
                      v-if="desktopMode"
                      class="messenger-inline-btn"
                      type="button"
                      @click="
                        openDesktopContainerSettings(
                          fileScope === 'user' ? USER_CONTAINER_ID : selectedFileContainerId
                        )
                      "
                    >
                      {{ t('desktop.containers.manage') }}
                    </button>
                  </div>
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
                <div
                  class="messenger-workspace-scope chat-shell messenger-files-workspace"
                >
                  <WorkspacePanel
                    :key="workspacePanelKey"
                    :agent-id="selectedFileAgentIdForApi"
                    :container-id="selectedFileContainerId"
                    :title="fileScope === 'user' ? t('messenger.files.userContainer') : t('messenger.files.title')"
                    :empty-text="fileScope === 'user' ? t('messenger.files.userEmpty') : t('workspace.empty')"
                    @stats="handleFileWorkspaceStats"
                  />
                </div>
                <DesktopContainerManagerPanel
                  v-if="desktopMode"
                  ref="desktopContainerManagerPanelRef"
                  :active-container-id="selectedFileContainerId"
                  @roots-change="handleDesktopContainerRootsChange"
                />
              </div>
            </template>

            <template v-else-if="sessionHub.activeSection === 'more'">
              <div v-if="settingsPanelMode === 'prompts'" class="messenger-chat-settings-block">
                <UserPromptSettingsPanel />
              </div>
              <DesktopSystemSettingsPanel
                v-else-if="
                  desktopMode &&
                  (settingsPanelMode === 'desktop-models' ||
                    settingsPanelMode === 'desktop-remote' ||
                    settingsPanelMode === 'desktop-lan')
                "
                :panel="
                  settingsPanelMode === 'desktop-remote'
                    ? 'remote'
                    : settingsPanelMode === 'desktop-lan'
                      ? 'lan'
                      : 'models'
                "
              />
              <MessengerSettingsPanel
                v-else
                :mode="generalSettingsPanelMode"
                :username="currentUsername"
                :user-id="currentUserId"
                :language-label="currentLanguageLabel"
                :send-key="messengerSendKey"
                :desktop-local-mode="desktopLocalMode"
                :theme-palette="themeStore.palette"
                :performance-mode="performanceStore.mode"
                :ui-font-size="uiFontSize"
                :username-saving="usernameSaving"
                :devtools-available="debugToolsAvailable"
                :update-available="desktopUpdateAvailable"
                :profile-avatar-icon="currentUserAvatarIcon"
                :profile-avatar-color="currentUserAvatarColor"
                :profile-avatar-options="profileAvatarOptions"
                :profile-avatar-colors="profileAvatarColors"
                @toggle-language="toggleLanguage"
                @check-update="checkClientUpdate"
                @toggle-devtools="openDebugTools"
                @update:send-key="updateSendKey"
                @update:theme-palette="updateThemePalette"
                @update:performance-mode="updatePerformanceMode"
                @update:ui-font-size="updateUiFontSize"
                @update:username="updateCurrentUsername"
                @update:profile-avatar-icon="updateCurrentUserAvatarIcon"
                @update:profile-avatar-color="updateCurrentUserAvatarColor"
              />
            </template>
          </div>
        </template>

        <template v-else>
          <div v-if="bootLoading" class="messenger-chat-empty">{{ t('common.loading') }}</div>
          <div v-else-if="!hasAnyMixedConversations || !sessionHub.activeConversation" class="messenger-chat-empty-state">
            <div class="messenger-chat-empty-icon">
              <i class="fa-regular fa-comments" aria-hidden="true"></i>
            </div>
            <div class="messenger-chat-empty-title">{{ t('messenger.empty.selectConversation') }}</div>
            <div class="messenger-chat-empty-subtitle">{{ t('messenger.section.messages.desc') }}</div>
          </div>

          <template v-else-if="isAgentConversationActive">
            <div
              v-if="messageVirtualTopSpacerHeight > 0"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${messageVirtualTopSpacerHeight}px` }"
              aria-hidden="true"
            ></div>
            <div
              v-for="item in visibleAgentMessages"
              :key="item.key"
              class="messenger-message"
              :class="{ mine: item.message.role === 'user' }"
              :data-virtual-key="item.key"
            >
                <div
                  v-if="item.message.role === 'user'"
                  class="messenger-message-avatar messenger-message-avatar--mine-profile"
                  :style="currentUserAvatarStyle"
                >
                <img
                  v-if="currentUserAvatarImageUrl"
                  class="messenger-settings-profile-avatar-image"
                  :src="currentUserAvatarImageUrl"
                  alt=""
                />
                <span v-else>{{ avatarLabel(currentUsername) }}</span>
              </div>
              <AgentAvatar
                v-else
                size="sm"
                :state="resolveMessageAgentAvatarState(item.message)"
                :title="activeAgentName"
              />
              <div class="messenger-message-main">
                <div class="messenger-message-meta">
                  <span>{{ item.message.role === 'user' ? t('chat.message.user') : activeAgentName }}</span>
                  <span>{{ formatTime(item.message.created_at) }}</span>
                  <MessageThinking
                    v-if="item.message.role === 'assistant'"
                    :content="String(item.message.reasoning || '')"
                    :streaming="Boolean(item.message.reasoningStreaming)"
                  />
                </div>
                <div v-if="item.message.role === 'assistant'" class="messenger-workflow-scope chat-shell">
                  <MessageToolWorkflow
                    :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                    :loading="Boolean(item.message.workflowStreaming)"
                    :visible="
                      Boolean(
                        item.message.workflowStreaming ||
                          (Array.isArray(item.message.workflowItems) && item.message.workflowItems.length > 0)
                      )
                    "
                    @layout-change="handleMessageWorkflowLayoutChange"
                  />
                </div>
                <div
                  v-if="item.message.role === 'user' || hasMessageContent(item.message.content)"
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
                  <div
                    v-else
                    class="markdown-body"
                    v-html="renderAgentMarkdown(item.message, item.sourceIndex)"
                  ></div>
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
                      @click="openImagePreview(imageItem.src, imageItem.name, imageItem.workspacePath)"
                    >
                      <img :src="imageItem.src" :alt="imageItem.name" class="message-user-image" />
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
                <div
                  v-if="hasMessageContent(item.message.content) || shouldShowMessageStats(item.message)"
                  class="messenger-message-extra"
                >
                  <div v-if="shouldShowMessageStats(item.message)" class="messenger-message-stats">
                    <span
                      v-for="entry in buildMessageStatsEntries(item.message)"
                      :key="entry.label"
                      class="messenger-message-stat"
                    >
                      <span class="messenger-message-stat-label">{{ entry.label }}:</span>
                      <span class="messenger-message-stat-value">{{ entry.value }}</span>
                    </span>
                  </div>
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
            <div
              v-if="messageVirtualBottomSpacerHeight > 0"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${messageVirtualBottomSpacerHeight}px` }"
              aria-hidden="true"
            ></div>
          </template>

          <template v-else-if="isWorldConversationActive">
            <div
              v-if="messageVirtualTopSpacerHeight > 0"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${messageVirtualTopSpacerHeight}px` }"
              aria-hidden="true"
            ></div>
            <div
              v-for="item in visibleWorldMessages"
              :key="item.key"
              class="messenger-message"
              :id="item.domId"
              :class="{ mine: isOwnMessage(item.message) }"
              :data-virtual-key="item.key"
            >
              <div
                class="messenger-message-avatar"
                :class="{ 'messenger-message-avatar--mine-profile': isOwnMessage(item.message) }"
                :style="isOwnMessage(item.message) ? currentUserAvatarStyle : undefined"
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
              </div>
              <div class="messenger-message-main">
                <div class="messenger-message-meta">
                  <span>{{ resolveWorldMessageSender(item.message) }}</span>
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
                  <div v-else class="markdown-body" v-html="renderWorldMarkdown(item.message)"></div>
                </div>
                <div
                  v-if="!isWorldVoiceMessage(item.message) && hasMessageContent(item.message.content)"
                  class="messenger-message-extra"
                >
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
            <div
              v-if="messageVirtualBottomSpacerHeight > 0"
              class="messenger-message-virtual-spacer"
              :style="{ height: `${messageVirtualBottomSpacerHeight}px` }"
              aria-hidden="true"
            ></div>
          </template>
          <div v-else class="messenger-chat-empty">
            {{ t('messenger.empty.selectConversation') }}
          </div>
        </template>
      </div>

      <footer
        v-if="showChatComposerFooter"
        ref="chatFooterRef"
        class="messenger-chat-footer"
      >
        <button
          v-if="showScrollBottomButton"
          class="messenger-scroll-bottom-btn"
          type="button"
          :title="t('chat.toBottom')"
          :aria-label="t('chat.toBottom')"
          @click="jumpToMessageBottom"
        >
          <i class="fa-solid fa-angles-down" aria-hidden="true"></i>
        </button>
        <div v-if="isAgentConversationActive" class="messenger-agent-composer messenger-composer-scope chat-shell">
          <InquiryPanel
            v-if="activeAgentInquiryPanel"
            :panel="activeAgentInquiryPanel.panel"
            @update:selected="handleAgentInquirySelection"
          />
          <PlanPanel
            v-if="activeAgentPlan"
            v-model:expanded="agentPlanExpanded"
            :plan="activeAgentPlan"
            @remove="dismissActiveAgentPlan"
          />
          <ToolApprovalComposer
            v-if="activeSessionApproval"
            :approval="activeSessionApproval"
            :busy="approvalResponding"
            @decide="handleSessionApprovalDecision"
          />
          <ChatComposer
            v-else
            world-style
            :loading="agentSessionLoading"
            :send-key="messengerSendKey"
            :draft-key="agentComposerDraftKey"
            :inquiry-active="Boolean(activeAgentInquiryPanel)"
            :inquiry-selection="agentInquirySelection"
            :voice-supported="agentVoiceSupported"
            :voice-recording="agentVoiceRecording"
            :voice-duration-ms="agentVoiceDurationMs"
            :show-approval-label="showAgentComposerApprovalHint"
            :approval-label="agentComposerApprovalHintLabel"
            :model-name="agentHeaderModelDisplayName"
            :model-jump-enabled="agentHeaderModelJumpEnabled"
            @send="sendAgentMessage"
            @stop="stopAgentMessage"
            @toggle-voice-record="toggleAgentVoiceRecord"
            @open-model-settings="openDesktopModelSettingsFromHeader"
          />
        </div>
        <MessengerWorldComposer
          v-else-if="isWorldConversationActive"
          ref="worldComposerViewRef"
          :style="worldComposerStyle"
          :quick-panel-mode="worldQuickPanelMode"
          :recent-emojis="worldRecentEmojis"
          :emoji-catalog="worldEmojiCatalog"
          :draft="worldDraft"
          :send-key="messengerSendKey"
          :can-send="canSendWorldMessage"
          :uploading="worldUploading"
          :screenshot-supported="worldDesktopScreenshotSupported"
          :voice-recording="worldVoiceRecording"
          :voice-duration-ms="worldVoiceDurationMs"
          :voice-supported="worldVoiceSupported"
          @update:draft="worldDraft = $event"
          @resize-mousedown="startWorldComposerResize"
          @open-quick-panel="openWorldQuickPanel"
          @toggle-quick-panel="toggleWorldQuickPanel"
          @clear-quick-panel-close="clearWorldQuickPanelClose"
          @schedule-quick-panel-close="scheduleWorldQuickPanelClose"
          @insert-emoji="insertWorldEmoji"
          @trigger-container-pick="openWorldContainerPicker"
          @trigger-upload="triggerWorldUpload"
          @toggle-voice-record="toggleWorldVoiceRecord"
          @trigger-screenshot="triggerWorldScreenshot"
          @open-history="openWorldHistoryDialog"
          @focus-input="worldQuickPanelMode = ''"
          @enter="handleWorldComposerEnterKeydown"
          @send="sendWorldMessage"
          @upload-change="handleWorldUploadInput"
        />
      </footer>
    </section>

    <MessengerRightDock
      ref="rightDockRef"
      v-if="showAgentRightDock"
      :collapsed="rightDockCollapsed"
      :show-agent-panels="showRightAgentPanels"
      :agent-id-for-api="rightPanelAgentIdForApi"
      :container-id="rightPanelContainerId"
      :active-session-id="String(chatStore.activeSessionId || '')"
      :session-history="rightPanelSessionHistory"
      @toggle-collapse="rightDockCollapsed = !rightDockCollapsed"
      @restore-session="restoreTimelineSession"
      @set-main="setTimelineSessionMain"
      @open-session-detail="openTimelineSessionDetail"
      @delete-session="deleteTimelineSession"
      @open-container="openContainerFromRightDock"
      @open-container-settings="openContainerSettingsFromRightDock"
    />
    <MessengerGroupDock
      ref="rightDockRef"
      v-else-if="showGroupRightDock"
      :collapsed="rightDockCollapsed"
      :group-id="activeWorldGroupId"
      @toggle-collapse="rightDockCollapsed = !rightDockCollapsed"
    />

    <MessengerFileContainerMenu
      ref="fileContainerMenuViewRef"
      :visible="fileContainerContextMenu.visible"
      :style="fileContainerContextMenuStyle"
      @open="handleFileContainerMenuOpen"
      @copy-id="handleFileContainerMenuCopyId"
      @settings="handleFileContainerMenuSettings"
    />

    <MessengerDialogsHost
      v-model:world-history-dialog-visible="worldHistoryDialogVisible"
      v-model:world-history-keyword="worldHistoryKeyword"
      v-model:world-history-active-tab="worldHistoryActiveTab"
      v-model:world-history-date-range="worldHistoryDateRange"
      :world-history-tab-options="worldHistoryTabOptions"
      :filtered-world-history-records="filteredWorldHistoryRecords"
      :format-time="formatTime"
      :locate-world-history-message="locateWorldHistoryMessage"
      v-model:timeline-detail-dialog-visible="timelineDetailDialogVisible"
      :timeline-detail-session-id="timelineDetailSessionId"
      v-model:world-container-picker-visible="worldContainerPickerVisible"
      :world-container-picker-loading="worldContainerPickerLoading"
      :world-container-picker-path="worldContainerPickerPath"
      :world-container-picker-path-label="worldContainerPickerPathLabel"
      v-model:world-container-picker-keyword="worldContainerPickerKeyword"
      :world-container-picker-display-entries="worldContainerPickerDisplayEntries"
      :open-world-container-picker-parent="openWorldContainerPickerParent"
      :refresh-world-container-picker="refreshWorldContainerPicker"
      :handle-world-container-picker-entry="handleWorldContainerPickerEntry"
      v-model:agent-prompt-preview-visible="agentPromptPreviewVisible"
      :agent-prompt-preview-loading="agentPromptPreviewLoading"
      :active-agent-prompt-preview-text="activeAgentPromptPreviewText"
      :image-preview-visible="imagePreviewVisible"
      :image-preview-url="imagePreviewUrl"
      :image-preview-title="imagePreviewTitle"
      :image-preview-workspace-path="imagePreviewWorkspacePath"
      :handle-image-preview-download="handleImagePreviewDownload"
      :close-image-preview="closeImagePreview"
      v-model:group-create-visible="groupCreateVisible"
      v-model:group-create-name="groupCreateName"
      v-model:group-create-keyword="groupCreateKeyword"
      v-model:group-create-member-ids="groupCreateMemberIds"
      :group-creating="groupCreating"
      :filtered-group-create-contacts="filteredGroupCreateContacts"
      :resolve-unit-label="resolveUnitLabel"
      :submit-group-create="submitGroupCreate"
    />
    <AgentCreateDialog
      v-model="agentCreateVisible"
      :copy-from-agents="agentCreateCopyFromAgents"
      :beeroom-groups="beeroomGroupOptions"
      :default-beeroom-group-id="defaultAgentCreateBeeroomGroupId"
      @submit="submitAgentCreate"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElLoading, ElMessage } from 'element-plus';

import { listAgentUserRounds, listRunningAgents } from '@/api/agents';
import { fetchOrgUnits, updateProfile } from '@/api/auth';
import { listChannelBindings } from '@/api/channels';
import {
  getSession as getChatSessionApi,
  fetchSessionSystemPrompt,
  fetchRealtimeSystemPrompt
} from '@/api/chat';
import { fetchCronJobs } from '@/api/cron';
import { fetchDesktopSettings } from '@/api/desktop';
import { fetchExternalLinks } from '@/api/externalLinks';
import { downloadUserWorldFile } from '@/api/userWorld';
import { fetchUserToolsCatalog, fetchUserToolsSummary } from '@/api/userTools';
import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent, uploadWunderWorkspace } from '@/api/workspace';
import BeeroomWorkbench from '@/components/beeroom/BeeroomWorkbench.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import AgentCreateDialog from '@/components/messenger/AgentCreateDialog.vue';
import {
  scheduleMessengerBootstrapBackgroundTasks,
  settleMessengerBootstrapTasks,
  splitMessengerBootstrapTasks,
  type MessengerBootstrapTask
} from '@/views/messenger/bootstrap';
import MessengerMiddlePane from '@/views/messenger/sections/MessengerMiddlePane.vue';
import MessengerDialogsHost from '@/views/messenger/sections/MessengerDialogsHost.vue';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import {
  InquiryPanel,
  MessageThinking,
  MessageToolWorkflow,
  PlanPanel,
  ToolApprovalComposer,
  WorkspacePanel
} from '@/views/messenger/lazyMessageBlocks';
import {
  MessengerFileContainerMenu,
  MessengerGroupDock,
  MessengerRightDock
} from '@/views/messenger/lazyShell';
import {
  AgentCronPanel,
  AgentRuntimeRecordsPanel,
  AgentSettingsPanel,
  DesktopContainerManagerPanel,
  DesktopSystemSettingsPanel,
  MessengerLocalFileSearchPanel,
  MessengerSettingsPanel,
  MessengerWorldComposer,
  UserChannelSettingsPanel,
  UserKnowledgePane,
  UserMcpPane,
  UserPromptSettingsPanel,
  UserSharedToolsPanel,
  UserSkillPane
} from '@/views/messenger/lazyPanels';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { getRuntimeConfig } from '@/config/runtime';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useBeeroomStore, type BeeroomGroup } from '@/stores/beeroom';
import { useChatStore } from '@/stores/chat';
import { usePerformanceStore } from '@/stores/performance';
import { useThemeStore } from '@/stores/theme';
import {
  useSessionHubStore,
  resolveSectionFromRoute,
  type MessengerSection
} from '@/stores/sessionHub';
import { useUserWorldStore } from '@/stores/userWorld';
import { hydrateExternalMarkdownImages, renderMarkdown } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { showApiError } from '@/utils/apiError';
import { copyText } from '@/utils/clipboard';
import { confirmWithFallback } from '@/utils/confirm';
import { buildAssistantMessageStatsEntries } from '@/utils/messageStats';
import {
  isAudioRecordingSupported,
  startAudioRecording,
  type AudioRecordingResult,
  type AudioRecordingSession
} from '@/utils/audioRecorder';
import { collectAbilityDetails, collectAbilityNames } from '@/utils/toolSummary';
import {
  buildWorkspaceImagePersistentCacheKey,
  readWorkspaceImagePersistentCache,
  writeWorkspaceImagePersistentCache
} from '@/utils/workspaceImagePersistentCache';
import {
  isImagePath,
  resolveWorkspaceRelativePathFromLocal,
  normalizeWorkspaceRelativeMarkdownPath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import {
  normalizeAvatarColor,
  normalizeAvatarIcon,
  normalizeThemeMode,
  normalizeThemePalette,
  type ThemePalette,
  type UserAppearancePreferences
} from '@/utils/userPreferences';
import {
  classifyWorldHistoryMessage,
  normalizeWorldHistoryText,
  resolveWorldHistoryIcon
} from '@/views/messenger/worldHistory';
import { loadUserAppearance, saveUserAppearance } from '@/views/messenger/userAppearanceSync';
import {
  buildWorldVoicePayloadContent,
  formatWorldVoiceDuration,
  isWorldVoiceContentType,
  parseWorldVoicePayload
} from '@/views/messenger/worldVoice';
import {
  buildUnitTreeFromFlat,
  buildUnitTreeRows,
  collectUnitNodeIds,
  flattenUnitNodes,
  normalizeUnitNode,
  normalizeUnitShortLabel,
  normalizeUnitText,
  resolveUnitIdKey,
  resolveUnitTreeRowStyle
} from '@/views/messenger/orgUnits';
import {
  AGENT_CONTAINER_IDS,
  AGENT_MAIN_READ_AT_STORAGE_PREFIX,
  AGENT_MAIN_UNREAD_STORAGE_PREFIX,
  AGENT_TOOL_OVERRIDE_NONE,
  DEFAULT_AGENT_KEY,
  DISMISSED_AGENT_STORAGE_PREFIX,
  MESSENGER_SEND_KEY_STORAGE_KEY,
  MESSENGER_UI_FONT_SIZE_STORAGE_KEY,
  USER_CONTAINER_ID,
  USER_WORLD_UPLOAD_BASE,
  UNIT_UNGROUPED_ID,
  WORLD_COMPOSER_HEIGHT_STORAGE_KEY,
  WORLD_EMOJI_CATALOG,
  WORLD_QUICK_EMOJI_STORAGE_KEY,
  WORLD_UPLOAD_SIZE_LIMIT,
  sectionRouteMap,
  type AgentFileContainer,
  type AgentLocalCommand,
  type AgentOverviewCard,
  type AgentRuntimeState,
  type DesktopBridge,
  type DesktopInstallResult,
  type DesktopScreenshotResult,
  type DesktopUpdateState,
  type FileContainerMenuTarget,
  type MessengerPerfTrace,
  type MessengerSendKeyMode,
  type MixedConversation,
  type ToolEntry,
  type UnitTreeNode,
  type UnitTreeRow,
  type WorldComposerViewRef,
  type WorldHistoryCategory,
  type WorldHistoryRecord
} from '@/views/messenger/model';

/**
 * NOTE FOR CONTRIBUTORS:
 * This view has become too large and is now in maintenance mode.
 * Do not add new business logic directly in `MessengerView.vue`.
 * Add new features in dedicated files (for example under `views/messenger/` or composables),
 * then import and wire them here.
 */
const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const authStore = useAuthStore();
const agentStore = useAgentStore();
const chatStore = useChatStore();
const beeroomStore = useBeeroomStore();
const performanceStore = usePerformanceStore();
const themeStore = useThemeStore();
const userWorldStore = useUserWorldStore();
const sessionHub = useSessionHubStore();

const DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY = 'messenger_desktop_first_launch_default_agent_hint_v1';
const PROFILE_AVATAR_IMAGE_FILES = import.meta.glob('../assets/qq-avatars/avatar-????.jpg', {
  eager: true,
  import: 'default'
}) as Record<string, string>;
const PROFILE_AVATAR_IMAGE_OPTIONS = Object.entries(PROFILE_AVATAR_IMAGE_FILES)
  .map(([path, image]) => {
    const fileName = path.split('/').pop() || '';
    const stem = fileName.replace(/\.jpg$/i, '').trim();
    const numericPart = stem.replace(/^avatar-/, '').trim();
    const sequence = Number.parseInt(numericPart, 10);
    const label = Number.isFinite(sequence)
      ? `QQ Avatar ${String(sequence).padStart(4, '0')}`
      : `QQ Avatar ${stem}`;
    return {
      key: `qq-${stem}`,
      image,
      label
    };
  })
  .sort((left, right) =>
    left.key.localeCompare(right.key, 'en', { numeric: true, sensitivity: 'base' })
  );
const PROFILE_AVATAR_IMAGE_MAP = new Map(
  PROFILE_AVATAR_IMAGE_OPTIONS.map((item) => [item.key, item.image])
);
const PROFILE_AVATAR_OPTION_KEYS = new Set<string>([
  'initial',
  ...PROFILE_AVATAR_IMAGE_OPTIONS.map((item) => item.key)
]);
const PROFILE_AVATAR_COLORS = [
  '#f97316',
  '#ef4444',
  '#ec4899',
  '#8b5cf6',
  '#6366f1',
  '#3b82f6',
  '#06b6d4',
  '#14b8a6',
  '#10b981',
  '#84cc16',
  '#f59e0b',
  '#64748b'
] as const;

const bootLoading = ref(true);
const selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);
const agentOverviewMode = ref<'detail' | 'grid'>('detail');
const selectedContactUserId = ref('');
const selectedGroupId = ref('');
const agentCreateVisible = ref(false);
const selectedContactUnitId = ref('');
const selectedToolCategory = ref<'admin' | 'mcp' | 'skills' | 'knowledge' | 'shared' | ''>('');
const worldDraft = ref('');
const worldDraftMap = new Map<string, string>();
const dismissedAgentConversationMap = ref<Record<string, number>>({});
const dismissedAgentStorageKey = ref('');
const leftRailRef = ref<HTMLElement | null>(null);
const middlePaneRef = ref<HTMLElement | null>(null);
const rightDockRef = ref<{ $el?: HTMLElement } | null>(null);
const worldComposerViewRef = ref<WorldComposerViewRef | null>(null);
const worldUploading = ref(false);
const worldVoiceRecording = ref(false);
const worldVoiceDurationMs = ref(0);
const agentVoiceRecording = ref(false);
const agentVoiceDurationMs = ref(0);
const worldVoicePlaybackCurrentMs = ref(0);
const worldVoicePlaybackDurationMs = ref(0);
const agentVoiceModelHearingSupported = ref<boolean | null>(null);
const desktopDefaultModelDisplayName = ref('');
const worldVoicePlayingMessageKey = ref('');
const worldVoiceLoadingMessageKey = ref('');
const worldComposerHeight = ref(188);
const worldQuickPanelMode = ref<'' | 'emoji'>('');
const worldHistoryDialogVisible = ref(false);
const helperAppsWorkspaceMode = ref(false);
const helperAppsActiveKind = ref<'offline' | 'online' | ''>('');
const helperAppsActiveKey = ref('');
type HelperAppOfflineItem = {
  key: string;
  title: string;
  description: string;
  icon: string;
};
type HelperAppExternalItem = {
  linkId: string;
  title: string;
  description: string;
  url: string;
  icon: string;
  sortOrder: number;
};
const helperAppsOnlineLoading = ref(false);
const helperAppsOnlineLoaded = ref(false);
const helperAppsOnlineItems = ref<HelperAppExternalItem[]>([]);
const worldHistoryKeyword = ref('');
const worldHistoryActiveTab = ref<WorldHistoryCategory>('all');
const worldHistoryDateRange = ref<[string, string] | []>([]);
const worldContainerPickerVisible = ref(false);
const worldContainerPickerLoading = ref(false);
const worldContainerPickerPath = ref('');
const worldContainerPickerKeyword = ref('');
type WorldContainerPickerEntry = {
  path: string;
  name: string;
  type: 'dir' | 'file';
};
const worldContainerPickerEntries = ref<WorldContainerPickerEntry[]>([]);
const agentPromptPreviewVisible = ref(false);
const agentPromptPreviewLoading = ref(false);
const agentPromptPreviewContent = ref('');
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const imagePreviewWorkspacePath = ref('');
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
const messageListRef = ref<HTMLElement | null>(null);
const chatFooterRef = ref<HTMLElement | null>(null);
const messageVirtualScrollTop = ref(0);
const messageVirtualViewportHeight = ref(0);
const messageVirtualLayoutVersion = ref(0);
const messageVirtualHeightCache = new Map<string, number>();
const agentRuntimeStateMap = ref<Map<string, AgentRuntimeState>>(new Map());
const agentUserRoundsMap = ref<Map<string, number>>(new Map());
const runtimeStateOverrides = ref<Map<string, { state: AgentRuntimeState; expiresAt: number }>>(new Map());
const cronAgentIds = ref<Set<string>>(new Set());
const channelBoundAgentIds = ref<Set<string>>(new Set());
const cronPermissionDenied = ref(false);
const agentSettingMode = ref<'agent' | 'cron' | 'channel' | 'runtime'>('agent');
const settingsPanelMode = ref<
  'general' | 'profile' | 'prompts' | 'desktop-models' | 'desktop-remote' | 'desktop-lan'
>('general');
const rightDockCollapsed = ref(false);
const desktopInitialSectionPinned = ref(false);
const desktopShowFirstLaunchDefaultAgentHint = ref(false);
const desktopFirstLaunchDefaultAgentHintAt = ref(0);
const usernameSaving = ref(false);
const appearanceHydrating = ref(false);
const currentUserAvatarIcon = ref('initial');
const currentUserAvatarColor = ref('#3b82f6');
const toolsCatalogLoading = ref(false);
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
const fileContainerMenuViewRef = ref<{ getMenuElement: () => HTMLElement | null } | null>(null);
const desktopContainerManagerPanelRef = ref<{
  openManager: (containerId?: number) => Promise<void> | void;
} | null>(null);
const fileContainerContextMenu = ref<{
  visible: boolean;
  x: number;
  y: number;
  target: FileContainerMenuTarget | null;
}>({
  visible: false,
  x: 0,
  y: 0,
  target: null
});
const desktopContainerRootMap = ref<Record<number, string>>({});
const timelinePreviewMap = ref<Map<string, string>>(new Map());
const timelinePreviewLoadingSet = ref<Set<string>>(new Set());
const timelineDetailDialogVisible = ref(false);
const timelineDetailSessionId = ref('');
const approvalResponding = ref(false);
const messengerSendKey = ref<MessengerSendKeyMode>('ctrl_enter');
const uiFontSize = ref(14);
const orgUnitPathMap = ref<Record<string, string>>({});
const orgUnitTree = ref<UnitTreeNode[]>([]);
const contactUnitExpandedIds = ref<Set<string>>(new Set());
const showScrollBottomButton = ref(false);
const autoStickToBottom = ref(true);
const agentInquirySelection = ref<number[]>([]);
const agentPlanExpanded = ref(false);
const dismissedPlanMessages = ref<WeakSet<Record<string, unknown>>>(new WeakSet());
const dismissedPlanVersion = ref(0);
const groupCreateVisible = ref(false);
const groupCreateName = ref('');
const groupCreateKeyword = ref('');
const groupCreateMemberIds = ref<string[]>([]);
const groupCreating = ref(false);
const viewportWidth = ref(typeof window !== 'undefined' ? window.innerWidth : 1440);
const middlePaneOverlayVisible = ref(false);
const quickCreatingAgent = ref(false);
const agentMainReadAtMap = ref<Record<string, number>>({});
const agentMainUnreadCountMap = ref<Record<string, number>>({});
const agentUnreadStorageKeys = ref<{ readAt: string; unread: string }>({ readAt: '', unread: '' });
const keywordInput = ref('');
const contactVirtualListRef = ref<HTMLElement | null>(null);
const contactVirtualScrollTop = ref(0);
const contactVirtualViewportHeight = ref(0);

const setContactVirtualListRef = (element: HTMLElement | null) => {
  contactVirtualListRef.value = element;
};

let statusTimer: number | null = null;
let lifecycleTimer: number | null = null;
let worldQuickPanelCloseTimer: number | null = null;
let timelinePrefetchTimer: number | null = null;
let middlePaneOverlayHideTimer: number | null = null;
let keywordDebounceTimer: number | null = null;
let messageScrollFrame: number | null = null;
let messageVirtualMeasureFrame: number | null = null;
let contactVirtualFrame: number | null = null;
let viewportResizeHandler: (() => void) | null = null;
let audioRecordingSupportHandler: (() => void) | null = null;
let audioRecordingSupportRetryTimer: number | null = null;
let worldComposerResizeRuntime: { startY: number; startHeight: number } | null = null;
type WorldVoiceRecordingRuntime = {
  session: AudioRecordingSession;
  startedAt: number;
  timerId: number | null;
  conversationId: string;
};
type AgentVoiceRecordingRuntime = {
  session: AudioRecordingSession;
  startedAt: number;
  timerId: number | null;
  draftIdentity: string;
};
type WorldVoicePlaybackRuntime = {
  audio: HTMLAudioElement;
  objectUrlCache: Map<string, string>;
  currentMessageKey: string;
  currentResourceKey: string;
};
let worldVoiceRecordingRuntime: WorldVoiceRecordingRuntime | null = null;
let agentVoiceRecordingRuntime: AgentVoiceRecordingRuntime | null = null;
let worldVoicePlaybackRuntime: WorldVoicePlaybackRuntime | null = null;
let cronAgentIdsLoadVersion = 0;
let channelBoundAgentIdsLoadVersion = 0;
let desktopDefaultModelMetaFetchPromise: Promise<{
  hearingSupported: boolean;
  modelDisplayName: string;
}> | null = null;
let agentVoiceModelSupportCheckedAt = 0;
const agentUnreadRefreshInFlight = new Set<string>();
const MARKDOWN_CACHE_LIMIT = 280;
const MARKDOWN_STREAM_THROTTLE_MS = 80;
const CONTACT_VIRTUAL_ITEM_HEIGHT = 60;
const CONTACT_VIRTUAL_OVERSCAN = 8;
const MESSAGE_VIRTUAL_THRESHOLD = 180;
const MESSAGE_VIRTUAL_OVERSCAN = 8;
const MESSAGE_VIRTUAL_ESTIMATED_HEIGHT = 118;
const MESSAGE_VIRTUAL_GAP = 12;
const AGENT_VOICE_MODEL_SUPPORT_CACHE_MS = 30_000;
const markdownCache = new Map<string, { source: string; html: string; updatedAt: number }>();
type WorkspaceResourceCachePayload = { objectUrl: string; filename: string };
type WorkspaceResourceCacheEntry = {
  objectUrl?: string;
  filename?: string;
  promise?: Promise<WorkspaceResourceCachePayload>;
};
type AttachmentResourceState = {
  objectUrl?: string;
  filename?: string;
  error?: boolean;
  loading?: boolean;
};
const WORKSPACE_RESOURCE_LOADING_LABEL_DELAY_MS = 160;
const KEYWORD_INPUT_DEBOUNCE_MS = 120;
const workspaceResourceCache = new Map<string, WorkspaceResourceCacheEntry>();
const userAttachmentResourceCache = ref(new Map<string, AttachmentResourceState>());
let workspaceResourceHydrationFrame: number | null = null;
let workspaceResourceHydrationPending = false;
let pendingAssistantCenter = false;
let pendingAssistantCenterCount = 0;
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

const sectionOptions = computed(() => {
  return [
    { key: 'messages' as MessengerSection, icon: 'fa-solid fa-comment-dots', label: t('messenger.section.messages') },
    { key: 'agents' as MessengerSection, icon: 'fa-solid fa-robot', label: t('messenger.section.agents') },
    { key: 'swarms' as MessengerSection, icon: 'fa-solid fa-hexagon-nodes', label: t('messenger.section.swarms') },
    { key: 'users' as MessengerSection, icon: 'fa-solid fa-user-group', label: t('messenger.section.users') },
    { key: 'groups' as MessengerSection, icon: 'fa-solid fa-comments', label: t('messenger.section.groups') },
    { key: 'tools' as MessengerSection, icon: 'fa-solid fa-wrench', label: t('messenger.section.tools') },
    { key: 'files' as MessengerSection, icon: 'fa-solid fa-folder-open', label: t('messenger.section.files') },
    { key: 'more' as MessengerSection, icon: 'fa-solid fa-gear', label: t('messenger.section.settings') }
  ];
});

const leftRailMainSectionOptions = computed(() =>
  sectionOptions.value.filter(
    (item) =>
      item.key === 'messages' ||
      item.key === 'agents' ||
      item.key === 'swarms' ||
      item.key === 'tools' ||
      item.key === 'files'
  )
);

const leftRailSocialSectionOptions = computed(() =>
  sectionOptions.value.filter((item) => item.key === 'users' || item.key === 'groups')
);

const isLeftNavSectionActive = (section: MessengerSection): boolean => {
  if (helperAppsWorkspaceMode.value && sessionHub.activeSection === 'groups') {
    return false;
  }
  return sessionHub.activeSection === section;
};

const basePrefix = computed(() => {
  if (route.path.startsWith('/desktop')) return '/desktop';
  if (route.path.startsWith('/demo')) return '/demo';
  return '/app';
});

const getDesktopBridge = (): DesktopBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopBridge }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
};

const desktopMode = computed(() => isDesktopModeEnabled());
const desktopLocalMode = computed(() => desktopMode.value && !isDesktopRemoteAuthMode());
const settingsLogoutDisabled = computed(
  () => desktopMode.value && !isDesktopRemoteAuthMode()
);
const debugToolsAvailable = computed(() => typeof getDesktopBridge()?.toggleDevTools === 'function');
const desktopUpdateAvailable = computed(() => typeof getDesktopBridge()?.checkForUpdates === 'function');
const worldDesktopScreenshotSupported = computed(
  () => desktopMode.value && typeof getDesktopBridge()?.captureScreenshot === 'function'
);
const detectAudioRecordingSupport = (): boolean => {
  try {
    return isAudioRecordingSupported();
  } catch {
    return false;
  }
};
const audioRecordingSupported = ref(detectAudioRecordingSupport());
const refreshAudioRecordingSupport = () => {
  audioRecordingSupported.value = detectAudioRecordingSupport();
};
const worldVoiceSupported = computed(() => audioRecordingSupported.value);
const agentVoiceSupported = computed(() => audioRecordingSupported.value);

const resolveVoiceRecordingErrorText = (error: unknown): string => {
  const text = String((error as { message?: unknown } | null)?.message || error || '')
    .trim()
    .toLowerCase();
  if (!text) {
    return '';
  }
  if (
    text.includes('microphone permission denied') ||
    text.includes('permission denied') ||
    text.includes('notallowederror') ||
    text.includes('denied permission')
  ) {
    return t('messenger.world.voice.permissionDenied');
  }
  if (text.includes('audio recording is not supported') || text.includes('not supported')) {
    return t('messenger.world.voice.unsupported');
  }
  return '';
};

const keyword = computed(() => sessionHub.keyword);

const currentUsername = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.username || user?.id || t('user.guest'));
});
const currentUserId = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.id || '');
});
const profileAvatarOptions = computed(() =>
  [
    {
      key: 'initial',
      label: t('portal.agent.avatar.icon.initial')
    },
    ...PROFILE_AVATAR_IMAGE_OPTIONS
  ]
);
const profileAvatarColors = computed(() => [...PROFILE_AVATAR_COLORS]);
const currentUserAvatarImageUrl = computed(
  () => PROFILE_AVATAR_IMAGE_MAP.get(String(currentUserAvatarIcon.value || '').trim()) || ''
);
const currentUserAvatarStyle = computed(() => ({
  background: currentUserAvatarImageUrl.value
    ? 'transparent'
    : String(currentUserAvatarColor.value || '#3b82f6')
}));
const userWorldPermissionDenied = computed(() => userWorldStore.permissionDenied === true);

const activeSectionTitle = computed(() => {
  if (helperAppsWorkspaceMode.value && sessionHub.activeSection === 'groups') {
    return t('userWorld.helperApps.title');
  }
  return sessionHub.activeSection === 'more'
    ? t('messenger.section.settings')
    : t(`messenger.section.${sessionHub.activeSection}`);
});
const activeSectionSubtitle = computed(() => {
  if (helperAppsWorkspaceMode.value && sessionHub.activeSection === 'groups') {
    return t('userWorld.helperApps.subtitle');
  }
  return sessionHub.activeSection === 'more'
    ? t('messenger.section.settings.desc')
    : t(`messenger.section.${sessionHub.activeSection}.desc`);
});
const currentLanguageLabel = computed(() =>
  getCurrentLanguage() === 'zh-CN' ? t('language.zh-CN') : t('language.en-US')
);
const searchPlaceholder = computed(() => t(`messenger.search.${sessionHub.activeSection}`));
const isMiddlePaneOverlay = computed(() => viewportWidth.value <= 960);
const isRightDockOverlay = computed(() => viewportWidth.value <= 1200);
const showMiddlePane = computed(
  () => !isMiddlePaneOverlay.value || middlePaneOverlayVisible.value
);
const middlePaneTransitionName = computed(() => 'messenger-middle-pane-slide');

const ownedAgents = computed(() => (Array.isArray(agentStore.agents) ? agentStore.agents : []));
const sharedAgents = computed(() =>
  desktopLocalMode.value ? [] : (Array.isArray(agentStore.sharedAgents) ? agentStore.sharedAgents : [])
);

const defaultAgentApprovalMode = computed(() => 'full_auto');
const agentMap = computed(() => {
  const map = new Map<string, Record<string, unknown>>();
  map.set(DEFAULT_AGENT_KEY, {
    id: DEFAULT_AGENT_KEY,
    name: t('messenger.defaultAgent'),
    description: t('messenger.defaultAgentDesc'),
    sandbox_container_id: 1,
    approval_mode: defaultAgentApprovalMode.value
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

const asObjectRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

type AgentApprovalMode = 'suggest' | 'auto_edit' | 'full_auto';

const normalizeAgentApprovalMode = (value: unknown): AgentApprovalMode => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase();
  if (normalized === 'suggest') return 'suggest';
  if (normalized === 'full_auto' || normalized === 'full-auto') return 'full_auto';
  return 'auto_edit';
};

const tryParseJsonRecord = (value: unknown): Record<string, unknown> | null => {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!text || !text.startsWith('{')) return null;
  try {
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
};

const resolveModelNameFromRecord = (value: unknown): string => {
  const source = tryParseJsonRecord(value) || asObjectRecord(value);
  if (!Object.keys(source).length) return '';
  const directKeys = [
    'model_name',
    'modelName',
    'model',
    'llm_model',
    'llmModel',
    'llm_model_name',
    'llmModelName'
  ] as const;
  for (const key of directKeys) {
    const candidate = source[key];
    if (typeof candidate === 'string' || typeof candidate === 'number') {
      const text = String(candidate).trim();
      if (text) return text;
      const parsed = tryParseJsonRecord(candidate);
      if (parsed) {
        const parsedName = resolveModelNameFromRecord(parsed);
        if (parsedName) return parsedName;
      }
      continue;
    }
    const nested = asObjectRecord(candidate);
    const nestedText = String(nested.name || nested.model || nested.id || '').trim();
    if (nestedText) return nestedText;
    const nestedName = resolveModelNameFromRecord(nested);
    if (nestedName) return nestedName;
  }
  const nestedContainerKeys = ['payload', 'data', 'request', 'response', 'detail', 'args'] as const;
  for (const key of nestedContainerKeys) {
    const nestedName = resolveModelNameFromRecord(source[key]);
    if (nestedName) return nestedName;
  }
  const meta = source.meta;
  if (meta && typeof meta === 'object' && meta !== value) {
    const nested = resolveModelNameFromRecord(meta);
    if (nested) return nested;
  }
  return '';
};

const resolveMessageModelName = (message: Record<string, unknown>): string => {
  const direct = resolveModelNameFromRecord(message);
  if (direct) return direct;
  const workflowItems = Array.isArray(message.workflowItems)
    ? (message.workflowItems as unknown[])
    : [];
  for (let cursor = workflowItems.length - 1; cursor >= 0; cursor -= 1) {
    const item = workflowItems[cursor];
    const fromItem = resolveModelNameFromRecord(item);
    if (fromItem) {
      return fromItem;
    }
    const fromDetail = resolveModelNameFromRecord(asObjectRecord(item).detail);
    if (fromDetail) {
      return fromDetail;
    }
  }
  return '';
};

const activeAgentSessionModelName = computed(() =>
  resolveModelNameFromRecord(activeAgentSession.value)
);

const activeAgentRuntimeModelName = computed(() => {
  if (!isAgentConversationActive.value) return '';
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  for (let cursor = messages.length - 1; cursor >= 0; cursor -= 1) {
    const message = asObjectRecord(messages[cursor]);
    if (String(message.role || '').trim().toLowerCase() !== 'assistant') {
      continue;
    }
    const modelName = resolveMessageModelName(message);
    if (modelName) return modelName;
  }
  return '';
});

const agentHeaderModelDisplayName = computed(() => {
  if (!isAgentConversationActive.value) return '';
  const sessionModelName = activeAgentSessionModelName.value;
  if (sessionModelName) return sessionModelName;
  const runtimeModelName = activeAgentRuntimeModelName.value;
  if (runtimeModelName) return runtimeModelName;
  if (desktopMode.value) {
    const desktopModelName = String(desktopDefaultModelDisplayName.value || '').trim();
    if (desktopModelName) return desktopModelName;
    if (desktopLocalMode.value) return t('desktop.system.modelUnnamed');
  }
  return t('common.unknown');
});

const agentHeaderModelJumpEnabled = computed(
  () => desktopMode.value || route.path.startsWith('/desktop')
);

const activeAgentApprovalMode = computed<AgentApprovalMode>(() => {
  if (!desktopLocalMode.value) {
    return 'full_auto';
  }
  const session = asObjectRecord(activeAgentSession.value);
  const sessionMode = String(session.approval_mode || session.approvalMode || '').trim();
  if (sessionMode) {
    return normalizeAgentApprovalMode(sessionMode);
  }
  if (activeAgentId.value === DEFAULT_AGENT_KEY && desktopLocalMode.value) {
    return 'full_auto';
  }
  const agent = asObjectRecord(activeAgent.value);
  return normalizeAgentApprovalMode(agent.approval_mode || agent.approvalMode || 'auto_edit');
});

const resolveCompactApprovalOptionLabel = (value: string): string => {
  const source = String(value || '').trim();
  if (!source) return '';
  const splitIndex = ['（', '(']
    .map((marker) => source.indexOf(marker))
    .filter((index) => index > 0)
    .sort((left, right) => left - right)[0];
  return typeof splitIndex === 'number' ? source.slice(0, splitIndex).trim() : source;
};

const agentComposerApprovalHintLabel = computed(() => {
  const optionLabel = t(`portal.agent.permission.option.${activeAgentApprovalMode.value}`);
  const compactOption = resolveCompactApprovalOptionLabel(optionLabel) || optionLabel;
  return `${t('portal.agent.permission.title')}: ${compactOption}`;
});

const showAgentComposerApprovalHint = computed(
  () => desktopLocalMode.value && isAgentConversationActive.value
);

const activeSessionApproval = computed(() => {
  if (!isAgentConversationActive.value) return null;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId || !Array.isArray(chatStore.pendingApprovals)) return null;
  return (
    chatStore.pendingApprovals.find(
      (item) => String(item?.session_id || '').trim() === sessionId
    ) || null
  );
});

const pendingApprovalAgentIdSet = computed(() => {
  const approvals = Array.isArray(chatStore.pendingApprovals) ? chatStore.pendingApprovals : [];
  const result = new Set<string>();
  if (!approvals.length) {
    return result;
  }
  const sessionAgentMap = new Map<string, string>();
  (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).forEach((sessionRaw) => {
    const session = (sessionRaw || {}) as Record<string, unknown>;
    const sessionId = String(session?.id || '').trim();
    if (!sessionId) return;
    const resolvedAgentId =
      normalizeAgentId(
        session?.agent_id || (session?.is_default === true ? DEFAULT_AGENT_KEY : '')
      ) || DEFAULT_AGENT_KEY;
    sessionAgentMap.set(sessionId, resolvedAgentId);
  });
  approvals.forEach((item) => {
    const sessionId = String((item as Record<string, unknown>)?.session_id || '').trim();
    if (!sessionId) return;
    const fromMap = sessionAgentMap.get(sessionId);
    if (fromMap) {
      result.add(fromMap);
      return;
    }
    if (sessionId === String(chatStore.activeSessionId || '').trim()) {
      result.add(
        normalizeAgentId(activeAgentId.value || selectedAgentId.value || DEFAULT_AGENT_KEY)
      );
    }
  });
  return result;
});

const resolveCurrentUserScope = (): string => String(currentUserId.value || '').trim() || 'guest';

const resolveAgentDraftIdentity = (): string => {
  const identity = activeConversation.value;
  if (identity?.kind === 'agent') {
    const conversationId = String(identity.id || '').trim();
    if (conversationId) return `conversation:${conversationId}`;
    const agentId = normalizeAgentId(identity.agentId || activeAgentId.value || selectedAgentId.value);
    return `draft:${agentId || DEFAULT_AGENT_KEY}`;
  }
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (sessionId) return `session:${sessionId}`;
  const draftAgentId = normalizeAgentId(chatStore.draftAgentId || activeAgentId.value || selectedAgentId.value);
  return `draft:${draftAgentId || DEFAULT_AGENT_KEY}`;
};

const agentComposerDraftKey = computed(() =>
  `messenger:agent:${resolveCurrentUserScope()}:${resolveAgentDraftIdentity()}`
);

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

const buildAbilitySkillNameSet = (summary: Record<string, unknown>): Set<string> => {
  const names = collectAbilityNames(summary);
  return new Set<string>(names.skills || []);
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
  const skillNameSet = buildAbilitySkillNameSet(summary);
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
  if (desktopMode.value && skillNameSet.size > 0 && sourceOverrides.length > 0) {
    skillNameSet.forEach((name) => {
      if (allowedSet.has(name)) {
        selectedNames.add(name);
      }
    });
  }
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
  const parsed = Number.parseInt(String(value ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
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
    sandbox_container_id: 1
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

const resolveWorkspaceRootPrefix = (): { root: string; separator: '/' | '\\' } => {
  const runtimeRoot = String(getRuntimeConfig().workspace_root || '')
    .trim()
    .replace(/[\\/]+$/, '');
  const root = runtimeRoot || '/workspaces';
  return {
    root,
    separator: root.includes('\\') ? '\\' : '/'
  };
};

const withTrailingSeparator = (path: string): string => {
  const trimmed = String(path || '').trim();
  if (!trimmed) return '';
  const separator = trimmed.includes('\\') ? '\\' : '/';
  if (trimmed.endsWith('/') || trimmed.endsWith('\\')) {
    return trimmed;
  }
  return `${trimmed}${separator}`;
};

const resolveWorkspaceScopeSuffix = (): string => {
  const userId = String(currentUserId.value || '').trim() || 'anonymous';
  if (fileScope.value === 'user' || selectedFileContainerId.value === USER_CONTAINER_ID) {
    return userId;
  }
  return `${userId}__c__${selectedFileContainerId.value}`;
};

const fileContainerCloudLocation = computed(() => {
  const { root } = resolveWorkspaceRootPrefix();
  const scope = resolveWorkspaceScopeSuffix();
  return `${root.replace(/\\/g, '/')}/${scope}/`;
});

const fileContainerLocalLocation = computed(() => {
  const containerId =
    fileScope.value === 'user' || selectedFileContainerId.value === USER_CONTAINER_ID
      ? USER_CONTAINER_ID
      : selectedFileContainerId.value;
  const mapped = String(desktopContainerRootMap.value[containerId] || '').trim();
  if (mapped) {
    return withTrailingSeparator(mapped);
  }
  const { root, separator } = resolveWorkspaceRootPrefix();
  const scope = resolveWorkspaceScopeSuffix();
  return `${root}${separator}${scope}${separator}`;
});

const workspacePanelKey = computed(() =>
  `${fileScope.value}:${selectedFileContainerId.value}:${selectedFileAgentIdForApi.value || 'default'}`
);

const fileContainerContextMenuStyle = computed(() => ({
  left: `${fileContainerContextMenu.value.x}px`,
  top: `${fileContainerContextMenu.value.y}px`
}));

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

const settingsAgentIdForPanel = computed(() => normalizeAgentId(settingsAgentId.value));
const isSettingsDefaultAgentReadonly = computed(() => false);

const settingsAgentIdForApi = computed(() => {
  const value = normalizeAgentId(settingsAgentId.value);
  return value === DEFAULT_AGENT_KEY ? '' : value;
});

const settingsRuntimeAgentIdForApi = computed(() => {
  const value = normalizeAgentId(settingsAgentId.value);
  if (value === DEFAULT_AGENT_KEY) {
    return '__default__';
  }
  return value;
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

const selectedBeeroomGroup = computed<BeeroomGroup | null>(
  () => beeroomStore.activeGroup || beeroomStore.activeGroupSummary || null
);

const beeroomWorkbenchMode = ref<'text' | 'canvas'>('text');

const showChatSettingsView = computed(() => sessionHub.activeSection !== 'messages');
const showHelperAppsWorkspace = computed(
  () => sessionHub.activeSection === 'groups' && helperAppsWorkspaceMode.value
);
const showChatComposerFooter = computed(() => {
  const routeSection = resolveSectionFromRoute(route.path, route.query.section);
  if (routeSection !== 'messages') {
    return false;
  }
  return !showChatSettingsView.value && (isAgentConversationActive.value || isWorldConversationActive.value);
});

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

const showAgentGridOverview = computed(
  () => sessionHub.activeSection === 'agents' && agentOverviewMode.value === 'grid'
);

const agentOverviewCards = computed<AgentOverviewCard[]>(() => {
  const cards: AgentOverviewCard[] = [];
  const seen = new Set<string>();
  const pushCard = (agent: Record<string, unknown>, options: { shared?: boolean; isDefault?: boolean } = {}) => {
    const id = normalizeAgentId(agent?.id || DEFAULT_AGENT_KEY);
    if (!id || seen.has(id)) return;
    seen.add(id);
    const containerId = normalizeSandboxContainerId(agent?.sandbox_container_id);
    cards.push({
      id,
      name: String(agent?.name || id),
      description: String(agent?.description || ''),
      shared: options.shared === true,
      isDefault: options.isDefault === true,
      runtimeState: resolveAgentRuntimeState(id),
      hasCron: hasCronTask(id),
      hasChannelBinding: channelBoundAgentIds.value.has(id),
      containerId,
      userRounds: resolveAgentUserRounds(id)
    });
  };

  pushCard(
    {
      id: DEFAULT_AGENT_KEY,
      name: t('messenger.defaultAgent'),
      description: t('messenger.defaultAgentDesc'),
      sandbox_container_id: 1
    },
    { isDefault: true }
  );
  filteredOwnedAgents.value.forEach((item) => pushCard(item as Record<string, unknown>));
  filteredSharedAgents.value.forEach((item) => pushCard(item as Record<string, unknown>, { shared: true }));
  return cards;
});

const normalizeUiFontSize = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 14;
  return Math.min(20, Math.max(12, Math.round(parsed)));
};

const normalizeMessengerSendKey = (value: unknown): MessengerSendKeyMode =>
  (() => {
    const text = String(value || '').trim().toLowerCase();
    if (text === 'enter') return 'enter';
    if (text === 'none' || text === 'off' || text === 'disabled') return 'none';
    return 'ctrl_enter';
  })();

const applyUiFontSize = (value: number) => {
  if (typeof document === 'undefined') return;
  const normalized = normalizeUiFontSize(value);
  document.documentElement.style.setProperty('--messenger-font-size', `${normalized}px`);
  document.documentElement.style.setProperty('--messenger-font-scale', String(normalized / 14));
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

const buildCurrentUserFallbackUnitTree = (): UnitTreeNode[] => {
  const user = authStore.user as Record<string, unknown> | null;
  const unitId = normalizeUnitText(user?.unit_id || user?.unitId);
  if (!unitId) return [];
  const label = normalizeUnitShortLabel(
    user?.unit_name ||
      user?.unitName ||
      user?.unit_display_name ||
      user?.unitDisplayName ||
      user?.path_name ||
      user?.pathName ||
      user?.unit_path ||
      user?.unitPath
  );
  return [
    {
      id: unitId,
      label: label || unitId,
      parentId: '',
      sortOrder: 0,
      children: []
    }
  ];
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

const contactUnitTreeRows = computed<UnitTreeRow[]>(() => {
  const directCountMap = contactUnitDirectCountMap.value;
  const treeRows = buildUnitTreeRows(orgUnitTree.value, 0, directCountMap, isContactUnitExpanded).rows;
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

const contactVirtualRange = computed(() => {
  const total = filteredContacts.value.length;
  if (!total) {
    return { start: 0, end: 0 };
  }
  const viewportHeight =
    contactVirtualViewportHeight.value ||
    contactVirtualListRef.value?.clientHeight ||
    CONTACT_VIRTUAL_ITEM_HEIGHT * 8;
  const start = Math.max(
    0,
    Math.floor(contactVirtualScrollTop.value / CONTACT_VIRTUAL_ITEM_HEIGHT) - CONTACT_VIRTUAL_OVERSCAN
  );
  const visibleCount = Math.ceil(viewportHeight / CONTACT_VIRTUAL_ITEM_HEIGHT) + CONTACT_VIRTUAL_OVERSCAN * 2;
  const end = Math.min(total, start + visibleCount);
  return { start, end };
});

const visibleFilteredContacts = computed(() =>
  filteredContacts.value.slice(contactVirtualRange.value.start, contactVirtualRange.value.end)
);

const contactVirtualTopPadding = computed(() => contactVirtualRange.value.start * CONTACT_VIRTUAL_ITEM_HEIGHT);

const contactVirtualBottomPadding = computed(() => {
  const remaining = filteredContacts.value.length - contactVirtualRange.value.end;
  return Math.max(0, remaining * CONTACT_VIRTUAL_ITEM_HEIGHT);
});

const filteredGroups = computed(() => {
  const text = keyword.value.toLowerCase();
  return (Array.isArray(userWorldStore.groups) ? userWorldStore.groups : []).filter((item) => {
    const name = String(item?.group_name || '').toLowerCase();
    const groupId = String(item?.group_id || '').toLowerCase();
    return !text || name.includes(text) || groupId.includes(text);
  });
});

const filteredBeeroomGroups = computed(() => {
  const text = keyword.value.toLowerCase();
  return (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).filter((item) => {
    const name = String(item?.name || '').toLowerCase();
    const groupId = String(item?.group_id || item?.hive_id || '').toLowerCase();
    const description = String(item?.description || '').toLowerCase();
    return !text || name.includes(text) || groupId.includes(text) || description.includes(text);
  });
});

const beeroomGroupOptions = computed(() =>
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).map((item) => ({
    group_id: String(item?.group_id || item?.hive_id || '').trim(),
    name: String(item?.name || item?.group_id || item?.hive_id || '').trim()
  }))
);

const preferredBeeroomGroupId = computed(() => {
  const activeHiveId = String(
    (activeAgent.value as Record<string, unknown> | null)?.hive_id ||
      (activeAgent.value as Record<string, unknown> | null)?.hiveId ||
      ''
  ).trim();
  if (activeHiveId) return activeHiveId;
  const selectedAgent = ownedAgents.value.find(
    (item) => normalizeAgentId(item?.id) === normalizeAgentId(selectedAgentId.value)
  );
  const selectedHiveId = String(selectedAgent?.hive_id || selectedAgent?.hiveId || '').trim();
  if (selectedHiveId) return selectedHiveId;
  const defaultGroup = beeroomStore.groups.find((item) => item.is_default);
  return String(defaultGroup?.group_id || defaultGroup?.hive_id || '').trim();
});

const defaultAgentCreateBeeroomGroupId = computed(() => {
  if (sessionHub.activeSection === 'swarms') {
    return String(beeroomStore.activeGroupId || '').trim();
  }
  return preferredBeeroomGroupId.value;
});

const beeroomCandidateAgents = computed(() => {
  const currentGroupId = String(beeroomStore.activeGroupId || '').trim();
  const memberIds = new Set(
    beeroomStore.activeAgents.map((item) => String(item?.agent_id || '').trim()).filter(Boolean)
  );
  return ownedAgents.value
    .filter((item) => normalizeAgentId(item?.id) !== DEFAULT_AGENT_KEY)
    .filter((item) => {
      if (!currentGroupId) return true;
      const agentHiveId = String(item?.hive_id || item?.hiveId || '').trim();
      const agentId = String(item?.id || '').trim();
      return agentHiveId !== currentGroupId && !memberIds.has(agentId);
    })
    .map((item) => ({
      id: String(item?.id || '').trim(),
      name: String(item?.name || item?.id || '').trim()
    }))
    .filter((item) => item.id);
});

const agentCreateCopyFromAgents = computed(() => {
  const seen = new Set<string>();
  return [...ownedAgents.value, ...sharedAgents.value]
    .map((item) => ({
      id: String(item?.id || '').trim(),
      name: String(item?.name || item?.id || '').trim()
    }))
    .filter((item) => {
      if (!item.id || seen.has(item.id)) return false;
      seen.add(item.id);
      return true;
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
  return '';
});

const adminToolGroups = computed(() => [
  { key: 'builtin', title: t('toolManager.system.builtin'), items: builtinTools.value },
  { key: 'mcp', title: t('toolManager.system.mcp'), items: mcpTools.value },
  { key: 'skills', title: t('toolManager.system.skills'), items: skillTools.value },
  { key: 'knowledge', title: t('toolManager.system.knowledge'), items: knowledgeTools.value }
]);

const resolveAdminToolDetail = (item: ToolEntry): string => {
  const name = String(item?.name || '').trim();
  const description = String(item?.description || '').trim();
  const detail = description || t('common.noDescription');
  return name ? `${name}\n${detail}` : detail;
};

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
        unread: Math.max(0, Math.floor(Number(agentMainUnreadCountMap.value[agentId] || 0))),
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

  const entries = [...agentItems, ...worldItems];
  if (desktopShowFirstLaunchDefaultAgentHint.value && !entries.length) {
    const defaultAgent = agentMap.value.get(DEFAULT_AGENT_KEY) || null;
    entries.push({
      key: `agent:${DEFAULT_AGENT_KEY}`,
      kind: 'agent',
      sourceId: '',
      agentId: DEFAULT_AGENT_KEY,
      title: String((defaultAgent as Record<string, unknown> | null)?.name || t('messenger.defaultAgent')),
      preview: t('messenger.defaultAgentDesc'),
      unread: 0,
      lastAt: desktopFirstLaunchDefaultAgentHintAt.value || Date.now()
    } as MixedConversation);
  }

  return entries.sort((left, right) => right.lastAt - left.lastAt);
});

const filteredMixedConversations = computed(() => {
  const text = keyword.value.toLowerCase();
  return mixedConversations.value.filter((item) => {
    if (!text) return true;
    return item.title.toLowerCase().includes(text) || item.preview.toLowerCase().includes(text);
  });
});

const hasAnyMixedConversations = computed(() => mixedConversations.value.length > 0);

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

const generalSettingsPanelMode = computed<'general' | 'profile'>(() =>
  settingsPanelMode.value === 'profile' ? 'profile' : 'general'
);

const chatPanelTitle = computed(() => {
  if (!showChatSettingsView.value) {
    return activeConversationTitle.value;
  }
  if (showAgentGridOverview.value) {
    return t('messenger.agent.overviewTitle');
  }
  if (showAgentSettingsPanel.value) {
    if (settingsAgentId.value === DEFAULT_AGENT_KEY) {
      return t('messenger.defaultAgent');
    }
    const target = agentMap.value.get(normalizeAgentId(settingsAgentId.value));
    return String(target?.name || settingsAgentId.value || t('messenger.section.agents'));
  }
  if (showHelperAppsWorkspace.value) {
    return helperAppsActiveTitle.value || '';
  }
  if (sessionHub.activeSection === 'users') {
    return String(selectedContact.value?.username || selectedContact.value?.user_id || t('messenger.section.users'));
  }
  if (sessionHub.activeSection === 'groups') {
    return String(selectedGroup.value?.group_name || selectedGroup.value?.group_id || t('messenger.section.groups'));
  }
  if (sessionHub.activeSection === 'tools') {
    if (selectedToolCategory.value) return toolCategoryLabel(selectedToolCategory.value);
  }
  if (sessionHub.activeSection === 'more') {
    if (settingsPanelMode.value === 'profile') return t('user.profile.enter');
    if (settingsPanelMode.value === 'prompts') return t('messenger.prompt.title');
    if (settingsPanelMode.value === 'desktop-models') return t('desktop.system.llm');
    if (settingsPanelMode.value === 'desktop-lan') return t('desktop.system.lan.title');
    if (settingsPanelMode.value === 'desktop-remote') return t('desktop.system.remote.title');
  }
  return activeSectionTitle.value;
});

const chatPanelSubtitle = computed(() => {
  if (!showChatSettingsView.value) {
    return activeConversationSubtitle.value;
  }
  if (showAgentGridOverview.value) {
    return t('messenger.agent.overviewDesc');
  }
  if (showAgentSettingsPanel.value) {
    return t('messenger.agent.subtitle');
  }
  if (showHelperAppsWorkspace.value) {
    return helperAppsActiveDescription.value || '';
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
    if (settingsPanelMode.value === 'prompts') return t('messenger.prompt.desc');
    if (settingsPanelMode.value === 'desktop-models') return t('desktop.system.llmHint');
    if (settingsPanelMode.value === 'desktop-lan') return t('desktop.system.lan.hint');
    if (settingsPanelMode.value === 'desktop-remote') return t('desktop.system.remote.hint');
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
    !worldVoiceRecording.value &&
    Boolean(worldDraft.value.trim())
);

const worldContainerPickerPathLabel = computed(() =>
  worldContainerPickerPath.value ? `/${worldContainerPickerPath.value}` : '/'
);

const worldContainerPickerDisplayEntries = computed(() => {
  const keyword = String(worldContainerPickerKeyword.value || '').trim().toLowerCase();
  if (!keyword) {
    return worldContainerPickerEntries.value;
  }
  return worldContainerPickerEntries.value.filter((entry) => {
    const name = String(entry.name || '').toLowerCase();
    const path = String(entry.path || '').toLowerCase();
    return name.includes(keyword) || path.includes(keyword);
  });
});

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

const normalizeNumericMap = (value: unknown): Record<string, number> => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return {};
  }
  return Object.entries(value as Record<string, unknown>).reduce<Record<string, number>>((acc, [key, raw]) => {
    const normalizedKey = normalizeAgentId(key);
    const numeric = Number(raw);
    if (!normalizedKey || !Number.isFinite(numeric) || numeric <= 0) {
      return acc;
    }
    acc[normalizedKey] = Math.floor(numeric);
    return acc;
  }, {});
};

const resolveAgentUnreadStorageKeys = (userId: unknown) => {
  const cleaned = String(userId || '').trim() || 'anonymous';
  return {
    readAt: `${AGENT_MAIN_READ_AT_STORAGE_PREFIX}:${cleaned}`,
    unread: `${AGENT_MAIN_UNREAD_STORAGE_PREFIX}:${cleaned}`
  };
};

const persistAgentUnreadState = () => {
  if (typeof window === 'undefined') return;
  const { readAt, unread } = agentUnreadStorageKeys.value;
  if (!readAt || !unread) return;
  try {
    window.localStorage.setItem(readAt, JSON.stringify(agentMainReadAtMap.value));
    window.localStorage.setItem(unread, JSON.stringify(agentMainUnreadCountMap.value));
  } catch {
    // ignore localStorage errors
  }
};

const ensureAgentUnreadState = (force = false) => {
  if (typeof window === 'undefined') {
    agentMainReadAtMap.value = {};
    agentMainUnreadCountMap.value = {};
    agentUnreadStorageKeys.value = { readAt: '', unread: '' };
    return;
  }
  const targetKeys = resolveAgentUnreadStorageKeys(currentUserId.value);
  const currentKeys = agentUnreadStorageKeys.value;
  if (!force && currentKeys.readAt === targetKeys.readAt && currentKeys.unread === targetKeys.unread) {
    return;
  }
  agentUnreadStorageKeys.value = targetKeys;
  try {
    const readRaw = window.localStorage.getItem(targetKeys.readAt);
    const unreadRaw = window.localStorage.getItem(targetKeys.unread);
    agentMainReadAtMap.value = readRaw ? normalizeNumericMap(JSON.parse(readRaw)) : {};
    agentMainUnreadCountMap.value = unreadRaw ? normalizeNumericMap(JSON.parse(unreadRaw)) : {};
  } catch {
    agentMainReadAtMap.value = {};
    agentMainUnreadCountMap.value = {};
  }
};

type AgentMainSessionEntry = {
  agentId: string;
  sessionId: string;
  lastAt: number;
};

const collectMainAgentSessionEntries = (): AgentMainSessionEntry[] => {
  const grouped = new Map<string, Array<Record<string, unknown>>>();
  (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).forEach((sessionRaw) => {
    const session = (sessionRaw || {}) as Record<string, unknown>;
    const agentId = normalizeAgentId(session.agent_id);
    if (!grouped.has(agentId)) {
      grouped.set(agentId, []);
    }
    grouped.get(agentId)?.push(session);
  });
  return Array.from(grouped.entries())
    .map(([agentId, sessions]) => {
      const sorted = [...sessions].sort(
        (left, right) =>
          normalizeTimestamp(right.updated_at || right.last_message_at || right.created_at) -
          normalizeTimestamp(left.updated_at || left.last_message_at || left.created_at)
      );
      const main = sorted.find((item) => Boolean(item?.is_main)) || sorted[0];
      const sessionId = String(main?.id || '').trim();
      if (!sessionId) {
        return null;
      }
      return {
        agentId,
        sessionId,
        lastAt: normalizeTimestamp(main?.last_message_at || main?.updated_at || main?.created_at)
      } as AgentMainSessionEntry;
    })
    .filter((item): item is AgentMainSessionEntry => Boolean(item));
};

const setAgentMainUnreadCount = (agentId: string, count: number) => {
  const normalizedAgentId = normalizeAgentId(agentId);
  const normalizedCount = Math.max(0, Math.floor(Number(count) || 0));
  const current = Math.max(0, Math.floor(Number(agentMainUnreadCountMap.value[normalizedAgentId] || 0)));
  if (current === normalizedCount) return;
  agentMainUnreadCountMap.value = {
    ...agentMainUnreadCountMap.value,
    [normalizedAgentId]: normalizedCount
  };
};

const setAgentMainReadAt = (agentId: string, timestamp: number) => {
  const normalizedAgentId = normalizeAgentId(agentId);
  const normalizedTimestamp = Math.max(0, Math.floor(Number(timestamp) || 0));
  if (!normalizedTimestamp) return;
  const current = Math.max(0, Math.floor(Number(agentMainReadAtMap.value[normalizedAgentId] || 0)));
  if (current >= normalizedTimestamp) return;
  agentMainReadAtMap.value = {
    ...agentMainReadAtMap.value,
    [normalizedAgentId]: normalizedTimestamp
  };
};

const trimAgentMainUnreadState = (entries: AgentMainSessionEntry[]) => {
  const validAgentIds = new Set(entries.map((item) => item.agentId));
  const trimmedReadAt = Object.entries(agentMainReadAtMap.value).reduce<Record<string, number>>((acc, [key, raw]) => {
    const agentId = normalizeAgentId(key);
    if (!validAgentIds.has(agentId)) return acc;
    const value = Math.max(0, Math.floor(Number(raw) || 0));
    if (!value) return acc;
    acc[agentId] = value;
    return acc;
  }, {});
  const trimmedUnread = Object.entries(agentMainUnreadCountMap.value).reduce<Record<string, number>>(
    (acc, [key, raw]) => {
      const agentId = normalizeAgentId(key);
      if (!validAgentIds.has(agentId)) return acc;
      const value = Math.max(0, Math.floor(Number(raw) || 0));
      if (!value) return acc;
      acc[agentId] = value;
      return acc;
    },
    {}
  );
  agentMainReadAtMap.value = trimmedReadAt;
  agentMainUnreadCountMap.value = trimmedUnread;
};

const refreshAgentMainUnreadCount = async (entry: AgentMainSessionEntry, readAt: number) => {
  const requestKey = `${entry.agentId}:${entry.sessionId}:${readAt}`;
  if (agentUnreadRefreshInFlight.has(requestKey)) {
    return;
  }
  agentUnreadRefreshInFlight.add(requestKey);
  try {
    const response = await getChatSessionApi(entry.sessionId);
    const messages = Array.isArray(response?.data?.data?.messages) ? response.data.data.messages : [];
    const unreadCount = messages.filter((message: Record<string, unknown>) => {
      if (String(message?.role || '') !== 'assistant') {
        return false;
      }
      const timestamp = normalizeTimestamp(message?.created_at);
      return timestamp > readAt;
    }).length;
    const activeEntries = collectMainAgentSessionEntries();
    const currentMain = activeEntries.find((item) => item.agentId === entry.agentId);
    if (!currentMain || currentMain.sessionId !== entry.sessionId) {
      return;
    }
    const currentReadAt = Math.max(0, Math.floor(Number(agentMainReadAtMap.value[entry.agentId] || 0)));
    if (currentReadAt !== readAt) {
      return;
    }
    if (currentMain.lastAt <= currentReadAt) {
      setAgentMainUnreadCount(entry.agentId, 0);
      persistAgentUnreadState();
      return;
    }
    setAgentMainUnreadCount(entry.agentId, unreadCount > 0 ? unreadCount : 1);
    persistAgentUnreadState();
  } catch {
    // unread refresh is best-effort; keep previous value if request fails
  } finally {
    agentUnreadRefreshInFlight.delete(requestKey);
  }
};

const refreshAgentMainUnreadFromSessions = () => {
  const entries = collectMainAgentSessionEntries();
  trimAgentMainUnreadState(entries);
  const identity = activeConversation.value;
  entries.forEach((entry) => {
    const isViewingMain =
      identity?.kind === 'agent' &&
      String(identity?.id || '').trim() === entry.sessionId &&
      normalizeAgentId(identity?.agentId) === entry.agentId;
    if (isViewingMain) {
      const targetReadAt = entry.lastAt || Date.now();
      setAgentMainReadAt(entry.agentId, targetReadAt);
      setAgentMainUnreadCount(entry.agentId, 0);
      return;
    }
    const readAt = Math.max(0, Math.floor(Number(agentMainReadAtMap.value[entry.agentId] || 0)));
    if (!readAt) {
      setAgentMainReadAt(entry.agentId, entry.lastAt || Date.now());
      setAgentMainUnreadCount(entry.agentId, 0);
      return;
    }
    if (entry.lastAt <= readAt) {
      setAgentMainUnreadCount(entry.agentId, 0);
      return;
    }
    void refreshAgentMainUnreadCount(entry, readAt);
  });
  persistAgentUnreadState();
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

const activeWorldConversationId = computed(() => {
  if (!isWorldConversationActive.value) return '';
  return String(activeConversation.value?.id || '').trim();
});

const buildWorldDraftKey = (conversationId: unknown): string => {
  const normalizedConversationId = String(conversationId || '').trim();
  if (!normalizedConversationId) return '';
  return `messenger:world:${resolveCurrentUserScope()}:${normalizedConversationId}`;
};

const readWorldDraft = (conversationId: unknown): string => {
  const draftKey = buildWorldDraftKey(conversationId);
  if (!draftKey) return '';
  return String(worldDraftMap.get(draftKey) || '');
};

const writeWorldDraft = (conversationId: unknown, value: unknown) => {
  const draftKey = buildWorldDraftKey(conversationId);
  if (!draftKey) return;
  const normalized = String(value || '');
  if (!normalized.trim()) {
    worldDraftMap.delete(draftKey);
    return;
  }
  worldDraftMap.set(draftKey, normalized);
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

const worldHistoryRecords = computed<WorldHistoryRecord[]>(() => {
  const messages = Array.isArray(userWorldStore.activeMessages) ? userWorldStore.activeMessages : [];
  return messages
    .slice()
    .reverse()
    .map((item, index) => {
      const source = item as Record<string, unknown>;
      const rawContent = String(source.content || '').trim();
      if (!rawContent) return null;
      const category = classifyWorldHistoryMessage(source);
      const preview = normalizeWorldHistoryText(rawContent).slice(0, 260) || t('messenger.preview.empty');
      const messageId = Number.parseInt(String(source.message_id || ''), 10);
      const createdAt = normalizeWorldMessageTimestamp(source.created_at);
      return {
        key: `history:${source.message_id || index}:${createdAt}`,
        messageId: Number.isFinite(messageId) ? messageId : 0,
        sender: resolveWorldMessageSender(source),
        createdAt,
        preview,
        rawContent,
        category,
        icon: resolveWorldHistoryIcon(category)
      } as WorldHistoryRecord;
    })
    .filter((item): item is WorldHistoryRecord => Boolean(item));
});

const worldHistoryTabOptions = computed(() => [
  { key: 'all' as WorldHistoryCategory, label: t('messenger.world.historyTabAll') },
  { key: 'media' as WorldHistoryCategory, label: t('messenger.world.historyTabMedia') },
  { key: 'document' as WorldHistoryCategory, label: t('messenger.world.historyTabDocument') },
  { key: 'other_file' as WorldHistoryCategory, label: t('messenger.world.historyTabOtherFile') }
]);

const filteredWorldHistoryRecords = computed(() => {
  const keyword = String(worldHistoryKeyword.value || '').trim().toLowerCase();
  const [rangeStartRaw, rangeEndRaw] = Array.isArray(worldHistoryDateRange.value)
    ? worldHistoryDateRange.value
    : [];
  const rangeStart = Number(rangeStartRaw);
  const rangeEnd = Number(rangeEndRaw);
  const hasDateRange = Number.isFinite(rangeStart) && Number.isFinite(rangeEnd);
  return worldHistoryRecords.value.filter((item) => {
    if (worldHistoryActiveTab.value !== 'all' && item.category !== worldHistoryActiveTab.value) {
      return false;
    }
    if (keyword) {
      const haystack = `${item.preview}\n${item.rawContent}\n${item.sender}`.toLowerCase();
      if (!haystack.includes(keyword)) {
        return false;
      }
    }
    if (hasDateRange && item.createdAt > 0) {
      const safeStart = Math.min(rangeStart, rangeEnd);
      const safeEnd = Math.max(rangeStart, rangeEnd) + 24 * 60 * 60 * 1000 - 1;
      if (item.createdAt < safeStart || item.createdAt > safeEnd) {
        return false;
      }
    }
    return true;
  });
});

const worldEmojiCatalog = computed(() =>
  WORLD_EMOJI_CATALOG.filter((emoji) => !worldRecentEmojis.value.includes(emoji))
);

const clampWorldComposerHeight = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 188;
  return Math.min(340, Math.max(168, Math.round(parsed)));
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
  worldHistoryKeyword.value = '';
  worldHistoryActiveTab.value = 'all';
  worldHistoryDateRange.value = [];
  worldHistoryDialogVisible.value = true;
};

const resolveWorldContainerPickerParent = (path: string): string => {
  const normalized = normalizeUploadPath(path);
  if (!normalized) return '';
  const pivot = normalized.lastIndexOf('/');
  if (pivot < 0) return '';
  return normalized.slice(0, pivot);
};

const normalizeWorldContainerPickerEntry = (raw: unknown): WorldContainerPickerEntry | null => {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return null;
  }
  const source = raw as Record<string, unknown>;
  const path = normalizeUploadPath(source.path);
  if (!path) {
    return null;
  }
  const rawName = String(source.name || '').trim();
  const fallbackName = path.split('/').pop() || path;
  const normalizedType = String(source.type || '').toLowerCase();
  const isDirectory = normalizedType === 'dir' || normalizedType === 'directory' || normalizedType === 'folder';
  return {
    path,
    name: rawName || fallbackName,
    type: isDirectory ? 'dir' : 'file'
  };
};

const sortWorldContainerPickerEntries = (
  left: WorldContainerPickerEntry,
  right: WorldContainerPickerEntry
): number => {
  if (left.type !== right.type) {
    return left.type === 'dir' ? -1 : 1;
  }
  return left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' });
};

const loadWorldContainerPickerEntries = async (path: string) => {
  const normalizedPath = normalizeUploadPath(path);
  worldContainerPickerLoading.value = true;
  try {
    const { data } = await fetchWunderWorkspaceContent({
      path: normalizedPath,
      include_content: true,
      depth: 1,
      container_id: USER_CONTAINER_ID
    });
    const payload = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
    const payloadRecord = payload as Record<string, unknown>;
    worldContainerPickerPath.value = normalizeUploadPath(
      payloadRecord.path ?? normalizedPath
    );
    const rawEntries = payloadRecord.entries;
    const entries = Array.isArray(rawEntries) ? rawEntries : [];
    worldContainerPickerEntries.value = entries
      .map((entry) => normalizeWorldContainerPickerEntry(entry))
      .filter((entry): entry is WorldContainerPickerEntry => Boolean(entry))
      .sort(sortWorldContainerPickerEntries);
  } catch (error) {
    worldContainerPickerEntries.value = [];
    showApiError(error, t('userWorld.attachments.pickFailed'));
  } finally {
    worldContainerPickerLoading.value = false;
  }
};

const openWorldContainerPickerPath = async (path: string) => {
  worldContainerPickerKeyword.value = '';
  await loadWorldContainerPickerEntries(path);
};

const openWorldContainerPicker = async () => {
  if (!isWorldConversationActive.value || worldUploading.value) return;
  worldQuickPanelMode.value = '';
  worldContainerPickerVisible.value = true;
  await openWorldContainerPickerPath(worldContainerPickerPath.value);
};

const openWorldContainerPickerParent = () => {
  if (worldContainerPickerLoading.value || !worldContainerPickerPath.value) return;
  const parentPath = resolveWorldContainerPickerParent(worldContainerPickerPath.value);
  void openWorldContainerPickerPath(parentPath);
};

const refreshWorldContainerPicker = () => {
  if (worldContainerPickerLoading.value) return;
  void loadWorldContainerPickerEntries(worldContainerPickerPath.value);
};

const handleWorldContainerPickerEntry = (entry: WorldContainerPickerEntry) => {
  if (entry.type === 'dir') {
    void openWorldContainerPickerPath(entry.path);
    return;
  }
  appendWorldAttachmentTokens([entry.path]);
  worldContainerPickerVisible.value = false;
  focusWorldTextareaToEnd();
};

const rememberWorldEmoji = (emoji: string) => {
  const cleaned = String(emoji || '').trim();
  if (!cleaned) return;
  worldRecentEmojis.value = [cleaned, ...worldRecentEmojis.value.filter((item) => item !== cleaned)].slice(0, 12);
  saveStoredStringArray(WORLD_QUICK_EMOJI_STORAGE_KEY, worldRecentEmojis.value);
};

const focusWorldTextareaToEnd = () => {
  nextTick(() => {
    const textarea = worldComposerViewRef.value?.getTextareaElement() || null;
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

const locateWorldHistoryMessage = async (entry: WorldHistoryRecord) => {
  const targetId = resolveWorldMessageDomId({ message_id: entry.messageId });
  worldHistoryDialogVisible.value = false;
  if (shouldVirtualizeMessages.value && isWorldConversationActive.value) {
    const targetIndex = worldRenderableMessages.value.findIndex((item) => item.domId === targetId);
    if (targetIndex >= 0) {
      scrollVirtualMessageToIndex(
        worldRenderableMessages.value.map((item) => item.key),
        targetIndex,
        'center'
      );
      await nextTick();
    }
  }
  await nextTick();
  const target = typeof document !== 'undefined' ? document.getElementById(targetId) : null;
  if (!target) return;
  target.scrollIntoView({ behavior: 'smooth', block: 'center' });
  target.classList.add('is-history-target');
  window.setTimeout(() => {
    target.classList.remove('is-history-target');
  }, 1400);
  scheduleMessageVirtualMeasure();
};

const closeWorldQuickPanelWhenOutside = (event: Event) => {
  const target = event.target as Node | null;
  if (!target) {
    return;
  }
  if (fileContainerContextMenu.value.visible) {
    const menu = fileContainerMenuViewRef.value?.getMenuElement() || null;
    if (!menu || !menu.contains(target)) {
      closeFileContainerMenu();
    }
  }
  if (worldQuickPanelMode.value) {
    const composerElement = worldComposerViewRef.value?.getComposerElement() || null;
    if (!composerElement || !composerElement.contains(target)) {
      clearWorldQuickPanelClose();
      worldQuickPanelMode.value = '';
    }
  }

  if (isRightDockOverlay.value && showRightDock.value && !rightDockCollapsed.value) {
    const pointerEvent = event as PointerEvent | null;
    const isSecondaryClick = Boolean(pointerEvent && typeof pointerEvent.button === 'number' && pointerEvent.button === 2);
    const targetElement = target instanceof Element ? target : null;
    const rightDockElement = rightDockRef.value?.$el || null;
    const hitInsideRightDock = Boolean(
      (rightDockElement && rightDockElement.contains(target)) ||
      targetElement?.closest('.messenger-right-dock') ||
      targetElement?.closest('.messenger-files-context-menu') ||
      targetElement?.closest('.workspace-context-menu')
    );
    if (!isSecondaryClick && !hitInsideRightDock) {
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

const activeWorldGroupId = computed(() => {
  if (!isWorldConversationActive.value) return '';
  const conversationId = String(activeWorldConversationId.value || '').trim();
  if (!conversationId) return '';
  const conversation = userWorldStore.conversations.find(
    (item) => String(item?.conversation_id || '').trim() === conversationId
  );
  if (String(conversation?.conversation_type || '').trim().toLowerCase() !== 'group') {
    return '';
  }
  const fallbackGroup = userWorldStore.groups.find(
    (item) => String(item?.conversation_id || '').trim() === conversationId
  );
  return (
    String(conversation?.group_id || '').trim() ||
    String(fallbackGroup?.group_id || '').trim() ||
    String(selectedGroup.value?.group_id || '').trim()
  );
});

const showAgentRightDock = computed(() => {
  if (sessionHub.activeSection === 'agents') return !showAgentGridOverview.value;
  return sessionHub.activeSection === 'messages' && isAgentConversationActive.value;
});

const showGroupRightDock = computed(
  () =>
    sessionHub.activeSection === 'messages' &&
    isWorldConversationActive.value &&
    Boolean(activeWorldGroupId.value)
);

const showRightDock = computed(() => showAgentRightDock.value || showGroupRightDock.value);

const showRightAgentPanels = computed(() => showAgentRightDock.value);

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
  if (!showAgentRightDock.value) return [];
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
  if (
    pendingQuestion ||
    raw === 'pending_question' ||
    raw === 'pending-question' ||
    raw === 'pending_confirm' ||
    raw === 'pending-confirm' ||
    raw === 'pending_confirmation' ||
    raw === 'awaiting_confirmation' ||
    raw === 'awaiting-confirmation' ||
    raw === 'awaiting_approval' ||
    raw === 'awaiting-approval' ||
    raw === 'approval_pending' ||
    raw === 'approval-pending' ||
    raw === 'pending' ||
    raw === 'waiting' ||
    raw === 'queued' ||
    raw === 'await_confirm' ||
    raw === 'question' ||
    raw === 'questioning' ||
    raw === 'asking'
  ) {
    return 'pending';
  }
  if (
    raw === 'running' ||
    raw === 'executing' ||
    raw === 'processing' ||
    raw === 'cancelling'
  ) {
    return 'running';
  }
  if (raw === 'done' || raw === 'completed' || raw === 'finish' || raw === 'finished') return 'done';
  if (raw === 'error' || raw === 'failed' || raw === 'timeout' || raw === 'aborted' || raw === 'terminated') {
    return 'error';
  }
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
  if (pendingApprovalAgentIdSet.value.has(key)) {
    return 'pending';
  }
  const inquiryAgentId = activeAgentInquiryPanel.value
    ? normalizeAgentId(activeAgentId.value || selectedAgentId.value)
    : '';
  if (inquiryAgentId && inquiryAgentId === key) {
    return 'pending';
  }
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

const normalizeAgentUserRoundsKey = (value: unknown): string => {
  const raw = String(value || '').trim();
  if (!raw) return DEFAULT_AGENT_KEY;
  return normalizeAgentId(raw) || DEFAULT_AGENT_KEY;
};

const resolveAgentUserRounds = (agentId: unknown): number => {
  const key = normalizeAgentUserRoundsKey(agentId);
  return agentUserRoundsMap.value.get(key) ?? 0;
};

const formatUserRounds = (value: number): string => {
  const normalized = Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
  return normalized.toLocaleString();
};

const formatAgentRuntimeState = (state: AgentRuntimeState): string => {
  if (state === 'running') return t('portal.card.running');
  if (state === 'pending') return t('portal.card.waiting');
  if (state === 'done') return t('portal.card.done');
  if (state === 'error') return t('portal.card.error');
  return t('portal.card.idle');
};

let agentRuntimeStateSnapshot = new Map<string, AgentRuntimeState>();
let agentRuntimeStateHydrated = false;
let systemNotificationPermissionRequested = false;

const resolveAgentDisplayName = (agentId: string): string => {
  const normalized = normalizeAgentId(agentId);
  const agent = agentMap.value.get(normalized);
  const name = String(agent?.name || '').trim();
  if (name) return name;
  if (normalized === DEFAULT_AGENT_KEY) return t('messenger.defaultAgent');
  return normalized || t('messenger.defaultAgent');
};

const requestSystemNotificationPermission = async (): Promise<NotificationPermission | ''> => {
  if (systemNotificationPermissionRequested) {
    return typeof window !== 'undefined' ? window.Notification?.permission ?? '' : '';
  }
  systemNotificationPermissionRequested = true;
  if (typeof window === 'undefined' || !('Notification' in window)) return '';
  try {
    return await window.Notification.requestPermission();
  } catch {
    return '';
  }
};

const sendDesktopNotification = async (title: string, body: string): Promise<boolean> => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.notify !== 'function') return false;
  try {
    const result = await bridge.notify({ title, body });
    return result === true;
  } catch {
    return false;
  }
};

const sendSystemNotification = async (title: string, body: string): Promise<boolean> => {
  const desktopNotified = await sendDesktopNotification(title, body);
  if (desktopNotified) return true;
  if (typeof window === 'undefined' || !('Notification' in window)) return false;
  try {
    if (window.Notification.permission === 'granted') {
      new window.Notification(title, { body });
      return true;
    }
    if (window.Notification.permission === 'default') {
      const permission = await requestSystemNotificationPermission();
      if (permission === 'granted') {
        new window.Notification(title, { body });
        return true;
      }
    }
  } catch {
    return false;
  }
  return false;
};

const notifyAgentTaskCompleted = async (agentId: string) => {
  const title = t('messenger.agent.taskCompletedTitle');
  const message = t('messenger.agent.taskCompleted', { name: resolveAgentDisplayName(agentId) });
  if (agentHeaderModelJumpEnabled.value) {
    const notified = await sendSystemNotification(title, message);
    if (notified) return;
  }
  ElMessage.success(message);
};

const shouldNotifyAgentCompletion = (
  previousState: AgentRuntimeState,
  nextState: AgentRuntimeState
): boolean => {
  if (nextState === 'done') return previousState !== 'done';
  if (nextState === 'idle') return previousState === 'running' || previousState === 'pending';
  return false;
};

const handleAgentRuntimeStateUpdate = (stateMap: Map<string, AgentRuntimeState>) => {
  if (agentRuntimeStateHydrated) {
    const keys = new Set<string>([
      ...Array.from(agentRuntimeStateSnapshot.keys()),
      ...Array.from(stateMap.keys())
    ]);
    keys.forEach((agentId) => {
      const previousState = agentRuntimeStateSnapshot.get(agentId) ?? 'idle';
      const nextState = stateMap.get(agentId) ?? 'idle';
      if (previousState === nextState) return;
      if (shouldNotifyAgentCompletion(previousState, nextState)) {
        void notifyAgentTaskCompleted(agentId);
      }
    });
  }
  agentRuntimeStateSnapshot = new Map(stateMap);
  agentRuntimeStateHydrated = true;
  agentRuntimeStateMap.value = stateMap;
};

const hasMessageContent = (value: unknown): boolean => Boolean(String(value || '').trim());

const AUDIO_ATTACHMENT_EXTENSIONS = new Set(['mp3', 'wav', 'ogg', 'opus', 'aac', 'flac', 'm4a', 'webm']);

const resolveAttachmentContentType = (item: Record<string, unknown>): string => {
  const raw =
    String(item?.content_type ?? item?.mime_type ?? item?.mimeType ?? '')
      .trim()
      .toLowerCase();
  return raw;
};

const resolveAttachmentPublicPath = (item: Record<string, unknown>): string => {
  const rawPublic = String(item?.public_path ?? item?.publicPath ?? '').trim();
  if (rawPublic) {
    return parseWorkspaceResourceUrl(rawPublic)?.publicPath || '';
  }
  const rawContent = String(item?.content ?? '').trim();
  if (!rawContent || rawContent.startsWith('data:')) return '';
  return parseWorkspaceResourceUrl(rawContent)?.publicPath || '';
};

const isAudioPath = (path: string): boolean => {
  const value = String(path || '').trim();
  if (!value) return false;
  const suffix = value.split('?')[0].split('#')[0].split('.').pop();
  if (!suffix) return false;
  return AUDIO_ATTACHMENT_EXTENSIONS.has(suffix.toLowerCase());
};

const getUserAttachmentResourceState = (publicPath: string): AttachmentResourceState | null =>
  userAttachmentResourceCache.value.get(publicPath) || null;

const resolveUserImageAttachments = (message: Record<string, unknown>) => {
  const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
  return attachments
    .map((item, index) => {
      const record = (item || {}) as Record<string, unknown>;
      const content = String(record?.content || '').trim();
      const contentType = resolveAttachmentContentType(record);
      const publicPath = resolveAttachmentPublicPath(record);
      const isDataImage = content.startsWith('data:image/');
      const isWorkspaceImage =
        Boolean(publicPath) && (contentType.startsWith('image/') || isImagePath(publicPath));
      if (!isDataImage && !isWorkspaceImage) return null;
      const fallbackName = `image-${index + 1}`;
      const name = String(record?.name || fallbackName).trim() || fallbackName;
      let src = '';
      if (isDataImage) {
        src = content;
      }
      if (!src && publicPath) {
        const cached = getUserAttachmentResourceState(publicPath);
        if (cached?.objectUrl) {
          src = cached.objectUrl;
        } else if (cached?.error) {
          return null;
        }
      }
      if (!src) return null;
      return {
        key: `${name}-${index}`,
        src,
        name,
        workspacePath: publicPath || ''
      };
    })
    .filter(Boolean);
};

const resolveUserAudioAttachments = (message: Record<string, unknown>) => {
  const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
  return attachments
    .map((item, index) => {
      const record = (item || {}) as Record<string, unknown>;
      const content = String(record?.content || '').trim();
      const contentType = resolveAttachmentContentType(record);
      const publicPath = resolveAttachmentPublicPath(record);
      const isDataAudio = content.startsWith('data:audio/');
      const isWorkspaceAudio =
        Boolean(publicPath) && (contentType.startsWith('audio/') || isAudioPath(publicPath));
      if (!isDataAudio && !isWorkspaceAudio) return null;
      const fallbackName = `audio-${index + 1}`;
      const name = String(record?.name || fallbackName).trim() || fallbackName;
      let src = '';
      if (isDataAudio) {
        src = content;
      }
      if (!src && publicPath) {
        const cached = getUserAttachmentResourceState(publicPath);
        if (cached?.objectUrl) {
          src = cached.objectUrl;
        } else if (cached?.error) {
          return null;
        }
      }
      if (!src) return null;
      return {
        key: `${name}-${index}`,
        src,
        name,
        workspacePath: publicPath || ''
      };
    })
    .filter(Boolean);
};

const userAttachmentWorkspacePaths = computed(() => {
  const _ = currentUserId.value;
  const paths = new Set<string>();
  chatStore.messages.forEach((message) => {
    if (String((message as Record<string, unknown>)?.role || '') !== 'user') return;
    const attachments = Array.isArray((message as Record<string, unknown>)?.attachments)
      ? ((message as Record<string, unknown>).attachments as unknown[])
      : [];
    attachments.forEach((item) => {
      const record = (item || {}) as Record<string, unknown>;
      const publicPath = resolveAttachmentPublicPath(record);
      if (!publicPath) return;
      const content = String(record?.content || '').trim();
      if (content.startsWith('data:')) return;
      const contentType = resolveAttachmentContentType(record);
      const isImage = contentType.startsWith('image/') || isImagePath(publicPath);
      const isAudio = contentType.startsWith('audio/') || isAudioPath(publicPath);
      if (isImage || isAudio) {
        paths.add(publicPath);
      }
    });
  });
  return Array.from(paths);
});

const hasUserImageAttachments = (message: Record<string, unknown>): boolean =>
  resolveUserImageAttachments(message).length > 0;

const hasUserAudioAttachments = (message: Record<string, unknown>): boolean =>
  resolveUserAudioAttachments(message).length > 0;

const hasWorkflowOrThinking = (message: Record<string, unknown>): boolean =>
  Boolean(message?.workflowStreaming) ||
  Boolean(message?.reasoningStreaming) ||
  Boolean((message?.workflowItems as unknown[])?.length) ||
  hasMessageContent(message?.reasoning);

const shouldRenderAgentMessage = (message: Record<string, unknown>): boolean => {
  if (String(message?.role || '') === 'user') return true;
  return hasMessageContent(message?.content) || hasWorkflowOrThinking(message);
};

type AgentRenderableMessage = {
  key: string;
  sourceIndex: number;
  message: Record<string, unknown>;
};

type WorldRenderableMessage = {
  key: string;
  sourceIndex: number;
  domId: string;
  message: Record<string, unknown>;
};

type MessageVirtualWindow = {
  start: number;
  end: number;
  topSpacer: number;
  bottomSpacer: number;
  total: number;
};

const agentRenderableMessages = computed<AgentRenderableMessage[]>(() =>
  chatStore.messages.reduce<AgentRenderableMessage[]>((acc, rawMessage, sourceIndex) => {
    const message = (rawMessage || {}) as Record<string, unknown>;
    if (!shouldRenderAgentMessage(message)) {
      return acc;
    }
    acc.push({
      key: resolveAgentMessageKey(message, sourceIndex),
      sourceIndex,
      message
    });
    return acc;
  }, [])
);

const worldRenderableMessages = computed<WorldRenderableMessage[]>(() =>
  (Array.isArray(userWorldStore.activeMessages) ? userWorldStore.activeMessages : []).map((rawMessage, sourceIndex) => {
    const message = (rawMessage || {}) as Record<string, unknown>;
    return {
      key: resolveWorldMessageKey(message),
      sourceIndex,
      domId: resolveWorldMessageDomId(message),
      message
    };
  })
);

const activeVirtualMessageKeys = computed<string[]>(() => {
  if (isAgentConversationActive.value) {
    return agentRenderableMessages.value.map((item) => item.key);
  }
  if (isWorldConversationActive.value) {
    return worldRenderableMessages.value.map((item) => item.key);
  }
  return [];
});

const shouldVirtualizeMessages = computed(
  () =>
    !showChatSettingsView.value &&
    (isAgentConversationActive.value || isWorldConversationActive.value) &&
    activeVirtualMessageKeys.value.length > MESSAGE_VIRTUAL_THRESHOLD
);

const resolveVirtualMessageHeight = (key: string): number => {
  const normalized = String(key || '').trim();
  if (!normalized) {
    return MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
  }
  return messageVirtualHeightCache.get(normalized) || MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
};

const estimateVirtualOffsetTop = (keys: string[], index: number): number => {
  const safeIndex = Math.max(0, Math.min(keys.length, Math.trunc(index)));
  let offset = 0;
  for (let cursor = 0; cursor < safeIndex; cursor += 1) {
    offset += resolveVirtualMessageHeight(keys[cursor]);
    if (cursor < keys.length - 1) {
      offset += MESSAGE_VIRTUAL_GAP;
    }
  }
  return offset;
};

const estimateVirtualTotalHeight = (keys: string[]): number => {
  if (!keys.length) {
    return 0;
  }
  let total = 0;
  for (let cursor = 0; cursor < keys.length; cursor += 1) {
    total += resolveVirtualMessageHeight(keys[cursor]);
    if (cursor < keys.length - 1) {
      total += MESSAGE_VIRTUAL_GAP;
    }
  }
  return total;
};

const messageVirtualWindow = computed<MessageVirtualWindow>(() => {
  // Depend on measured height cache revisions.
  void messageVirtualLayoutVersion.value;
  const keys = activeVirtualMessageKeys.value;
  const total = keys.length;
  if (!total) {
    return { start: 0, end: 0, topSpacer: 0, bottomSpacer: 0, total: 0 };
  }
  if (!shouldVirtualizeMessages.value) {
    return { start: 0, end: total, topSpacer: 0, bottomSpacer: 0, total };
  }

  const viewportHeight =
    messageVirtualViewportHeight.value ||
    messageListRef.value?.clientHeight ||
    MESSAGE_VIRTUAL_ESTIMATED_HEIGHT * 6;
  const scrollTop = Math.max(0, messageVirtualScrollTop.value);

  let start = 0;
  let cursorTop = 0;
  while (start < total) {
    const height = resolveVirtualMessageHeight(keys[start]);
    const itemBottom = cursorTop + height;
    if (itemBottom >= scrollTop) {
      break;
    }
    cursorTop = itemBottom + MESSAGE_VIRTUAL_GAP;
    start += 1;
  }

  const overscanStart = Math.max(0, start - MESSAGE_VIRTUAL_OVERSCAN);
  let end = overscanStart;
  let covered = 0;
  const targetCoverage =
    viewportHeight + MESSAGE_VIRTUAL_OVERSCAN * MESSAGE_VIRTUAL_ESTIMATED_HEIGHT * 2;
  while (end < total && (covered < targetCoverage || end === overscanStart)) {
    covered += resolveVirtualMessageHeight(keys[end]) + MESSAGE_VIRTUAL_GAP;
    end += 1;
  }

  const offsetBeforeStart = estimateVirtualOffsetTop(keys, overscanStart);
  const topSpacer = overscanStart > 0
    ? Math.max(0, offsetBeforeStart - MESSAGE_VIRTUAL_GAP)
    : 0;
  const offsetBeforeEnd = estimateVirtualOffsetTop(keys, end);
  const totalHeight = estimateVirtualTotalHeight(keys);
  const bottomSpacer = Math.max(0, totalHeight - offsetBeforeEnd);
  return {
    start: overscanStart,
    end,
    topSpacer,
    bottomSpacer,
    total
  };
});

const visibleAgentMessages = computed<AgentRenderableMessage[]>(() => {
  const items = agentRenderableMessages.value;
  if (!isAgentConversationActive.value || !shouldVirtualizeMessages.value) {
    return items;
  }
  return items.slice(messageVirtualWindow.value.start, messageVirtualWindow.value.end);
});

const visibleWorldMessages = computed<WorldRenderableMessage[]>(() => {
  const items = worldRenderableMessages.value;
  if (!isWorldConversationActive.value || !shouldVirtualizeMessages.value) {
    return items;
  }
  return items.slice(messageVirtualWindow.value.start, messageVirtualWindow.value.end);
});

const messageVirtualTopSpacerHeight = computed(() =>
  shouldVirtualizeMessages.value ? messageVirtualWindow.value.topSpacer : 0
);

const messageVirtualBottomSpacerHeight = computed(() =>
  shouldVirtualizeMessages.value ? messageVirtualWindow.value.bottomSpacer : 0
);

const isGreetingMessage = (message: Record<string, unknown>): boolean =>
  String(message?.role || '') === 'assistant' && Boolean(message?.isGreeting);

const resolveMessageAgentAvatarState = (message: Record<string, unknown>): AgentRuntimeState => {
  if (String(message?.role || '') !== 'assistant') return 'idle';
  const questionPanelStatus = String(
    ((message?.questionPanel as Record<string, unknown> | null)?.status || '')
  )
    .trim()
    .toLowerCase();
  const pendingQuestion =
    questionPanelStatus === 'pending' ||
    Boolean(message?.pending_question) ||
    Boolean(message?.pendingQuestion) ||
    Boolean(message?.awaiting_confirmation) ||
    Boolean(message?.requires_confirmation);
  if (pendingQuestion) return 'pending';
  if (
    Boolean(message?.stream_incomplete) ||
    Boolean(message?.workflowStreaming) ||
    Boolean(message?.reasoningStreaming)
  ) {
    return 'running';
  }
  const messageState = normalizeRuntimeState(message?.state, pendingQuestion);
  if (messageState !== 'idle') return messageState;
  return 'done';
};

const buildMessageStatsEntries = (message: Record<string, unknown>) =>
  buildAssistantMessageStatsEntries(message as Record<string, any>, t);

const shouldShowMessageStats = (message: Record<string, unknown>): boolean =>
  buildMessageStatsEntries(message).length > 0;

const hasPlanSteps = (plan: unknown): boolean =>
  Array.isArray((plan as { steps?: unknown[] } | null)?.steps) &&
  ((plan as { steps?: unknown[] } | null)?.steps?.length || 0) > 0;

const isPlanMessageDismissed = (message: Record<string, unknown>): boolean =>
  dismissedPlanMessages.value.has(message);

const markPlanMessageDismissed = (message: Record<string, unknown>) => {
  dismissedPlanMessages.value.add(message);
  dismissedPlanVersion.value += 1;
};

const activeAgentPlanMessage = computed<Record<string, unknown> | null>(() => {
  // Trigger recompute when manual dismiss state changes.
  void dismissedPlanVersion.value;
  if (!isAgentConversationActive.value) return null;
  for (let index = chatStore.messages.length - 1; index >= 0; index -= 1) {
    const message = chatStore.messages[index] as Record<string, unknown> | undefined;
    if (String(message?.role || '') !== 'assistant') continue;
    if (!hasPlanSteps(message?.plan)) continue;
    if (message && isPlanMessageDismissed(message)) {
      return null;
    }
    return message || null;
  }
  return null;
});

const activeAgentPlan = computed(() => {
  const message = activeAgentPlanMessage.value as { plan?: unknown } | null;
  return message?.plan || null;
});

const dismissActiveAgentPlan = () => {
  const target = activeAgentPlanMessage.value;
  if (!target) return;
  markPlanMessageDismissed(target);
  agentPlanExpanded.value = false;
};

type AgentInquiryPanelRoute = { label: string; description?: string };
type AgentInquiryPanelData = { question?: string; routes?: AgentInquiryPanelRoute[]; status?: string };
type ActiveAgentInquiryPanel = { message: Record<string, unknown>; panel: AgentInquiryPanelData };

const activeAgentInquiryPanel = computed<ActiveAgentInquiryPanel | null>(() => {
  if (!isAgentConversationActive.value) return null;
  for (let index = chatStore.messages.length - 1; index >= 0; index -= 1) {
    const message = chatStore.messages[index] as Record<string, unknown> | undefined;
    if (String(message?.role || '') !== 'assistant') continue;
    const panel = (message?.questionPanel || null) as AgentInquiryPanelData | null;
    if (panel?.status === 'pending') {
      return {
        message: message || {},
        panel
      };
    }
  }
  return null;
});

const handleAgentInquirySelection = (selected: unknown) => {
  if (!Array.isArray(selected)) {
    agentInquirySelection.value = [];
    return;
  }
  agentInquirySelection.value = selected
    .map((item) => Number(item))
    .filter((item) => Number.isInteger(item) && item >= 0);
};

const resolveAgentInquirySelectionRoutes = (
  panel: AgentInquiryPanelData | null | undefined,
  selected: number[]
): AgentInquiryPanelRoute[] => {
  if (!panel || !Array.isArray(selected) || !selected.length) {
    return [];
  }
  return selected
    .map((index) => panel.routes?.[index])
    .filter((route): route is AgentInquiryPanelRoute => Boolean(route?.label));
};

const buildAgentInquiryReply = (panel: AgentInquiryPanelData, routes: AgentInquiryPanelRoute[]): string => {
  const header = t('chat.askPanelPrefix');
  const question = panel?.question ? t('chat.askPanelQuestion', { question: panel.question }) : '';
  const lines = routes.map((route) => {
    const detail = route.description ? `：${route.description}` : '';
    return `- ${route.label}${detail}`;
  });
  return [header, question, ...lines].filter(Boolean).join('\n');
};

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

const resolveOnlineFlag = (value: unknown): boolean => {
  if (typeof value === 'boolean') return value;
  if (typeof value === 'number') return Number.isFinite(value) && value > 0;
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    return normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'online';
  }
  return false;
};

const isContactOnline = (contact: unknown): boolean => {
  const source = (contact || {}) as Record<string, unknown>;
  return resolveOnlineFlag(source.online);
};

const formatContactPresence = (contact: unknown): string =>
  isContactOnline(contact) ? t('presence.online') : t('presence.offline');

const isAdminUser = (user: Record<string, unknown> | null): boolean =>
  Array.isArray(user?.roles) &&
  user.roles.some((role) => role === 'admin' || role === 'super_admin');

const normalizeWorkspaceOwnerId = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/[^a-zA-Z0-9_-]/g, '_');

const encodeWorkspacePath = (value: string): string =>
  String(value || '')
    .split('/')
    .map((part) => encodeURIComponent(part))
    .join('/');

const buildWorkspacePublicPath = (
  ownerId: string,
  relativePath: string,
  containerId?: number | null
): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  const normalized = normalizeUploadPath(relativePath);
  if (!safeOwner || !normalized) return '';
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    return `/workspaces/${safeOwner}__c__${Number(containerId)}/${encodeWorkspacePath(normalized)}`;
  }
  return `/workspaces/${safeOwner}/${encodeWorkspacePath(normalized)}`;
};

const buildWorkspaceScopeId = (ownerId: string, containerId?: number | null): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  if (!safeOwner) return '';
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    return `${safeOwner}__c__${Number(containerId)}`;
  }
  return safeOwner;
};

const resolveDesktopWorkspaceRoot = (): string => String(getRuntimeConfig().workspace_root || '').trim();

const resolveDesktopContainerRoot = (containerId?: number | null): string => {
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    const mapped = String(desktopContainerRootMap.value[Number(containerId)] || '').trim();
    if (mapped) return mapped;
  }
  return resolveDesktopWorkspaceRoot();
};

const resolveLocalWorkspaceMarkdownPath = (
  rawPath: string,
  ownerId: string,
  containerId?: number | null
): string => {
  if (!desktopLocalMode.value) return '';
  const workspaceId = buildWorkspaceScopeId(ownerId, containerId);
  if (!workspaceId) return '';
  const workspaceRoot = resolveDesktopContainerRoot(containerId);
  const localRelative = resolveWorkspaceRelativePathFromLocal(rawPath, workspaceId, workspaceRoot);
  if (!localRelative) return '';
  return buildWorkspacePublicPath(ownerId, localRelative, containerId);
};

const resolveAgentMarkdownWorkspacePath = (rawPath: string): string => {
  const ownerId = normalizeWorkspaceOwnerId(authStore.user?.id);
  if (!ownerId) return '';
  const normalized = normalizeWorkspaceRelativeMarkdownPath(rawPath);
  if (normalized) {
    return buildWorkspacePublicPath(ownerId, normalized, currentContainerId.value);
  }
  return resolveLocalWorkspaceMarkdownPath(rawPath, ownerId, currentContainerId.value);
};

const resolveWorldMarkdownWorkspacePath = (rawPath: string, senderUserId: string): string => {
  const ownerId = normalizeWorkspaceOwnerId(senderUserId);
  if (!ownerId) return '';
  const normalized = normalizeWorkspaceRelativeMarkdownPath(rawPath);
  if (normalized) {
    return buildWorkspacePublicPath(ownerId, normalized, USER_CONTAINER_ID);
  }
  return resolveLocalWorkspaceMarkdownPath(rawPath, ownerId, USER_CONTAINER_ID);
};

const WORLD_AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;
const WORLD_AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?，。；：！？》】]+)?$/;

const decodeWorldAtPathToken = (value: string): string => {
  if (!/%[0-9a-fA-F]{2}/.test(value)) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const replaceWorldAtPathTokens = (content: string, senderUserId: string): string => {
  if (!content) return '';
  const ownerId = normalizeWorkspaceOwnerId(senderUserId);
  if (!ownerId) return content;
  return content.replace(WORLD_AT_PATH_RE, (match, prefix, token, doubleQuoted, singleQuoted) => {
    const raw = doubleQuoted ?? singleQuoted ?? token ?? '';
    if (!raw) return match;
    let value = raw;
    let suffix = '';
    if (!doubleQuoted && !singleQuoted) {
      const split = WORLD_AT_PATH_SUFFIX_RE.exec(value);
      if (split) {
        value = split[1] ?? value;
        suffix = split[2] ?? '';
      }
    }
    const decoded = decodeWorldAtPathToken(String(value || '').trim());
    const normalized = normalizeUploadPath(decoded);
    if (!normalized) return match;
    const pathLike =
      decoded.startsWith('/') ||
      decoded.startsWith('./') ||
      decoded.startsWith('../') ||
      normalized.includes('/') ||
      normalized.includes('.');
    if (!pathLike) return match;
    const publicPath = buildWorkspacePublicPath(ownerId, normalized, USER_CONTAINER_ID);
    if (!publicPath) return match;
    const label = decoded;
    const replacement = isImagePath(normalized)
      ? `![${label}](${publicPath})`
      : `[${label}](${publicPath})`;
    return `${prefix}${replacement}${suffix}`;
  });
};

type WorkspaceResolvedResource = ReturnType<typeof parseWorkspaceResourceUrl> & {
  requestUserId: string | null;
  requestAgentId: string | null;
  requestContainerId: number | null;
  allowed: boolean;
};

const resolveWorkspaceResource = (publicPath: string): WorkspaceResolvedResource | null => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const user = authStore.user as Record<string, unknown> | null;
  if (!user) return null;
  const currentId = normalizeWorkspaceOwnerId(user.id);
  const workspaceId = parsed.workspaceId || parsed.userId;
  const ownerId = parsed.ownerId || workspaceId;
  const agentId = parsed.agentId || '';
  const containerId =
    typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
      ? parsed.containerId
      : null;
  const isOwner =
    Boolean(currentId) &&
    (workspaceId === currentId ||
      workspaceId.startsWith(`${currentId}__agent__`) ||
      workspaceId.startsWith(`${currentId}__a__`) ||
      workspaceId.startsWith(`${currentId}__c__`));
  if (isOwner) {
    return {
      ...parsed,
      requestUserId: null,
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
  // 非管理员仍优先尝试按当前登录用户上下文读取，避免不同展示ID导致的误拦截。
  return {
    ...parsed,
    requestUserId: null,
    requestAgentId: agentId || null,
    requestContainerId: containerId,
    allowed: true
  };
};

const buildWorkspaceResourcePersistentCacheKey = (resource: WorkspaceResolvedResource): string => {
  const currentUserId = normalizeWorkspaceOwnerId((authStore.user as Record<string, unknown> | null)?.id);
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

const getFilenameFromHeaders = (headers: Record<string, unknown> | undefined, fallback: string): string => {
  const disposition = String(headers?.['content-disposition'] || headers?.['Content-Disposition'] || '').trim();
  if (!disposition) return fallback;
  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch {
      return utf8Match[1];
    }
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match?.[1] || fallback;
};

const getFileExtension = (filename: string): string => {
  const base = String(filename || '').split('?')[0].split('#')[0];
  const parts = base.split('.');
  if (parts.length < 2) return '';
  return String(parts.pop() || '').toLowerCase();
};

const normalizeWorkspaceImageBlob = (blob: Blob, filename: string, contentType: string): Blob => {
  if (!(blob instanceof Blob)) return blob;
  if (getFileExtension(filename) !== 'svg') return blob;
  const expectedType = 'image/svg+xml';
  if (blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.includes('image/svg')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob.slice(0, blob.size, expectedType);
};

const saveBlobUrl = (url: string, filename: string) => {
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

const fetchWorkspaceResource = async (resource: WorkspaceResolvedResource) => {
  const cacheKey = resource.publicPath;
  const cached = workspaceResourceCache.get(cacheKey);
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
        workspaceResourceCache.set(cacheKey, entry);
        return entry;
      }
    }
    const params: Record<string, string> = {
      path: String(resource.relativePath || '')
    };
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
      const filename = getFilenameFromHeaders(
        response?.headers as Record<string, unknown>,
        resource.filename || 'download'
      );
      const contentType = String(
        (response?.headers as Record<string, unknown>)?.['content-type'] ||
          (response?.headers as Record<string, unknown>)?.['Content-Type'] ||
          ''
      );
      const normalizedBlob = normalizeWorkspaceImageBlob(response.data as Blob, filename, contentType);
      const objectUrl = URL.createObjectURL(normalizedBlob);
      const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
      workspaceResourceCache.set(cacheKey, entry);
      if (allowPersistentCache && persistentCacheKey) {
        void writeWorkspaceImagePersistentCache(persistentCacheKey, {
          blob: normalizedBlob,
          filename
        });
      }
      return entry;
    } catch (error) {
      workspaceResourceCache.delete(cacheKey);
      throw error;
    }
  })()
    .catch((error) => {
      workspaceResourceCache.delete(cacheKey);
      throw error;
    });
  workspaceResourceCache.set(cacheKey, { promise });
  return promise;
};

const setUserAttachmentResourceState = (publicPath: string, state: AttachmentResourceState) => {
  const next = new Map(userAttachmentResourceCache.value);
  next.set(publicPath, state);
  userAttachmentResourceCache.value = next;
};

const ensureUserAttachmentResource = async (publicPath: string) => {
  const normalized = String(publicPath || '').trim();
  if (!normalized) return;
  const existing = userAttachmentResourceCache.value.get(normalized);
  if (existing) return;
  const resource = resolveWorkspaceResource(normalized);
  if (!resource) return;
  if (!resource.allowed) {
    setUserAttachmentResourceState(normalized, { error: true });
    return;
  }
  setUserAttachmentResourceState(normalized, { loading: true });
  try {
    const entry = await fetchWorkspaceResource(resource);
    setUserAttachmentResourceState(normalized, {
      objectUrl: entry.objectUrl,
      filename: entry.filename
    });
  } catch (error) {
    setUserAttachmentResourceState(normalized, { error: true });
  }
};

const isWorkspaceResourceMissing = (error: unknown): boolean => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status || 0);
  if (status === 404 || status === 410) return true;
  const raw =
    (error as { response?: { data?: { detail?: string; message?: string } } })?.response?.data?.detail ||
    (error as { response?: { data?: { message?: string } } })?.response?.data?.message ||
    (error as { message?: string })?.message ||
    '';
  const message = typeof raw === 'string' ? raw : String(raw || '');
  return /not found|no such|涓嶅瓨鍦▅鎵句笉鍒皘宸插垹闄宸茬Щ闄removed/i.test(message);
};

const hydrateWorkspaceResourceCard = async (card: HTMLElement) => {
  if (!card || card.dataset.workspaceState) return;
  const kind = String(card.dataset.workspaceKind || 'image');
  if (kind !== 'image') {
    card.dataset.workspaceState = 'ready';
    return;
  }
  const publicPath = String(card.dataset.workspacePath || '').trim();
  const status = card.querySelector('.ai-resource-status') as HTMLElement | null;
  const preview = card.querySelector('.ai-resource-preview') as HTMLImageElement | null;
  if (!publicPath || !preview) return;
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource || !resource.allowed) {
    if (status) status.textContent = t('chat.resourceUnavailable');
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
    return;
  }
  card.dataset.workspaceState = 'loading';
  card.classList.remove('is-error');
  card.classList.remove('is-ready');
  const loadingTimerId = scheduleWorkspaceLoadingLabel(card, status);
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
  } finally {
    clearWorkspaceLoadingLabelTimer(loadingTimerId);
  }
};

const hydrateWorkspaceResources = () => {
  const container = messageListRef.value;
  if (!container) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    void hydrateWorkspaceResourceCard(card as HTMLElement);
  });
  hydrateExternalMarkdownImages(container);
};

const scheduleWorkspaceResourceHydration = () => {
  if (workspaceResourceHydrationFrame !== null || workspaceResourceHydrationPending) return;
  workspaceResourceHydrationPending = true;
  void nextTick(() => {
    workspaceResourceHydrationPending = false;
    if (workspaceResourceHydrationFrame !== null || typeof window === 'undefined') return;
    workspaceResourceHydrationFrame = window.requestAnimationFrame(() => {
      workspaceResourceHydrationFrame = null;
      hydrateWorkspaceResources();
    });
  });
};

const clearWorkspaceResourceCache = () => {
  if (workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
    window.cancelAnimationFrame(workspaceResourceHydrationFrame);
    workspaceResourceHydrationFrame = null;
  }
  workspaceResourceHydrationPending = false;
  workspaceResourceCache.forEach((entry) => {
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
  });
  workspaceResourceCache.clear();
  userAttachmentResourceCache.value = new Map();
};

const downloadWorkspaceResource = async (publicPath: string) => {
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource || !resource.allowed) return;
  try {
    const entry = await fetchWorkspaceResource(resource);
    saveBlobUrl(entry.objectUrl, entry.filename || resource.filename || 'download');
  } catch (error) {
    ElMessage.error(
      isWorkspaceResourceMissing(error) ? t('chat.resourceMissing') : t('chat.resourceDownloadFailed')
    );
  }
};

const openImagePreview = (src: string, title = '', workspacePath = '') => {
  const normalizedSrc = String(src || '').trim();
  if (!normalizedSrc) return;
  imagePreviewUrl.value = normalizedSrc;
  imagePreviewTitle.value = String(title || '').trim() || t('chat.imagePreview');
  imagePreviewWorkspacePath.value = String(workspacePath || '').trim();
  imagePreviewVisible.value = true;
};

const handleImagePreviewDownload = async () => {
  const workspacePath = String(imagePreviewWorkspacePath.value || '').trim();
  if (!workspacePath) return;
  await downloadWorkspaceResource(workspacePath);
};

const closeImagePreview = () => {
  imagePreviewVisible.value = false;
  imagePreviewUrl.value = '';
  imagePreviewTitle.value = '';
  imagePreviewWorkspacePath.value = '';
};

const handleMessageContentClick = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  const previewImage = target.closest('img.ai-resource-preview') as HTMLImageElement | null;
  if (previewImage) {
    const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
    if (card?.dataset?.workspaceState !== 'ready') return;
    const src = String(previewImage.getAttribute('src') || '').trim();
    if (!src) return;
    const title = String(card?.querySelector('.ai-resource-name')?.textContent || '').trim();
    const workspacePath = String(card?.dataset?.workspacePath || '').trim();
    openImagePreview(src, title, workspacePath);
    return;
  }
  const resourceButton = target.closest('[data-workspace-action]') as HTMLElement | null;
  if (resourceButton) {
    const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
    const publicPath = String(container?.dataset?.workspacePath || '').trim();
    if (!publicPath) return;
    event.preventDefault();
    await downloadWorkspaceResource(publicPath);
    return;
  }
  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
  if (resourceLink) {
    const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
    if (!publicPath) return;
    event.preventDefault();
    await downloadWorkspaceResource(publicPath);
    return;
  }
  const copyButton = target.closest('.ai-code-copy') as HTMLElement | null;
  if (!copyButton) return;
  event.preventDefault();
  const codeBlock = copyButton.closest('.ai-code-block');
  const codeText = String(codeBlock?.querySelector('code')?.textContent || '').trim();
  if (!codeText) {
    ElMessage.warning(t('chat.message.copyEmpty'));
    return;
  }
  const copied = await copyText(codeText);
  if (copied) {
    ElMessage.success(t('chat.message.copySuccess'));
  } else {
    ElMessage.warning(t('chat.message.copyFailed'));
  }
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
  options: {
    streaming?: boolean;
    resolveWorkspacePath?: (rawPath: string) => string;
    message?: Record<string, unknown>;
  } = {}
): string => {
  const source = prepareMessageMarkdownContent(content, options.message);
  const normalizedKey = String(cacheKey || '').trim();
  if (!source) {
    if (normalizedKey) {
      markdownCache.delete(normalizedKey);
    }
    return '';
  }
  if (!normalizedKey) {
    return renderMarkdown(source, { resolveWorkspacePath: options.resolveWorkspacePath });
  }
  const cached = markdownCache.get(normalizedKey);
  if (cached && cached.source === source) {
    return cached.html;
  }
  const now = Date.now();
  if (options.streaming && cached && now - cached.updatedAt < MARKDOWN_STREAM_THROTTLE_MS) {
    return cached.html;
  }
  const html = renderMarkdown(source, { resolveWorkspacePath: options.resolveWorkspacePath });
  markdownCache.set(normalizedKey, { source, html, updatedAt: now });
  trimMarkdownCache();
  return html;
};

const renderAgentMarkdown = (message: Record<string, unknown>, index: number): string => {
  const cacheKey = `agent:${resolveAgentMessageKey(message, index)}:c${currentContainerId.value}`;
  const streaming =
    Boolean(message?.stream_incomplete) ||
    Boolean(message?.workflowStreaming) ||
    Boolean(message?.reasoningStreaming);
  return renderMessageMarkdown(cacheKey, message?.content, {
    streaming,
    resolveWorkspacePath: resolveAgentMarkdownWorkspacePath,
    message
  });
};

const renderWorldMarkdown = (message: Record<string, unknown>): string => {
  const cacheKey = `world:${resolveWorldMessageKey(message)}`;
  const content = String(message?.content || '');
  const senderUserId = String(message?.sender_user_id || '').trim();
  const patched = replaceWorldAtPathTokens(content, senderUserId);
  return renderMessageMarkdown(cacheKey, patched, {
    message,
    resolveWorkspacePath: (rawPath: string) => resolveWorldMarkdownWorkspacePath(rawPath, senderUserId)
  });
};

const resolveWorldVoicePayloadFromMessage = (message: Record<string, unknown>) => {
  if (!isWorldVoiceContentType(message?.content_type)) return null;
  return parseWorldVoicePayload(message?.content);
};

const isWorldVoiceMessage = (message: Record<string, unknown>): boolean =>
  Boolean(resolveWorldVoicePayloadFromMessage(message));

const isWorldVoicePlaying = (message: Record<string, unknown>): boolean =>
  worldVoicePlayingMessageKey.value === resolveWorldMessageKey(message);

const isWorldVoiceLoading = (message: Record<string, unknown>): boolean =>
  worldVoiceLoadingMessageKey.value === resolveWorldMessageKey(message);

const resolveWorldVoiceTotalDurationMs = (message: Record<string, unknown>): number => {
  const payload = resolveWorldVoicePayloadFromMessage(message);
  const payloadDuration = Number(payload?.duration_ms || 0);
  if (!Number.isFinite(payloadDuration) || payloadDuration <= 0) {
    const messageKey = resolveWorldMessageKey(message);
    if (messageKey && messageKey === worldVoicePlayingMessageKey.value) {
      return Math.max(0, Number(worldVoicePlaybackDurationMs.value || 0));
    }
    return 0;
  }
  const messageKey = resolveWorldMessageKey(message);
  if (messageKey && messageKey === worldVoicePlayingMessageKey.value) {
    return Math.max(payloadDuration, Number(worldVoicePlaybackDurationMs.value || 0));
  }
  return payloadDuration;
};

const resolveWorldVoiceDurationLabel = (message: Record<string, unknown>): string => {
  const totalDurationMs = resolveWorldVoiceTotalDurationMs(message);
  if (!totalDurationMs) {
    return t('messenger.world.voice.durationUnknown');
  }
  if (!isWorldVoicePlaying(message)) {
    return formatWorldVoiceDuration(totalDurationMs);
  }
  const remainingMs = Math.max(0, totalDurationMs - Number(worldVoicePlaybackCurrentMs.value || 0));
  return t('messenger.world.voice.remaining', {
    duration: formatWorldVoiceDuration(remainingMs)
  });
};

const resolveWorldVoiceActionLabel = (message: Record<string, unknown>): string =>
  isWorldVoicePlaying(message) ? t('messenger.world.voice.pause') : t('messenger.world.voice.play');

const copyMessageContent = async (payload: unknown) => {
  const message = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : null;
  const text = prepareMessageMarkdownContent(message?.content ?? payload, message).trim();
  if (!text) return;
  const copied = await copyText(text);
  if (copied) {
    ElMessage.success(t('chat.message.copySuccess'));
  } else {
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

const resolveWorldMessageDomId = (message: Record<string, unknown>): string => {
  const messageId = Number.parseInt(String(message?.message_id || ''), 10);
  if (Number.isFinite(messageId) && messageId > 0) {
    return `uw-message-${messageId}`;
  }
  const fallbackKey = resolveWorldMessageKey(message).replace(/[^a-zA-Z0-9_-]/g, '_');
  return `uw-message-${fallbackKey}`;
};

const resetWorldVoicePlaybackProgress = () => {
  worldVoicePlaybackCurrentMs.value = 0;
  worldVoicePlaybackDurationMs.value = 0;
};

const syncWorldVoicePlaybackProgress = (audio: HTMLAudioElement) => {
  const currentMs = Number(audio.currentTime);
  worldVoicePlaybackCurrentMs.value =
    Number.isFinite(currentMs) && currentMs > 0 ? Math.round(currentMs * 1000) : 0;
  const durationMs = Number(audio.duration);
  if (Number.isFinite(durationMs) && durationMs > 0) {
    worldVoicePlaybackDurationMs.value = Math.round(durationMs * 1000);
  }
};

const ensureWorldVoicePlaybackRuntime = (): WorldVoicePlaybackRuntime | null => {
  if (typeof Audio === 'undefined') return null;
  if (worldVoicePlaybackRuntime) return worldVoicePlaybackRuntime;
  const audio = new Audio();
  audio.preload = 'none';
  audio.addEventListener('loadedmetadata', () => {
    syncWorldVoicePlaybackProgress(audio);
  });
  audio.addEventListener('durationchange', () => {
    syncWorldVoicePlaybackProgress(audio);
  });
  audio.addEventListener('timeupdate', () => {
    syncWorldVoicePlaybackProgress(audio);
  });
  audio.addEventListener('ended', () => {
    resetWorldVoicePlaybackProgress();
    worldVoicePlayingMessageKey.value = '';
    if (worldVoicePlaybackRuntime) {
      worldVoicePlaybackRuntime.currentMessageKey = '';
    }
  });
  audio.addEventListener('pause', () => {
    if (audio.ended) return;
    worldVoicePlaybackCurrentMs.value = 0;
    worldVoicePlayingMessageKey.value = '';
    if (worldVoicePlaybackRuntime) {
      worldVoicePlaybackRuntime.currentMessageKey = '';
    }
  });
  worldVoicePlaybackRuntime = {
    audio,
    objectUrlCache: new Map<string, string>(),
    currentMessageKey: '',
    currentResourceKey: ''
  };
  return worldVoicePlaybackRuntime;
};

const resolveWorldVoiceContainerId = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return USER_CONTAINER_ID;
  return Math.max(0, Math.round(parsed));
};

const buildWorldVoiceResourceKey = (
  conversationId: string,
  ownerUserId: string,
  containerId: number,
  path: string
): string => `${conversationId}|${ownerUserId}|${containerId}|${path}`;

const fetchWorldVoiceObjectUrl = async (
  message: Record<string, unknown>,
  payload: {
    path: string;
    container_id?: number;
    owner_user_id?: string;
  },
  runtime: WorldVoicePlaybackRuntime
): Promise<{ resourceKey: string; objectUrl: string }> => {
  const conversationId = String(message?.conversation_id || activeWorldConversationId.value || '').trim();
  if (!conversationId) {
    throw new Error(t('messenger.world.voice.playFailed'));
  }
  const path = normalizeUploadPath(payload.path);
  if (!path) {
    throw new Error(t('messenger.world.voice.playFailed'));
  }
  const ownerUserId =
    String(payload.owner_user_id || '').trim() ||
    String(message?.sender_user_id || '').trim() ||
    String(currentUserId.value || '').trim();
  if (!ownerUserId) {
    throw new Error(t('messenger.world.voice.playFailed'));
  }
  const containerId = resolveWorldVoiceContainerId(payload.container_id);
  const resourceKey = buildWorldVoiceResourceKey(conversationId, ownerUserId, containerId, path);
  const cached = runtime.objectUrlCache.get(resourceKey);
  if (cached) {
    return { resourceKey, objectUrl: cached };
  }
  const response = await downloadUserWorldFile({
    conversation_id: conversationId,
    owner_user_id: ownerUserId,
    container_id: containerId,
    path
  });
  const blob = response.data as Blob;
  if (!(blob instanceof Blob) || !blob.size) {
    throw new Error(t('messenger.world.voice.playFailed'));
  }
  const objectUrl = URL.createObjectURL(blob);
  runtime.objectUrlCache.set(resourceKey, objectUrl);
  return { resourceKey, objectUrl };
};

const stopWorldVoicePlayback = () => {
  const runtime = worldVoicePlaybackRuntime;
  if (!runtime) return;
  runtime.audio.pause();
  runtime.currentMessageKey = '';
  resetWorldVoicePlaybackProgress();
  worldVoicePlayingMessageKey.value = '';
  worldVoiceLoadingMessageKey.value = '';
};

const disposeWorldVoicePlayback = () => {
  const runtime = worldVoicePlaybackRuntime;
  if (!runtime) {
    resetWorldVoicePlaybackProgress();
    return;
  }
  stopWorldVoicePlayback();
  runtime.currentResourceKey = '';
  runtime.objectUrlCache.forEach((objectUrl) => {
    URL.revokeObjectURL(objectUrl);
  });
  runtime.objectUrlCache.clear();
  runtime.audio.removeAttribute('src');
  try {
    runtime.audio.load();
  } catch {
    // ignore runtime cleanup errors
  }
  resetWorldVoicePlaybackProgress();
  worldVoicePlaybackRuntime = null;
};

const toggleWorldVoicePlayback = async (message: Record<string, unknown>) => {
  if (!isWorldConversationActive.value) return;
  const payload = resolveWorldVoicePayloadFromMessage(message);
  if (!payload) return;
  const messageKey = resolveWorldMessageKey(message);
  if (!messageKey || worldVoiceLoadingMessageKey.value === messageKey) return;
  const runtime = ensureWorldVoicePlaybackRuntime();
  if (!runtime) {
    ElMessage.warning(t('messenger.world.voice.unsupported'));
    return;
  }
  if (runtime.currentMessageKey === messageKey && !runtime.audio.paused) {
    runtime.audio.pause();
    return;
  }
  worldVoiceLoadingMessageKey.value = messageKey;
  try {
    const { resourceKey, objectUrl } = await fetchWorldVoiceObjectUrl(message, payload, runtime);
    if (runtime.currentResourceKey !== resourceKey || runtime.audio.src !== objectUrl) {
      runtime.audio.pause();
      runtime.audio.src = objectUrl;
      runtime.currentResourceKey = resourceKey;
    }
    runtime.currentMessageKey = messageKey;
    await runtime.audio.play();
    syncWorldVoicePlaybackProgress(runtime.audio);
    worldVoicePlayingMessageKey.value = messageKey;
  } catch (error) {
    worldVoicePlayingMessageKey.value = '';
    showApiError(error, t('messenger.world.voice.playFailed'));
  } finally {
    if (worldVoiceLoadingMessageKey.value === messageKey) {
      worldVoiceLoadingMessageKey.value = '';
    }
  }
};

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
  const confirmed = await confirmWithFallback(
    t('chat.history.confirmDelete'),
    t('chat.history.confirmTitle'),
    {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    }
  );
  if (!confirmed) {
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

const switchSection = (
  section: MessengerSection,
  options: { preserveHelperWorkspace?: boolean; panelHint?: string; helperWorkspace?: boolean } = {}
) => {
  const preserveHelperWorkspace = options.preserveHelperWorkspace === true;
  const panelHint = String(options.panelHint || '').trim().toLowerCase();
  const helperWorkspace = options.helperWorkspace === true;
  closeFileContainerMenu();
  openMiddlePaneOverlay();
  if (!preserveHelperWorkspace) {
    helperAppsWorkspaceMode.value = false;
  } else if (helperWorkspace) {
    helperAppsWorkspaceMode.value = true;
  }
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
  if (panelHint && section === 'more') {
    nextQuery.panel = panelHint;
  } else {
    delete nextQuery.panel;
  }
  if (section === 'groups' && helperWorkspace) {
    nextQuery.helper = '1';
  } else {
    delete nextQuery.helper;
  }
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

const openDesktopModelSettingsFromHeader = () => {
  if (!agentHeaderModelJumpEnabled.value) return;
  switchSection('more', { panelHint: 'desktop-models' });
  settingsPanelMode.value = 'desktop-models';
};

const openProfilePage = () => {
  closeFileContainerMenu();
  settingsPanelMode.value = 'profile';
  sessionHub.setSection('more');
  sessionHub.setKeyword('');
  const nextQuery = { ...route.query, section: 'more' } as Record<string, any>;
  delete nextQuery.session_id;
  delete nextQuery.agent_id;
  delete nextQuery.entry;
  delete nextQuery.conversation_id;
  delete nextQuery.panel;
  router.push({ path: `${basePrefix.value}/profile`, query: nextQuery }).catch(() => undefined);
};

const handleSettingsLogout = () => {
  if (settingsLogoutDisabled.value) {
    return;
  }
  if (desktopMode.value) {
    router.push('/desktop/home').catch(() => undefined);
    return;
  }
  authStore.logout();
  router.push('/login').catch(() => undefined);
};

const applyCurrentUserAppearance = (appearance: UserAppearancePreferences) => {
  appearanceHydrating.value = true;
  themeStore.setMode(normalizeThemeMode(appearance.themeMode));
  themeStore.setPalette(normalizeThemePalette(appearance.themePalette));
  currentUserAvatarIcon.value = normalizeAvatarIcon(appearance.avatarIcon, PROFILE_AVATAR_OPTION_KEYS);
  currentUserAvatarColor.value = normalizeAvatarColor(appearance.avatarColor);
  appearanceHydrating.value = false;
};

const resolveCurrentUserAppearance = (): UserAppearancePreferences => ({
  themeMode: normalizeThemeMode(themeStore.mode),
  themePalette: normalizeThemePalette(themeStore.palette),
  avatarIcon: normalizeAvatarIcon(currentUserAvatarIcon.value, PROFILE_AVATAR_OPTION_KEYS),
  avatarColor: normalizeAvatarColor(currentUserAvatarColor.value),
  updatedAt: 0
});

const hydrateCurrentUserAppearance = async () => {
  const scopedUserId = String(currentUserId.value || '').trim();
  if (!scopedUserId) {
    applyCurrentUserAppearance({
      ...resolveCurrentUserAppearance(),
      avatarIcon: 'initial',
      avatarColor: '#3b82f6'
    });
    return;
  }
  appearanceHydrating.value = true;
  try {
    const appearance = await loadUserAppearance(scopedUserId, PROFILE_AVATAR_OPTION_KEYS);
    if (String(currentUserId.value || '').trim() !== scopedUserId) return;
    applyCurrentUserAppearance(appearance);
  } finally {
    appearanceHydrating.value = false;
  }
};

const persistCurrentUserAppearance = async () => {
  if (appearanceHydrating.value) return;
  const scopedUserId = String(currentUserId.value || '').trim();
  if (!scopedUserId) return;
  const appearance = resolveCurrentUserAppearance();
  const persisted = await saveUserAppearance(scopedUserId, appearance, PROFILE_AVATAR_OPTION_KEYS);
  if (String(currentUserId.value || '').trim() !== scopedUserId) return;
  applyCurrentUserAppearance(persisted);
};

const updateCurrentUserAvatarIcon = (value: unknown) => {
  currentUserAvatarIcon.value = normalizeAvatarIcon(value, PROFILE_AVATAR_OPTION_KEYS);
  void persistCurrentUserAppearance();
};

const updateCurrentUserAvatarColor = (value: unknown) => {
  currentUserAvatarColor.value = normalizeAvatarColor(value);
  void persistCurrentUserAppearance();
};

const initDesktopLaunchBehavior = () => {
  desktopShowFirstLaunchDefaultAgentHint.value = false;
  desktopFirstLaunchDefaultAgentHintAt.value = 0;
  if (!desktopMode.value || typeof window === 'undefined') return;
  try {
    const alreadyShown =
      String(window.localStorage.getItem(DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY) || '').trim() === '1';
    if (!alreadyShown) {
      desktopShowFirstLaunchDefaultAgentHint.value = true;
      desktopFirstLaunchDefaultAgentHintAt.value = Date.now();
      window.localStorage.setItem(DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY, '1');
    }
  } catch {
    desktopShowFirstLaunchDefaultAgentHint.value = false;
    desktopFirstLaunchDefaultAgentHintAt.value = 0;
  }
};

const clearMiddlePaneOverlayHide = () => {
  if (typeof window !== 'undefined' && middlePaneOverlayHideTimer) {
    window.clearTimeout(middlePaneOverlayHideTimer);
    middlePaneOverlayHideTimer = null;
  }
};

const clearKeywordDebounce = () => {
  if (typeof window === 'undefined' || keywordDebounceTimer === null) return;
  window.clearTimeout(keywordDebounceTimer);
  keywordDebounceTimer = null;
};

const resetContactVirtualScroll = () => {
  contactVirtualScrollTop.value = 0;
  const container = contactVirtualListRef.value;
  if (container && container.scrollTop !== 0) {
    container.scrollTop = 0;
  }
};

const syncContactVirtualMetrics = () => {
  const container = contactVirtualListRef.value;
  if (!container) {
    contactVirtualViewportHeight.value = 0;
    contactVirtualScrollTop.value = 0;
    return;
  }
  contactVirtualViewportHeight.value = container.clientHeight;
  contactVirtualScrollTop.value = container.scrollTop;
  const maxScroll = Math.max(
    0,
    filteredContacts.value.length * CONTACT_VIRTUAL_ITEM_HEIGHT - contactVirtualViewportHeight.value
  );
  if (contactVirtualScrollTop.value > maxScroll) {
    contactVirtualScrollTop.value = maxScroll;
    container.scrollTop = maxScroll;
  }
};

const handleContactVirtualScroll = () => {
  if (typeof window === 'undefined') {
    syncContactVirtualMetrics();
    return;
  }
  if (contactVirtualFrame !== null) return;
  contactVirtualFrame = window.requestAnimationFrame(() => {
    contactVirtualFrame = null;
    syncContactVirtualMetrics();
  });
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
  agentCreateVisible.value = true;
};

const submitAgentCreate = async (payload: Record<string, unknown>) => {
  if (quickCreatingAgent.value) return;
  quickCreatingAgent.value = true;
  try {
    const created = await agentStore.createAgent(payload);
    agentCreateVisible.value = false;
    ElMessage.success(t('portal.agent.createSuccess'));
    const tasks: Promise<unknown>[] = [loadRunningAgents(), beeroomStore.loadGroups()];
    if (!cronPermissionDenied.value) {
      tasks.push(loadCronAgentIds());
    }
    await Promise.all(tasks);
    const createdHiveId = String(created?.hive_id || payload.hive_id || '').trim();
    if (sessionHub.activeSection === 'swarms' || createdHiveId || String(payload.hive_name || '').trim()) {
      if (createdHiveId) {
        beeroomStore.setActiveGroup(createdHiveId);
      }
      await beeroomStore.loadActiveGroup().catch(() => null);
    }
    if (created?.id) {
      openCreatedAgentSettings(created.id);
    }
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    quickCreatingAgent.value = false;
  }
};

const refreshActiveBeeroom = async () => {
  try {
    await Promise.all([beeroomStore.loadGroups(), beeroomStore.loadActiveGroup({ silent: true })]);
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
};


const handleBeeroomMoveAgents = async (agentIds: string[]) => {
  const groupId = String(beeroomStore.activeGroupId || '').trim();
  if (!groupId || !agentIds.length) return;
  try {
    await beeroomStore.moveAgents(groupId, agentIds);
    await agentStore.loadAgents();
    ElMessage.success(t('beeroom.message.agentMoved'));
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
};

const handleSearchCreateAction = async () => {
  if (sessionHub.activeSection === 'groups') {
    if (userWorldPermissionDenied.value) {
      ElMessage.warning(t('auth.login.noPermission'));
      return;
    }
    groupCreateName.value = '';
    groupCreateKeyword.value = '';
    groupCreateMemberIds.value = [];
    groupCreateVisible.value = true;
    return;
  }
  if (sessionHub.activeSection === 'swarms') {
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

const selectBeeroomGroup = async (group: Record<string, unknown>) => {
  const groupId = String(group?.group_id || group?.hive_id || '').trim();
  if (!groupId) return;
  beeroomStore.setActiveGroup(groupId);
  await beeroomStore.loadActiveGroup().catch(() => null);
};

const openContactConversationFromList = async (contact: Record<string, unknown>) => {
  selectContact(contact);
  await openContactConversation(contact);
};

const openWorldConversation = async (
  conversationId: string,
  kind: 'direct' | 'group',
  mode: 'detail' | 'messages' = 'detail'
) => {
  if (userWorldPermissionDenied.value) return;
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
  agentOverviewMode.value = 'detail';
  selectedAgentId.value = normalizeAgentId(agentId);
};

const toggleAgentOverviewMode = () => {
  agentOverviewMode.value = agentOverviewMode.value === 'grid' ? 'detail' : 'grid';
};

const enterSelectedAgentConversation = async () => {
  const target = settingsAgentId.value || DEFAULT_AGENT_KEY;
  await openAgentById(target);
};

const openActiveAgentSettings = () => {
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  agentOverviewMode.value = 'detail';
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

const loadAgentToolSummary = async (options: { force?: boolean } = {}) => {
  const force = options.force === true;
  if (agentToolSummaryLoading.value) {
    return agentPromptToolSummary.value;
  }
  if (!force && agentPromptToolSummary.value) {
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
  void loadAgentToolSummary({ force: true });
  void updateAgentAbilityTooltip();
};

const handleAgentAbilityTooltipHide = () => {
  agentAbilityTooltipVisible.value = false;
};

const openAgentPromptPreview = async () => {
  agentPromptPreviewVisible.value = true;
  agentPromptPreviewLoading.value = true;
  agentPromptPreviewContent.value = '';
  const summaryPromise = loadAgentToolSummary({ force: true });
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
    agentPromptPreviewContent.value = String(promptPayload.prompt || '').replace(
      /<<WUNDER_HISTORY_MEMORY>>/g,
      ''
    );
  } catch (error) {
    showApiError(error, t('chat.systemPromptFailed'));
    agentPromptPreviewContent.value = '';
  } finally {
    agentPromptPreviewLoading.value = false;
  }
};

const openContactConversation = async (targetContact: Record<string, unknown> | null | undefined) => {
  if (userWorldPermissionDenied.value) {
    ElMessage.warning(t('auth.login.noPermission'));
    return;
  }
  if (!targetContact) return;
  const perfTrace = startMessengerPerfTrace('openSelectedContactConversation', {
    selectedContactUserId: String(targetContact?.user_id || '').trim()
  });
  const peerUserId = String(targetContact.user_id || '').trim();
  const listMatchedConversationId = (Array.isArray(userWorldStore.conversations) ? userWorldStore.conversations : [])
    .find((item) => {
      const kind = String(item?.conversation_type || '').trim().toLowerCase();
      return kind !== 'group' && String(item?.peer_user_id || '').trim() === peerUserId;
    })
    ?.conversation_id;
  const conversationId = String(targetContact.conversation_id || listMatchedConversationId || '').trim();
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

const openSelectedContactConversation = async () => {
  await openContactConversation(selectedContact.value);
};

const openSelectedGroupConversation = async () => {
  if (userWorldPermissionDenied.value) {
    ElMessage.warning(t('auth.login.noPermission'));
    return;
  }
  if (!selectedGroup.value) return;
  const conversationId = String(selectedGroup.value.conversation_id || '').trim();
  if (!conversationId) return;
  await openWorldConversation(conversationId, 'group', 'messages');
};

const submitGroupCreate = async () => {
  if (userWorldPermissionDenied.value) {
    ElMessage.warning(t('auth.login.noPermission'));
    return;
  }
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
    const mainEntry = collectMainAgentSessionEntries().find((item) => item.agentId === targetAgentId);
    if (mainEntry?.sessionId === sessionId) {
      setAgentMainReadAt(targetAgentId, mainEntry.lastAt || Date.now());
      setAgentMainUnreadCount(targetAgentId, 0);
      persistAgentUnreadState();
    }
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

const openTimelineSessionDetail = (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  timelineDetailSessionId.value = targetId;
  timelineDetailDialogVisible.value = true;
};

watch(
  () => timelineDetailDialogVisible.value,
  (visible) => {
    if (!visible) {
      timelineDetailSessionId.value = '';
    }
  }
);

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
  const confirmed = await confirmWithFallback(
    t('chat.history.confirmDelete'),
    t('chat.history.confirmTitle'),
    {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    }
  );
  if (!confirmed) {
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

const closeFileContainerMenu = () => {
  fileContainerContextMenu.value.visible = false;
};

const openDesktopContainerSettings = async (containerId?: number) => {
  if (desktopMode.value) {
    if (sessionHub.activeSection !== 'files') {
      switchSection('files');
      await nextTick();
    }
    const fallbackContainerId =
      fileScope.value === 'user' ? USER_CONTAINER_ID : selectedFileContainerId.value;
    const normalized = Math.min(
      10,
      Math.max(0, Number.parseInt(String(containerId ?? fallbackContainerId), 10) || 0)
    );
    desktopContainerManagerPanelRef.value?.openManager(normalized);
    return;
  }
  settingsPanelMode.value = 'general';
  sessionHub.setSection('more');
  sessionHub.setKeyword('');
  const nextQuery = {
    ...route.query,
    section: 'more'
  } as Record<string, any>;
  delete nextQuery.session_id;
  delete nextQuery.agent_id;
  delete nextQuery.entry;
  delete nextQuery.conversation_id;
  delete nextQuery.panel;
  router.push({ path: `${basePrefix.value}/settings`, query: nextQuery }).catch(() => undefined);
};

const openFileContainerMenu = async (
  event: MouseEvent,
  scope: 'user' | 'agent',
  containerId: number
) => {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const targetElement = (event.target as HTMLElement | null) || currentTarget;
  const fallbackRect = (currentTarget || targetElement)?.getBoundingClientRect();
  const baseX =
    Number.isFinite(event.clientX) && event.clientX > 0
      ? event.clientX
      : Math.round((fallbackRect?.left || 0) + (fallbackRect?.width || 0) / 2);
  const baseY =
    Number.isFinite(event.clientY) && event.clientY > 0
      ? event.clientY
      : Math.round((fallbackRect?.top || 0) + (fallbackRect?.height || 0) / 2);

  const normalizedId =
    scope === 'user'
      ? USER_CONTAINER_ID
      : Math.min(10, Math.max(1, Number.parseInt(String(containerId || 1), 10) || 1));
  if (scope === 'agent' && !agentFileContainers.value.some((item) => item.id === normalizedId)) {
    ElMessage.warning(t('messenger.files.agentContainerEmpty'));
    return;
  }
  selectContainer(scope === 'user' ? 'user' : normalizedId);
  fileContainerContextMenu.value.target = { scope, id: normalizedId };
  fileContainerContextMenu.value.visible = true;
  fileContainerContextMenu.value.x = Math.max(8, Math.round(baseX + 2));
  fileContainerContextMenu.value.y = Math.max(8, Math.round(baseY + 2));
  await nextTick();
  const menuRect = fileContainerMenuViewRef.value?.getMenuElement()?.getBoundingClientRect();
  if (!menuRect) return;
  const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
  const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
  fileContainerContextMenu.value.x = Math.min(Math.max(8, fileContainerContextMenu.value.x), maxLeft);
  fileContainerContextMenu.value.y = Math.min(Math.max(8, fileContainerContextMenu.value.y), maxTop);
};

const handleFileContainerMenuOpen = () => {
  const target = fileContainerContextMenu.value.target;
  closeFileContainerMenu();
  if (!target) return;
  selectContainer(target.scope === 'user' ? 'user' : target.id);
};

const handleFileContainerMenuCopyId = async () => {
  const target = fileContainerContextMenu.value.target;
  closeFileContainerMenu();
  if (!target) return;
  const copied = await copyText(String(target.id));
  if (copied) {
    ElMessage.success(t('messenger.files.copyIdSuccess', { id: target.id }));
  } else {
    ElMessage.warning(t('messenger.files.copyIdFailed'));
  }
};

const handleFileContainerMenuSettings = () => {
  const target = fileContainerContextMenu.value.target;
  closeFileContainerMenu();
  void openDesktopContainerSettings(target?.id);
};

const selectContainer = (containerId: number | 'user') => {
  closeFileContainerMenu();
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

const openContainerFromRightDock = (containerId: number) => {
  const normalized = Math.min(10, Math.max(1, Number.parseInt(String(containerId || 1), 10) || 1));
  switchSection('files');
  selectContainer(normalized === USER_CONTAINER_ID ? 'user' : normalized);
};

const openContainerSettingsFromRightDock = (containerId: number) => {
  openContainerFromRightDock(containerId);
  void openDesktopContainerSettings(containerId);
};

const handleFileWorkspaceStats = (payload: unknown) => {
  const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  fileContainerEntryCount.value = Math.max(0, Number(source.entryCount || 0));
  fileContainerLatestUpdatedAt.value = normalizeTimestamp(source.latestUpdatedAt);
  fileLifecycleNowTick.value = Date.now();
};

const handleDesktopContainerRootsChange = (roots: Record<number, string>) => {
  const normalized: Record<number, string> = {};
  Object.entries(roots || {}).forEach(([key, value]) => {
    const containerId = Math.min(10, Math.max(0, Number.parseInt(String(key), 10) || 0));
    normalized[containerId] = String(value || '').trim();
  });
  desktopContainerRootMap.value = normalized;
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
    if (!selectedToolCategory.value) {
      selectedToolCategory.value = 'admin';
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
    if (orgUnitTree.value.length > 0) {
      return;
    }
    const fallbackTree = buildCurrentUserFallbackUnitTree();
    if (!fallbackTree.length) {
      orgUnitPathMap.value = {};
      orgUnitTree.value = [];
      contactUnitExpandedIds.value = new Set();
      return;
    }
    const fallbackMap: Record<string, string> = {};
    fallbackTree.forEach((node) => {
      fallbackMap[node.id] = node.label;
    });
    orgUnitPathMap.value = fallbackMap;
    orgUnitTree.value = fallbackTree;
    contactUnitExpandedIds.value = new Set(fallbackTree.map((node) => node.id));
  }
};

const selectToolCategory = (category: 'admin' | 'mcp' | 'skills' | 'knowledge' | 'shared') => {
  if (category === 'shared' && desktopLocalMode.value) {
    selectedToolCategory.value = 'admin';
    toolPaneStatus.value = '';
    return;
  }
  toolPaneStatus.value = '';
  selectedToolCategory.value = category;
};

const toolCategoryLabel = (category: string) => {
  if (category === 'admin') return t('messenger.tools.adminTitle');
  if (category === 'mcp') return t('toolManager.system.mcp');
  if (category === 'skills') return t('toolManager.system.skills');
  if (category === 'knowledge') return t('toolManager.system.knowledge');
  if (category === 'shared') return t('messenger.tools.sharedTitle');
  return category;
};

const handleAgentSettingsSaved = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    loadRunningAgents(),
    loadAgentUserRounds(),
    loadChannelBoundAgentIds()
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds());
  }
  await Promise.allSettled(tasks);
};

const handleAgentDeleted = async () => {
  selectedAgentId.value = DEFAULT_AGENT_KEY;
  const tasks: Promise<unknown>[] = [
    chatStore.loadSessions(),
    loadRunningAgents(),
    loadAgentUserRounds(),
    loadChannelBoundAgentIds()
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds());
  }
  await Promise.allSettled(tasks);
};

const clearMessagePanelWhenConversationEmpty = () => {
  if (sessionHub.activeSection !== 'messages') return;
  if (hasAnyMixedConversations.value) return;
  if (sessionHub.activeConversation) {
    sessionHub.clearActiveConversation();
  }
  if (String(userWorldStore.activeConversationId || '').trim()) {
    userWorldStore.activeConversationId = '';
  }
  if (
    String(chatStore.activeSessionId || '').trim() ||
    String(chatStore.draftAgentId || '').trim() ||
    (Array.isArray(chatStore.messages) && chatStore.messages.length > 0)
  ) {
    chatStore.activeSessionId = null;
    chatStore.draftAgentId = '';
    chatStore.draftToolOverrides = null;
    chatStore.messages = [];
  }
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

  if (sessionHub.activeSection === 'swarms') {
    if (!beeroomStore.activeGroupId && filteredBeeroomGroups.value.length > 0) {
      const preferredGroup = preferredBeeroomGroupId.value;
      const matchedGroup = preferredGroup
        ? filteredBeeroomGroups.value.find(
            (item) => String(item?.group_id || item?.hive_id || '').trim() === preferredGroup
          )
        : null;
      beeroomStore.setActiveGroup(matchedGroup?.group_id || filteredBeeroomGroups.value[0]?.group_id || '');
    }
    return;
  }

  if (sessionHub.activeSection === 'tools') {
    if (desktopLocalMode.value && selectedToolCategory.value === 'shared') {
      selectedToolCategory.value = 'admin';
    }
    if (!selectedToolEntryKey.value) {
      selectedToolCategory.value = 'admin';
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
  if (!hasAnyMixedConversations.value) {
    clearMessagePanelWhenConversationEmpty();
    return;
  }
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
  const activeInquiry = activeAgentInquiryPanel.value;
  const selectedRoutes = resolveAgentInquirySelectionRoutes(activeInquiry?.panel, agentInquirySelection.value);
  const hasInquirySelection = selectedRoutes.length > 0;
  if (!content && attachments.length === 0 && !hasInquirySelection) return;
  const localCommand = parseAgentLocalCommand(content);
  if (localCommand && !hasInquirySelection) {
    if (activeInquiry) {
      chatStore.resolveInquiryPanel(activeInquiry.message, { status: 'dismissed' });
    }
    if (attachments.length > 0) {
      appendAgentLocalCommandMessages(content, t('chat.command.attachmentsUnsupported'));
      agentInquirySelection.value = [];
      await scrollMessagesToBottom();
      return;
    }
    await handleAgentLocalCommand(localCommand, content);
    agentInquirySelection.value = [];
    return;
  }

  let finalContent = content;
  if (activeInquiry) {
    if (hasInquirySelection) {
      chatStore.resolveInquiryPanel(activeInquiry.message, {
        status: 'answered',
        selected: selectedRoutes.map((route) => route.label)
      });
      const selectionText = buildAgentInquiryReply(activeInquiry.panel, selectedRoutes);
      if (content) {
        finalContent = `${selectionText}\n\n${t('chat.askPanelUserAppend', { content })}`;
      } else {
        finalContent = selectionText;
      }
    } else {
      chatStore.resolveInquiryPanel(activeInquiry.message, { status: 'dismissed' });
    }
  }

  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  autoStickToBottom.value = true;
  setRuntimeStateOverride(targetAgentId, 'running', 30_000);
  pendingAssistantCenter = true;
  pendingAssistantCenterCount = chatStore.messages.length;
  try {
    await chatStore.sendMessage(finalContent, {
      attachments,
      suppressQueuedNotice: hasInquirySelection,
      approvalMode: activeAgentApprovalMode.value
    });
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
    pendingAssistantCenter = false;
    pendingAssistantCenterCount = 0;
    setRuntimeStateOverride(targetAgentId, 'error', 8_000);
    showApiError(error, t('chat.error.requestFailed'));
  } finally {
    agentInquirySelection.value = [];
  }
};

const stopAgentMessage = async () => {
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  setRuntimeStateOverride(targetAgentId, 'done', 20_000);
  pendingAssistantCenter = false;
  pendingAssistantCenterCount = 0;
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

const normalizeHexColor = (value: unknown) => {
  const cleaned = String(value || '').trim();
  if (!cleaned) return '';
  const matched = cleaned.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!matched) return '';
  let hex = matched[1].toLowerCase();
  if (hex.length === 3) {
    hex = hex
      .split('')
      .map((part) => part + part)
      .join('');
  }
  return `#${hex}`;
};

const resolveExternalIconConfig = (icon: unknown) => {
  const raw = String(icon || '').trim();
  if (!raw) {
    return { name: 'fa-globe', color: '' };
  }
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') {
      const name = String((parsed as Record<string, unknown>)?.name || '').trim();
      const match = name.split(/\s+/).find((part) => part.startsWith('fa-'));
      return {
        name: match || 'fa-globe',
        color: normalizeHexColor((parsed as Record<string, unknown>)?.color)
      };
    }
  } catch {
    // fallback to plain icon name
  }
  const match = raw.split(/\s+/).find((part) => part.startsWith('fa-'));
  return {
    name: match || 'fa-globe',
    color: ''
  };
};

const normalizeExternalLink = (item: Record<string, unknown>): HelperAppExternalItem => ({
  linkId: String(item?.link_id || '').trim(),
  title: String(item?.title || '').trim(),
  description: String(item?.description || '').trim(),
  url: String(item?.url || '').trim(),
  icon: String(item?.icon || '').trim(),
  sortOrder: Number.isFinite(Number(item?.sort_order)) ? Number(item.sort_order) : 0
});

const resolveExternalIcon = (icon: unknown) => resolveExternalIconConfig(icon).name;

const resolveExternalIconStyle = (icon: unknown) => {
  const color = resolveExternalIconConfig(icon).color;
  return color ? { color } : {};
};

const resolveExternalHost = (url: unknown) => {
  const value = String(url || '').trim();
  if (!value) return '-';
  try {
    const parsed = new URL(value);
    return parsed.host || value;
  } catch {
    return value;
  }
};

const helperAppsOfflineItems = computed<HelperAppOfflineItem[]>(() => [
  {
    key: 'local-file-search',
    title: t('userWorld.helperApps.localFileSearch.cardTitle'),
    description: t('userWorld.helperApps.localFileSearch.cardDesc'),
    icon: 'fa-folder-tree'
  }
]);

const helperAppsActiveOfflineItem = computed(() => {
  if (helperAppsActiveKind.value !== 'offline') return null;
  return helperAppsOfflineItems.value.find((item) => item.key === helperAppsActiveKey.value) || null;
});

const helperAppsActiveExternalItem = computed(() => {
  if (helperAppsActiveKind.value !== 'online') return null;
  return helperAppsOnlineItems.value.find((item) => item.linkId === helperAppsActiveKey.value) || null;
});

const helperAppsActiveTitle = computed(() => {
  if (helperAppsActiveKind.value === 'offline') {
    return helperAppsActiveOfflineItem.value?.title || '';
  }
  if (helperAppsActiveKind.value === 'online') {
    return helperAppsActiveExternalItem.value?.title || '';
  }
  return '';
});

const helperAppsActiveDescription = computed(() => {
  if (helperAppsActiveKind.value === 'offline') {
    return helperAppsActiveOfflineItem.value?.description || '';
  }
  if (helperAppsActiveKind.value === 'online') {
    const item = helperAppsActiveExternalItem.value;
    if (!item) return '';
    return item.description || resolveExternalHost(item.url);
  }
  return '';
});

const isHelperAppActive = (kind: 'offline' | 'online', key: string) =>
  helperAppsActiveKind.value === kind && helperAppsActiveKey.value === key;

const selectHelperApp = (kind: 'offline' | 'online', key: string) => {
  helperAppsActiveKind.value = kind;
  helperAppsActiveKey.value = key;
  if (kind === 'online') {
    loadHelperExternalApps();
  }
};

const ensureHelperAppsSelection = () => {
  if (helperAppsActiveKind.value && helperAppsActiveKey.value) return;
  const fallback = helperAppsOfflineItems.value[0];
  if (fallback) {
    helperAppsActiveKind.value = 'offline';
    helperAppsActiveKey.value = fallback.key;
  }
};

const loadHelperExternalApps = async () => {
  if (helperAppsOnlineLoading.value || helperAppsOnlineLoaded.value) return;
  helperAppsOnlineLoading.value = true;
  try {
    const { data } = await fetchExternalLinks();
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    helperAppsOnlineItems.value = items
      .map((item) => normalizeExternalLink(item as Record<string, unknown>))
      .filter((item) => item.linkId && item.title && item.url)
      .sort((left, right) => left.sortOrder - right.sortOrder);
    if (helperAppsActiveKind.value === 'online') {
      const activeKey = helperAppsActiveKey.value;
      const hasActive =
        Boolean(activeKey) && helperAppsOnlineItems.value.some((item) => item.linkId === activeKey);
      if (!hasActive) {
        helperAppsActiveKey.value = helperAppsOnlineItems.value[0]?.linkId || '';
      }
    }
  } catch {
    helperAppsOnlineItems.value = [];
  } finally {
    helperAppsOnlineLoading.value = false;
    helperAppsOnlineLoaded.value = true;
  }
};

const openHelperAppsDialog = () => {
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = true;
  helperAppsWorkspaceMode.value = true;
  ensureHelperAppsSelection();
  loadHelperExternalApps();
  switchSection('groups', { preserveHelperWorkspace: true, helperWorkspace: true });
  selectedGroupId.value = '';
};

const closeWorldAttachmentPanels = () => {
  worldQuickPanelMode.value = '';
  worldContainerPickerVisible.value = false;
};

const findWorldOversizedFile = (files: File[]): File | undefined =>
  files.find((file) => Number(file.size || 0) > WORLD_UPLOAD_SIZE_LIMIT);

const resolveUploadedWorldPath = (value: unknown): string => {
  if (typeof value === 'string') {
    return normalizeUploadPath(value);
  }
  if (value && typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return normalizeUploadPath(record.path ?? record.relative_path ?? record.relativePath ?? '');
  }
  return '';
};

const uploadWorldFilesToUserContainer = async (
  files: File[],
  options: { appendTokens?: boolean } = {}
): Promise<string[]> => {
  if (!files.length) return [];
  const formData = new FormData();
  formData.append('path', USER_WORLD_UPLOAD_BASE);
  formData.append('container_id', String(USER_CONTAINER_ID));
  files.forEach((file) => {
    formData.append('files', file as Blob);
  });
  const { data } = await uploadWunderWorkspace(formData);
  const uploaded = (Array.isArray(data?.files) ? data.files : [])
    .map((item) => resolveUploadedWorldPath(item))
    .filter(Boolean);
  if (uploaded.length && options.appendTokens !== false) {
    appendWorldAttachmentTokens(uploaded);
    emitWorkspaceRefresh({
      reason: 'messenger-world-upload',
      containerId: USER_CONTAINER_ID
    });
  }
  return uploaded;
};

const screenshotDataUrlToFile = (dataUrl: string, fileName: string, mimeTypeHint = ''): File => {
  const normalizedDataUrl = String(dataUrl || '').trim();
  const commaIndex = normalizedDataUrl.indexOf(',');
  if (!normalizedDataUrl.startsWith('data:image/') || commaIndex <= 0) {
    throw new Error(t('chat.attachments.screenshotFailed'));
  }
  const metadata = normalizedDataUrl.slice(5, commaIndex);
  const payload = normalizedDataUrl.slice(commaIndex + 1);
  if (!/;base64$/i.test(metadata)) {
    throw new Error(t('chat.attachments.screenshotFailed'));
  }
  const binary = atob(payload);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  const mimeType = String(mimeTypeHint || metadata.split(';')[0] || 'image/png').trim() || 'image/png';
  return new File([bytes], fileName, { type: mimeType });
};

const appendScreenshotFileNameSuffix = (fileName: string, suffix: string): string => {
  const normalized = String(fileName || '').trim();
  if (!normalized) return `screenshot${suffix}.png`;
  const dotIndex = normalized.lastIndexOf('.');
  if (dotIndex <= 0) return `${normalized}${suffix}`;
  return `${normalized.slice(0, dotIndex)}${suffix}${normalized.slice(dotIndex)}`;
};

type WorldScreenshotCaptureOption = {
  hideWindow?: boolean;
  region?: boolean;
};

const captureWorldScreenshotData = async (
  option: WorldScreenshotCaptureOption
): Promise<{ dataUrl: string; fileName: string; mimeType: string }> => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.captureScreenshot !== 'function') {
    throw new Error(t('chat.attachments.screenshotUnavailable'));
  }
  const result = (await bridge.captureScreenshot({
    hideWindow: option.hideWindow === true,
    region: option.region === true
  })) as DesktopScreenshotResult | null;
  if (result?.canceled) {
    throw new Error('__SCREENSHOT_CANCELED__');
  }
  if (!result || result.ok === false) {
    const reason = String(result?.message || t('chat.attachments.screenshotFailed')).trim();
    throw new Error(reason || t('chat.attachments.screenshotFailed'));
  }
  const fileName = String(result.name || '').trim() || `screenshot-${Date.now()}.png`;
  const mimeType = String(result.mimeType || '').trim() || 'image/png';
  const dataUrl = String(result.dataUrl || '').trim();
  if (!dataUrl.startsWith('data:image/')) {
    throw new Error(t('chat.attachments.screenshotFailed'));
  }
  return { dataUrl, fileName, mimeType };
};

const resolveDesktopDefaultModelMeta = (
  settings: unknown
): { hearingSupported: boolean; modelDisplayName: string } => {
  const root = asObjectRecord(settings);
  const llm = asObjectRecord(root.llm);
  const defaultModelKey = String(llm.default || '').trim();
  const models = asObjectRecord(llm.models);
  const currentModel = asObjectRecord(defaultModelKey ? models[defaultModelKey] : null);
  const configuredModelName = String(
    currentModel.model || currentModel.model_name || currentModel.name || ''
  ).trim();
  const supportHearing = currentModel.support_hearing;
  return {
    hearingSupported: supportHearing === false ? false : true,
    modelDisplayName: configuredModelName || defaultModelKey
  };
};

const readDesktopDefaultModelMeta = async (
  force = false
): Promise<{ hearingSupported: boolean; modelDisplayName: string }> => {
  if (!desktopMode.value) {
    agentVoiceModelHearingSupported.value = true;
    desktopDefaultModelDisplayName.value = '';
    return { hearingSupported: true, modelDisplayName: '' };
  }
  const now = Date.now();
  if (
    !force &&
    agentVoiceModelHearingSupported.value !== null &&
    now - agentVoiceModelSupportCheckedAt <= AGENT_VOICE_MODEL_SUPPORT_CACHE_MS
  ) {
    return {
      hearingSupported: agentVoiceModelHearingSupported.value,
      modelDisplayName: String(desktopDefaultModelDisplayName.value || '').trim()
    };
  }
  if (desktopDefaultModelMetaFetchPromise) {
    return desktopDefaultModelMetaFetchPromise;
  }
  desktopDefaultModelMetaFetchPromise = (async () => {
    try {
      const response = await fetchDesktopSettings();
      const settings = (response?.data?.data || {}) as Record<string, unknown>;
      const meta = resolveDesktopDefaultModelMeta(settings);
      agentVoiceModelHearingSupported.value = meta.hearingSupported;
      desktopDefaultModelDisplayName.value = meta.modelDisplayName;
      return meta;
    } catch {
      agentVoiceModelHearingSupported.value = null;
      desktopDefaultModelDisplayName.value = '';
      return { hearingSupported: true, modelDisplayName: '' };
    } finally {
      agentVoiceModelSupportCheckedAt = Date.now();
      desktopDefaultModelMetaFetchPromise = null;
    }
  })();
  return desktopDefaultModelMetaFetchPromise;
};

const readAgentVoiceModelSupport = async (force = false): Promise<boolean> => {
  const meta = await readDesktopDefaultModelMeta(force);
  return meta.hearingSupported;
};

const WORLD_VOICE_RECORDING_TICK_MS = 120;

const clearAgentVoiceRecordingTimer = (runtime: AgentVoiceRecordingRuntime | null) => {
  if (!runtime) return;
  if (runtime.timerId !== null && typeof window !== 'undefined') {
    window.clearInterval(runtime.timerId);
  }
  runtime.timerId = null;
};

const resetAgentVoiceRecordingState = () => {
  agentVoiceRecording.value = false;
  agentVoiceDurationMs.value = 0;
};

const cancelAgentVoiceRecording = async () => {
  const runtime = agentVoiceRecordingRuntime;
  if (!runtime) return;
  agentVoiceRecordingRuntime = null;
  clearAgentVoiceRecordingTimer(runtime);
  resetAgentVoiceRecordingState();
  await runtime.session.cancel().catch(() => undefined);
};

const startAgentVoiceRecording = async () => {
  if (!isAgentConversationActive.value || agentSessionLoading.value) return;
  refreshAudioRecordingSupport();
  if (agentVoiceRecordingRuntime) return;
  const draftIdentity = resolveAgentDraftIdentity();
  if (!draftIdentity) return;
  try {
    const session = await startAudioRecording();
    const runtime: AgentVoiceRecordingRuntime = {
      session,
      startedAt: Date.now(),
      timerId: null,
      draftIdentity
    };
    agentVoiceRecordingRuntime = runtime;
    agentVoiceRecording.value = true;
    agentVoiceDurationMs.value = 0;
    if (typeof window !== 'undefined') {
      runtime.timerId = window.setInterval(() => {
        agentVoiceDurationMs.value = Math.max(0, Date.now() - runtime.startedAt);
      }, WORLD_VOICE_RECORDING_TICK_MS);
    }
  } catch (error) {
    resetAgentVoiceRecordingState();
    const message = resolveVoiceRecordingErrorText(error);
    if (message) {
      ElMessage.warning(message);
      return;
    }
    showApiError(error, t('messenger.world.voice.startFailed'));
  }
};

const buildAgentVoiceFileName = (): string => `agent-voice-${Date.now()}.wav`;

const stopAgentVoiceRecordingAndSend = async () => {
  const runtime = agentVoiceRecordingRuntime;
  if (!runtime) return;
  agentVoiceRecordingRuntime = null;
  clearAgentVoiceRecordingTimer(runtime);
  resetAgentVoiceRecordingState();
  let recording: AudioRecordingResult;
  try {
    recording = await runtime.session.stop();
  } catch (error) {
    showApiError(error, t('messenger.world.voice.stopFailed'));
    return;
  }
  if (!(recording?.blob instanceof Blob) || !recording.blob.size) {
    ElMessage.warning(t('messenger.world.voice.empty'));
    return;
  }
  if (runtime.draftIdentity !== resolveAgentDraftIdentity()) {
    return;
  }
  try {
    const voiceFile = new File([recording.blob], buildAgentVoiceFileName(), { type: 'audio/wav' });
    const uploadedPaths = await uploadWorldFilesToUserContainer([voiceFile], { appendTokens: false });
    const uploadedPath = String(uploadedPaths[0] || '').trim();
    if (!uploadedPath) {
      throw new Error(t('workspace.upload.failed'));
    }
    const attachmentToken = buildWorldAttachmentToken(uploadedPath);
    await sendAgentMessage({
      content: attachmentToken || uploadedPath,
      attachments: [
        {
          type: 'file',
          name: voiceFile.name,
          content: uploadedPath,
          mime_type: 'audio/wav'
        }
      ]
    });
  } catch (error) {
    showApiError(error, t('chat.error.requestFailed'));
  }
};

const toggleAgentVoiceRecord = async () => {
  if (agentVoiceRecordingRuntime) {
    await stopAgentVoiceRecordingAndSend();
    return;
  }
  await startAgentVoiceRecording();
};

const clearWorldVoiceRecordingTimer = (runtime: WorldVoiceRecordingRuntime | null) => {
  if (!runtime) return;
  if (runtime.timerId !== null && typeof window !== 'undefined') {
    window.clearInterval(runtime.timerId);
  }
  runtime.timerId = null;
};

const resetWorldVoiceRecordingState = () => {
  worldVoiceRecording.value = false;
  worldVoiceDurationMs.value = 0;
};

const cancelWorldVoiceRecording = async () => {
  const runtime = worldVoiceRecordingRuntime;
  if (!runtime) return;
  worldVoiceRecordingRuntime = null;
  clearWorldVoiceRecordingTimer(runtime);
  resetWorldVoiceRecordingState();
  await runtime.session.cancel().catch(() => undefined);
};

const startWorldVoiceRecording = async () => {
  if (!isWorldConversationActive.value || worldUploading.value || userWorldStore.sending) return;
  refreshAudioRecordingSupport();
  if (worldVoiceRecordingRuntime) return;
  const conversationId = String(activeConversation.value?.id || '').trim();
  if (!conversationId) return;
  closeWorldAttachmentPanels();
  try {
    const session = await startAudioRecording();
    const runtime: WorldVoiceRecordingRuntime = {
      session,
      startedAt: Date.now(),
      timerId: null,
      conversationId
    };
    worldVoiceRecordingRuntime = runtime;
    worldVoiceRecording.value = true;
    worldVoiceDurationMs.value = 0;
    if (typeof window !== 'undefined') {
      runtime.timerId = window.setInterval(() => {
        worldVoiceDurationMs.value = Math.max(0, Date.now() - runtime.startedAt);
      }, WORLD_VOICE_RECORDING_TICK_MS);
    }
  } catch (error) {
    resetWorldVoiceRecordingState();
    const message = resolveVoiceRecordingErrorText(error);
    if (message) {
      ElMessage.warning(message);
      return;
    }
    showApiError(error, t('messenger.world.voice.startFailed'));
  }
};

const buildWorldVoiceFileName = (): string => `voice-${Date.now()}.wav`;

const stopWorldVoiceRecordingAndSend = async () => {
  const runtime = worldVoiceRecordingRuntime;
  if (!runtime) return;
  worldVoiceRecordingRuntime = null;
  clearWorldVoiceRecordingTimer(runtime);
  resetWorldVoiceRecordingState();
  let recording: AudioRecordingResult;
  try {
    recording = await runtime.session.stop();
  } catch (error) {
    showApiError(error, t('messenger.world.voice.stopFailed'));
    return;
  }
  if (!(recording?.blob instanceof Blob) || !recording.blob.size) {
    ElMessage.warning(t('messenger.world.voice.empty'));
    return;
  }
  if (runtime.conversationId !== String(activeConversation.value?.id || '').trim()) {
    return;
  }
  worldUploading.value = true;
  try {
    const voiceFile = new File([recording.blob], buildWorldVoiceFileName(), { type: 'audio/wav' });
    const uploadedPaths = await uploadWorldFilesToUserContainer([voiceFile], { appendTokens: false });
    const uploadedPath = String(uploadedPaths[0] || '').trim();
    if (!uploadedPath) {
      throw new Error(t('workspace.upload.failed'));
    }
    const senderUserId = String((authStore.user as Record<string, unknown> | null)?.id || '').trim();
    const payloadText = buildWorldVoicePayloadContent({
      path: uploadedPath,
      durationMs: recording.durationMs,
      mimeType: 'audio/wav',
      name: voiceFile.name,
      size: voiceFile.size,
      containerId: USER_CONTAINER_ID,
      ownerUserId: senderUserId
    });
    await userWorldStore.sendToActiveConversation(payloadText, { contentType: 'voice' });
    await scrollMessagesToBottom();
  } catch (error) {
    showApiError(error, t('userWorld.input.sendFailed'));
  } finally {
    worldUploading.value = false;
  }
};

const toggleWorldVoiceRecord = async () => {
  if (worldVoiceRecordingRuntime) {
    await stopWorldVoiceRecordingAndSend();
    return;
  }
  await startWorldVoiceRecording();
};

const triggerWorldUpload = () => {
  const uploadInput = worldComposerViewRef.value?.getUploadInputElement() || null;
  if (!isWorldConversationActive.value || worldUploading.value || worldVoiceRecording.value || !uploadInput) return;
  closeWorldAttachmentPanels();
  uploadInput.value = '';
  uploadInput.click();
};

const triggerWorldScreenshot = async (option?: WorldScreenshotCaptureOption) => {
  if (!isWorldConversationActive.value || worldUploading.value || worldVoiceRecording.value) return;
  if (!worldDesktopScreenshotSupported.value) {
    ElMessage.warning(t('chat.attachments.screenshotUnavailable'));
    return;
  }
  closeWorldAttachmentPanels();
  const screenshotOption: WorldScreenshotCaptureOption = {
    hideWindow: option?.hideWindow === true,
    region: option?.region === true
  };
  worldUploading.value = true;
  try {
    const captured = await captureWorldScreenshotData(screenshotOption);
    let finalFileName = captured.fileName;
    if (screenshotOption.region && !/[-_]region(\.[^./]+)?$/i.test(finalFileName)) {
      finalFileName = appendScreenshotFileNameSuffix(finalFileName, '-region');
    }
    const screenshotFile = screenshotDataUrlToFile(
      captured.dataUrl,
      finalFileName,
      captured.mimeType
    );
    const uploaded = await uploadWorldFilesToUserContainer([screenshotFile]);
    if (!uploaded.length) {
      throw new Error(t('workspace.upload.failed'));
    }
    ElMessage.success(t('chat.attachments.screenshotAdded', { name: screenshotFile.name }));
    focusWorldTextareaToEnd();
  } catch (error) {
    if ((error as { message?: string })?.message === '__SCREENSHOT_CANCELED__') {
      return;
    }
    showApiError(error, t('chat.attachments.screenshotFailed'));
  } finally {
    worldUploading.value = false;
  }
};

const handleWorldUploadInput = async (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  if (worldVoiceRecording.value) {
    if (target) target.value = '';
    return;
  }
  const files = target?.files ? Array.from(target.files) : [];
  if (!files.length) return;
  const oversized = findWorldOversizedFile(files);
  if (oversized) {
    ElMessage.warning(t('workspace.upload.tooLarge', { limit: '200 MB' }));
    if (target) target.value = '';
    return;
  }
  worldUploading.value = true;
  try {
    const uploaded = await uploadWorldFilesToUserContainer(files);
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
  if (worldVoiceRecording.value) return;
  if (!canSendWorldMessage.value) return;
  const text = worldDraft.value.trim();
  if (!text) return;
  const senderUserId = String((authStore.user as Record<string, unknown> | null)?.id || '').trim();
  const normalizedText = replaceWorldAtPathTokens(text, senderUserId);
  worldQuickPanelMode.value = '';
  worldDraft.value = '';
  try {
    await userWorldStore.sendToActiveConversation(normalizedText);
    await scrollMessagesToBottom();
  } catch (error) {
    worldDraft.value = text;
    showApiError(error, t('userWorld.input.sendFailed'));
  }
};

const handleWorldComposerEnterKeydown = async (event: KeyboardEvent) => {
  if (event.isComposing) {
    return;
  }
  if (messengerSendKey.value === 'none') {
    return;
  }
  const hasPrimaryModifier = Boolean(
    event.ctrlKey ||
      event.metaKey ||
      event.getModifierState?.('Control') ||
      event.getModifierState?.('Meta')
  );
  const hasBackupModifier = Boolean(event.altKey && !hasPrimaryModifier);
  if (hasPrimaryModifier || hasBackupModifier) {
    event.preventDefault();
    await sendWorldMessage();
    return;
  }
  if (messengerSendKey.value === 'ctrl_enter') {
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

const toggleLanguage = async () => {
  const next = getCurrentLanguage() === 'zh-CN' ? 'en-US' : 'zh-CN';
  await setLanguage(next);
  ElMessage.success(t('messenger.more.languageChanged'));
};

const normalizeDesktopUpdatePhase = (state?: DesktopUpdateState | null) =>
  String(state?.phase || '')
    .trim()
    .toLowerCase();

const resolveDesktopUpdateProgress = (state?: DesktopUpdateState | null) => {
  const raw = Number(state?.progress);
  if (!Number.isFinite(raw)) {
    return 0;
  }
  return Math.max(0, Math.min(100, Math.round(raw)));
};

const isDesktopUpdatePending = (phase: string) =>
  phase === 'checking' || phase === 'available' || phase === 'downloading';

const isDesktopUpdateTerminal = (phase: string) =>
  phase === 'downloaded' ||
  phase === 'error' ||
  phase === 'not-available' ||
  phase === 'idle' ||
  phase === 'unsupported';

const buildDesktopUpdateStatusText = (state?: DesktopUpdateState | null) => {
  const phase = normalizeDesktopUpdatePhase(state);
  if (phase === 'checking') {
    return t('desktop.settings.checkingUpdate');
  }
  if (phase === 'downloading' || phase === 'available') {
    const progress = resolveDesktopUpdateProgress(state);
    if (progress > 0) {
      return t('desktop.settings.updateDownloadingProgress', { progress });
    }
    return t('desktop.settings.updateDownloading');
  }
  return t('desktop.settings.updateDownloading');
};

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const pollDesktopUpdateState = async (
  bridge: DesktopBridge,
  initialState: DesktopUpdateState,
  onTick: (state: DesktopUpdateState) => void
) => {
  if (typeof bridge.getUpdateState !== 'function') {
    onTick(initialState);
    return initialState;
  }

  let state = initialState;
  const started = Date.now();
  const timeoutMs = 15 * 60 * 1000;
  while (Date.now() - started < timeoutMs) {
    onTick(state);
    const phase = normalizeDesktopUpdatePhase(state);
    if (isDesktopUpdateTerminal(phase) || !isDesktopUpdatePending(phase)) {
      return state;
    }
    await wait(700);
    try {
      state = await bridge.getUpdateState();
    } catch {
      return state;
    }
  }
  return state;
};

const checkClientUpdate = async () => {
  if (!desktopMode.value) {
    ElMessage.success(t('common.refreshSuccess'));
    return;
  }

  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.checkForUpdates !== 'function') {
    ElMessage.warning(t('desktop.settings.updateUnsupported'));
    return;
  }

  const loading = ElLoading.service({
    lock: false,
    text: t('desktop.settings.checkingUpdate'),
    background: 'rgba(0, 0, 0, 0.06)'
  });

  try {
    let state = await bridge.checkForUpdates();
    let lastStatusText = '';
    const updateLoadingText = (nextState: DesktopUpdateState) => {
      const nextText = buildDesktopUpdateStatusText(nextState);
      if (nextText && nextText !== lastStatusText) {
        loading.setText(nextText);
        lastStatusText = nextText;
      }
    };
    state = await pollDesktopUpdateState(bridge, state, updateLoadingText);
    loading.close();

    const phase = String(state?.phase || '').trim().toLowerCase();
    const latestVersion = String(state?.latestVersion || '').trim();

    if (phase === 'not-available' || phase === 'idle') {
      ElMessage.success(t('desktop.settings.updateNotAvailable'));
      return;
    }

    if (phase === 'unsupported') {
      ElMessage.warning(t('desktop.settings.updateUnsupported'));
      return;
    }

    if (phase === 'error') {
      const reason = String(state?.message || '').trim() || t('common.unknown');
      ElMessage.error(t('desktop.settings.updateCheckFailed', { reason }));
      return;
    }

    if (phase === 'downloading' || phase === 'available' || phase === 'checking') {
      const progress = resolveDesktopUpdateProgress(state);
      if (progress > 0) {
        ElMessage.info(t('desktop.settings.updateDownloadingProgress', { progress }));
      } else {
        ElMessage.info(t('desktop.settings.updateDownloading'));
      }
      return;
    }

    if (phase !== 'downloaded') {
      ElMessage.info(t('desktop.settings.updateUnknownState'));
      return;
    }

    const versionText = latestVersion || String(state?.currentVersion || '-');
    const confirmed = await confirmWithFallback(
      t('desktop.settings.updateReadyConfirm', { version: versionText }),
      t('desktop.settings.update'),
      {
        type: 'warning',
        confirmButtonText: t('desktop.settings.installNow'),
        cancelButtonText: t('common.cancel')
      }
    );
    if (!confirmed) {
      ElMessage.info(t('desktop.settings.updateReadyLater'));
      return;
    }

    if (typeof bridge.installUpdate !== 'function') {
      ElMessage.warning(t('desktop.settings.updateUnsupported'));
      return;
    }

    const installResult = await bridge.installUpdate();
    const installOk =
      typeof installResult === 'boolean' ? installResult : Boolean((installResult as DesktopInstallResult)?.ok);

    if (!installOk) {
      ElMessage.warning(t('desktop.settings.updateInstallFailed'));
      return;
    }

    ElMessage.success(t('desktop.settings.updateInstalling'));
  } catch (error) {
    loading.close();
    const reason = String((error as { message?: unknown })?.message || '').trim() || t('common.unknown');
    ElMessage.error(t('desktop.settings.updateCheckFailed', { reason }));
  }
};

const updateSendKey = (value: MessengerSendKeyMode) => {
  const normalized = normalizeMessengerSendKey(value);
  messengerSendKey.value = normalized;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(MESSENGER_SEND_KEY_STORAGE_KEY, normalized);
  }
};

const updateCurrentUsername = async (value: string) => {
  const normalized = String(value || '').trim();
  if (!normalized) {
    ElMessage.warning(t('profile.edit.usernameRequired'));
    return;
  }
  const current = String((authStore.user as Record<string, unknown> | null)?.username || '').trim();
  if (current === normalized || usernameSaving.value) {
    return;
  }
  usernameSaving.value = true;
  try {
    const { data } = await updateProfile({ username: normalized });
    const profile = data?.data;
    if (profile && typeof profile === 'object') {
      authStore.user = profile;
    } else {
      await authStore.loadProfile();
    }
    ElMessage.success(t('profile.edit.saved'));
  } catch (error) {
    showApiError(error, t('profile.edit.saveFailed'));
  } finally {
    usernameSaving.value = false;
  }
};

const handleSessionApprovalDecision = async (
  decision: 'approve_once' | 'approve_session' | 'deny'
) => {
  const approval = activeSessionApproval.value;
  if (!approval || approvalResponding.value) return;
  approvalResponding.value = true;
  try {
    await chatStore.respondApproval(decision, approval.approval_id);
    if (decision !== 'deny') {
      ElMessage.success(t('chat.approval.sent'));
    }
  } catch (error) {
    showApiError(error, t('chat.approval.sendFailed'));
  } finally {
    approvalResponding.value = false;
  }
};

const updateThemePalette = (value: ThemePalette) => {
  themeStore.setPalette(normalizeThemePalette(value));
};

const updatePerformanceMode = (value: 'high' | 'low') => {
  performanceStore.setMode(value);
};

const updateUiFontSize = (value: number) => {
  const normalized = normalizeUiFontSize(value);
  uiFontSize.value = normalized;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY, String(normalized));
  }
  applyUiFontSize(normalized);
};

const openDebugTools = async () => {
  if (typeof window === 'undefined') return;
  try {
    const bridge = getDesktopBridge();
    if (typeof bridge?.toggleDevTools === 'function') {
      await bridge.toggleDevTools();
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
    handleAgentRuntimeStateUpdate(stateMap);
  } catch {
    agentRuntimeStateMap.value = new Map<string, AgentRuntimeState>();
    agentRuntimeStateSnapshot = new Map<string, AgentRuntimeState>();
    agentRuntimeStateHydrated = false;
  }
};

const loadAgentUserRounds = async () => {
  try {
    const response = await listAgentUserRounds();
    const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
    const roundsMap = new Map<string, number>();
    items.forEach((item: Record<string, unknown>) => {
      const key = normalizeAgentUserRoundsKey(item?.agent_id);
      const raw = Number(item?.user_rounds ?? item?.rounds ?? 0);
      const value = Number.isFinite(raw) ? Math.max(0, Math.floor(raw)) : 0;
      roundsMap.set(key, value);
    });
    agentUserRoundsMap.value = roundsMap;
  } catch {
    agentUserRoundsMap.value = new Map<string, number>();
  }
};

const resolveHttpStatus = (error: unknown): number => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

const isAuthDeniedStatus = (status: number): boolean => status === 401 || status === 403;

const handleCronPanelChanged = (payload?: { agentId?: string; hasJobs?: boolean }) => {
  const normalizeChangedAgentId = (value: unknown): string => {
    const raw = String(value || '').trim();
    if (!raw) return DEFAULT_AGENT_KEY;
    const lowered = raw.toLowerCase();
    if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
      return DEFAULT_AGENT_KEY;
    }
    return normalizeAgentId(raw);
  };
  const hasJobs = payload?.hasJobs;
  if (hasJobs === true || hasJobs === false) {
    const next = new Set(cronAgentIds.value);
    const changedAgentId = normalizeChangedAgentId(payload?.agentId);
    if (hasJobs) {
      next.add(changedAgentId);
    } else {
      next.delete(changedAgentId);
    }
    cronAgentIds.value = next;
  }
  void loadCronAgentIds();
};

const loadCronAgentIds = async () => {
  const loadVersion = ++cronAgentIdsLoadVersion;
  if (cronPermissionDenied.value) {
    if (loadVersion === cronAgentIdsLoadVersion) {
      cronAgentIds.value = new Set<string>();
    }
    return;
  }
  try {
    const normalizeCronAgentKey = (value: unknown): string => {
      const raw = String(value || '').trim();
      if (!raw) return '';
      const lowered = raw.toLowerCase();
      if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
        return DEFAULT_AGENT_KEY;
      }
      return normalizeAgentId(raw);
    };
    const sessionAgentMap = new Map<string, string>();
    const sessions = Array.isArray(chatStore.sessions) ? chatStore.sessions : [];
    sessions.forEach((session: Record<string, unknown>) => {
      const sessionId = String(session?.id || '').trim();
      if (!sessionId) return;
      const explicitAgent = normalizeCronAgentKey(session?.agent_id ?? session?.agentId);
      const fallbackAgent = session?.is_main === true ? DEFAULT_AGENT_KEY : '';
      const resolvedAgent = explicitAgent || fallbackAgent;
      if (resolvedAgent) {
        sessionAgentMap.set(sessionId, resolvedAgent);
      }
    });
    const response = await fetchCronJobs();
    if (loadVersion !== cronAgentIdsLoadVersion) {
      return;
    }
    const jobs = Array.isArray(response?.data?.data?.jobs)
      ? response.data.data.jobs
      : Array.isArray(response?.data?.data?.items)
        ? response.data.data.items
        : [];
    const result = new Set<string>();
    jobs.forEach((job: Record<string, unknown>) => {
      const rawAgentId = String(
        job?.agent_id ??
          job?.agentId ??
          (job?.agent as Record<string, unknown> | undefined)?.id ??
          (job?.agent as Record<string, unknown> | undefined)?.agent_id ??
          ''
      ).trim();
      const mappedSessionAgent = sessionAgentMap.get(
        String(job?.session_id ?? job?.sessionId ?? '').trim()
      );
      const target = String(
        job?.session_target ?? job?.sessionTarget ?? job?.session ?? ''
      ).trim().toLowerCase();
      const defaultTarget =
        target === '' ||
        target === 'main' ||
        target === 'default' ||
        target === 'system' ||
        target === '__default__';
      const resolved =
        rawAgentId ||
        mappedSessionAgent ||
        (defaultTarget ||
        job?.is_default === true ||
        job?.isDefault === true
          ? DEFAULT_AGENT_KEY
          : '');
      if (!resolved) return;
      result.add(normalizeCronAgentKey(resolved));
    });
    if (loadVersion !== cronAgentIdsLoadVersion) {
      return;
    }
    cronAgentIds.value = result;
    cronPermissionDenied.value = false;
  } catch (error) {
    if (loadVersion !== cronAgentIdsLoadVersion) {
      return;
    }
    const status = resolveHttpStatus(error);
    if (isAuthDeniedStatus(status)) {
      cronPermissionDenied.value = true;
      cronAgentIds.value = new Set<string>();
      return;
    }
  }
};

const loadChannelBoundAgentIds = async () => {
  const loadVersion = ++channelBoundAgentIdsLoadVersion;
  try {
    const normalizeChannelAgentKey = (value: unknown): string => {
      const raw = String(value || '').trim();
      if (!raw) return DEFAULT_AGENT_KEY;
      const lowered = raw.toLowerCase();
      if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
        return DEFAULT_AGENT_KEY;
      }
      return normalizeAgentId(raw);
    };
    const response = await listChannelBindings();
    if (loadVersion !== channelBoundAgentIdsLoadVersion) {
      return;
    }
    const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
    const bound = new Set<string>();
    items.forEach((item: Record<string, unknown>) => {
      const agentId = normalizeChannelAgentKey(
        item?.agent_id ??
          item?.agentId ??
          (item?.agent as Record<string, unknown> | undefined)?.id ??
          (item?.agent as Record<string, unknown> | undefined)?.agent_id ??
          (item?.config as Record<string, unknown> | undefined)?.agent_id ??
          (item?.raw_config as Record<string, unknown> | undefined)?.agent_id ??
          ''
      );
      bound.add(agentId);
    });
    if (loadVersion !== channelBoundAgentIdsLoadVersion) {
      return;
    }
    channelBoundAgentIds.value = bound;
  } catch (error) {
    if (loadVersion !== channelBoundAgentIdsLoadVersion) {
      return;
    }
    const status = resolveHttpStatus(error);
    if (isAuthDeniedStatus(status)) {
      channelBoundAgentIds.value = new Set<string>();
      return;
    }
  }
};

const refreshAll = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    beeroomStore.loadGroups(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(true),
    loadOrgUnits(),
    loadRunningAgents(),
    loadAgentUserRounds(),
    loadToolsCatalog(),
    loadChannelBoundAgentIds()
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds());
  }
  await Promise.allSettled(tasks);
  ensureSectionSelection();
  ElMessage.success(t('common.refreshSuccess'));
};

const syncMessageVirtualMetrics = () => {
  const container = messageListRef.value;
  if (!container || showChatSettingsView.value) {
    messageVirtualScrollTop.value = 0;
    messageVirtualViewportHeight.value = 0;
    return;
  }
  messageVirtualViewportHeight.value = container.clientHeight;
  messageVirtualScrollTop.value = container.scrollTop;
};

const pruneMessageVirtualHeightCache = () => {
  const keySet = new Set<string>([
    ...agentRenderableMessages.value.map((item) => item.key),
    ...worldRenderableMessages.value.map((item) => item.key)
  ]);
  let changed = false;
  messageVirtualHeightCache.forEach((_value, key) => {
    if (keySet.has(key)) {
      return;
    }
    messageVirtualHeightCache.delete(key);
    changed = true;
  });
  if (changed) {
    messageVirtualLayoutVersion.value += 1;
  }
};

const measureVisibleMessageHeights = () => {
  const container = messageListRef.value;
  if (!container || showChatSettingsView.value) {
    return;
  }
  const nodes = container.querySelectorAll<HTMLElement>('.messenger-message[data-virtual-key]');
  let changed = false;
  nodes.forEach((node) => {
    const key = String(node.dataset.virtualKey || '').trim();
    if (!key) return;
    const height = Math.max(1, Math.round(node.getBoundingClientRect().height));
    const cached = messageVirtualHeightCache.get(key);
    if (cached && Math.abs(cached - height) <= 1) {
      return;
    }
    messageVirtualHeightCache.set(key, height);
    changed = true;
  });
  if (changed) {
    messageVirtualLayoutVersion.value += 1;
  }
};

const scheduleMessageVirtualMeasure = () => {
  if (typeof window === 'undefined') return;
  if (messageVirtualMeasureFrame !== null) return;
  messageVirtualMeasureFrame = window.requestAnimationFrame(() => {
    messageVirtualMeasureFrame = null;
    measureVisibleMessageHeights();
  });
};

const handleMessageWorkflowLayoutChange = () => {
  syncMessageVirtualMetrics();
  scheduleMessageVirtualMeasure();
};

const updateMessageScrollState = () => {
  syncMessageVirtualMetrics();
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
  if (typeof window === 'undefined') {
    updateMessageScrollState();
    return;
  }
  if (messageScrollFrame !== null) return;
  messageScrollFrame = window.requestAnimationFrame(() => {
    messageScrollFrame = null;
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  });
};

const scrollMessagesToBottom = async (force = false) => {
  await nextTick();
  const container = messageListRef.value;
  if (!container) return;
  if (!force && !autoStickToBottom.value) {
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
    return;
  }
  container.scrollTop = container.scrollHeight;
  updateMessageScrollState();
  scheduleMessageVirtualMeasure();
};

const jumpToMessageBottom = async () => {
  autoStickToBottom.value = true;
  await scrollMessagesToBottom(true);
};

const scrollVirtualMessageToIndex = (keys: string[], index: number, align: 'center' | 'start' = 'center') => {
  const container = messageListRef.value;
  if (!container || !keys.length) return;
  const safeIndex = Math.max(0, Math.min(keys.length - 1, Math.trunc(index)));
  const top = estimateVirtualOffsetTop(keys, safeIndex);
  const height = resolveVirtualMessageHeight(keys[safeIndex]);
  const targetTop =
    align === 'center'
      ? top - container.clientHeight / 2 + height / 2
      : top;
  const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
  container.scrollTop = Math.max(0, Math.min(targetTop, maxTop));
  syncMessageVirtualMetrics();
};

const scrollLatestAssistantToCenter = async () => {
  if (!isAgentConversationActive.value) return;
  if (shouldVirtualizeMessages.value) {
    const latestIndex = (() => {
      for (let cursor = agentRenderableMessages.value.length - 1; cursor >= 0; cursor -= 1) {
        const item = agentRenderableMessages.value[cursor];
        if (String(item.message?.role || '') !== 'assistant') continue;
        return cursor;
      }
      return -1;
    })();
    if (latestIndex >= 0) {
      scrollVirtualMessageToIndex(
        agentRenderableMessages.value.map((item) => item.key),
        latestIndex,
        'center'
      );
      await nextTick();
    }
  }
  await nextTick();
  const container = messageListRef.value;
  if (!container) return;
  const items = container.querySelectorAll('.messenger-message:not(.mine)');
  if (!items.length) return;
  const target = items[items.length - 1] as HTMLElement;
  requestAnimationFrame(() => {
    const containerRect = container.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const targetCenter = targetRect.top - containerRect.top + targetRect.height / 2;
    const nextTop = container.scrollTop + targetCenter - container.clientHeight / 2;
    const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
    container.scrollTop = Math.max(0, Math.min(nextTop, maxTop));
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  });
};

const normalizeAgentId = (value: unknown): string => {
  const text = String(value || '').trim();
  return text || DEFAULT_AGENT_KEY;
};

const restoreConversationFromRoute = async () => {
  const query = route.query;
  const queryConversationId = String(query?.conversation_id || '').trim();
  if (queryConversationId) {
    if (userWorldPermissionDenied.value) {
      const nextQuery = { ...route.query } as Record<string, any>;
      delete nextQuery.conversation_id;
      router.replace({ path: route.path, query: nextQuery }).catch(() => undefined);
    }
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

  const preferredSection = desktopMode.value
    ? ('messages' as MessengerSection)
    : resolveSectionFromRoute(route.path, query.section);
  if (preferredSection === 'messages') {
    const first = mixedConversations.value[0];
    if (first) {
      await openMixedConversation(first);
      return;
    }
  }

  clearMessagePanelWhenConversationEmpty();
};

const bootstrap = async () => {
  bootLoading.value = true;
  if (!authStore.user && authStore.token) {
    try {
      await authStore.loadProfile();
    } catch (error) {
      const status = resolveHttpStatus(error);
      if (isAuthDeniedStatus(status)) {
        authStore.logout();
        bootLoading.value = false;
        router.replace('/login').catch(() => undefined);
        return;
      }
    }
  }
  const initialSection = desktopMode.value
    ? ('messages' as MessengerSection)
    : resolveSectionFromRoute(route.path, route.query.section);
  const { critical, background } = splitMessengerBootstrapTasks(initialSection, [
    {
      critical: true,
      run: () => agentStore.loadAgents()
    },
    {
      critical: true,
      sections: ['swarms'],
      run: () => beeroomStore.loadGroups()
    },
    {
      critical: true,
      run: () => chatStore.loadSessions()
    },
    {
      sections: ['messages', 'users', 'groups'],
      run: () => userWorldStore.bootstrap()
    },
    {
      sections: ['users', 'groups'],
      run: () => loadOrgUnits()
    },
    {
      run: () => loadRunningAgents()
    },
    {
      run: () => loadAgentUserRounds()
    },
    {
      sections: ['tools'],
      run: () => loadToolsCatalog()
    },
    {
      sections: ['tools'],
      run: () => loadChannelBoundAgentIds()
    },
    ...(cronPermissionDenied.value
      ? []
      : [
          {
            sections: ['tools'],
            run: () => loadCronAgentIds()
          } satisfies MessengerBootstrapTask
        ])
  ]);
  await settleMessengerBootstrapTasks(critical);
  ensureSectionSelection();
  bootLoading.value = false;
  void restoreConversationFromRoute();
  scheduleMessengerBootstrapBackgroundTasks(background);
};

watch(
  () => sessionHub.keyword,
  (value) => {
    const normalized = String(value || '');
    if (keywordInput.value !== normalized) {
      keywordInput.value = normalized;
    }
  },
  { immediate: true }
);

watch(keywordInput, (value) => {
  const normalized = String(value || '').trimStart();
  if (typeof window === 'undefined') {
    sessionHub.setKeyword(normalized);
    return;
  }
  clearKeywordDebounce();
  keywordDebounceTimer = window.setTimeout(() => {
    keywordDebounceTimer = null;
    sessionHub.setKeyword(normalized);
  }, KEYWORD_INPUT_DEBOUNCE_MS);
});

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
  () => [route.path, route.query.section, route.query.panel],
  () => {
    const panelHint = String(route.query.panel || '').trim().toLowerCase();
    if (route.path.includes('/profile')) {
      settingsPanelMode.value = 'profile';
    } else if (panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt') {
      settingsPanelMode.value = 'prompts';
    } else if (desktopMode.value && (panelHint === 'desktop-models' || panelHint === 'desktop')) {
      settingsPanelMode.value = 'desktop-models';
    } else if (desktopMode.value && panelHint === 'desktop-lan') {
      settingsPanelMode.value = 'desktop-lan';
    } else if (desktopMode.value && panelHint === 'desktop-remote') {
      settingsPanelMode.value = 'desktop-remote';
    } else {
      settingsPanelMode.value = 'general';
    }
    const sectionHint = String(route.query.section || '').trim().toLowerCase();
    const helperHint = String(route.query.helper || '').trim().toLowerCase();
    const helperWorkspaceEnabled =
      sectionHint === 'groups' && (helperHint === '1' || helperHint === 'true' || helperHint === 'yes');
    helperAppsWorkspaceMode.value = helperWorkspaceEnabled;
    if (helperWorkspaceEnabled) {
      ensureHelperAppsSelection();
      loadHelperExternalApps();
    }
    if (desktopMode.value && !desktopInitialSectionPinned.value) {
      desktopInitialSectionPinned.value = true;
      sessionHub.setSection('messages');
      return;
    }
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
    beeroomStore.resetState();
    void hydrateCurrentUserAppearance();
    cronPermissionDenied.value = false;
    cronAgentIds.value = new Set<string>();
    clearWorkspaceResourceCache();
    ensureDismissedAgentConversationState(true);
    ensureAgentUnreadState(true);
    refreshAgentMainUnreadFromSessions();
    scheduleWorkspaceResourceHydration();
  },
  { immediate: true }
);

watch(
  () => userAttachmentWorkspacePaths.value,
  (paths) => {
    paths.forEach((path) => {
      void ensureUserAttachmentResource(path);
    });
  },
  { immediate: true }
);

watch(
  () => [themeStore.mode, themeStore.palette],
  () => {
    if (appearanceHydrating.value) return;
    void persistCurrentUserAppearance();
  }
);

watch(
  () => sessionHub.activeSection,
  (section) => {
    closeFileContainerMenu();
    if (section !== 'swarms') {
      beeroomWorkbenchMode.value = 'text';
    }
    if (
      section === 'tools' &&
      !builtinTools.value.length &&
      !mcpTools.value.length &&
      !skillTools.value.length &&
      !knowledgeTools.value.length &&
      !sharedTools.value.length
    ) {
      loadToolsCatalog();
    }
    if (section === 'users' && !userWorldPermissionDenied.value) {
      resetContactVirtualScroll();
      void nextTick(syncContactVirtualMetrics);
    }
    if (section === 'swarms') {
      if (!beeroomStore.groups.length) {
        void beeroomStore
          .loadGroups()
          .then(() => ensureSectionSelection())
          .catch(() => null);
      }
      if (beeroomStore.activeGroupId) {
        void beeroomStore.loadActiveGroup().catch(() => null);
      }
    }
    ensureSectionSelection();
  },
  { immediate: true }
);

watch(
  () => beeroomStore.activeGroupId,
  (value) => {
    if (sessionHub.activeSection !== 'swarms' || !String(value || '').trim()) return;
    void beeroomStore.loadActiveGroup({ silent: true }).catch(() => null);
  }
);

watch(
  () => showAgentGridOverview.value,
  (visible) => {
    if (visible) {
      loadAgentUserRounds();
    }
  }
);

watch(
  () => [filteredContacts.value.length, sessionHub.activeSection, userWorldPermissionDenied.value],
  () => {
    if (sessionHub.activeSection !== 'users' || userWorldPermissionDenied.value) return;
    void nextTick(syncContactVirtualMetrics);
  }
);

watch(
  () => [keyword.value, selectedContactUnitId.value],
  () => {
    if (sessionHub.activeSection !== 'users' || userWorldPermissionDenied.value) return;
    resetContactVirtualScroll();
    void nextTick(syncContactVirtualMetrics);
  }
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
  () => [
    chatStore.sessions
      .map((session) =>
        [
          String(session?.id || ''),
          normalizeAgentId(session?.agent_id),
          session?.is_main ? '1' : '0',
          String(session?.last_message_at || session?.updated_at || session?.created_at || '')
        ].join(':')
      )
      .join('|'),
    sessionHub.activeConversationKey
  ],
  () => {
    refreshAgentMainUnreadFromSessions();
  },
  { immediate: true }
);

watch(
  () => [hasAnyMixedConversations.value, sessionHub.activeSection, sessionHub.activeConversationKey],
  () => {
    clearMessagePanelWhenConversationEmpty();
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
    clearWorkspaceResourceCache();
    pendingAssistantCenter = false;
    pendingAssistantCenterCount = 0;
    agentPlanExpanded.value = false;
    dismissedPlanMessages.value = new WeakSet<Record<string, unknown>>();
    dismissedPlanVersion.value += 1;
    agentInquirySelection.value = [];
    scheduleWorkspaceResourceHydration();
  }
);

watch(
  () => activeAgentPlan.value,
  (value) => {
    if (!value) {
      agentPlanExpanded.value = false;
    }
  }
);

watch(
  () => activeAgentInquiryPanel.value,
  (value) => {
    if (!value) {
      agentInquirySelection.value = [];
    }
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
  () => showChatSettingsView.value,
  () => {
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  }
);

watch(
  () => [chatStore.messages.length, userWorldStore.activeMessages.length, sessionHub.activeConversationKey],
  () => {
    pruneMessageVirtualHeightCache();
    void nextTick(() => {
      syncMessageVirtualMetrics();
      scheduleMessageVirtualMeasure();
    });
    scheduleWorkspaceResourceHydration();
    if (
      pendingAssistantCenter &&
      isAgentConversationActive.value &&
      chatStore.messages.length > pendingAssistantCenterCount
    ) {
      const lastMessage = chatStore.messages[chatStore.messages.length - 1] as
        | Record<string, unknown>
        | undefined;
      if (String(lastMessage?.role || '') === 'assistant') {
        pendingAssistantCenter = false;
        pendingAssistantCenterCount = chatStore.messages.length;
        autoStickToBottom.value = false;
        void scrollLatestAssistantToCenter();
        return;
      }
    }
    if (autoStickToBottom.value) {
      void scrollMessagesToBottom();
    } else {
      updateMessageScrollState();
    }
  }
);

watch(
  () => chatStore.messages[chatStore.messages.length - 1]?.content,
  () => {
    scheduleWorkspaceResourceHydration();
    scheduleMessageVirtualMeasure();
  }
);

watch(
  () => userWorldStore.activeMessages[userWorldStore.activeMessages.length - 1]?.content,
  () => {
    scheduleWorkspaceResourceHydration();
    scheduleMessageVirtualMeasure();
  }
);

watch(
  () => [agentRenderableMessages.value.length, worldRenderableMessages.value.length],
  () => {
    pruneMessageVirtualHeightCache();
    void nextTick(() => {
      syncMessageVirtualMetrics();
      scheduleMessageVirtualMeasure();
    });
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
      void cancelWorldVoiceRecording();
      disposeWorldVoicePlayback();
    }
  }
);

watch(
  () => [isAgentConversationActive.value, desktopMode.value] as const,
  ([active, desktop]) => {
    if (!active) {
      void cancelAgentVoiceRecording();
      return;
    }
    if (desktop) {
      void readDesktopDefaultModelMeta(false);
    }
  },
  { immediate: true }
);

watch(
  () => agentComposerDraftKey.value,
  (nextKey, previousKey) => {
    if (previousKey && previousKey !== nextKey) {
      void cancelAgentVoiceRecording();
    }
  }
);

watch(
  () => Boolean(activeSessionApproval.value),
  (visible) => {
    if (visible) {
      void cancelAgentVoiceRecording();
    }
  }
);

watch(
  () => activeWorldConversationId.value,
  (nextConversationId, previousConversationId) => {
    if (previousConversationId && previousConversationId !== nextConversationId) {
      void cancelWorldVoiceRecording();
      disposeWorldVoicePlayback();
    }
    if (previousConversationId) {
      writeWorldDraft(previousConversationId, worldDraft.value);
    }
    worldDraft.value = readWorldDraft(nextConversationId);
    clearWorldQuickPanelClose();
    worldQuickPanelMode.value = '';
    worldHistoryDialogVisible.value = false;
  }
);

watch(
  () => worldDraft.value,
  (value) => {
    writeWorldDraft(activeWorldConversationId.value, value);
  }
);

onUpdated(() => {
  scheduleWorkspaceResourceHydration();
});

onMounted(async () => {
  if (typeof window !== 'undefined') {
    viewportResizeHandler = () => {
      viewportWidth.value = window.innerWidth;
      closeFileContainerMenu();
      syncContactVirtualMetrics();
      syncMessageVirtualMetrics();
      scheduleMessageVirtualMeasure();
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
    window.addEventListener('pointerdown', closeWorldQuickPanelWhenOutside, true);
    document.addEventListener('scroll', closeFileContainerMenu, true);
    audioRecordingSupportHandler = () => {
      refreshAudioRecordingSupport();
    };
    window.addEventListener('focus', audioRecordingSupportHandler);
    window.addEventListener('pageshow', audioRecordingSupportHandler);
    document.addEventListener('visibilitychange', audioRecordingSupportHandler);
    refreshAudioRecordingSupport();
    if (audioRecordingSupportRetryTimer !== null) {
      window.clearTimeout(audioRecordingSupportRetryTimer);
    }
    audioRecordingSupportRetryTimer = window.setTimeout(() => {
      refreshAudioRecordingSupport();
      audioRecordingSupportRetryTimer = null;
    }, 1200);
  }
  initDesktopLaunchBehavior();
  applyUiFontSize(uiFontSize.value);
  await bootstrap();
  refreshAudioRecordingSupport();
  updateMessageScrollState();
  syncMessageVirtualMetrics();
  scheduleMessageVirtualMeasure();
  scheduleWorkspaceResourceHydration();
  lifecycleTimer = window.setInterval(() => {
    fileLifecycleNowTick.value = Date.now();
  }, 60_000);
  statusTimer = window.setInterval(() => {
    loadRunningAgents();
    if (!cronPermissionDenied.value) {
      loadCronAgentIds();
    }
    loadChannelBoundAgentIds();
    if (!userWorldPermissionDenied.value) {
      userWorldStore.refreshContacts().catch(() => {});
    }
  }, 12000);
});

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') {
    if (viewportResizeHandler) {
      window.removeEventListener('resize', viewportResizeHandler);
      viewportResizeHandler = null;
    }
    window.removeEventListener('pointerdown', closeWorldQuickPanelWhenOutside, true);
    document.removeEventListener('scroll', closeFileContainerMenu, true);
    if (audioRecordingSupportHandler) {
      window.removeEventListener('focus', audioRecordingSupportHandler);
      window.removeEventListener('pageshow', audioRecordingSupportHandler);
      document.removeEventListener('visibilitychange', audioRecordingSupportHandler);
      audioRecordingSupportHandler = null;
    }
    if (audioRecordingSupportRetryTimer !== null) {
      window.clearTimeout(audioRecordingSupportRetryTimer);
      audioRecordingSupportRetryTimer = null;
    }
  }
  closeFileContainerMenu();
  clearWorldQuickPanelClose();
  clearMiddlePaneOverlayHide();
  clearKeywordDebounce();
  closeImagePreview();
  stopWorldComposerResize();
  void cancelAgentVoiceRecording();
  void cancelWorldVoiceRecording();
  disposeWorldVoicePlayback();
  if (typeof window !== 'undefined' && messageScrollFrame !== null) {
    window.cancelAnimationFrame(messageScrollFrame);
    messageScrollFrame = null;
  }
  if (typeof window !== 'undefined' && messageVirtualMeasureFrame !== null) {
    window.cancelAnimationFrame(messageVirtualMeasureFrame);
    messageVirtualMeasureFrame = null;
  }
  if (typeof window !== 'undefined' && contactVirtualFrame !== null) {
    window.cancelAnimationFrame(contactVirtualFrame);
    contactVirtualFrame = null;
  }
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
  messageVirtualHeightCache.clear();
  clearWorkspaceResourceCache();
  timelinePreviewMap.value.clear();
  timelinePreviewLoadingSet.value.clear();
  userWorldStore.stopAllWatchers();
});
</script>
