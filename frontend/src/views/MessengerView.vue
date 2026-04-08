<template>
  <div
    ref="messengerRootRef"
    class="messenger-view"
    :class="{
      'messenger-view--without-right': !showRightDock,
      'messenger-view--without-middle': !showMiddlePane,
      'messenger-view--right-collapsed': showRightDock && rightDockCollapsed,
      'messenger-view--nav-collapsed': navigationPaneCollapsed,
      'messenger-view--host-medium': isRightDockOverlay,
      'messenger-view--host-small': isMiddlePaneOverlay,
      'messenger-view--host-tight': viewportWidth <= MESSENGER_TIGHT_HOST_BREAKPOINT,
      'messenger-view--action-blocked': isMessengerInteractionBlocked
    }"
    @pointerenter="handleMessengerRootPointerMove"
    @pointermove="handleMessengerRootPointerMove"
    @pointerleave="handleMessengerRootPointerLeave"
    @mouseenter="handleMessengerRootPointerMove"
    @mousemove="handleMessengerRootPointerMove"
    @mouseleave="handleMessengerRootPointerLeave"
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
            @mouseenter="queuePreviewMiddlePaneSection(item.key)"
            @focus="previewMiddlePaneSection(item.key)"
            @click="switchSection(item.key)"
          >
            <i :class="item.icon" aria-hidden="true"></i>
          </button>
        </div>
      </div>
      <div class="messenger-left-more-wrap" :class="{ 'is-open': leftRailMoreExpanded }">
        <div class="messenger-left-more-panel" :class="{ 'is-open': leftRailMoreExpanded }">
          <button
            v-for="item in leftRailSocialSectionOptions"
            :key="`more-${item.key}`"
            class="messenger-left-nav-btn messenger-left-nav-btn--more-item"
            :class="{ active: isLeftNavSectionActive(item.key) }"
            type="button"
            :title="item.label"
            :aria-label="item.label"
            :tabindex="leftRailMoreExpanded ? 0 : -1"
            @mouseenter="queuePreviewMiddlePaneSection(item.key)"
            @focus="previewMiddlePaneSection(item.key)"
            @click="openMoreRailSection(item.key)"
          >
            <i :class="item.icon" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-left-nav-btn messenger-left-nav-btn--helper messenger-left-nav-btn--more-item"
            :class="{ active: isHelperAppsMiddlePaneActive && !leftRailMoreExpanded }"
            type="button"
            :title="t('userWorld.helperApps.title')"
            :aria-label="t('userWorld.helperApps.title')"
            :tabindex="leftRailMoreExpanded ? 0 : -1"
            @mouseenter="queuePreviewMiddlePaneSection('groups', { helperWorkspace: true })"
            @focus="previewMiddlePaneSection('groups', { helperWorkspace: true })"
            @click="openHelperAppsDialog"
          >
            <i class="fa-solid fa-toolbox" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-left-nav-btn messenger-left-nav-btn--more-item"
            :class="{ active: isLeftNavSectionActive('more') }"
            type="button"
            :title="t('messenger.section.settings')"
            :aria-label="t('messenger.section.settings')"
            :tabindex="leftRailMoreExpanded ? 0 : -1"
            @mouseenter="queuePreviewMiddlePaneSection('more')"
            @focus="previewMiddlePaneSection('more')"
            @click="openSettingsPage"
          >
            <i class="fa-solid fa-gear" aria-hidden="true"></i>
          </button>
        </div>
        <button
          class="messenger-left-nav-btn messenger-left-nav-btn--more-toggle"
          :class="{ active: isLeftRailMoreActive }"
          type="button"
          :title="leftRailMoreToggleTitle"
          :aria-label="leftRailMoreToggleTitle"
          :aria-expanded="leftRailMoreExpanded ? 'true' : 'false'"
          @click="toggleLeftRailMoreMenu"
        >
          <i :class="leftRailMoreExpanded ? 'fa-solid fa-xmark' : 'fa-solid fa-ellipsis'" aria-hidden="true"></i>
        </button>
      </div>
    </aside>

    <section
      v-if="middlePaneMounted && !isEmbeddedChatRoute"
      v-show="showMiddlePane"
      ref="middlePaneRef"
      class="messenger-middle-pane messenger-middle-pane--overlay"
      @mouseenter="cancelMiddlePaneOverlayHide"
      @mouseleave="scheduleMiddlePaneOverlayHide"
    >
      <MessengerMiddlePane
          v-model:keyword="keywordInput"
          v-model:selected-contact-unit-id="selectedContactUnitId"
          v-model:selected-agent-hive-group-id="selectedAgentHiveGroupId"
          v-model:settings-panel-mode="settingsPanelMode"
          :active-section="middlePaneActiveSection"
          :active-section-title="middlePaneActiveSectionTitle"
          :active-section-subtitle="middlePaneActiveSectionSubtitle"
          :show-helper-apps-workspace="showMiddlePaneHelperAppsWorkspace"
          :search-placeholder="middlePaneSearchPlaceholder"
          :agent-overview-mode="agentOverviewMode"
          :user-world-permission-denied="userWorldPermissionDenied"
          :handle-search-create-action="handleSearchCreateAction"
          :handle-agent-batch-export="handleAgentBatchExport"
          :handle-agent-batch-delete="handleAgentBatchDelete"
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
          :preload-mixed-conversation="preloadMixedConversation"
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
          @delete-beeroom-group="handleDeleteBeeroomGroup"
          :filtered-groups="filteredGroups"
          :selected-group-id="selectedGroupId"
          :select-group="selectGroup"
          :agent-hive-total-count="agentHiveTotalCount"
          :agent-hive-tree-rows="agentHiveTreeRows"
          :filtered-owned-agents="filteredOwnedAgents"
          :filtered-shared-agents="filteredSharedAgents"
          :show-default-agent-entry="showDefaultAgentEntry"
          :selected-agent-id="selectedAgentId"
          :default-agent-key="DEFAULT_AGENT_KEY"
          :default-agent-icon="defaultAgentProfile?.icon"
          :select-agent-for-settings="selectAgentForSettings"
          :open-agent-by-id="openAgentById"
          :preload-agent-by-id="preloadAgentById"
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
          @activate-settings-panel="activateSettingsPanel"
      />
    </section>

    <div
      v-if="showNavigationCollapseToggle"
      class="messenger-nav-toggle-hitbox"
      aria-hidden="true"
    ></div>
    <button
      v-if="showNavigationCollapseToggle"
      class="messenger-nav-toggle"
      type="button"
      :title="navigationPaneToggleTitle"
      :aria-label="navigationPaneToggleTitle"
      :aria-expanded="navigationPaneCollapsed ? 'false' : 'true'"
      @click="toggleNavigationPaneCollapsed"
    >
      <i
        class="fa-solid"
        :class="navigationPaneCollapsed ? 'fa-chevron-right' : 'fa-chevron-left'"
        aria-hidden="true"
      ></i>
    </button>

    <section class="messenger-chat chat-shell">
      <header
        v-if="
          sessionHub.activeSection !== 'swarms' &&
          !(sessionHub.activeSection === 'more' && settingsPanelMode === 'help-manual')
        "
        class="messenger-chat-header"
      >
        <div class="messenger-chat-heading">
          <div class="messenger-chat-title-row">
            <div class="messenger-chat-title">{{ chatPanelTitle }}</div>
          </div>
          <div v-if="chatPanelSubtitle" class="messenger-chat-subtitle">{{ chatPanelSubtitle }}</div>
        </div>
        <div class="messenger-chat-header-actions">
          <button
            v-if="
              showChatSettingsView &&
              sessionHub.activeSection === 'agents' &&
              !showAgentGridOverview &&
              agentSettingMode === 'agent' &&
              !isSettingsDefaultAgentReadonly
            "
            class="messenger-header-action-text messenger-header-action-text--danger"
            type="button"
            @click="triggerAgentSettingsDelete"
          >
            <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
            <span>{{ t('portal.agent.delete') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'agents' && !showAgentGridOverview && agentSettingMode === 'agent'"
            class="messenger-header-action-text"
            type="button"
            @click="triggerAgentSettingsReload"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ t('common.refresh') }}</span>
          </button>
          <button
            v-if="
              showChatSettingsView &&
              sessionHub.activeSection === 'agents' &&
              !showAgentGridOverview &&
              agentSettingMode === 'agent' &&
              !isSettingsDefaultAgentReadonly
            "
            class="messenger-header-action-text"
            type="button"
            @click="triggerAgentSettingsSave"
          >
            <i class="fa-solid fa-floppy-disk" aria-hidden="true"></i>
            <span>{{ t('portal.agent.save') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'agents' && !showAgentGridOverview && agentSettingMode === 'agent'"
            class="messenger-header-action-text"
            type="button"
            @click="triggerAgentSettingsExport"
          >
            <i class="fa-solid fa-file-export" aria-hidden="true"></i>
            <span>{{ t('common.export') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'agents' && !showAgentGridOverview"
            class="messenger-header-action-text"
            type="button"
            @click="enterSelectedAgentConversation"
          >
            <i class="fa-solid fa-comments" aria-hidden="true"></i>
            <span>{{ t('messenger.action.openConversation') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'users' && selectedContact"
            class="messenger-header-action-text"
            type="button"
            @click="openSelectedContactConversation"
          >
            <i class="fa-solid fa-comments" aria-hidden="true"></i>
            <span>{{ t('messenger.action.openConversation') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'groups' && selectedGroup && !showHelperAppsWorkspace"
            class="messenger-header-action-text"
            type="button"
            @click="openSelectedGroupConversation"
          >
            <i class="fa-solid fa-comments" aria-hidden="true"></i>
            <span>{{ t('messenger.action.openConversation') }}</span>
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn messenger-header-btn--text"
            type="button"
            :disabled="creatingAgentSession || isMessengerInteractionBlocked"
            :title="t('chat.newSession')"
            :aria-label="t('chat.newSession')"
            @click="startNewSession"
          >
            <i class="fa-solid fa-plus" aria-hidden="true"></i>
            {{ t('chat.newSession') }}
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn"
            type="button"
            :title="t('chat.history')"
            :aria-label="t('chat.history')"
            @click="timelineDialogVisible = true"
          >
            <i class="fa-solid fa-clock-rotate-left" aria-hidden="true"></i>
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn"
            type="button"
            :disabled="isMessengerInteractionBlocked"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            @click="handleChatPageRefresh"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
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
        <button
          v-if="showScrollTopButton"
          class="messenger-scroll-top-btn"
          type="button"
          :title="t('chat.toTop')"
          :aria-label="t('chat.toTop')"
          @click="jumpToMessageTop"
        >
          <i class="fa-solid fa-angles-up" aria-hidden="true"></i>
        </button>
      </header>

      <div
        ref="messageListRef"
        class="messenger-chat-body"
        :class="{
          'is-settings': showChatSettingsView && !showHelperAppsWorkspace,
          'is-messages': !showChatSettingsView && !showHelperAppsWorkspace,
          'is-helper-workspace': showHelperAppsWorkspace,
          'is-beeroom': sessionHub.activeSection === 'swarms',
          'is-beeroom-canvas': sessionHub.activeSection === 'swarms',
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
              <GlobeAppPanel
                v-else-if="helperAppsActiveKind === 'offline' && helperAppsActiveKey === 'globe'"
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

        <template v-else-if="sessionHub.activeSection === 'swarms'">
          <div
            class="messenger-chat-settings messenger-chat-settings--beeroom messenger-chat-settings--beeroom-canvas"
          >
            <BeeroomWorkbench
              :group="selectedBeeroomGroup"
              :agents="beeroomStore.activeAgents"
              :missions="beeroomStore.activeMissions"
              :available-agents="beeroomCandidateAgents"
              :loading="beeroomStore.detailLoading || beeroomStore.loading"
              :refreshing="beeroomStore.refreshing"
              :error="beeroomStore.error"
              @refresh="refreshActiveBeeroom"
              @move-agents="handleBeeroomMoveAgents"
              @open-agent="openAgentById"
            />
          </div>
        </template>

        <template v-else-if="showChatSettingsView">
          <div
            :key="settingsPanelRenderKey"
            class="messenger-chat-settings"
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
                        <AgentAvatar
                          size="md"
                          :state="card.runtimeState"
                          :icon="card.icon"
                          :name="card.name"
                        />
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
                <div class="messenger-inline-actions messenger-inline-actions--agent-settings">
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
                    :class="{ active: agentSettingMode === 'memory' }"
                    type="button"
                    @click="agentSettingMode = 'memory'"
                  >
                    {{ t('messenger.memory.button') }}
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
                  <button
                    class="messenger-inline-btn"
                    :class="{ active: agentSettingMode === 'archived' }"
                    type="button"
                    @click="agentSettingMode = 'archived'"
                  >
                    {{ t('chat.history.archivedButton') }}
                  </button>
                </div>

                <div
                  v-if="mountedAgentSettingModes.agent"
                  v-show="agentSettingMode === 'agent'"
                  class="messenger-chat-settings-block"
                >
                  <AgentSettingsPanel
                    ref="agentSettingsPanelRef"
                    :agent-id="settingsAgentIdForPanel"
                    :readonly="isSettingsDefaultAgentReadonly"
                    :focus-target="agentSettingsFocusTarget"
                    :focus-token="agentSettingsFocusToken"
                    @delete-start="handleAgentDeleteStart"
                    @saved="handleAgentSettingsSaved"
                    @deleted="handleAgentDeleted"
                    @focus-consumed="handleAgentSettingsFocusConsumed"
                  />
                </div>

                <div
                  v-if="mountedAgentSettingModes.cron"
                  v-show="agentSettingMode === 'cron'"
                  class="messenger-chat-settings-block"
                >
                  <AgentCronPanel
                    :agent-id="settingsAgentIdForApi"
                    :active="agentSettingMode === 'cron'"
                    @changed="handleCronPanelChanged"
                  />
                </div>

                <div
                  v-if="mountedAgentSettingModes.channel"
                  v-show="agentSettingMode === 'channel'"
                  class="messenger-chat-settings-block messenger-channel-panel-wrap"
                >
                  <UserChannelSettingsPanel
                    mode="page"
                    :agent-id="settingsAgentIdForApi"
                    :active="agentSettingMode === 'channel'"
                    @changed="() => loadChannelBoundAgentIds({ force: true })"
                  />
                </div>
                <div
                  v-if="mountedAgentSettingModes.runtime"
                  v-show="agentSettingMode === 'runtime'"
                  class="messenger-chat-settings-block"
                >
                  <AgentRuntimeRecordsPanel
                    :agent-id="settingsRuntimeAgentIdForApi"
                    :active="agentSettingMode === 'runtime'"
                  />
                </div>
                <div
                  v-if="mountedAgentSettingModes.memory"
                  v-show="agentSettingMode === 'memory'"
                  class="messenger-chat-settings-block"
                >
                  <AgentMemoryPanel
                    :agent-id="settingsAgentIdForApi"
                    :active="agentSettingMode === 'memory'"
                  />
                </div>
                <div
                  v-if="mountedAgentSettingModes.archived"
                  v-show="agentSettingMode === 'archived'"
                  class="messenger-chat-settings-block"
                >
                  <ArchivedThreadManager
                    :agent-id="settingsAgentIdForApi"
                    :active="agentSettingMode === 'archived'"
                    @open-session-detail="openTimelineSessionDetail"
                    @session-deleted="handleArchivedSessionRemoved"
                  />
                </div>

              </template>
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

            <MessengerToolsSection
              v-else-if="sessionHub.activeSection === 'tools'"
              :tools-catalog-loading="toolsCatalogLoading"
              :selected-tool-category="selectedToolCategory"
              :admin-tool-groups="adminToolGroups"
              :resolve-admin-tool-detail="resolveAdminToolDetail"
            />

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
                    <div v-if="!desktopLocalMode" class="messenger-entity-field">
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
                    :empty-text="resolveFileWorkspaceEmptyText({ fileScope, t })"
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
              <div
                v-else-if="settingsPanelMode === 'help-manual'"
                class="messenger-chat-settings-block messenger-chat-settings-block--manual messenger-chat-settings-block--overlay-host"
              >
                <MessengerHelpManualPanel @loading-change="handleHelpManualLoadingChange" />
                <HoneycombWaitingOverlay
                  :visible="showHelpManualWaitingOverlay"
                  :title="t('messenger.waiting.title')"
                  :target-name="t('messenger.settings.helpManual')"
                  :phase-label="t('messenger.waiting.phase.loading')"
                  :summary-label="t('messenger.waiting.summary.helpManual')"
                  :progress="42"
                  :teleport-to-body="false"
                />
              </div>
              <div
                v-else-if="
                  desktopMode &&
                  (settingsPanelMode === 'desktop-models' ||
                    settingsPanelMode === 'desktop-remote' ||
                    settingsPanelMode === 'desktop-lan')
                "
                class="messenger-chat-settings-scroll messenger-chat-settings-scroll--desktop-system"
              >
                <DesktopSystemSettingsPanel
                  :panel="
                    settingsPanelMode === 'desktop-remote'
                      ? 'remote'
                      : settingsPanelMode === 'desktop-lan'
                        ? 'lan'
                        : 'models'
                  "
                  @desktop-model-meta-changed="handleDesktopModelMetaChanged"
                />
              </div>
              <div v-else class="messenger-chat-settings-scroll">
                <MessengerSettingsPanel
                  :mode="generalSettingsPanelMode"
                  :username="currentUsername"
                  :user-id="currentUserId"
                  :language-label="currentLanguageLabel"
                  :send-key="messengerSendKey"
                  :desktop-local-mode="desktopLocalMode"
                  :theme-palette="themeStore.palette"
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
                  @update:ui-font-size="updateUiFontSize"
                  @update:username="updateCurrentUsername"
                  @update:profile-avatar-icon="updateCurrentUserAvatarIcon"
                  @update:profile-avatar-color="updateCurrentUserAvatarColor"
                />
              </div>
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
          </div>

          <template v-else-if="isAgentConversationActive">
            <template v-for="item in agentRenderableMessages" :key="item.key">
            <div
              v-if="
                !isHiddenInternalMessage(item.message)
                  && (!isCompactionMarkerMessage(item.message) || shouldShowCompactionDivider(item.message))
              "
              class="messenger-message"
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
                  />
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
                />
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
                    :key="buildMessageWorkflowRenderKey(item.message, item.key)"
                    :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                    :loading="Boolean(item.message.workflowStreaming)"
                    :visible="
                      Boolean(
                        item.message.workflowStreaming ||
                          (Array.isArray(item.message.workflowItems) && item.message.workflowItems.length > 0)
                      )
                    "
                    @layout-change="handleMessageWorkflowLayoutChange(item.key)"
                  />
                  <MessageSubagentPanel
                    :session-id="chatStore.activeSessionId"
                    :items="Array.isArray(item.message.subagents) ? item.message.subagents : []"
                  />
                </div>
                <div
                  v-if="item.message.role === 'user' || shouldShowAgentMessageBubble(item.message)"
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
                <MessageKnowledgeCitation
                  v-if="item.message.role === 'assistant'"
                  :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                />
                <div
                  v-if="hasMessageContent(item.message.content) || shouldShowMessageStats(item.message)"
                  class="messenger-message-extra"
                >
                  <div v-if="shouldShowMessageStats(item.message)" class="messenger-message-stats">
                    <span
                      v-for="entry in buildMessageStatsEntries(item.message)"
                      :key="entry.key"
                      :class="[
                        'messenger-message-stat',
                        entry.kind === 'status' ? 'is-status' : 'is-metric',
                        entry.tone ? `is-${entry.tone}` : '',
                        entry.live ? 'is-live' : ''
                      ]"
                    >
                      <template v-if="entry.kind === 'status'">
                        <span class="messenger-message-stat-dot" aria-hidden="true"></span>
                        <span class="messenger-message-stat-value">{{ entry.value }}</span>
                      </template>
                      <template v-else>
                        <span class="messenger-message-stat-label">{{ entry.label }}:</span>
                        <span class="messenger-message-stat-value">{{ entry.value }}</span>
                      </template>
                    </span>
                  </div>
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

          <template v-else-if="isWorldConversationActive">
            <div
              v-for="item in worldRenderableMessages"
              :key="item.key"
              class="messenger-message"
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
            :preset-questions="activeAgentPresetQuestions"
            :voice-supported="agentVoiceSupported"
            :voice-recording="agentVoiceRecording"
            :voice-duration-ms="agentVoiceDurationMs"
            :show-approval-label="showAgentComposerApprovalHint"
            :approval-label="agentComposerApprovalHintLabel"
            :approval-mode="composerApprovalMode"
            :approval-mode-options="agentComposerApprovalModeOptions"
            :approval-mode-editable="showAgentComposerApprovalSelector"
            :approval-mode-syncing="composerApprovalModeSyncing"
            :model-name="agentHeaderModelDisplayName"
            :model-jump-enabled="agentHeaderModelJumpEnabled"
            :model-jump-hint="t('messenger.agent.openSettings')"
            @send="sendAgentMessage"
            @stop="stopAgentMessage"
            @toggle-voice-record="toggleAgentVoiceRecord"
            @update:approval-mode="updateComposerApprovalMode"
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
      :edge-active="rightDockEdgeHover"
      :show-agent-panels="showRightAgentPanels"
      :agent-id-for-api="rightPanelAgentIdForApi"
      :container-id="rightPanelContainerId"
      :skills-loading="rightDockSkillsLoading"
      :skills-uploading="skillDockUploading"
      :enabled-skills="rightDockEnabledSkills"
      :disabled-skills="rightDockDisabledSkills"
      @toggle-collapse="rightDockCollapsed = !rightDockCollapsed"
      @upload-skill-archive="handleRightDockSkillArchiveUpload"
      @open-skill-detail="openRightDockSkillDetail"
      @open-container="openContainerFromRightDock"
      @open-container-settings="openContainerSettingsFromRightDock"
    />
    <MessengerGroupDock
      ref="rightDockRef"
      v-else-if="showGroupRightDock"
      :collapsed="rightDockCollapsed"
      :edge-active="rightDockEdgeHover"
      :group-id="activeWorldGroupId"
      @toggle-collapse="rightDockCollapsed = !rightDockCollapsed"
    />

    <div
      v-if="isMessengerInteractionBlocked"
      class="messenger-action-blocker"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <div class="messenger-action-blocker-card">
        <span class="messenger-action-blocker-spinner" aria-hidden="true"></span>
        <div class="messenger-action-blocker-title">{{ messengerInteractionBlockingLabel }}</div>
        <div class="messenger-action-blocker-subtitle">{{ t('common.loading') }}</div>
      </div>
    </div>

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
      :active-agent-prompt-preview-html="activeAgentPromptPreviewHtml"
      :agent-prompt-preview-memory-mode="agentPromptPreviewMemoryMode"
      :agent-prompt-preview-tooling-mode="agentPromptPreviewToolingMode"
      :agent-prompt-preview-tooling-content="agentPromptPreviewToolingContent"
      :agent-prompt-preview-tooling-items="agentPromptPreviewToolingItems"
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
    <MessengerTimelineDialog
      v-model:visible="timelineDialogVisible"
      :active-session-id="String(chatStore.activeSessionId || '')"
      :session-history="rightPanelSessionHistory"
      @activate-session="handleTimelineDialogActivateSession"
      @open-session-detail="openTimelineSessionDetail"
      @archive-session="archiveTimelineSession"
      @rename-session="renameTimelineSession"
    />
    <el-dialog
      v-model="rightDockSkillDialogVisible"
      class="messenger-dialog messenger-skill-detail-dialog"
      :title="rightDockSkillDialogTitle"
      width="760px"
      :close-on-click-modal="false"
      append-to-body
      destroy-on-close
    >
      <div class="messenger-skill-detail-body">
        <div class="messenger-skill-detail-toolbar">
          <div class="messenger-skill-detail-path" :title="rightDockSkillDialogPath || undefined">
            {{ rightDockSkillDialogPath }}
          </div>
          <div class="messenger-skill-detail-toggle">
            <span>{{ t('common.enabled') }}</span>
            <el-switch
              :model-value="rightDockSelectedSkillEnabled"
              :disabled="rightDockSkillToggleSaving || !rightDockSelectedSkillName"
              :loading="rightDockSkillToggleSaving"
              @change="handleRightDockSkillEnabledToggle"
            />
          </div>
        </div>
        <div v-if="rightDockSkillContentLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
        <pre
          v-else-if="rightDockSkillContent"
          class="messenger-skill-detail-content"
        ><code>{{ rightDockSkillContent }}</code></pre>
        <div v-else class="messenger-list-empty">{{ t('chat.ability.noDesc') }}</div>
      </div>
    </el-dialog>
    <AgentQuickCreateDialog
      v-model="agentQuickCreateVisible"
      :creating="quickCreatingAgent"
      :copy-from-agents="agentQuickCreateCopyFromAgents"
      @submit="submitAgentQuickCreate"
    />
    <input
      ref="workerCardImportInputRef"
      type="file"
      accept=".json,application/json"
      style="display: none"
      @change="handleWorkerCardImportInput"
    />
    <HoneycombWaitingOverlay
      :visible="Boolean(messengerPageWaitingState)"
      :title="messengerPageWaitingState?.title || t('messenger.waiting.title')"
      :target-name="messengerPageWaitingState?.targetName || ''"
      :phase-label="messengerPageWaitingState?.phaseLabel || ''"
      :summary-label="messengerPageWaitingState?.summaryLabel || ''"
      :progress="messengerPageWaitingState?.progress ?? 0"
    />
    <WorkerCardImportWaitingOverlay
      :visible="workerCardImportOverlayVisible"
      :phase="workerCardImportOverlayPhase"
      :progress="workerCardImportOverlayProgress"
      :target-name="workerCardImportOverlayTargetName"
      :current="workerCardImportOverlayCurrent"
      :total="workerCardImportOverlayTotal"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus';

import { createAgent as createAgentApi, deleteAgent as deleteAgentApi, listAgentUserRounds, listRunningAgents } from '@/api/agents';
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
import {
  fetchUserSkillContent,
  uploadUserSkillZip
} from '@/api/userTools';
import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent, uploadWunderWorkspace } from '@/api/workspace';
import BeeroomWorkbench from '@/components/beeroom/BeeroomWorkbench.vue';
import AbilityTooltipListItem from '@/components/common/AbilityTooltipListItem.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import AgentQuickCreateDialog from '@/components/messenger/AgentQuickCreateDialog.vue';
import {
  scheduleMessengerBootstrapBackgroundTasks,
  settleMessengerBootstrapTasks,
  splitMessengerBootstrapTasks
} from '@/views/messenger/bootstrap';
import { resolveAgentSelectionAfterRemoval } from '@/views/messenger/agentSelection';
import { createBeeroomRealtimeSync } from '@/views/messenger/beeroomRealtimeSync';
import { createMessageViewportRuntime, type MessageViewportRuntime } from '@/views/messenger/messageViewportRuntime';
import { useStableMixedConversationOrder } from '@/views/messenger/mixedConversationOrder';
import { createMessengerRealtimePulse } from '@/views/messenger/realtimePulse';
import { useMessengerHostWidth } from '@/views/messenger/hostWidth';
import { useMessengerInteractionBlocker } from '@/views/messenger/interactionBlocker';
import MessengerMiddlePane from '@/views/messenger/sections/MessengerMiddlePane.vue';
import MessengerDialogsHost from '@/views/messenger/sections/MessengerDialogsHost.vue';
import MessengerToolsSection from '@/views/messenger/sections/MessengerToolsSection.vue';
import { useMiddlePaneOverlayPreview } from '@/views/messenger/middlePaneOverlayPreview';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';
import {
  InquiryPanel,
  MessageCompactionDivider,
  MessageFeedbackActions,
  MessageKnowledgeCitation,
  MessageSubagentPanel,
  MessageThinking,
  PlanPanel,
  ToolApprovalComposer,
  WorkspacePanel
} from '@/views/messenger/lazyMessageBlocks';
import {
  MessengerFileContainerMenu,
  MessengerGroupDock,
  MessengerRightDock,
  MessengerTimelineDialog
} from '@/views/messenger/lazyShell';
import {
  AgentCronPanel,
  AgentMemoryPanel,
  AgentRuntimeRecordsPanel,
  AgentSettingsPanel,
  ArchivedThreadManager,
  DesktopContainerManagerPanel,
  DesktopSystemSettingsPanel,
  GlobeAppPanel,
  MessengerHelpManualPanel,
  MessengerLocalFileSearchPanel,
  MessengerSettingsPanel,
  MessengerWorldComposer,
  preloadAgentSettingsPanels,
  preloadMessengerSettingsPanels,
  UserChannelSettingsPanel,
  UserPromptSettingsPanel
} from '@/views/messenger/lazyPanels';
import {
  resolveFileContainerLifecycleText,
  resolveFileWorkspaceEmptyText
} from '@/views/messenger/fileWorkspacePresentation';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { getRuntimeConfig } from '@/config/runtime';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useBeeroomStore, type BeeroomGroup } from '@/stores/beeroom';
import { useChatStore } from '@/stores/chat';
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
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { buildDeclaredDependencyPayload, resolveAgentDependencyStatus } from '@/utils/agentDependencyStatus';
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import WorkerCardImportWaitingOverlay from '@/components/agent/WorkerCardImportWaitingOverlay.vue';
import { downloadWorkerCardBundle, parseWorkerCardText, workerCardToAgentPayload } from '@/utils/workerCard';
import { redirectToLoginAfterLogout } from '@/utils/authNavigation';
import { copyText } from '@/utils/clipboard';
import { confirmWithFallback } from '@/utils/confirm';
import {
  buildAssistantDisplayContent,
  resolveAssistantFailureNotice
} from '@/utils/assistantFailureNotice';
import { hasRunningAssistantMessage } from '@/utils/chatSessionRuntime';
import { buildAssistantMessageStatsEntries } from '@/utils/messageStats';
import {
  isCompactionOnlyWorkflowItems,
  isCompactionRunningFromWorkflowItems,
  resolveLatestCompactionSnapshot
} from '@/utils/chatCompactionWorkflow';
import {
  isAudioRecordingSupported,
  startAudioRecording,
  type AudioRecordingResult,
  type AudioRecordingSession
} from '@/utils/audioRecorder';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import {
  extractPromptToolingPreview,
  type PromptToolingPreviewItem
} from '@/utils/promptToolingPreview';
import { collectAbilityDetails, collectAbilityGroupDetails, collectAbilityNames } from '@/utils/toolSummary';
import {
  buildWorkspaceImagePersistentCacheKey,
  readWorkspaceImagePersistentCache,
  writeWorkspaceImagePersistentCache
} from '@/utils/workspaceImagePersistentCache';
import {
  buildWorkspacePublicPath,
  normalizeWorkspaceOwnerId,
  resolveMarkdownWorkspacePath
} from '@/utils/messageWorkspacePath';
import {
  isImagePath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import { emitWorkspaceRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { emitUserToolsUpdated, onUserToolsUpdated } from '@/utils/userToolsEvents';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import {
  invalidateAllUserToolsCaches,
  invalidateUserSkillsCache,
  invalidateUserToolsCatalogCache,
  invalidateUserToolsSummaryCache,
  loadUserSkillsCache,
  loadUserToolsCatalogCache,
  loadUserToolsSummaryCache
} from '@/utils/userToolsCache';
import {
  normalizeAvatarColor,
  normalizeAvatarIcon,
  normalizeThemePalette,
  type ThemePalette,
  type UserAppearancePreferences
} from '@/utils/userPreferences';
import {
  PROFILE_AVATAR_COLORS,
  PROFILE_AVATAR_IMAGE_KEYS,
  PROFILE_AVATAR_IMAGE_MAP,
  PROFILE_AVATAR_OPTION_KEYS
} from '@/utils/avatarCatalog';
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
  buildAgentApprovalOptions,
  normalizeAgentApprovalMode,
  useComposerApprovalMode,
  type AgentApprovalMode
} from '@/views/messenger/composerApprovalMode';
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
const themeStore = useThemeStore();
const userWorldStore = useUserWorldStore();
const sessionHub = useSessionHubStore();

const DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY = 'messenger_desktop_first_launch_default_agent_hint_v1';

const bootLoading = ref(true);
const selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);
const deletingAgentSelectionSnapshot = ref<string[]>([]);
const selectedAgentHiveGroupId = ref('');
const agentOverviewMode = ref<'detail' | 'grid'>('detail');
const selectedContactUserId = ref('');
const selectedGroupId = ref('');
const agentQuickCreateVisible = ref(false);
const workerCardImportInputRef = ref<HTMLInputElement | null>(null);
const workerCardImporting = ref(false);
const workerCardImportOverlayVisible = ref(false);
const workerCardImportOverlayPhase = ref<'preparing' | 'creating' | 'refreshing'>('preparing');
const workerCardImportOverlayProgress = ref(0);
const workerCardImportOverlayTargetName = ref('');
const workerCardImportOverlayCurrent = ref(0);
const workerCardImportOverlayTotal = ref(0);
const selectedContactUnitId = ref('');
const selectedToolCategory = ref<'admin' | 'mcp' | 'skills' | 'knowledge' | ''>('');
const worldDraft = ref('');
const worldDraftMap = new Map<string, string>();
const dismissedAgentConversationMap = ref<Record<string, number>>({});
const dismissedAgentStorageKey = ref('');
const leftRailRef = ref<HTMLElement | null>(null);
const middlePaneRef = ref<HTMLElement | null>(null);
const rightDockRef = ref<{
  $el?: HTMLElement;
  refreshWorkspace?: (options?: { background?: boolean }) => Promise<boolean>;
} | null>(null);
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
const serverDefaultModelDisplayName = ref('');
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
const agentPromptPreviewMemoryMode = ref<'none' | 'pending' | 'frozen'>('none');
const agentPromptPreviewToolingMode = ref('');
const agentPromptPreviewToolingContent = ref('');
const agentPromptPreviewToolingItems = ref<PromptToolingPreviewItem[]>([]);
const agentPromptPreviewSelectedNames = ref<string[] | null>(null);
const AGENT_PROMPT_PREVIEW_CACHE_MS = 5000;
let agentPromptPreviewPayloadPromise: Promise<Record<string, unknown>> | null = null;
let agentPromptPreviewPayloadPromiseKey = '';
let agentPromptPreviewPayloadCache:
  | { key: string; payload: Record<string, unknown>; updatedAt: number }
  | null = null;
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const imagePreviewWorkspacePath = ref('');
const agentPromptToolSummary = ref<Record<string, unknown> | null>(null);
const agentToolSummaryLoading = ref(false);
const agentToolSummaryError = ref('');
let agentToolSummaryPromise: Promise<Record<string, unknown> | null> | null = null;
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
type AgentSettingMode = 'agent' | 'cron' | 'channel' | 'runtime' | 'memory' | 'archived';
const agentSettingMode = ref<AgentSettingMode>('agent');
const mountedAgentSettingModes = ref<Record<AgentSettingMode, boolean>>({
  agent: true,
  cron: false,
  channel: false,
  runtime: false,
  memory: false,
  archived: false
});
const agentSettingsFocusTarget = ref<'' | 'model'>('');
const agentSettingsFocusToken = ref(0);
type SettingsPanelMode =
  | 'general'
  | 'profile'
  | 'prompts'
  | 'help-manual'
  | 'desktop-models'
  | 'desktop-remote'
  | 'desktop-lan';

const settingsPanelMode = ref<SettingsPanelMode>('general');

function resolveRouteSettingsPanelMode(
  routePath: string,
  panelValue: unknown,
  desktopEnabled: boolean
): SettingsPanelMode {
  const path = String(routePath || '').trim().toLowerCase();
  const panelHint = String(panelValue || '').trim().toLowerCase();
  if (path.includes('/profile')) {
    return 'profile';
  }
  if (panelHint === 'profile') {
    return 'profile';
  }
  if (panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt') {
    return 'prompts';
  }
  if (
    panelHint === 'help-manual' ||
    panelHint === 'manual' ||
    panelHint === 'help' ||
    panelHint === 'docs' ||
    panelHint === 'docs-site'
  ) {
    return 'help-manual';
  }
  if (desktopEnabled && panelHint === 'desktop-models') {
    return 'desktop-models';
  }
  if (desktopEnabled && panelHint === 'desktop-lan') {
    return 'desktop-lan';
  }
  if (desktopEnabled && panelHint === 'desktop-remote') {
    return 'desktop-remote';
  }
  return 'general';
}

function resolveRouteHelperWorkspaceEnabled(
  sectionValue: unknown,
  helperValue: unknown
): boolean {
  const sectionHint = String(sectionValue || '').trim().toLowerCase();
  const helperHint = String(helperValue || '').trim().toLowerCase();
  return (
    sectionHint === 'groups' &&
    (helperHint === '1' || helperHint === 'true' || helperHint === 'yes')
  );
}

const rightDockCollapsed = ref(false);
const rightDockEdgeHover = ref(false);
const desktopInitialSectionPinned = ref(false);
const desktopShowFirstLaunchDefaultAgentHint = ref(false);
const desktopFirstLaunchDefaultAgentHintAt = ref(0);
const usernameSaving = ref(false);
const appearanceHydrating = ref(false);
const currentUserAvatarIcon = ref('initial');
const currentUserAvatarColor = ref('#3b82f6');
const helpManualLoading = ref(false);
const toolsCatalogLoading = ref(false);
const toolsCatalogLoaded = ref(false);
const builtinTools = ref<ToolEntry[]>([]);
const mcpTools = ref<ToolEntry[]>([]);
const skillTools = ref<ToolEntry[]>([]);
const knowledgeTools = ref<ToolEntry[]>([]);
const fileScope = ref<'agent' | 'user'>('agent');
const selectedFileContainerId = ref(USER_CONTAINER_ID);
const fileContainerLatestUpdatedAt = ref(0);
const fileContainerEntryCount = ref(0);
const fileLifecycleNowTick = ref(Date.now());
const fileContainerMenuViewRef = ref<{ getMenuElement: () => HTMLElement | null } | null>(null);
const desktopContainerManagerPanelRef = ref<{
  openManager: (containerId?: number) => Promise<void> | void;
} | null>(null);
const agentSettingsPanelRef = ref<{
  triggerReload: () => Promise<void> | void;
  triggerSave: () => Promise<void> | void;
  triggerDelete: () => Promise<void> | void;
  triggerExportWorkerCard: () => Promise<void> | void;
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
type RightDockSkillItem = {
  name: string;
  description: string;
  enabled: boolean;
};
type RightDockSkillCatalogItem = {
  name: string;
  description: string;
  path: string;
  source: string;
  builtin: boolean;
  readonly: boolean;
};
const rightDockSkillCatalog = ref<RightDockSkillCatalogItem[]>([]);
const rightDockSkillCatalogLoading = ref(false);
const rightDockSkillDialogVisible = ref(false);
const rightDockSelectedSkillName = ref('');
const rightDockSkillContentLoading = ref(false);
const rightDockSkillContent = ref('');
const rightDockSkillContentPath = ref('');
const rightDockSkillToggleSaving = ref(false);
const timelineDialogVisible = ref(false);
const timelineDetailDialogVisible = ref(false);
const timelineDetailSessionId = ref('');
const skillDockUploading = ref(false);
const approvalResponding = ref(false);
const messengerSendKey = ref<MessengerSendKeyMode>('enter');
const uiFontSize = ref(14);
const orgUnitPathMap = ref<Record<string, string>>({});
const orgUnitTree = ref<UnitTreeNode[]>([]);
const contactUnitExpandedIds = ref<Set<string>>(new Set());
const showScrollTopButton = ref(false);
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
const creatingAgentSession = ref(false);
const { hostRootRef: messengerRootRef, hostWidth: viewportWidth, refreshHostWidth } = useMessengerHostWidth();
const {
  isBlocked: isMessengerInteractionBlocked,
  label: messengerInteractionBlockingLabel,
  activeReason: messengerInteractionBlockReason,
  runWithBlock: runWithMessengerInteractionBlock
} = useMessengerInteractionBlocker({
  rootRef: messengerRootRef,
  resolveLabel: (reason) => (reason === 'new_session' ? t('chat.newSession') : t('common.refresh'))
});
const middlePaneOverlayVisible = ref(false);
const middlePaneMounted = ref(false);
const standardNavigationCollapsed = ref(false);
const leftRailMoreExpanded = ref(false);
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

let lifecycleTimer: number | null = null;
let worldQuickPanelCloseTimer: number | null = null;
let timelinePrefetchTimer: number | null = null;
let sessionDetailPrefetchTimer: number | null = null;
let middlePaneOverlayHideTimer: number | null = null;
let middlePanePrewarmTimer: number | null = null;
let keywordDebounceTimer: number | null = null;
let contactVirtualFrame: number | null = null;
let viewportResizeFrame: number | null = null;
let viewportResizeHandler: (() => void) | null = null;
let audioRecordingSupportHandler: (() => void) | null = null;
let audioRecordingSupportRetryTimer: number | null = null;
let startRealtimePulse: (() => void) | null = null;
let stopRealtimePulse: (() => void) | null = null;
let triggerRealtimePulseRefresh: ((reason?: string) => void) | null = null;
let startBeeroomRealtimeSync: (() => void) | null = null;
let stopBeeroomRealtimeSync: (() => void) | null = null;
let triggerBeeroomRealtimeSyncRefresh: ((reason?: string) => void) | null = null;
let messageViewportRuntime: MessageViewportRuntime | null = null;
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
let runningAgentsLoadVersion = 0;
let agentUserRoundsLoadVersion = 0;
let cronAgentIdsLoadVersion = 0;
let channelBoundAgentIdsLoadVersion = 0;
let runningAgentsLoadPromise: Promise<void> | null = null;
let runningAgentsLoadedAt = 0;
let cronAgentIdsLoadPromise: Promise<void> | null = null;
let cronAgentIdsLoadedAt = 0;
let channelBoundAgentIdsLoadPromise: Promise<void> | null = null;
let channelBoundAgentIdsLoadedAt = 0;
let toolsCatalogLoadVersion = 0;
let rightDockSkillCatalogLoadVersion = 0;
let rightDockSkillContentLoadVersion = 0;
let rightDockSkillAutoRetryTimer: number | null = null;
let desktopDefaultModelMetaFetchPromise: Promise<{
  hearingSupported: boolean;
  modelDisplayName: string;
}> | null = null;
let serverDefaultModelCheckedAt = 0;
let serverDefaultModelFetchPromise: Promise<string> | null = null;
let agentVoiceModelSupportCheckedAt = 0;
let beeroomGroupsLastRefreshAt = 0;
const agentUnreadRefreshInFlight = new Set<string>();
const MARKDOWN_CACHE_LIMIT = 280;
const MARKDOWN_STREAM_THROTTLE_MS = 80;
const CONTACT_VIRTUAL_ITEM_HEIGHT = 60;
const CONTACT_VIRTUAL_OVERSCAN = 8;
const MESSAGE_VIRTUAL_ESTIMATED_HEIGHT = 118;
const AGENT_VOICE_MODEL_SUPPORT_CACHE_MS = 30_000;
const SERVER_DEFAULT_MODEL_CACHE_MS = 30_000;
const AGENT_META_REQUEST_CACHE_MS = 1_500;
const SESSION_DETAIL_PREFETCH_DELAY_MS = 90;
const BEEROOM_GROUPS_REFRESH_MIN_MS_HOT = 2800;
const BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE = 7000;
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
const RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS = 1200;
const workspaceResourceCache = new Map<string, WorkspaceResourceCacheEntry>();
const userAttachmentResourceCache = ref(new Map<string, AttachmentResourceState>());
let workspaceResourceHydrationFrame: number | null = null;
let workspaceResourceHydrationPending = false;
let stopWorkspaceRefreshListener: (() => void) | null = null;
let stopUserToolsUpdatedListener: (() => void) | null = null;
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
    { key: 'swarms' as MessengerSection, icon: 'fa-solid fa-bee', label: t('messenger.section.swarms') },
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
  return isSectionButtonActive(section);
};

const closeLeftRailMoreMenu = () => {
  leftRailMoreExpanded.value = false;
  clearMiddlePaneOverlayPreview();
};

const toggleLeftRailMoreMenu = () => {
  clearMiddlePaneOverlayHide();
  clearMiddlePaneOverlayPreview();
  leftRailMoreExpanded.value = !leftRailMoreExpanded.value;
};

const basePrefix = computed(() => {
  if (route.path.startsWith('/desktop')) return '/desktop';
  if (route.path.startsWith('/demo')) return '/demo';
  return '/app';
});

const isEmbeddedChatRoute = computed(() => /\/embed\/chat$/.test(String(route.path || '').trim()));
const allowNavigationCollapse = computed(() => !isEmbeddedChatRoute.value);
const navigationPaneCollapsed = computed(() => {
  if (isEmbeddedChatRoute.value) {
    return true;
  }
  return standardNavigationCollapsed.value;
});
const navigationPaneToggleTitle = computed(() =>
  navigationPaneCollapsed.value ? t('common.expand') : t('common.collapse')
);

function setNavigationPaneCollapsed(collapsed: boolean): void {
  if (!allowNavigationCollapse.value) {
    standardNavigationCollapsed.value = false;
    return;
  }
  standardNavigationCollapsed.value = collapsed;
  if (collapsed) {
    leftRailMoreExpanded.value = false;
    clearMiddlePaneOverlayHide();
    clearMiddlePaneOverlayPreview();
    middlePaneOverlayVisible.value = false;
    return;
  }
  if (isMiddlePaneOverlay.value) {
    openMiddlePaneOverlay();
  }
}

function toggleNavigationPaneCollapsed(): void {
  setNavigationPaneCollapsed(!navigationPaneCollapsed.value);
}

function resolveChatShellPath(): string {
  return isEmbeddedChatRoute.value ? String(route.path || '').trim() : `${basePrefix.value}/chat`;
}

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
let currentUserContextInitialized = false;
const buildProfileAvatarOptionLabel = (key: string): string => {
  const match = String(key || '').trim().match(/^qq-avatar-(\d{4})$/);
  if (match) {
    return `QQ Avatar ${match[1]}`;
  }
  return `QQ Avatar ${String(key || '').trim()}`;
};
const profileAvatarOptions = computed(() =>
  settingsPanelMode.value === 'profile'
    ? [
        {
          key: 'initial',
          label: t('portal.agent.avatar.icon.initial')
        },
        ...PROFILE_AVATAR_IMAGE_KEYS.map((key) => ({
          key,
          label: buildProfileAvatarOptionLabel(key),
          image: PROFILE_AVATAR_IMAGE_MAP.get(key) || ''
        }))
      ]
    : []
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
  if (sessionHub.activeSection === 'messages') {
    return '';
  }
  return sessionHub.activeSection === 'more'
    ? t('messenger.section.settings.desc')
    : t(`messenger.section.${sessionHub.activeSection}.desc`);
});
const currentLanguageLabel = computed(() =>
  getCurrentLanguage() === 'zh-CN' ? t('language.zh-CN') : t('language.en-US')
);
const searchableMiddlePaneSections = new Set(['messages', 'users', 'groups', 'swarms', 'agents']);
const isSearchableMiddlePaneSection = (section: string): boolean =>
  searchableMiddlePaneSections.has(String(section || '').trim());
const searchPlaceholder = computed(() => t(`messenger.search.${sessionHub.activeSection}`));
const MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT = 1120;
const MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT = 1360;
const MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT = 1820;
const MESSENGER_TIGHT_HOST_BREAKPOINT = 900;
const isMiddlePaneOverlay = computed(() => viewportWidth.value <= MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT);
const isRightDockOverlay = computed(() => {
  const inAgentSettingsDetail =
    sessionHub.activeSection === 'agents' && agentOverviewMode.value === 'detail';
  const breakpoint = inAgentSettingsDetail
    ? MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT
    : MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT;
  return viewportWidth.value <= breakpoint;
});
const showMiddlePane = computed(() => {
  if (isEmbeddedChatRoute.value) {
    return false;
  }
  return !navigationPaneCollapsed.value && (!isMiddlePaneOverlay.value || middlePaneOverlayVisible.value);
});
const showNavigationCollapseToggle = computed(
  () => allowNavigationCollapse.value && (showMiddlePane.value || navigationPaneCollapsed.value)
);
const middlePaneTransitionName = computed(() => 'messenger-middle-pane-slide');

const scheduleMiddlePanePrewarm = () => {
  if (middlePaneMounted.value || isEmbeddedChatRoute.value || !isMiddlePaneOverlay.value) {
    return;
  }
  if (typeof window === 'undefined') {
    middlePaneMounted.value = true;
    return;
  }
  if (middlePanePrewarmTimer !== null) {
    return;
  }
  middlePanePrewarmTimer = window.setTimeout(() => {
    middlePanePrewarmTimer = null;
    if (isEmbeddedChatRoute.value) {
      return;
    }
    middlePaneMounted.value = true;
  }, 240);
};

const {
  clearMiddlePaneOverlayPreview,
  effectiveHelperAppsWorkspace: showMiddlePaneHelperAppsWorkspace,
  effectiveSearchPlaceholder: middlePaneSearchPlaceholder,
  effectiveSection: middlePaneActiveSection,
  effectiveSectionSubtitle: middlePaneActiveSectionSubtitle,
  effectiveSectionTitle: middlePaneActiveSectionTitle,
  isHelperWorkspaceButtonActive: isHelperAppsMiddlePaneActive,
  isSectionButtonActive,
  queuePreviewMiddlePaneSection,
  previewMiddlePaneSection
} = useMiddlePaneOverlayPreview({
  activeSection: computed(() => sessionHub.activeSection),
  helperAppsWorkspaceMode,
  isMiddlePaneOverlay,
  middlePaneOverlayVisible,
  t
});

const isLeftRailMoreActive = computed(
  () =>
    leftRailMoreExpanded.value ||
    isLeftNavSectionActive('users') ||
    isLeftNavSectionActive('groups') ||
    isLeftNavSectionActive('more') ||
    isHelperAppsMiddlePaneActive.value
);
const leftRailMoreToggleTitle = computed(() =>
  `${t('common.more')} · ${t(leftRailMoreExpanded.value ? 'common.collapse' : 'common.expand')}`
);

const DEFAULT_BEEROOM_GROUP_ID = 'default';

const ownedAgents = computed(() => (Array.isArray(agentStore.agents) ? agentStore.agents : []));
const sharedAgents = computed(() => (Array.isArray(agentStore.sharedAgents) ? agentStore.sharedAgents : []));

const normalizeAgentHiveGroupId = (value: unknown): string => {
  const normalized = String(value || '').trim();
  return normalized || DEFAULT_BEEROOM_GROUP_ID;
};

const defaultBeeroomGroupId = computed(() => {
  const defaultGroup = beeroomStore.groups.find((item) => item.is_default);
  const normalized = String(defaultGroup?.group_id || defaultGroup?.hive_id || '').trim();
  return normalized || DEFAULT_BEEROOM_GROUP_ID;
});

const resolveAgentHiveGroupId = (agent: unknown): string => {
  if (!agent || typeof agent !== 'object') {
    return defaultBeeroomGroupId.value;
  }
  const source = agent as Record<string, unknown>;
  return normalizeAgentHiveGroupId(source.hive_id || source.hiveId || defaultBeeroomGroupId.value);
};

const agentHiveLabelMap = computed(() => {
  const map = new Map<string, string>();
  map.set(defaultBeeroomGroupId.value, t('messenger.agentGroup.defaultOption'));
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).forEach((item) => {
    const hiveId = String(item?.group_id || item?.hive_id || '').trim();
    if (!hiveId) return;
    const label = String(
      item?.is_default ? t('messenger.agentGroup.defaultOption') : (item?.name || hiveId)
    ).trim();
    if (label) {
      map.set(hiveId, label);
    }
  });
  [...ownedAgents.value, ...sharedAgents.value].forEach((agent) => {
    const hiveId = resolveAgentHiveGroupId(agent);
    if (map.has(hiveId)) return;
    const source = agent as Record<string, unknown>;
    const label = String(
      hiveId === defaultBeeroomGroupId.value
        ? t('messenger.agentGroup.defaultOption')
        : (source.hive_name || source.hiveName || source.hive_id || source.hiveId || hiveId)
    ).trim();
    if (label) {
      map.set(hiveId, label);
    }
  });
  return map;
});

const agentHiveEntries = computed(() => {
  const entries: Array<{ agentId: string; hiveId: string }> = [
    {
      agentId: DEFAULT_AGENT_KEY,
      hiveId: defaultBeeroomGroupId.value
    }
  ];
  const seenAgentIds = new Set<string>([DEFAULT_AGENT_KEY]);
  [...ownedAgents.value, ...sharedAgents.value].forEach((agent) => {
    const agentId = normalizeAgentId(agent?.id);
    if (!agentId || seenAgentIds.has(agentId)) return;
    seenAgentIds.add(agentId);
    entries.push({
      agentId,
      hiveId: resolveAgentHiveGroupId(agent)
    });
  });
  return entries;
});

const agentHiveTotalCount = computed(() => agentHiveEntries.value.length);

const agentHiveTreeRows = computed(() => {
  const countMap = new Map<string, number>();
  agentHiveEntries.value.forEach((entry) => {
    countMap.set(entry.hiveId, (countMap.get(entry.hiveId) || 0) + 1);
  });
  return Array.from(countMap.entries())
    .filter(([, count]) => count > 0)
    .map(([id, count]) => ({
      id,
      label: agentHiveLabelMap.value.get(id) || id,
      count,
      depth: 0,
      expanded: false,
      hasChildren: false
    }))
    .sort((left, right) => {
      if (left.id === defaultBeeroomGroupId.value) return -1;
      if (right.id === defaultBeeroomGroupId.value) return 1;
      return String(left.label || left.id).localeCompare(String(right.label || right.id), 'zh-Hans-CN');
    });
});

const matchesAgentKeyword = (agent: unknown, text: string) => {
  const source = agent && typeof agent === 'object' ? (agent as Record<string, unknown>) : {};
  const id = String(source.id || '').toLowerCase();
  const name = String(source.name || '').toLowerCase();
  const desc = String(source.description || '').toLowerCase();
  const hiveId = String(resolveAgentHiveGroupId(source) || '').toLowerCase();
  const hiveLabel = String(
    agentHiveLabelMap.value.get(resolveAgentHiveGroupId(source)) || resolveAgentHiveGroupId(source)
  ).toLowerCase();
  return !text || id.includes(text) || name.includes(text) || desc.includes(text) || hiveId.includes(text) || hiveLabel.includes(text);
};

const matchesAgentHiveSelection = (agent: unknown) => {
  const selectedHiveId = String(selectedAgentHiveGroupId.value || '').trim();
  if (!selectedHiveId) return true;
  return resolveAgentHiveGroupId(agent) === normalizeAgentHiveGroupId(selectedHiveId);
};

const defaultAgentMatchesKeyword = computed(() =>
  matchesAgentKeyword(
    {
      id: DEFAULT_AGENT_KEY,
      name: t('messenger.defaultAgent'),
      description: t('messenger.defaultAgentDesc'),
      hive_id: defaultBeeroomGroupId.value
    },
    keyword.value.toLowerCase()
  )
);

const showDefaultAgentEntry = computed(
  () =>
    defaultAgentMatchesKeyword.value &&
    (!selectedAgentHiveGroupId.value ||
      normalizeAgentHiveGroupId(selectedAgentHiveGroupId.value) === defaultBeeroomGroupId.value)
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
const activeAgentDetailProfile = ref<Record<string, unknown> | null>(null);
const defaultAgentProfile = ref<Record<string, unknown> | null>(null);
const activeAgentIdForApi = computed(() =>
  activeAgentId.value === DEFAULT_AGENT_KEY ? '' : activeAgentId.value
);
const activeAgentPresetQuestions = computed(() => {
  if (activeAgentId.value === DEFAULT_AGENT_KEY) {
    return normalizeAgentPresetQuestions(defaultAgentProfile.value?.preset_questions);
  }
  return normalizeAgentPresetQuestions((activeAgent.value as Record<string, unknown> | null)?.preset_questions);
});
const activeAgentName = computed(() =>
  String(
    (activeAgent.value as Record<string, unknown> | null)?.name || t('messenger.defaultAgent')
  )
);
const activeAgentIcon = computed(() =>
  activeAgentId.value === DEFAULT_AGENT_KEY
    ? (defaultAgentProfile.value as Record<string, unknown> | null)?.icon
    : (activeAgent.value as Record<string, unknown> | null)?.icon
);
const activeAgentGreetingOverride = computed(() => {
  if (activeAgentId.value === DEFAULT_AGENT_KEY) {
    return String((defaultAgentProfile.value as Record<string, unknown> | null)?.description || '').trim();
  }
  const profile =
    (activeAgentDetailProfile.value as Record<string, unknown> | null) ||
    (activeAgent.value as Record<string, unknown> | null);
  return String(profile?.description || '').trim();
});

const resolveAgentIconForDisplay = (
  agentId: string,
  fallback: Record<string, unknown> | null = null
): unknown => {
  const normalized = normalizeAgentId(agentId);
  if (normalized === DEFAULT_AGENT_KEY) {
    return (defaultAgentProfile.value as Record<string, unknown> | null)?.icon ?? fallback?.icon;
  }
  return fallback?.icon;
};

const loadDefaultAgentProfile = async () => {
  defaultAgentProfile.value =
    ((await agentStore.getAgent(DEFAULT_AGENT_KEY, { force: true }).catch(() => null)) as Record<
      string,
      unknown
    > | null) || null;
};


async function readServerDefaultModelName(force = false): Promise<string> {
  if (desktopMode.value) {
    serverDefaultModelDisplayName.value = '';
    return '';
  }
  const now = Date.now();
  if (
    !force &&
    String(serverDefaultModelDisplayName.value || '').trim() &&
    now - serverDefaultModelCheckedAt <= SERVER_DEFAULT_MODEL_CACHE_MS
  ) {
    return String(serverDefaultModelDisplayName.value || '').trim();
  }
  if (serverDefaultModelFetchPromise) {
    return serverDefaultModelFetchPromise;
  }
  serverDefaultModelFetchPromise = (async () => {
    try {
      const profile =
        ((await agentStore.getAgent(DEFAULT_AGENT_KEY, { force }).catch(() => null)) as Record<
          string,
          unknown
        > | null) || null;
      if (profile) {
        defaultAgentProfile.value = profile;
      }
      const resolved = String(resolveModelNameFromRecord(profile) || '').trim();
      serverDefaultModelDisplayName.value = resolved;
      return resolved;
    } finally {
      serverDefaultModelCheckedAt = Date.now();
      serverDefaultModelFetchPromise = null;
    }
  })();
  return serverDefaultModelFetchPromise;
}

watch(
  () => activeAgentId.value,
  (value) => {
    if (value === DEFAULT_AGENT_KEY) {
      activeAgentDetailProfile.value = null;
      void loadDefaultAgentProfile();
      return;
    }
    const targetAgentId = normalizeAgentId(value);
    if (!targetAgentId) {
      activeAgentDetailProfile.value = null;
      return;
    }
    void agentStore
      .getAgent(targetAgentId, { force: true })
      .then((profile) => {
        if (normalizeAgentId(activeAgentId.value) !== targetAgentId) return;
        activeAgentDetailProfile.value =
          (profile as Record<string, unknown> | null) || null;
      })
      .catch(() => null);
  },
  { immediate: true }
);

watch(
  () => [chatStore.activeSessionId, activeAgentId.value, selectedAgentId.value, chatStore.draftAgentId] as const,
  () => {
    agentPromptPreviewPayloadCache = null;
    if (agentPromptPreviewPayloadPromise) {
      agentPromptPreviewPayloadPromiseKey = '';
    }
  }
);

watch(
  () => activeAgentGreetingOverride.value,
  (value, oldValue) => {
    if (value === oldValue) return;
    chatStore.setGreetingOverride(value);
  },
  { immediate: true }
);

watch([() => chatStore.activeSessionId, () => activeAgentId.value], () => {
  agentPromptPreviewSelectedNames.value = null;
});

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

const activeAgentProfileForModelResolution = computed(() =>
  activeAgentId.value === DEFAULT_AGENT_KEY ? defaultAgentProfile.value : activeAgent.value
);

const isDefaultModelSelectorValue = (value: unknown): boolean => {
  const lowered = String(value || '').trim().toLowerCase();
  return !lowered || lowered === 'default' || lowered === '__default__' || lowered === 'system';
};

const isSameModelName = (left: unknown, right: unknown): boolean => {
  const leftValue = String(left || '').trim();
  const rightValue = String(right || '').trim();
  if (!leftValue || !rightValue) return false;
  return leftValue.toLowerCase() === rightValue.toLowerCase();
};

const resolveExplicitAgentModelName = (profileValue: unknown): string => {
  const profile = asObjectRecord(profileValue);
  const configuredRaw = profile.configured_model_name ?? profile.configuredModelName;
  const configuredResolved = resolveModelNameFromRecord(configuredRaw);
  const configured = configuredResolved || String(configuredRaw || '').trim();
  if (!isDefaultModelSelectorValue(configured)) {
    return configured;
  }

  const fallback = resolveModelNameFromRecord(profile);
  if (isDefaultModelSelectorValue(fallback)) return '';
  // API fallback may contain effective default model_name when agent has no explicit model.
  if (desktopLocalMode.value && isSameModelName(fallback, desktopDefaultModelDisplayName.value)) {
    return '';
  }
  if (!desktopLocalMode.value && isSameModelName(fallback, serverDefaultModelDisplayName.value)) {
    return '';
  }
  return fallback;
};

const activeAgentDirectConfiguredModelName = computed(() => {
  if (!isAgentConversationActive.value) return '';
  return resolveExplicitAgentModelName(activeAgentProfileForModelResolution.value);
});

const activeAgentConfiguredModelName = computed(() => {
  if (!isAgentConversationActive.value) return '';
  const directModelName = activeAgentDirectConfiguredModelName.value;
  if (directModelName) return directModelName;
  if (desktopMode.value) {
    return String(desktopDefaultModelDisplayName.value || '').trim();
  }
  return String(serverDefaultModelDisplayName.value || '').trim();
});

const activeAgentUsingDesktopDefaultModel = computed(
  () =>
    desktopLocalMode.value &&
    isAgentConversationActive.value &&
    !String(activeAgentDirectConfiguredModelName.value || '').trim()
);

const agentHeaderModelDisplayName = computed(() => {
  if (!isAgentConversationActive.value) return '';
  const configuredModelName = activeAgentConfiguredModelName.value;
  // Keep composer label stable by preferring configured model alias over runtime model id.
  if (configuredModelName) return configuredModelName;
  const sessionModelName = activeAgentSessionModelName.value;
  if (sessionModelName) return sessionModelName;
  const runtimeModelName = activeAgentRuntimeModelName.value;
  if (runtimeModelName) return runtimeModelName;
  if (desktopMode.value && desktopLocalMode.value) {
    return t('desktop.system.modelUnnamed');
  }
  return t('common.unknown');
});

const agentHeaderModelJumpEnabled = computed(
  () => desktopMode.value || route.path.startsWith('/desktop')
);

const activeAgentApprovalMode = computed<AgentApprovalMode>(() => {
  if (activeAgentId.value === DEFAULT_AGENT_KEY) {
    return 'full_auto';
  }
  const agent = asObjectRecord(activeAgent.value);
  const agentMode = String(agent.approval_mode || agent.approvalMode || '').trim();
  if (agentMode) {
    return normalizeAgentApprovalMode(agentMode);
  }
  const session = asObjectRecord(activeAgentSession.value);
  const sessionMode = String(session.approval_mode || session.approvalMode || '').trim();
  if (sessionMode) {
    return normalizeAgentApprovalMode(sessionMode);
  }
  return 'full_auto';
});

const resolveCompactApprovalOptionLabel = (value: string): string => {
  const source = String(value || '').trim();
  if (!source) return '';
  const splitIndex = ['\uff08', '(']
    .map((marker) => source.indexOf(marker))
    .filter((index) => index > 0)
    .sort((left, right) => left - right)[0];
  return typeof splitIndex === 'number' ? source.slice(0, splitIndex).trim() : source;
};

const agentComposerApprovalModeOptions = computed(() =>
  buildAgentApprovalOptions((mode) => {
    const optionLabel = t(`portal.agent.permission.option.${mode}`);
    return resolveCompactApprovalOptionLabel(optionLabel) || optionLabel;
  })
);

const showAgentComposerApprovalSelector = computed(
  () => isAgentConversationActive.value
);

const resolveComposerApprovalPersistAgentId = () =>
  normalizeAgentId(activeAgentId.value || selectedAgentId.value || chatStore.draftAgentId) ||
  DEFAULT_AGENT_KEY;

const {
  composerApprovalMode,
  composerApprovalModeSyncing,
  updateComposerApprovalMode
} = useComposerApprovalMode({
  isAgentConversationActive,
  activeAgentId,
  activeAgentApprovalMode,
  resolvePersistAgentId: resolveComposerApprovalPersistAgentId,
  persistApprovalMode: async (agentId, mode) => {
    await agentStore.updateAgent(agentId, { approval_mode: mode });
    if (agentId === DEFAULT_AGENT_KEY) {
      await loadDefaultAgentProfile().catch(() => null);
    }
  },
  onPersistError: (error) => {
    showApiError(error, t('portal.agent.saveFailed'));
  }
});

const agentComposerApprovalHintMode = computed<AgentApprovalMode>(() =>
  showAgentComposerApprovalSelector.value ? composerApprovalMode.value : activeAgentApprovalMode.value
);

const agentComposerApprovalHintLabel = computed(() => {
  const optionLabel = t(`portal.agent.permission.option.${agentComposerApprovalHintMode.value}`);
  const compactOption = resolveCompactApprovalOptionLabel(optionLabel) || optionLabel;
  return `${t('portal.agent.permission.title')}: ${compactOption}`;
});

const showAgentComposerApprovalHint = computed(
  () => isAgentConversationActive.value
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

const buildSessionAgentMap = (): Map<string, string> => {
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
  return sessionAgentMap;
};

const pendingApprovalAgentIdSet = computed(() => {
  const approvals = Array.isArray(chatStore.pendingApprovals) ? chatStore.pendingApprovals : [];
  const result = new Set<string>();
  if (!approvals.length) {
    return result;
  }
  const sessionAgentMap = buildSessionAgentMap();
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

const isSessionBusy = (sessionId: unknown): boolean =>
  Boolean(chatStore.isSessionBusy?.(sessionId) || chatStore.isSessionLoading?.(sessionId));

const TERMINAL_RUNTIME_STATUS_SET = new Set(['idle', 'not_loaded', 'system_error']);

const resolveSessionRuntimeStatus = (sessionId: string): string =>
  String(chatStore.sessionRuntimeStatus?.(sessionId) || '')
    .trim()
    .toLowerCase();

const resolveSessionLoadingFlag = (sessionId: string): boolean => {
  const loadingBySession =
    (chatStore.loadingBySession && typeof chatStore.loadingBySession === 'object'
      ? chatStore.loadingBySession
      : {}) as Record<string, unknown>;
  return Boolean(loadingBySession[sessionId]);
};

const streamingAgentIdSet = computed(() => {
  const sessionAgentMap = buildSessionAgentMap();
  const loadingBySession =
    (chatStore.loadingBySession && typeof chatStore.loadingBySession === 'object'
      ? chatStore.loadingBySession
      : {}) as Record<string, unknown>;
  const sessionIds = new Set<string>([
    ...Array.from(sessionAgentMap.keys()),
    ...Object.keys(loadingBySession).map((id) => String(id || '').trim())
  ]);
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (activeSessionId) {
    sessionIds.add(activeSessionId);
  }
  const result = new Set<string>();
  sessionIds.forEach((sessionId) => {
    if (!sessionId || !isSessionBusy(sessionId)) return;
    const mappedAgentId = sessionAgentMap.get(sessionId);
    if (mappedAgentId) {
      result.add(mappedAgentId);
      return;
    }
    if (sessionId === activeSessionId) {
      const fallbackAgentId =
        normalizeAgentId(activeAgentId.value || selectedAgentId.value || chatStore.draftAgentId) ||
        DEFAULT_AGENT_KEY;
      result.add(fallbackAgentId);
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

const normalizeAbilityNameList = (values: unknown): string[] => {
  if (!Array.isArray(values)) return [];
  const output: string[] = [];
  const seen = new Set<string>();
  values.forEach((item) => {
    const name = String(item || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push(name);
  });
  return output;
};

const resolveSelectedAbilityNamesFromAgentProfile = (agent: Record<string, unknown> | null): string[] => {
  const source = agent || {};
  const abilities = source.abilities as Record<string, unknown> | null | undefined;
  const abilitySource = Array.isArray(source.ability_items)
    ? source.ability_items
    : Array.isArray(abilities?.items)
      ? abilities.items
      : [];
  const output: string[] = [];
  const seen = new Set<string>();
  abilitySource.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    const ability = item as Record<string, unknown>;
    if (ability.selected === false) return;
    const name = String(ability.runtime_name || ability.runtimeName || ability.name || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push(name);
  });
  return output;
};

const resolveAgentConfiguredAbilityNames = (agent: Record<string, unknown> | null): string[] => {
  const declared = normalizeAbilityNameList([
    ...normalizeAbilityNameList(agent?.declared_tool_names),
    ...normalizeAbilityNameList(agent?.declared_skill_names)
  ]);
  if (declared.length > 0) {
    return declared;
  }
  // tool_names is the persisted selection source; keep it ahead of legacy ability_items.
  const selectedFromToolNames = normalizeAbilityNameList([
    ...normalizeAbilityNameList(agent?.tool_names),
    ...normalizeAbilityNameList(agent?.toolNames)
  ]);
  if (selectedFromToolNames.length > 0) {
    return selectedFromToolNames;
  }
  const selectedFromItems = resolveSelectedAbilityNamesFromAgentProfile(agent);
  if (selectedFromItems.length > 0) {
    return selectedFromItems;
  }
  return [];
};

const extractPromptPreviewSelectedAbilityNames = (payload: unknown): string[] => {
  const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  const tooling =
    source.tooling_preview && typeof source.tooling_preview === 'object'
      ? (source.tooling_preview as Record<string, unknown>)
      : {};
  return normalizeAbilityNameList(tooling.selected_tool_names);
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
  const filterUnifiedItems = (list: unknown) =>
    Array.isArray(list)
      ? list.filter((item) => {
          if (!item || typeof item !== 'object') return false;
          const source = item as Record<string, unknown>;
          const name = String(
            source.runtime_name ||
              source.runtimeName ||
              source.name ||
              source.tool_name ||
              source.toolName ||
              source.id ||
              ''
          ).trim();
          return Boolean(name) && selectedNames.has(name);
        })
      : [];
  return {
    ...summary,
    builtin_tools: filterList(summary.builtin_tools),
    mcp_tools: filterList(summary.mcp_tools),
    a2a_tools: filterList(summary.a2a_tools),
    knowledge_tools: filterList(summary.knowledge_tools),
    user_tools: filterList(summary.user_tools),
    shared_tools: filterList(summary.shared_tools),
    skills: filterList(summary.skills),
    skill_list: filterList(summary.skill_list),
    skillList: filterList(summary.skillList),
    items: filterUnifiedItems(summary.items),
    itemList: filterUnifiedItems(summary.itemList)
  };
};

const effectiveAgentToolSummary = computed<Record<string, unknown> | null>(() => {
  const summary = agentPromptToolSummary.value;
  if (!summary) return null;
  const allowedSet = buildAbilityAllowedNameSet(summary);
  if (!allowedSet.size) return summary;
  if (agentPromptPreviewSelectedNames.value !== null) {
    const selectedNames = new Set<string>();
    agentPromptPreviewSelectedNames.value.forEach((item) => {
      const name = String(item || '').trim();
      if (name && allowedSet.has(name)) {
        selectedNames.add(name);
      }
    });
    return filterAbilitySummaryByNames(summary, selectedNames);
  }
  const activeAgentProfile =
    activeAgentId.value === DEFAULT_AGENT_KEY
      ? (defaultAgentProfile.value as Record<string, unknown> | null)
      : ((activeAgentDetailProfile.value as Record<string, unknown> | null) ||
          (activeAgent.value as Record<string, unknown> | null));
  const agentDefaults = normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile));
  const sourceOverrides = agentDefaults;
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
  return filterAbilitySummaryByNames(summary, selectedNames);
});
const activeAgentPromptPreviewHtml = computed(() =>
  renderSystemPromptHighlight(
    activeAgentPromptPreviewText.value,
    (effectiveAgentToolSummary.value || {}) as Record<string, unknown>
  )
);

const agentAbilitySections = computed(() => {
  const groups = collectAbilityGroupDetails(
    (effectiveAgentToolSummary.value || {}) as Record<string, unknown>
  );
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
const hasAgentAbilitySummary = computed(() =>
  agentAbilitySections.value.some((section) => section.items.length > 0)
);

function normalizeRightDockSkillRuntimeName(value: unknown): string {
  const normalized = String(value || '').trim();
  if (!normalized) return '';
  if (rightDockSkillCatalog.value.some((item) => item.name === normalized)) {
    return normalized;
  }
  const separatorIndex = normalized.indexOf('@');
  if (separatorIndex <= 0 || separatorIndex >= normalized.length - 1) {
    return normalized;
  }
  const legacyName = normalized.slice(separatorIndex + 1).trim();
  if (!legacyName) {
    return normalized;
  }
  return rightDockSkillCatalog.value.some((item) => item.name === legacyName)
    ? legacyName
    : normalized;
}

function normalizeRightDockSkillNameList(values: string[]): string[] {
  const output: string[] = [];
  const seen = new Set<string>();
  values.forEach((value) => {
    const normalized = normalizeRightDockSkillRuntimeName(value);
    if (!normalized || seen.has(normalized)) {
      return;
    }
    seen.add(normalized);
    output.push(normalized);
  });
  return output;
}

const normalizeRightDockSkillCatalog = (list: unknown): RightDockSkillCatalogItem[] => {
  if (!Array.isArray(list)) return [];
  const output: RightDockSkillCatalogItem[] = [];
  const seen = new Set<string>();
  list.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    const source = item as Record<string, unknown>;
    const name = String(source.name || source.tool_name || source.toolName || source.id || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push({
      name,
      description: String(source.description || source.desc || source.summary || '').trim(),
      path: String(source.path || '').trim(),
      source: String(source.source || '').trim().toLowerCase(),
      builtin: Boolean(source.builtin),
      readonly: Boolean(source.readonly)
    });
  });
  return output;
};

const normalizeRightDockSkillSummaryItems = (
  list: unknown
): Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> => {
  if (!Array.isArray(list)) return [];
  const output: Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> = [];
  const seen = new Set<string>();
  list.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    const source = item as Record<string, unknown>;
    const name = normalizeRightDockSkillRuntimeName(
      String(source.name || source.tool_name || source.toolName || source.id || '')
    );
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push({
      name,
      description: String(source.description || source.desc || source.summary || '').trim()
    });
  });
  return output;
};

const rightDockSkillEnabledNameSet = computed<Set<string>>(() => {
  const activeAgentProfile =
    activeAgentId.value === DEFAULT_AGENT_KEY
      ? (defaultAgentProfile.value as Record<string, unknown> | null)
      : ((activeAgentDetailProfile.value as Record<string, unknown> | null) ||
          (activeAgent.value as Record<string, unknown> | null));
  const selectedByProfile = normalizeRightDockSkillNameList(
    normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile))
  );
  return new Set(selectedByProfile);
});

const rightDockSkillItems = computed<RightDockSkillItem[]>(() => {
  const enabledSet = rightDockSkillEnabledNameSet.value;
  const merged = new Map<string, RightDockSkillItem>();

  rightDockSkillCatalog.value.forEach((item) => {
    merged.set(item.name, {
      name: item.name,
      description: item.description,
      enabled: enabledSet.has(item.name)
    });
  });

  const allSkills = collectAbilityDetails(
    (agentPromptToolSummary.value || {}) as Record<string, unknown>
  );
  normalizeRightDockSkillSummaryItems(allSkills.skills).forEach((item) => {
    const existing = merged.get(item.name);
    if (existing) {
      if (!existing.description && item.description) {
        existing.description = item.description;
      }
      return;
    }
    merged.set(item.name, {
      name: item.name,
      description: item.description,
      enabled: enabledSet.has(item.name)
    });
  });

  return Array.from(merged.values()).sort((left, right) =>
    left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' })
  );
});

const rightDockEnabledSkills = computed<RightDockSkillItem[]>(() =>
  rightDockSkillItems.value.filter((item) => item.enabled)
);

const rightDockDisabledSkills = computed<RightDockSkillItem[]>(() =>
  rightDockSkillItems.value.filter((item) => !item.enabled)
);
const rightDockSkillsLoading = computed(
  () => rightDockSkillCatalogLoading.value && rightDockSkillItems.value.length === 0
);
const rightDockSelectedSkill = computed<RightDockSkillCatalogItem | null>(() => {
  const name = String(rightDockSelectedSkillName.value || '').trim();
  if (!name) return null;
  return rightDockSkillCatalog.value.find((item) => item.name === name) || null;
});
const rightDockSkillDialogTitle = computed(() => {
  const name = String(rightDockSelectedSkillName.value || '').trim();
  return name ? `技能 skill · ${name}` : '技能 skill';
});
const rightDockSkillDialogPath = computed(() => {
  const path = String(rightDockSkillContentPath.value || rightDockSelectedSkill.value?.path || '').trim();
  return path || 'SKILL.md';
});
const rightDockSelectedSkillEnabled = computed(() => {
  const name = String(rightDockSelectedSkillName.value || '').trim();
  if (!name) return false;
  return rightDockSkillEnabledNameSet.value.has(name);
});

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
  if (!desktopMode.value) {
    return '';
  }
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

const showChatSettingsView = computed(() => sessionHub.activeSection !== 'messages');
const showHelperAppsWorkspace = computed(
  () => sessionHub.activeSection === 'groups' && helperAppsWorkspaceMode.value
);
const settingsPanelRenderKey = computed(() => ['settings', sessionHub.activeSection].join(':'));
const routeSectionIntent = computed<MessengerSection>(() => {
  if (desktopMode.value && desktopInitialSectionPinned.value) {
    return sessionHub.activeSection;
  }
  return resolveSectionFromRoute(route.path, route.query.section);
});
const routeSettingsPanelModeIntent = computed<SettingsPanelMode>(() =>
  resolveRouteSettingsPanelMode(route.path, route.query.panel, desktopMode.value)
);
const showHelpManualWaitingOverlay = computed(
  () =>
    sessionHub.activeSection === 'more' &&
    settingsPanelMode.value === 'help-manual' &&
    helpManualLoading.value
);
const suppressMessengerPageWaitingOverlay = computed(
  () =>
    (routeSectionIntent.value === 'agents' &&
      agentSettingMode.value === 'agent' &&
      !showAgentGridOverview.value) ||
    (routeSectionIntent.value === 'more' &&
      routeSettingsPanelModeIntent.value === 'help-manual') ||
    showHelpManualWaitingOverlay.value
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
  return ownedAgents.value.filter(
    (agent) => matchesAgentHiveSelection(agent) && matchesAgentKeyword(agent, text)
  );
});

watch(
  () => agentSettingMode.value,
  (mode) => {
    mountedAgentSettingModes.value[mode] = true;
  },
  { immediate: true }
);

const handleHelpManualLoadingChange = (value: boolean) => {
  helpManualLoading.value = value === true;
};

const filteredSharedAgents = computed(() => {
  const text = keyword.value.toLowerCase();
  return sharedAgents.value.filter(
    (agent) => matchesAgentHiveSelection(agent) && matchesAgentKeyword(agent, text)
  );
});

const visibleAgentIdsForSelection = computed(() => {
  const ids: string[] = [];
  if (showDefaultAgentEntry.value) {
    ids.push(DEFAULT_AGENT_KEY);
  }
  filteredOwnedAgents.value.forEach((agent) => {
    const agentId = normalizeAgentId(agent?.id);
    if (agentId && !ids.includes(agentId)) {
      ids.push(agentId);
    }
  });
  filteredSharedAgents.value.forEach((agent) => {
    const agentId = normalizeAgentId(agent?.id);
    if (agentId && !ids.includes(agentId)) {
      ids.push(agentId);
    }
  });
  return ids;
});

const showAgentGridOverview = computed(
  () => sessionHub.activeSection === 'agents' && agentOverviewMode.value === 'grid'
);

watch(
  () => [sessionHub.activeSection, showAgentGridOverview.value] as const,
  ([section, showGrid]) => {
    if (section !== 'agents' || showGrid) {
      return;
    }
    void preloadAgentSettingsPanels();
    warmMessengerUserToolsData({
      catalog: true,
      summary: true
    });
  },
  { immediate: true }
);

watch(
  () => [sessionHub.activeSection, settingsPanelMode.value] as const,
  ([section, panelMode]) => {
    if (section === 'more' && panelMode === 'help-manual') {
      helpManualLoading.value = true;
      return;
    }
    helpManualLoading.value = false;
  },
  { immediate: true }
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
      icon: agent?.icon,
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

  if (showDefaultAgentEntry.value) {
    pushCard(
      {
        id: DEFAULT_AGENT_KEY,
        name: t('messenger.defaultAgent'),
        description: t('messenger.defaultAgentDesc'),
        icon: (defaultAgentProfile.value as Record<string, unknown> | null)?.icon,
        sandbox_container_id: 1
      },
      { isDefault: true }
    );
  }
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
    return 'enter';
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
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).map((item) => {
    const groupId = String(item?.group_id || item?.hive_id || '').trim();
    return {
      group_id: groupId,
      name: String(
        item?.is_default ? t('messenger.agentGroup.defaultOption') : (item?.name || groupId)
      ).trim()
    };
  })
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

const agentQuickCreateCopyFromAgents = computed(() => {
  const seen = new Set<string>();
  const defaultAgentName =
    String((defaultAgentProfile.value as Record<string, unknown> | null)?.name || '').trim() ||
    t('messenger.defaultAgent');
  return [
    {
      id: DEFAULT_AGENT_KEY,
      name: defaultAgentName
    },
    ...ownedAgents.value,
    ...sharedAgents.value
  ]
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

function resolveSessionActivityTimestamp(session: Record<string, unknown>): number {
  // Keep conversation ordering aligned to real message activity to avoid list jumps on UI-only updates.
  return normalizeTimestamp(session.last_message_at || session.updated_at || session.created_at);
}

const sortedMixedConversations = computed<MixedConversation[]>(() => {
  const dismissedMap = dismissedAgentConversationMap.value;
  const sessionsByAgent = new Map<
    string,
    Array<{ session: Record<string, unknown>; lastAt: number; isMain: boolean }>
  >();
  (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).forEach((sessionRaw) => {
    const session = (sessionRaw || {}) as Record<string, unknown>;
    const agentId = normalizeAgentId(session.agent_id || (session.is_default === true ? DEFAULT_AGENT_KEY : ''));
    if (!agentId) {
      return;
    }
    const list = sessionsByAgent.get(agentId) || [];
    list.push({
      session,
      lastAt: resolveSessionActivityTimestamp(session),
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
        icon: resolveAgentIconForDisplay(agentId, agent as Record<string, unknown> | null),
        title,
        preview,
        unread: Math.max(0, Math.floor(Number(agentMainUnreadCountMap.value[agentId] || 0))),
        lastAt: Number(latest?.lastAt || main?.lastAt || 0)
      } as MixedConversation;
    })
    .filter((item) => agentMap.value.has(item.agentId))
    .filter((item) => {
      const dismissedAt = Number(dismissedMap[item.agentId] || 0);
      if (!dismissedAt) return true;
      return item.lastAt > dismissedAt;
    });

  const draftIdentity = activeConversation.value;
  if (draftIdentity?.kind === 'agent' && draftIdentity.id.startsWith('draft:')) {
    const draftAgentId = normalizeAgentId(draftIdentity.agentId || draftIdentity.id.slice('draft:'.length));
    const draftDismissedAt = Number(dismissedMap[draftAgentId] || 0);
    if (
      agentMap.value.has(draftAgentId) &&
      !agentItems.some((item) => item.agentId === draftAgentId) &&
      !draftDismissedAt
    ) {
      const agent = agentMap.value.get(draftAgentId) || null;
      agentItems.unshift({
        key: `agent:${draftAgentId}`,
        kind: 'agent',
        sourceId: '',
        agentId: draftAgentId,
        icon: resolveAgentIconForDisplay(draftAgentId, agent as Record<string, unknown> | null),
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

const mixedConversations = useStableMixedConversationOrder(sortedMixedConversations);

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
    if (settingsPanelMode.value === 'help-manual') return t('messenger.settings.helpManual');
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
    return '';
  }
  if (sessionHub.activeSection === 'more') {
    if (settingsPanelMode.value === 'profile') return currentUsername.value;
    if (settingsPanelMode.value === 'prompts') return t('messenger.prompt.desc');
    if (settingsPanelMode.value === 'help-manual') return t('messenger.settings.helpManualHint');
    if (settingsPanelMode.value === 'desktop-models') return t('desktop.system.llmHint');
    if (settingsPanelMode.value === 'desktop-lan') return t('desktop.system.lan.hint');
    if (settingsPanelMode.value === 'desktop-remote') return t('desktop.system.remote.hint');
  }
  return activeSectionSubtitle.value;
});

type MessengerPageWaitingState = {
  title: string;
  targetName: string;
  phaseLabel: string;
  summaryLabel: string;
  progress: number;
};

const resolveMessengerPageWaitingTarget = (): string => {
  if (showHelperAppsWorkspace.value && helperAppsActiveKind.value === 'online') {
    return helperAppsActiveTitle.value || t('userWorld.helperApps.title');
  }
  const chatTitle = String(chatPanelTitle.value || '').trim();
  if (chatTitle) {
    return chatTitle;
  }
  const sectionTitle = String(activeSectionTitle.value || '').trim();
  if (sectionTitle) {
    return sectionTitle;
  }
  return t('common.loading');
};

const resolveMessengerPageWaitingSummary = (): string => {
  if (showHelperAppsWorkspace.value && helperAppsActiveKind.value === 'online') {
    return t('messenger.waiting.summary.helperApps');
  }
  switch (sessionHub.activeSection) {
    case 'agents':
      return t('messenger.waiting.summary.agents');
    case 'tools':
      return t('messenger.waiting.summary.tools');
    case 'more':
      return t('messenger.waiting.summary.settings');
    case 'messages':
      return t('messenger.waiting.summary.messages');
    default:
      return t('messenger.waiting.summary.general');
  }
};

const messengerPageWaitingState = computed<MessengerPageWaitingState | null>(() => {
  if (
    workerCardImportOverlayVisible.value ||
    isMessengerInteractionBlocked.value ||
    suppressMessengerPageWaitingOverlay.value
  ) {
    return null;
  }

  if (bootLoading.value) {
    return {
      title: t('messenger.waiting.title'),
      targetName: resolveMessengerPageWaitingTarget(),
      phaseLabel: t('messenger.waiting.phase.preparing'),
      summaryLabel: resolveMessengerPageWaitingSummary(),
      progress: 22
    };
  }

  if (
    showHelperAppsWorkspace.value &&
    helperAppsActiveKind.value === 'online' &&
    helperAppsOnlineLoading.value
  ) {
    return {
      title: t('messenger.waiting.title'),
      targetName: resolveMessengerPageWaitingTarget(),
      phaseLabel: t('messenger.waiting.phase.syncing'),
      summaryLabel: t('messenger.waiting.summary.helperApps'),
      progress: 48
    };
  }

  if (sessionHub.activeSection === 'tools' && toolsCatalogLoading.value) {
    return {
      title: t('messenger.waiting.title'),
      targetName: resolveMessengerPageWaitingTarget(),
      phaseLabel: t('messenger.waiting.phase.loading'),
      summaryLabel: t('messenger.waiting.summary.tools'),
      progress: 56
    };
  }

  return null;
});

const chatPanelKindLabel = computed(() => {
  if (!showChatSettingsView.value) return activeConversationKindLabel.value;
  return '';
});

const agentSessionLoading = computed(() => {
  if (!isAgentConversationActive.value) return false;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) return false;
  const runtimeStatus = resolveSessionRuntimeStatus(sessionId);
  const loadingBySession = resolveSessionLoadingFlag(sessionId);
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  const hasTrailingRunningAssistant = hasRunningAssistantMessage(messages);
  const busyByStoreGetter = isSessionBusy(sessionId);
  if (
    !loadingBySession &&
    TERMINAL_RUNTIME_STATUS_SET.has(runtimeStatus) &&
    hasTrailingRunningAssistant
  ) {
    chatDebugLog('messenger.busy', 'force-idle-after-terminal-runtime', {
      sessionId,
      runtimeStatus,
      loadingBySession,
      busyByStoreGetter,
      messageCount: messages.length
    });
    return false;
  }
  return busyByStoreGetter;
});

const buildActiveSessionBusyDebugSnapshot = () => {
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  const runtimeStatus = activeSessionId
    ? resolveSessionRuntimeStatus(activeSessionId)
    : '';
  const loadingBySession = activeSessionId ? resolveSessionLoadingFlag(activeSessionId) : false;
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  let lastUserIndex = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (String((messages[index] as Record<string, unknown> | null)?.role || '') === 'user') {
      lastUserIndex = index;
      break;
    }
  }
  let tailAssistant: Record<string, unknown> | null = null;
  for (let index = messages.length - 1; index > lastUserIndex; index -= 1) {
    const item = messages[index] as Record<string, unknown> | null;
    if (String(item?.role || '') === 'assistant') {
      tailAssistant = item;
      break;
    }
  }
  return {
    activeSessionId,
    section: sessionHub.activeSection,
    loadingProp: agentSessionLoading.value,
    isBusyGetter: activeSessionId ? Boolean(chatStore.isSessionBusy?.(activeSessionId)) : false,
    isLoadingGetter: activeSessionId ? Boolean(chatStore.isSessionLoading?.(activeSessionId)) : false,
    loadingBySession,
    runtimeStatus,
    pendingApprovals: Array.isArray(chatStore.pendingApprovals) ? chatStore.pendingApprovals.length : 0,
    messageCount: messages.length,
    hasRunningAssistantAfterLatestUser: hasRunningAssistantMessage(messages),
    lastUserIndex,
    hasTailAssistant: Boolean(tailAssistant),
    tailAssistantState: tailAssistant ? String(tailAssistant.state || '') : '',
    tailAssistantStreamIncomplete: Boolean(tailAssistant?.stream_incomplete),
    tailAssistantWorkflowStreaming: Boolean(tailAssistant?.workflowStreaming),
    tailAssistantReasoningStreaming: Boolean(tailAssistant?.reasoningStreaming),
    tailAssistantCompactionRunning: tailAssistant
      ? isCompactionRunningFromWorkflowItems(tailAssistant.workflowItems)
      : false,
    interactionBlocked: isMessengerInteractionBlocked.value,
    interactionBlockReason: messengerInteractionBlockReason.value
  };
};

watch(
  [
    () => String(chatStore.activeSessionId || ''),
    () => agentSessionLoading.value,
    () => resolveSessionRuntimeStatus(String(chatStore.activeSessionId || '').trim()),
    () => Array.isArray(chatStore.messages) ? chatStore.messages.length : 0,
    () => Boolean(isMessengerInteractionBlocked.value)
  ],
  () => {
    chatDebugLog('messenger.busy', 'snapshot-change', buildActiveSessionBusyDebugSnapshot());
  },
  { immediate: true }
);

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
          resolveSessionActivityTimestamp(right) -
          resolveSessionActivityTimestamp(left)
      );
      const main = sorted.find((item) => Boolean(item?.is_main)) || sorted[0];
      const sessionId = String(main?.id || '').trim();
      if (!sessionId) {
        return null;
      }
      return {
        agentId,
        sessionId,
        lastAt: resolveSessionActivityTimestamp(main as Record<string, unknown>)
      } as AgentMainSessionEntry;
    })
    .filter((item): item is AgentMainSessionEntry => Boolean(item));
};

const resolvePreferredAgentSessionId = (agentId: unknown): string => {
  const normalizedAgentId = normalizeAgentId(agentId);
  const sessions = Array.isArray(chatStore.sessions) ? chatStore.sessions : [];
  return chatStore.resolveInitialSessionId(normalizedAgentId, sessions);
};

const queuedSessionDetailPrefetchIds = new Set<string>();

const flushSessionDetailPrefetchQueue = () => {
  if (typeof window !== 'undefined' && sessionDetailPrefetchTimer !== null) {
    window.clearTimeout(sessionDetailPrefetchTimer);
    sessionDetailPrefetchTimer = null;
  }
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  const sessionIds = Array.from(queuedSessionDetailPrefetchIds);
  queuedSessionDetailPrefetchIds.clear();
  sessionIds.forEach((sessionId) => {
    if (!sessionId || sessionId === activeSessionId) {
      return;
    }
    void chatStore.preloadSessionDetail(sessionId).catch(() => undefined);
  });
};

const queueSessionDetailPrefetch = (sessionId: unknown) => {
  const normalizedSessionId = String(sessionId || '').trim();
  if (!normalizedSessionId) {
    return;
  }
  if (normalizedSessionId === String(chatStore.activeSessionId || '').trim()) {
    return;
  }
  queuedSessionDetailPrefetchIds.add(normalizedSessionId);
  if (typeof window === 'undefined') {
    flushSessionDetailPrefetchQueue();
    return;
  }
  if (sessionDetailPrefetchTimer !== null) {
    return;
  }
  sessionDetailPrefetchTimer = window.setTimeout(() => {
    flushSessionDetailPrefetchQueue();
  }, SESSION_DETAIL_PREFETCH_DELAY_MS);
};

const preloadAgentById = (agentId: unknown) => {
  const sessionId = resolvePreferredAgentSessionId(agentId);
  if (!sessionId) {
    return;
  }
  queueSessionDetailPrefetch(sessionId);
};

const preloadMixedConversation = (item: MixedConversation | null | undefined) => {
  if (!item || item.kind !== 'agent') {
    return;
  }
  const sessionId = String(item.sourceId || '').trim() || resolvePreferredAgentSessionId(item.agentId);
  if (!sessionId) {
    return;
  }
  queueSessionDetailPrefetch(sessionId);
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
  const isInLeftRail = Boolean(leftRailRef.value?.contains(target));
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

  if (leftRailMoreExpanded.value && !isInLeftRail) {
    closeLeftRailMoreMenu();
  }

  if (isMiddlePaneOverlay.value && middlePaneOverlayVisible.value) {
    const isInMiddlePane = Boolean(middlePaneRef.value?.contains(target));
    if (!isInMiddlePane && !isInLeftRail) {
      clearMiddlePaneOverlayHide();
      middlePaneOverlayVisible.value = false;
    }
  }
};

const fileContainerLifecycleText = computed(() =>
  resolveFileContainerLifecycleText({
    t
  })
);

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

const shouldHideAgentSettingsRightDock = computed(() => {
  if (sessionHub.activeSection !== 'agents' || showAgentGridOverview.value) {
    return false;
  }
  return navigationPaneCollapsed.value || isMiddlePaneOverlay.value || viewportWidth.value <= 1820;
});

const showAgentRightDock = computed(() => {
  if (sessionHub.activeSection === 'agents') {
    return !showAgentGridOverview.value && !shouldHideAgentSettingsRightDock.value;
  }
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

watch(
  () => showAgentRightDock.value,
  (visible) => {
    if (!visible) {
      return;
    }
    warmMessengerUserToolsData({
      skills: true,
      summary: true
    });
  },
  { immediate: true }
);

const RIGHT_DOCK_EDGE_HOVER_THRESHOLD = 84;
let rightDockEdgeHoverFrame: number | null = null;
let pendingRightDockPointerX: number | null = null;
let cachedMessengerRootRight = 0;
let cachedMessengerRootWidth = 0;
let lastMessengerLayoutDebugSignature = '';

function resolveMessengerRootElement(): HTMLElement | null {
  const root = messengerRootRef.value as unknown;
  if (!root) return null;
  if (root instanceof HTMLElement) return root;
  const candidate = (root as { $el?: unknown }).$el;
  return candidate instanceof HTMLElement ? candidate : null;
}

function measureMessengerLayoutElement(element: Element | null): {
  width: number;
  left: number;
  right: number;
} | null {
  if (!(element instanceof HTMLElement)) return null;
  const rect = element.getBoundingClientRect();
  const width = Number.isFinite(rect.width) ? Math.round(rect.width) : 0;
  const left = Number.isFinite(rect.left) ? Math.round(rect.left) : 0;
  const right = Number.isFinite(rect.right) ? Math.round(rect.right) : 0;
  return { width, left, right };
}

function reportMessengerLayoutAnomaly(reason: string): void {
  if (typeof window === 'undefined') return;
  const root = resolveMessengerRootElement();
  if (!root) return;

  const rootRect = measureMessengerLayoutElement(root);
  const parentRect = measureMessengerLayoutElement(root.parentElement);
  const leftRailRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-left-rail'));
  const middlePaneRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-middle-pane'));
  const chatRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
  const chatBodyRect = measureMessengerLayoutElement(root.querySelector('.messenger-chat-body'));
  const footerRect = measureMessengerLayoutElement(chatFooterRef.value);
  const composerRect = measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
  const rightDockRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
  const sandboxPanelRect = measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--sandbox'));
  const skillsPanelRect = measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--skills'));

  const snapshot = {
    reason,
    route: route.fullPath,
    section: sessionHub.activeSection,
    windowWidth: Math.round(window.innerWidth || 0),
    viewportWidth: Math.round(viewportWidth.value || 0),
    root: rootRect,
    parent: parentRect,
    leftRail: leftRailRect,
    middlePane: middlePaneRect,
    chat: chatRect,
    chatBody: chatBodyRect,
    footer: footerRect,
    composer: composerRect,
    rightDock: rightDockRect,
    sandboxPanel: sandboxPanelRect,
    skillsPanel: skillsPanelRect,
    showMiddlePane: showMiddlePane.value,
    showRightDock: showRightDock.value,
    rightDockCollapsed: rightDockCollapsed.value,
    navigationPaneCollapsed: navigationPaneCollapsed.value,
    isMiddlePaneOverlay: isMiddlePaneOverlay.value,
    isRightDockOverlay: isRightDockOverlay.value,
    rootClasses: Array.from(root.classList.values()),
    gridTemplateColumns: window.getComputedStyle(root).gridTemplateColumns
  };

  const signature = JSON.stringify({
    reason,
    windowWidth: snapshot.windowWidth,
    viewportWidth: snapshot.viewportWidth,
    root: rootRect,
    chat: chatRect,
    footer: footerRect,
    composer: composerRect,
    rightDock: rightDockRect,
    gridTemplateColumns: snapshot.gridTemplateColumns,
    section: snapshot.section,
    showMiddlePane: snapshot.showMiddlePane,
    showRightDock: snapshot.showRightDock,
    rightDockCollapsed: snapshot.rightDockCollapsed,
    navigationPaneCollapsed: snapshot.navigationPaneCollapsed,
    isMiddlePaneOverlay: snapshot.isMiddlePaneOverlay,
    isRightDockOverlay: snapshot.isRightDockOverlay
  });
  if (signature === lastMessengerLayoutDebugSignature) return;
  lastMessengerLayoutDebugSignature = signature;
  console.warn('[messenger-layout-anomaly]', snapshot);
}

function detectMessengerLayoutAnomaly(): void {
  if (typeof window === 'undefined') return;
  const root = resolveMessengerRootElement();
  if (!root) return;
  const rootRect = measureMessengerLayoutElement(root);
  const chatRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
  const footerRect = measureMessengerLayoutElement(chatFooterRef.value);
  const composerRect = measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
  const rightDockRect = measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
  const windowWidth = Math.round(window.innerWidth || 0);

  if (windowWidth <= 0 || !rootRect) return;

  if (rootRect.width > 0 && rootRect.width < windowWidth - 240) {
    reportMessengerLayoutAnomaly('root-too-narrow');
    return;
  }

  if (
    windowWidth >= 900 &&
    ((chatRect && chatRect.width > 0 && chatRect.width < 220) ||
      (footerRect && footerRect.width > 0 && footerRect.width < 220) ||
      (composerRect && composerRect.width > 0 && composerRect.width < 220))
  ) {
    reportMessengerLayoutAnomaly('chat-too-narrow');
    return;
  }

  if (
    isRightDockOverlay.value &&
    rightDockRect &&
    rightDockRect.width > 0 &&
    rootRect.width >= windowWidth - 80 &&
    rightDockRect.left < Math.round(windowWidth * 0.5)
  ) {
    reportMessengerLayoutAnomaly('overlay-dock-shifted-left');
  }
}

function refreshMessengerRootBounds(): void {
  const root = resolveMessengerRootElement();
  if (!root) {
    cachedMessengerRootRight = 0;
    cachedMessengerRootWidth = 0;
    return;
  }
  const rect = root.getBoundingClientRect();
  cachedMessengerRootRight = Number.isFinite(rect.right) ? rect.right : 0;
  cachedMessengerRootWidth = Number.isFinite(rect.width) ? rect.width : 0;
  detectMessengerLayoutAnomaly();
}

function setRightDockEdgeHover(next: boolean): void {
  if (rightDockEdgeHover.value === next) return;
  rightDockEdgeHover.value = next;
}

function handleMessengerRootPointerMove(event: PointerEvent | MouseEvent): void {
  if (!showRightDock.value) {
    setRightDockEdgeHover(false);
    return;
  }
  const pointerX = Number(event.clientX);
  if (!Number.isFinite(pointerX)) {
    setRightDockEdgeHover(false);
    return;
  }
  pendingRightDockPointerX = pointerX;
  if (typeof window === 'undefined') {
    refreshMessengerRootBounds();
    if (!Number.isFinite(cachedMessengerRootRight) || cachedMessengerRootWidth <= 0) {
      setRightDockEdgeHover(false);
      return;
    }
    setRightDockEdgeHover(pointerX >= cachedMessengerRootRight - RIGHT_DOCK_EDGE_HOVER_THRESHOLD);
    return;
  }
  if (rightDockEdgeHoverFrame !== null) {
    return;
  }
  rightDockEdgeHoverFrame = window.requestAnimationFrame(() => {
    rightDockEdgeHoverFrame = null;
    if (!showRightDock.value) {
      setRightDockEdgeHover(false);
      return;
    }
    refreshMessengerRootBounds();
    if (!Number.isFinite(cachedMessengerRootRight) || cachedMessengerRootWidth <= 0) {
      setRightDockEdgeHover(false);
      return;
    }
    const nextPointerX = pendingRightDockPointerX;
    if (!Number.isFinite(nextPointerX)) {
      setRightDockEdgeHover(false);
      return;
    }
    setRightDockEdgeHover(nextPointerX >= cachedMessengerRootRight - RIGHT_DOCK_EDGE_HOVER_THRESHOLD);
  });
}

function handleMessengerRootPointerLeave(): void {
  pendingRightDockPointerX = null;
  setRightDockEdgeHover(false);
}

watch(
  () => showRightDock.value,
  (visible) => {
    if (!visible) {
      pendingRightDockPointerX = null;
      setRightDockEdgeHover(false);
      return;
    }
    refreshMessengerRootBounds();
  }
);

watch(
  () => [viewportWidth.value, navigationPaneCollapsed.value, rightDockCollapsed.value, showMiddlePane.value] as const,
  () => {
    refreshMessengerRootBounds();
  }
);

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
  const seenIds = new Set<string>();
  let mainAssigned = false;
  const result = (Array.isArray(chatStore.sessions) ? chatStore.sessions : [])
    .filter((session) => normalizeAgentId(session?.agent_id) === targetAgentId)
    .map((session) => ({
      id: String(session?.id || '').trim(),
      title: String(session?.title || t('chat.newSession')),
      preview: resolveSessionTimelinePreview(session as Record<string, unknown>),
      lastAt: resolveSessionActivityTimestamp((session || {}) as Record<string, unknown>),
      isMain: Boolean(session?.is_main)
    }))
    .filter((item) => item.id)
    .sort((left, right) => {
      if (left.isMain !== right.isMain) {
        return left.isMain ? -1 : 1;
      }
      return normalizeTimestamp(right.lastAt) - normalizeTimestamp(left.lastAt);
    })
    .filter((item) => {
      if (seenIds.has(item.id)) {
        return false;
      }
      seenIds.add(item.id);
      return true;
    })
    .map((item) => {
      if (!item.isMain || mainAssigned) {
        return { ...item, isMain: false };
      }
      mainAssigned = true;
      return item;
    });
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
    const sessionRecord =
      (Array.isArray(chatStore.sessions)
        ? chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId)
        : null) || null;
    const sessionPreview = sessionRecord
      ? resolveSessionTimelinePreview(sessionRecord as Record<string, unknown>)
      : '';
    if (sessionPreview) {
      timelinePreviewMap.value.set(targetId, sessionPreview);
      return;
    }
    const cachedMessages = chatStore.getCachedSessionMessages(targetId);
    if (Array.isArray(cachedMessages) && cachedMessages.length > 0) {
      const preview = extractLatestUserPreview(cachedMessages as unknown[]);
      timelinePreviewMap.value.set(targetId, preview);
      return;
    }
    const result = await getChatSessionApi(targetId).catch(() => null);
    const messages = Array.isArray(result?.data?.data?.messages) ? result.data.data.messages : [];
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
    triggerRealtimePulseRefresh?.('runtime-override-clear');
    return;
  }
  runtimeStateOverrides.value.set(key, {
    state,
    expiresAt: Date.now() + ttlMs
  });
  triggerRealtimePulseRefresh?.('runtime-override');
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
  if (streamingAgentIdSet.value.has(key)) {
    return 'running';
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

const hasHotRuntimeState = computed(() => {
  if (pendingApprovalAgentIdSet.value.size > 0 || streamingAgentIdSet.value.size > 0) {
    return true;
  }
  const now = Date.now();
  for (const state of agentRuntimeStateMap.value.values()) {
    if (state === 'running' || state === 'pending') {
      return true;
    }
  }
  for (const override of runtimeStateOverrides.value.values()) {
    if (override.expiresAt <= now) {
      continue;
    }
    if (override.state === 'running' || override.state === 'pending') {
      return true;
    }
  }
  return false;
});

const isHotBeeroomMissionStatus = (value: unknown): boolean => {
  const status = String(value || '').trim().toLowerCase();
  return (
    status === 'queued' ||
    status === 'running' ||
    status === 'awaiting_idle' ||
    status === 'pending' ||
    status === 'resuming' ||
    status === 'merging'
  );
};

const hasHotBeeroomRuntimeState = computed(() => {
  const activeMissions = Array.isArray(beeroomStore.activeMissions) ? beeroomStore.activeMissions : [];
  if (
    activeMissions.some((mission) =>
      isHotBeeroomMissionStatus(mission?.completion_status || mission?.status)
    )
  ) {
    return true;
  }
  if (Number(beeroomStore.activeGroup?.running_mission_total || 0) > 0) {
    return true;
  }
  const groups = Array.isArray(beeroomStore.groups) ? beeroomStore.groups : [];
  return groups.some((group) => Number(group?.running_mission_total || 0) > 0);
});

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

const buildWorkflowSurfaceDebugSnapshot = () => {
  const renderable = agentRenderableMessages.value;
  const tailAssistant =
    renderable.length > 0 ? renderable[renderable.length - 1].message : null;
  const workflowItems = Array.isArray(tailAssistant?.workflowItems)
    ? (tailAssistant.workflowItems as unknown[])
    : [];
  return {
    activeSessionId: chatStore.activeSessionId,
    renderableCount: renderable.length,
    tailRole: String(tailAssistant?.role || ''),
    tailHasWorkflowOrThinking: tailAssistant ? hasWorkflowOrThinking(tailAssistant) : false,
    tailWorkflowVisible: Boolean(tailAssistant?.workflowStreaming || workflowItems.length > 0),
    tailWorkflowItemCount: workflowItems.length,
    tailWorkflowStreaming: Boolean(tailAssistant?.workflowStreaming),
    tailReasoningStreaming: Boolean(tailAssistant?.reasoningStreaming),
    tailStreamIncomplete: Boolean(tailAssistant?.stream_incomplete),
    tailContentLength: String(tailAssistant?.content || '').length,
    tailReasoningLength: String(tailAssistant?.reasoning || '').length
  };
};

watch(
  () => {
    if (!isChatDebugEnabled()) return 'disabled';
    const snapshot = buildWorkflowSurfaceDebugSnapshot();
    return [
      snapshot.activeSessionId,
      snapshot.renderableCount,
      snapshot.tailRole,
      snapshot.tailHasWorkflowOrThinking,
      snapshot.tailWorkflowVisible,
      snapshot.tailWorkflowItemCount,
      snapshot.tailWorkflowStreaming,
      snapshot.tailReasoningStreaming,
      snapshot.tailStreamIncomplete,
      snapshot.tailContentLength,
      snapshot.tailReasoningLength
    ].join('::');
  },
  () => {
    if (!isChatDebugEnabled()) return;
    chatDebugLog('messenger.workflow-surface', 'snapshot-change', buildWorkflowSurfaceDebugSnapshot());
  },
  { immediate: true }
);

const worldRenderableMessages = computed<WorldRenderableMessage[]>(() =>
  (Array.isArray(userWorldStore.activeMessages) ? userWorldStore.activeMessages : []).map((rawMessage, sourceIndex) => {
    const message = (rawMessage || {}) as Record<string, unknown>;
    return {
      // Keep vnode keys strictly unique in a render pass to avoid component patch corruption.
      key: resolveWorldRenderKey(message, sourceIndex),
      sourceIndex,
      domId: resolveWorldMessageDomId(message),
      message
    };
  })
);

const latestAgentRenderableMessageKey = computed(() => {
  const latest = agentRenderableMessages.value[agentRenderableMessages.value.length - 1];
  return String(latest?.key || '').trim();
});

const buildLatestAssistantLayoutSignature = (message: Record<string, unknown> | undefined): string => {
  if (!message || String(message.role || '') !== 'assistant') {
    return 'non-assistant';
  }
  const workflowItems = Array.isArray(message.workflowItems)
    ? (message.workflowItems as unknown[])
    : [];
  const workflowSignature = workflowItems
    .map((item, index) => {
      const record = (item || {}) as Record<string, unknown>;
      return [
        index,
        String(record.id || record.toolCallId || record.eventType || '').trim(),
        String(record.status || '').trim(),
        String(record.title || record.toolName || '').length,
        String(record.detail || '').length
      ].join(':');
    })
    .join('|');
  const subagents = Array.isArray(message.subagents) ? message.subagents : [];
  return [
    latestAgentRenderableMessageKey.value,
    String(message.id || message.localId || '').trim(),
    String(message.content || '').length,
    String(message.reasoning || '').length,
    Boolean(message.workflowStreaming),
    Boolean(message.reasoningStreaming),
    Boolean(message.stream_incomplete),
    workflowItems.length,
    workflowSignature,
    subagents.length
  ].join('::');
};

const buildMessageWorkflowRenderKey = (
  message: Record<string, unknown>,
  baseKey: string
): string => {
  const workflowItems = Array.isArray(message?.workflowItems)
    ? (message.workflowItems as Array<Record<string, unknown>>)
    : [];
  const tail = workflowItems.length > 0 ? workflowItems[workflowItems.length - 1] : null;
  return [
    String(baseKey || '').trim(),
    workflowItems.length,
    String(tail?.id || tail?.toolCallId || tail?.eventType || '').trim(),
    String(tail?.status || '').trim(),
    String(tail?.detail || '').length,
    Boolean(message?.workflowStreaming),
    Boolean(message?.reasoningStreaming),
    Boolean(message?.stream_incomplete)
  ].join('::');
};

const latestWorldRenderableMessageKey = computed(() => {
  const latest = worldRenderableMessages.value[worldRenderableMessages.value.length - 1];
  return String(latest?.key || '').trim();
});

const shouldVirtualizeMessages = computed(
  // Messenger message virtualization is disabled because it repeatedly delayed live workflow rendering.
  () => false
);

const resolveVirtualMessageHeight = (key: string): number => {
  const normalized = String(key || '').trim();
  if (!normalized) {
    return MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
  }
  return messageVirtualHeightCache.get(normalized) || MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
};

const estimateVirtualOffsetTop = (_keys: string[], _index: number): number => 0;

const isGreetingMessage = (message: Record<string, unknown>): boolean =>
  String(message?.role || '') === 'assistant' && Boolean(message?.isGreeting);

const isHiddenInternalMessage = (message: Record<string, unknown>): boolean =>
  Boolean(message?.hiddenInternal);

const isCompactionMarkerMessage = (message: Record<string, unknown>): boolean => {
  if (String(message?.role || '') !== 'assistant') return false;
  if (hasMessageContent(message?.content)) return false;
  if (hasMessageContent(message?.reasoning)) return false;
  if (hasPlanSteps(message?.plan)) return false;
  const panelStatus = String(
    ((message?.questionPanel as Record<string, unknown> | null)?.status || '')
  )
    .trim()
    .toLowerCase();
  if (panelStatus === 'pending') return false;
  if (message?.manual_compaction_marker === true || message?.manualCompactionMarker === true) {
    return true;
  }
  if (!isCompactionOnlyWorkflowItems(message?.workflowItems)) return false;
  const isStreaming = Boolean(
    message?.workflowStreaming ||
      message?.reasoningStreaming ||
      message?.stream_incomplete
  );
  if (!isStreaming) return true;
  const snapshot = resolveLatestCompactionSnapshot(message?.workflowItems);
  const triggerMode = String(
    snapshot?.detail?.trigger_mode ?? snapshot?.detail?.triggerMode ?? ''
  )
    .trim()
    .toLowerCase();
  return triggerMode === 'manual';
};

const shouldShowCompactionDivider = (message: Record<string, unknown>): boolean => {
  if (!isCompactionMarkerMessage(message)) return false;
  if (
    (message?.manual_compaction_marker === true || message?.manualCompactionMarker === true) &&
    Boolean(
      message?.workflowStreaming ||
        message?.reasoningStreaming ||
        message?.stream_incomplete
    )
  ) {
    return true;
  }
  const snapshot = resolveLatestCompactionSnapshot(message?.workflowItems);
  if (!snapshot) return false;
  const detailStatus = String(snapshot.detail?.status || '').trim().toLowerCase();
  if (detailStatus === 'skipped') return false;
  return true;
};

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
  if (isCompactionRunningFromWorkflowItems(message?.workflowItems)) {
    return 'running';
  }
  const messageState = normalizeRuntimeState(message?.state, pendingQuestion);
  if (messageState === 'error') return 'error';
  if (resolveAssistantFailureNotice(message, t)) return 'error';
  if (messageState !== 'idle') return messageState;
  return 'done';
};

const shouldShowAgentMessageBubble = (message: Record<string, unknown>): boolean =>
  hasMessageContent(buildAssistantDisplayContent(message, t));

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

const resolveDesktopWorkspaceRoot = (): string => String(getRuntimeConfig().workspace_root || '').trim();

const resolveDesktopContainerRoot = (containerId?: number | null): string => {
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    const mapped = String(desktopContainerRootMap.value[Number(containerId)] || '').trim();
    if (mapped) return mapped;
  }
  return resolveDesktopWorkspaceRoot();
};

const resolveAgentMarkdownWorkspacePath = (rawPath: string): string => {
  const ownerId = normalizeWorkspaceOwnerId(authStore.user?.id);
  if (!ownerId) return '';
  return resolveMarkdownWorkspacePath({
    rawPath,
    ownerId,
    containerId: currentContainerId.value,
    desktopLocalMode: desktopLocalMode.value,
    workspaceRoot: resolveDesktopContainerRoot(currentContainerId.value)
  });
};

const resolveWorldMarkdownWorkspacePath = (rawPath: string, senderUserId: string): string => {
  const ownerId = normalizeWorkspaceOwnerId(senderUserId);
  if (!ownerId) return '';
  return resolveMarkdownWorkspacePath({
    rawPath,
    ownerId,
    containerId: USER_CONTAINER_ID,
    desktopLocalMode: desktopLocalMode.value,
    workspaceRoot: resolveDesktopContainerRoot(USER_CONTAINER_ID)
  });
};

const WORLD_AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;
const WORLD_AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?\uFF0C\u3002\uFF1B\uFF1A\uFF01\uFF1F\u300B\u3011]+)?$/;

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
  // Non-admin requests should prefer the current login context to avoid cross-display ID mismatches.
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
  return /not found|no such|娑撳秴鐡ㄩ崷鈻呴幍鍙ョ瑝閸掔殬瀹告彃鍨归梽顦㈠鑼╅梽顦emoved/i.test(message);
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

const parseWorkspaceRefreshContainerId = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) ? parsed : null;
};

const shouldHandleWorkspaceResourceRefresh = (detail: Record<string, unknown>) => {
  const eventAgentId = normalizeAgentId(detail.agentId ?? detail.agent_id);
  const eventContainerId = parseWorkspaceRefreshContainerId(
    detail.containerId ?? detail.container_id
  );
  if (isWorldConversationActive.value) {
    if (eventAgentId) return false;
    return !Number.isFinite(eventContainerId) || eventContainerId === USER_CONTAINER_ID;
  }
  if (!isAgentConversationActive.value) {
    return false;
  }
  const currentAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  if (eventAgentId && eventAgentId !== currentAgentId) {
    return false;
  }
  return !Number.isFinite(eventContainerId) || eventContainerId === currentContainerId.value;
};

const handleWorkspaceResourceRefresh = (event?: Event) => {
  const detail =
    (event as CustomEvent<Record<string, unknown>> | undefined)?.detail &&
    typeof (event as CustomEvent<Record<string, unknown>>).detail === 'object'
      ? ((event as CustomEvent<Record<string, unknown>>).detail as Record<string, unknown>)
      : {};
  if (!shouldHandleWorkspaceResourceRefresh(detail)) {
    return;
  }
  clearWorkspaceResourceCache();
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
  return renderMessageMarkdown(cacheKey, buildAssistantDisplayContent(message, t), {
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

const shouldShowAgentResumeButton = (message: Record<string, unknown>): boolean => {
  if (String(message?.role || '') !== 'assistant') return false;
  if (Boolean(message?.workflowStreaming) || Boolean(message?.reasoningStreaming)) return false;
  return Boolean(message?.resume_available || message?.slow_client);
};

const resumeAgentMessage = async (message: Record<string, unknown>) => {
  if (String(message?.role || '') !== 'assistant') return;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) return;
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  message.resume_available = false;
  message.slow_client = false;
  autoStickToBottom.value = true;
  setRuntimeStateOverride(targetAgentId, 'running', 30_000);
  try {
    await chatStore.resumeStream(sessionId, message, { force: true });
    setRuntimeStateOverride(targetAgentId, 'done', 8_000);
    await scrollMessagesToBottom();
  } catch (error) {
    message.resume_available = true;
    setRuntimeStateOverride(targetAgentId, 'error', 8_000);
    showApiError(error, t('chat.error.resumeFailed'));
  }
};

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

const resolveWorldRenderKey = (message: Record<string, unknown>, index: number): string => {
  const safeIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
  return `${resolveWorldMessageKey(message)}:${safeIndex}`;
};

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

const resolveAgentMessageKey = (message: Record<string, unknown>, index: number): string => {
  const base = String(message?.id || message?.message_id || message?.request_id || message?.role || 'm');
  const safeIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
  return `${base}:${safeIndex}`;
};

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

let sectionRouteSyncToken = 0;

const normalizeRouteQueryValue = (value: unknown): string[] => {
  if (Array.isArray(value)) {
    return value.map((item) => String(item ?? '').trim());
  }
  if (value === undefined || value === null) {
    return [];
  }
  return [String(value).trim()];
};

const buildRouteQuerySignature = (query: Record<string, any>): string =>
  Object.keys(query)
    .sort((left, right) => left.localeCompare(right))
    .map((key) => {
      const values = normalizeRouteQueryValue(query[key]).join(',');
      return `${key}=${values}`;
    })
    .join('&');

const isSameRouteLocation = (path: string, query: Record<string, any>): boolean => {
  const currentPath = String(route.path || '').trim();
  if (currentPath !== path) return false;
  const currentQuery = route.query as Record<string, any>;
  return buildRouteQuerySignature(currentQuery) === buildRouteQuerySignature(query);
};

const scheduleSectionRouteSync = (path: string, query: Record<string, any>) => {
  const normalizedPath = String(path || '').trim();
  if (!normalizedPath) return;
  const normalizedQuery = { ...query } as Record<string, any>;
  const ticket = ++sectionRouteSyncToken;
  Promise.resolve().then(() => {
    if (ticket !== sectionRouteSyncToken) return;
    if (isSameRouteLocation(normalizedPath, normalizedQuery)) return;
    router.replace({ path: normalizedPath, query: normalizedQuery }).catch(() => undefined);
  });
};

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
          router.replace({ path: resolveChatShellPath(), query: nextQuery }).catch(() => undefined);
        }
      }
    } else {
      await userWorldStore.dismissConversation(sourceId);
    }
    triggerRealtimePulseRefresh?.('delete-mixed-conversation');
    ElMessage.success(t('chat.history.delete'));
  } catch (error) {
    showApiError(error, t('chat.sessions.deleteFailed'));
  }
};

const switchSection = (
  section: MessengerSection,
  options: {
    preserveHelperWorkspace?: boolean;
    panelHint?: string;
    helperWorkspace?: boolean;
    settingsPanelMode?: string;
  } = {}
) => {
  const preserveHelperWorkspace = options.preserveHelperWorkspace === true;
  const panelHint = String(options.panelHint || '').trim().toLowerCase();
  const explicitSettingsPanelMode = normalizeSettingsPanelMode(options.settingsPanelMode);
  const helperWorkspace = options.helperWorkspace === true;
  closeLeftRailMoreMenu();
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
  if (section === 'more') {
    void preloadMessengerSettingsPanels({ desktopMode: desktopMode.value });
    settingsPanelMode.value =
      explicitSettingsPanelMode !== 'general'
        ? explicitSettingsPanelMode
        : desktopMode.value && panelHint === 'desktop-models'
          ? 'desktop-models'
          : desktopMode.value && panelHint === 'desktop-lan'
            ? 'desktop-lan'
            : desktopMode.value && panelHint === 'desktop-remote'
              ? 'desktop-remote'
              : panelHint === 'profile'
                ? 'profile'
                : panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt'
                  ? 'prompts'
                  : panelHint === 'help-manual' ||
                      panelHint === 'manual' ||
                      panelHint === 'help' ||
                      panelHint === 'docs' ||
                      panelHint === 'docs-site'
                    ? 'help-manual'
                  : 'general';
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
  const normalizedCurrentPath = String(route.path || '').trim();
  const normalizedBasePrefix = String(basePrefix.value || '').trim();
  // Keep navigation inside current messenger shell route to avoid route-level remount churn.
  const targetPath = normalizedCurrentPath.startsWith(`${normalizedBasePrefix}/`)
    ? normalizedCurrentPath
    : `${basePrefix.value}/${sectionRouteMap[section]}`;
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
  scheduleSectionRouteSync(targetPath, nextQuery);
  if (section === 'tools') {
    loadToolsCatalog();
  }
  ensureSectionSelection();
};

const activateSettingsPanel = (panelMode: string) => {
  const nextPanelMode = normalizeSettingsPanelMode(panelMode);
  const panelHint =
    nextPanelMode === 'profile' ||
    nextPanelMode === 'prompts' ||
    nextPanelMode === 'help-manual' ||
    nextPanelMode === 'desktop-models' ||
    nextPanelMode === 'desktop-lan' ||
    nextPanelMode === 'desktop-remote'
      ? nextPanelMode
      : '';
  // Commit the overlay preview to the real section before updating the settings panel,
  // otherwise the middle pane changes while the main content stays on the old section.
  if (sessionHub.activeSection !== 'more' || helperAppsWorkspaceMode.value) {
    switchSection('more', { panelHint, settingsPanelMode: nextPanelMode });
    return;
  }
  settingsPanelMode.value = nextPanelMode;
};

const openMoreRailSection = (section: MessengerSection) => {
  switchSection(section);
};

const openSettingsPage = () => {
  activateSettingsPanel('general');
};

const requestAgentSettingsFocus = (target: '' | 'model') => {
  if (!target) return;
  agentSettingsFocusTarget.value = target;
  agentSettingsFocusToken.value += 1;
};

const handleAgentSettingsFocusConsumed = (target: string) => {
  if (String(target || '').trim() !== agentSettingsFocusTarget.value) return;
  agentSettingsFocusTarget.value = '';
};

const openDesktopModelSettingsFromHeader = () => {
  if (!agentHeaderModelJumpEnabled.value) return;
  if (activeAgentUsingDesktopDefaultModel.value) {
    activateSettingsPanel('desktop-models');
    return;
  }
  openActiveAgentSettings({ focusSection: 'model' });
};

const openProfilePage = () => {
  closeFileContainerMenu();
  activateSettingsPanel('profile');
};

const handleSettingsLogout = () => {
  if (settingsLogoutDisabled.value) {
    return;
  }
  stopRealtimePulse?.();
  stopBeeroomRealtimeSync?.();
  authStore.logout();
  redirectToLoginAfterLogout((to) => router.replace(to));
};

const applyCurrentUserAppearance = (appearance: UserAppearancePreferences) => {
  appearanceHydrating.value = true;
  themeStore.setPalette(normalizeThemePalette(appearance.themePalette));
  currentUserAvatarIcon.value = normalizeAvatarIcon(appearance.avatarIcon, PROFILE_AVATAR_OPTION_KEYS);
  currentUserAvatarColor.value = normalizeAvatarColor(appearance.avatarColor);
  appearanceHydrating.value = false;
};

const resolveCurrentUserAppearance = (): UserAppearancePreferences => ({
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

const clearMiddlePanePrewarm = () => {
  if (typeof window !== 'undefined' && middlePanePrewarmTimer !== null) {
    window.clearTimeout(middlePanePrewarmTimer);
    middlePanePrewarmTimer = null;
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
  clearMiddlePaneOverlayPreview();
  middlePaneMounted.value = true;
  middlePaneOverlayVisible.value = true;
};

const normalizeSettingsPanelMode = (value: unknown): SettingsPanelMode => {
  const normalized = String(value || '').trim().toLowerCase();
  if (
    normalized === 'profile' ||
    normalized === 'prompts' ||
    normalized === 'help-manual' ||
    normalized === 'desktop-models' ||
    normalized === 'desktop-lan' ||
    normalized === 'desktop-remote'
  ) {
    return normalized;
  }
  return 'general';
};

const cancelMiddlePaneOverlayHide = () => {
  clearMiddlePaneOverlayHide();
};

const scheduleMiddlePaneOverlayHide = () => {
  if (!isMiddlePaneOverlay.value) return;
  clearMiddlePaneOverlayHide();
  if (typeof window === 'undefined') {
    middlePaneOverlayVisible.value = false;
    clearMiddlePaneOverlayPreview();
    return;
  }
  middlePaneOverlayHideTimer = window.setTimeout(() => {
    middlePaneOverlayHideTimer = null;
    middlePaneOverlayVisible.value = false;
    clearMiddlePaneOverlayPreview();
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
  if (quickCreatingAgent.value) return;
  agentQuickCreateVisible.value = true;
};

const submitAgentQuickCreate = async (payload: { copy_from_agent_id?: string }) => {
  const createPayload: Record<string, unknown> = {
    name: buildQuickAgentName()
  };
  const targetHiveId = String(preferredBeeroomGroupId.value || '').trim();
  if (targetHiveId) {
    createPayload.hive_id = targetHiveId;
  }
  const copyFromAgentId = String(payload?.copy_from_agent_id || '').trim();
  if (copyFromAgentId) {
    createPayload.copy_from_agent_id = copyFromAgentId;
  }
  const created = await submitAgentCreate(createPayload);
  if (created) {
    agentQuickCreateVisible.value = false;
  }
};

const submitAgentCreate = async (payload: Record<string, unknown>): Promise<boolean> => {
  if (quickCreatingAgent.value) return false;
  quickCreatingAgent.value = true;
  try {
    const created = await agentStore.createAgent(payload);
    ElMessage.success(t('portal.agent.createSuccess'));
    const tasks: Promise<unknown>[] = [loadRunningAgents({ force: true }), beeroomStore.loadGroups()];
    if (!cronPermissionDenied.value) {
      tasks.push(loadCronAgentIds({ force: true }));
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
    return true;
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
    return false;
  } finally {
    quickCreatingAgent.value = false;
  }
};

const refreshActiveBeeroom = async () => {
  if (sessionHub.activeSection !== 'swarms') {
    return;
  }
  try {
    if (String(beeroomStore.activeGroupId || '').trim()) {
      await beeroomStore.loadActiveGroup({ silent: true });
      return;
    }
    await beeroomStore.loadGroups();
    if (String(beeroomStore.activeGroupId || '').trim()) {
      await beeroomStore.loadActiveGroup({ silent: true });
    }
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
};

const refreshBeeroomRealtimeGroups = async () => {
  const now = Date.now();
  const minInterval = hasHotBeeroomRuntimeState.value
    ? BEEROOM_GROUPS_REFRESH_MIN_MS_HOT
    : BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE;
  const shouldRefreshGroups = now - beeroomGroupsLastRefreshAt >= minInterval;
  if (shouldRefreshGroups) {
    await beeroomStore.loadGroups();
    beeroomGroupsLastRefreshAt = Date.now();
  }
  await loadRunningAgents();
};

const refreshBeeroomRealtimeActiveGroup = async () => {
  const activeGroupId = String(beeroomStore.activeGroupId || '').trim();
  if (!activeGroupId) {
    return;
  }
  await beeroomStore.loadActiveGroup({ silent: true });
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

const refreshAgentMutationState = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    loadRunningAgents({ force: true }),
    beeroomStore.loadGroups()
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds({ force: true }));
  }
  await Promise.all(tasks);
};

const normalizeWorkerCardImportProgress = (value: number) =>
  Math.max(0, Math.min(100, Math.round(value)));

const resetWorkerCardImportOverlay = () => {
  workerCardImportOverlayVisible.value = false;
  workerCardImportOverlayPhase.value = 'preparing';
  workerCardImportOverlayProgress.value = 0;
  workerCardImportOverlayTargetName.value = '';
  workerCardImportOverlayCurrent.value = 0;
  workerCardImportOverlayTotal.value = 0;
};

const beginWorkerCardImportOverlay = (targetName) => {
  workerCardImportOverlayVisible.value = true;
  workerCardImportOverlayPhase.value = 'preparing';
  workerCardImportOverlayProgress.value = 6;
  workerCardImportOverlayTargetName.value = String(targetName || '').trim();
  workerCardImportOverlayCurrent.value = 0;
  workerCardImportOverlayTotal.value = 0;
};

const setWorkerCardImportCreatingOverlay = (targetName, current, total) => {
  const safeTotal = Math.max(1, Number(total || 0));
  const safeCurrent = Math.max(1, Math.min(safeTotal, Number(current || 0)));
  workerCardImportOverlayVisible.value = true;
  workerCardImportOverlayPhase.value = 'creating';
  workerCardImportOverlayTargetName.value = String(targetName || '').trim();
  workerCardImportOverlayCurrent.value = safeCurrent;
  workerCardImportOverlayTotal.value = safeTotal;
  workerCardImportOverlayProgress.value = normalizeWorkerCardImportProgress(
    18 + ((safeCurrent - 1) / safeTotal) * 64
  );
};

const setWorkerCardImportRefreshingOverlay = (targetName, total) => {
  const safeTotal = Math.max(0, Number(total || 0));
  workerCardImportOverlayVisible.value = true;
  workerCardImportOverlayPhase.value = 'refreshing';
  workerCardImportOverlayTargetName.value = String(targetName || '').trim();
  workerCardImportOverlayCurrent.value = safeTotal;
  workerCardImportOverlayTotal.value = safeTotal;
  workerCardImportOverlayProgress.value = 92;
};

const openWorkerCardImportPicker = () => {
  if (workerCardImporting.value || quickCreatingAgent.value) {
    return;
  }
  workerCardImportInputRef.value?.click();
};

const handleWorkerCardImportInput = async (event) => {
  const input = event?.target as HTMLInputElement | null;
  const file = input?.files?.[0];
  if (!file || quickCreatingAgent.value || workerCardImporting.value) return;
  workerCardImporting.value = true;
  beginWorkerCardImportOverlay(file.name);
  try {
    const dependencyCatalog = await loadUserToolsSummaryCache().catch(() => null);
    const documents = parseWorkerCardText(await file.text());
    workerCardImportOverlayTotal.value = documents.length;
    workerCardImportOverlayProgress.value = normalizeWorkerCardImportProgress(documents.length > 0 ? 12 : 18);
    const createdItems: Record<string, unknown>[] = [];
    const warnings: string[] = [];
    for (const [index, document] of documents.entries()) {
      setWorkerCardImportCreatingOverlay(document.metadata.name || file.name, index + 1, documents.length);
      const dependencyStatus = resolveAgentDependencyStatus(
        {
          declared_tool_names: document.abilities.tool_names,
          declared_skill_names: document.abilities.skills
        },
        dependencyCatalog
      );
      const response = await createAgentApi(workerCardToAgentPayload(document));
      const created = response?.data?.data;
      if (created) {
        createdItems.push(created);
      }
      if (dependencyStatus.missingToolNames.length || dependencyStatus.missingSkillNames.length) {
        warnings.push(
          t('portal.agent.workerCardImportMissingSummary', {
            name: document.metadata.name,
            tools: dependencyStatus.missingToolNames.length,
            skills: dependencyStatus.missingSkillNames.length
          })
        );
      }
    }
    setWorkerCardImportRefreshingOverlay(
      documents.length === 1 ? documents[0].metadata.name || file.name : file.name,
      documents.length
    );
    await refreshAgentMutationState();
    await loadAgentToolSummary({ force: true });
    workerCardImportOverlayProgress.value = 100;
    if (createdItems[0]?.id) {
      openCreatedAgentSettings(createdItems[0].id);
    }
    ElMessage.success(
      documents.length === 1
        ? t('portal.agent.workerCardImportSuccess', { name: documents[0].metadata.name })
        : t('portal.agent.workerCardImportBatchSuccess', { count: documents.length })
    );
    if (warnings.length) {
      ElMessage.warning(warnings.join('\uff1b'));
    }
  } catch (error) {
    showApiError(error, t('portal.agent.workerCardImportFailed'));
  } finally {
    workerCardImporting.value = false;
    resetWorkerCardImportOverlay();
    if (input) {
      input.value = '';
    }
  }
};

const handleAgentBatchExport = async (agentIds: string[]) => {
  const normalizedIds = Array.from(new Set(agentIds.map((item) => normalizeAgentId(item)).filter(Boolean)));
  if (!normalizedIds.length) return;
  try {
    const records: Record<string, unknown>[] = [];
    for (const agentId of normalizedIds) {
      const agent = await agentStore.getAgent(agentId, { force: true });
      if (agent) {
        records.push(agent as Record<string, unknown>);
      }
    }
    if (!records.length) {
      ElMessage.warning(t('portal.agent.loadingFailed'));
      return;
    }
    const filename = downloadWorkerCardBundle(records);
    ElMessage.success(t('portal.agent.workerCardExportSuccess', { name: filename }));
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  }
};

const handleAgentBatchDelete = async (agentIds: string[]) => {
  const normalizedIds = Array.from(new Set(agentIds.map((item) => normalizeAgentId(item)).filter(Boolean)));
  const ownedIds = new Set(
    (Array.isArray(agentStore.agents) ? agentStore.agents : []).map((agent) => normalizeAgentId(agent?.id))
  );
  const deletableIds = normalizedIds.filter((agentId) => agentId !== DEFAULT_AGENT_KEY && ownedIds.has(agentId));
  if (!deletableIds.length) {
    ElMessage.warning(t('portal.agent.deleteBatchUnavailable'));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t('portal.agent.deleteBatchConfirm', { count: deletableIds.length }),
      t('common.notice'),
      {
        confirmButtonText: t('portal.agent.delete'),
        cancelButtonText: t('portal.agent.cancel'),
        type: 'warning'
      }
    );
  } catch {
    return;
  }
  const results = await Promise.allSettled(deletableIds.map((agentId) => deleteAgentApi(agentId)));
  const successCount = results.filter((item) => item.status === 'fulfilled').length;
  const failedCount = results.length - successCount;
  if (successCount > 0) {
    await refreshAgentMutationState();
  }
  if (failedCount === 0) {
    ElMessage.success(t('portal.agent.deleteBatchSuccess', { count: successCount }));
    return;
  }
  if (successCount > 0) {
    ElMessage.warning(t('portal.agent.deleteBatchPartial', { success: successCount, failed: failedCount }));
    return;
  }
  const firstRejected = results.find((item) => item.status === 'rejected');
  showApiError((firstRejected as PromiseRejectedResult | undefined)?.reason, t('portal.agent.deleteFailed'));
};

const handleSearchCreateAction = async (command?: string) => {
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
    if (command === 'import_worker_card') {
      openWorkerCardImportPicker();
      return;
    }
    await createAgentQuickly();
  }
};

const openMixedConversation = async (item: MixedConversation) => {
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = false;
  if (
    sessionHub.activeSection === 'messages' &&
    !showChatSettingsView.value &&
    isMixedConversationActive(item)
  ) {
    return;
  }
  if (item.kind === 'agent') {
    const targetSessionId = String(item.sourceId || '').trim();
    if (targetSessionId) {
      await openAgentSession(targetSessionId, item.agentId);
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

const selectBeeroomGroup = async (group: Record<string, unknown>) => {
  const groupId = String(group?.group_id || group?.hive_id || '').trim();
  if (!groupId) return;
  beeroomStore.setActiveGroup(groupId);
  await beeroomStore.loadActiveGroup().catch(() => null);
};

const handleDeleteBeeroomGroup = async (group: Record<string, unknown>) => {
  const groupId = String(group?.group_id || group?.hive_id || '').trim();
  if (!groupId) {
    return;
  }
  const groupName = String(group?.name || groupId).trim() || groupId;
  try {
    await ElMessageBox.confirm(
      t('beeroom.message.deleteConfirm', { name: groupName }),
      t('common.delete'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
  } catch {
    return;
  }

  try {
    await beeroomStore.deleteGroup(groupId);
    await Promise.all([
      agentStore.loadAgents().catch(() => null),
      loadRunningAgents({ force: true }).catch(() => null)
    ]);
    ElMessage.success(t('beeroom.message.hiveDeleted'));
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
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
      path: mode === 'messages' ? resolveChatShellPath() : `${basePrefix.value}/user-world`,
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
  const preferredSessionId = resolvePreferredAgentSessionId(normalized);
  if (preferredSessionId) {
    await openAgentSession(preferredSessionId, normalized);
    return;
  }
  try {
    const freshSessionId = await openOrReuseFreshAgentSession(normalized);
    if (freshSessionId) {
      await openAgentSession(freshSessionId, normalized);
      return;
    }
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
  // Keep navigation usable when the backend is temporarily unavailable.
  await openAgentDraftSessionWithScroll(normalized);
};

const openAgentDraftSession = (agentId: unknown) => {
  const normalized = normalizeAgentId(agentId);
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
    path: resolveChatShellPath(),
    query: nextQuery
  }).catch(() => undefined);
};

const openAgentDraftSessionWithScroll = async (agentId: unknown) => {
  openAgentDraftSession(agentId);
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

const triggerAgentSettingsReload = () => {
  void agentSettingsPanelRef.value?.triggerReload();
};

const triggerAgentSettingsDelete = () => {
  void agentSettingsPanelRef.value?.triggerDelete();
};

const triggerAgentSettingsSave = () => {
  void agentSettingsPanelRef.value?.triggerSave();
};

const triggerAgentSettingsExport = () => {
  void agentSettingsPanelRef.value?.triggerExportWorkerCard();
};


const openActiveAgentSettings = (
  optionsOrEvent: { focusSection?: '' | 'model' } | Event = {}
) => {
  const options =
    optionsOrEvent instanceof Event
      ? {}
      : optionsOrEvent;
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  if (options.focusSection === 'model') {
    requestAgentSettingsFocus('model');
  }
  agentOverviewMode.value = 'detail';
  selectedAgentId.value = targetAgentId;
  switchSection('agents');
  const nextQuery = {
    ...route.query,
    section: 'agents',
    agent_id: targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId
  } as Record<string, any>;
  delete nextQuery.session_id;
  delete nextQuery.entry;
  delete nextQuery.conversation_id;
  scheduleSectionRouteSync(resolveChatShellPath(), nextQuery);
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

async function loadAgentToolSummary(options: { force?: boolean } = {}) {
  const force = options.force === true;
  if (agentToolSummaryPromise) {
    return agentToolSummaryPromise;
  }
  if (!force && agentPromptToolSummary.value) {
    return agentPromptToolSummary.value;
  }
  agentToolSummaryLoading.value = true;
  agentToolSummaryError.value = '';
  agentToolSummaryPromise = (async () => {
    try {
      const summary = (await loadUserToolsSummaryCache({ force })) as Record<string, unknown> | null;
      agentPromptToolSummary.value = summary;
      return summary;
    } catch (error) {
      agentToolSummaryError.value =
        (error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail ||
        t('chat.toolSummaryFailed');
      return null;
    } finally {
      agentToolSummaryLoading.value = false;
      agentToolSummaryPromise = null;
      if (agentAbilityTooltipVisible.value) {
        await updateAgentAbilityTooltip();
      }
    }
  })();
  return agentToolSummaryPromise;
}

const resolveActiveAgentPromptPreviewKey = (): string => {
  const sessionId = String(chatStore.activeSessionId || '').trim() || 'draft';
  const agentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value || chatStore.draftAgentId);
  return `${sessionId}:${agentId}`;
};

const fetchActiveAgentPromptPreviewPayload = async (
  options: { force?: boolean } = {}
): Promise<Record<string, unknown>> => {
  const force = options.force === true;
  const cacheKey = resolveActiveAgentPromptPreviewKey();
  const now = Date.now();
  if (
    !force &&
    agentPromptPreviewPayloadCache &&
    agentPromptPreviewPayloadCache.key === cacheKey &&
    now - agentPromptPreviewPayloadCache.updatedAt <= AGENT_PROMPT_PREVIEW_CACHE_MS
  ) {
    return agentPromptPreviewPayloadCache.payload;
  }
  if (agentPromptPreviewPayloadPromise && agentPromptPreviewPayloadPromiseKey === cacheKey) {
    return agentPromptPreviewPayloadPromise;
  }
  agentPromptPreviewPayloadPromiseKey = cacheKey;
  agentPromptPreviewPayloadPromise = (async () => {
  const currentAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value || chatStore.draftAgentId);
  const session = activeAgentSession.value as Record<string, unknown> | null;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  const sourceAgentId = normalizeAgentId(
    session?.agent_id || chatStore.draftAgentId || activeAgentId.value
  );
  const agentId = sourceAgentId === DEFAULT_AGENT_KEY ? '' : sourceAgentId;
  let previewAgentProfile =
    sourceAgentId === DEFAULT_AGENT_KEY
      ? (defaultAgentProfile.value as Record<string, unknown> | null)
      : ((activeAgentDetailProfile.value as Record<string, unknown> | null) ||
          (activeAgent.value as Record<string, unknown> | null));
  if (!sessionId) {
    if (sourceAgentId === DEFAULT_AGENT_KEY) {
      if (!previewAgentProfile) {
        previewAgentProfile =
          ((await agentStore.getAgent(DEFAULT_AGENT_KEY).catch(() => null)) as Record<string, unknown> | null) ||
          null;
        defaultAgentProfile.value = previewAgentProfile;
      }
    } else if (sourceAgentId) {
      const hasConfiguredAbilities = resolveAgentConfiguredAbilityNames(previewAgentProfile).length > 0;
      if (!hasConfiguredAbilities) {
        previewAgentProfile =
          ((await agentStore.getAgent(sourceAgentId).catch(() => null)) as Record<string, unknown> | null) ||
          previewAgentProfile;
        if (previewAgentProfile) {
          activeAgentDetailProfile.value = previewAgentProfile;
        }
      }
    }
  }
  const previewAgentDefaults = normalizeAbilityNameList(
    resolveAgentConfiguredAbilityNames(previewAgentProfile)
  );
  const overrides =
    previewAgentDefaults.length > 0
      ? previewAgentDefaults
      : [AGENT_TOOL_OVERRIDE_NONE];
  const payload =
    sessionId
      ? {
          ...(agentId ? { agent_id: agentId } : {})
        }
      : {
          ...(agentId ? { agent_id: agentId } : {}),
          ...(overrides ? { tool_overrides: overrides } : {})
        };
  const promptResult = sessionId
    ? await fetchSessionSystemPrompt(sessionId, payload)
    : await fetchRealtimeSystemPrompt(payload);
    const promptPayload = (promptResult?.data?.data || {}) as Record<string, unknown>;
    agentPromptPreviewPayloadCache = {
      key: cacheKey,
      payload: promptPayload,
      updatedAt: Date.now()
    };
    return promptPayload;
  })();
  try {
    return await agentPromptPreviewPayloadPromise;
  } finally {
    if (agentPromptPreviewPayloadPromiseKey === cacheKey) {
      agentPromptPreviewPayloadPromise = null;
      agentPromptPreviewPayloadPromiseKey = '';
    }
  }
};

const syncAgentPromptPreviewSelectedNames = async (options: { force?: boolean } = {}) => {
  if (agentPromptPreviewSelectedNames.value !== null && options.force !== true) {
    return agentPromptPreviewSelectedNames.value;
  }
  try {
    const promptPayload = await fetchActiveAgentPromptPreviewPayload(options);
    agentPromptPreviewSelectedNames.value = extractPromptPreviewSelectedAbilityNames(promptPayload);
    return agentPromptPreviewSelectedNames.value;
  } catch {
    agentPromptPreviewSelectedNames.value = null;
    return null;
  } finally {
    if (agentAbilityTooltipVisible.value) {
      await updateAgentAbilityTooltip();
    }
  }
};

const clearRightDockSkillAutoRetry = () => {
  if (typeof window === 'undefined') return;
  if (rightDockSkillAutoRetryTimer !== null) {
    window.clearTimeout(rightDockSkillAutoRetryTimer);
    rightDockSkillAutoRetryTimer = null;
  }
};

const scheduleRightDockSkillAutoRetry = () => {
  if (typeof window === 'undefined') return;
  if (rightDockSkillAutoRetryTimer !== null) return;
  rightDockSkillAutoRetryTimer = window.setTimeout(() => {
    rightDockSkillAutoRetryTimer = null;
    if (!showAgentRightDock.value) return;
    if (rightDockSkillCatalog.value.length > 0) return;
    void loadRightDockSkills({ force: true, silent: true });
  }, RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS);
};

async function loadRightDockSkills(
  options: { force?: boolean; silent?: boolean } = {}
) {
  const force = options.force === true;
  const silent = options.silent !== false;
  if (force) {
    clearRightDockSkillAutoRetry();
  }
  if (rightDockSkillCatalogLoading.value && !force) {
    return false;
  }
  const currentVersion = ++rightDockSkillCatalogLoadVersion;
  rightDockSkillCatalogLoading.value = true;
  try {
    const skills = await loadUserSkillsCache({ force });
    if (currentVersion !== rightDockSkillCatalogLoadVersion) return;
    rightDockSkillCatalog.value = normalizeRightDockSkillCatalog(skills);
    if (!force && rightDockSkillCatalog.value.length === 0) {
      // First pass may race with startup auth/cache warmup and return empty transiently.
      scheduleRightDockSkillAutoRetry();
    }
    return true;
  } catch (error) {
    if (currentVersion !== rightDockSkillCatalogLoadVersion) return;
    if (!silent) {
      showApiError(error, t('userTools.skills.loadFailed'));
    }
    if (!force) {
      scheduleRightDockSkillAutoRetry();
    }
    return false;
  } finally {
    if (currentVersion === rightDockSkillCatalogLoadVersion) {
      rightDockSkillCatalogLoading.value = false;
    }
  }
}

const openRightDockSkillDetail = async (name: unknown) => {
  const normalized = String(name || '').trim();
  if (!normalized) return;
  rightDockSelectedSkillName.value = normalized;
  rightDockSkillDialogVisible.value = true;
  rightDockSkillContent.value = '';
  rightDockSkillContentPath.value = String(
    rightDockSkillCatalog.value.find((item) => item.name === normalized)?.path || ''
  ).trim();
  const currentVersion = ++rightDockSkillContentLoadVersion;
  rightDockSkillContentLoading.value = true;
  try {
    const result = await fetchUserSkillContent(normalized);
    if (currentVersion !== rightDockSkillContentLoadVersion) return;
    const payload = (result?.data?.data || {}) as Record<string, unknown>;
    rightDockSkillContent.value = String(payload.content || '');
    rightDockSkillContentPath.value = String(payload.path || rightDockSkillContentPath.value || '').trim();
  } catch (error) {
    if (currentVersion !== rightDockSkillContentLoadVersion) return;
    rightDockSkillContent.value = '';
    rightDockSkillContentPath.value = '';
    showApiError(
      error,
      t('userTools.skills.file.readFailed', { message: t('common.requestFailed') })
    );
  } finally {
    if (currentVersion === rightDockSkillContentLoadVersion) {
      rightDockSkillContentLoading.value = false;
    }
  }
};

const handleRightDockSkillEnabledToggle = async (value: unknown) => {
  const targetName = String(rightDockSelectedSkillName.value || '').trim();
  if (!targetName || rightDockSkillToggleSaving.value) return;
  const targetAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value || chatStore.draftAgentId);
  if (!targetAgentId) return;
  const sourceProfile =
    targetAgentId === DEFAULT_AGENT_KEY
      ? ((defaultAgentProfile.value as Record<string, unknown> | null) ||
          ((await agentStore.getAgent(DEFAULT_AGENT_KEY, { force: true }).catch(() => null)) as Record<
            string,
            unknown
          > | null))
      : ((activeAgentDetailProfile.value as Record<string, unknown> | null) ||
          (activeAgent.value as Record<string, unknown> | null) ||
          ((await agentStore.getAgent(targetAgentId, { force: true }).catch(() => null)) as Record<
            string,
            unknown
          > | null));
  if (!sourceProfile) {
    ElMessage.warning(t('chat.features.agentMissing'));
    return;
  }
  const nextToolNameSet = new Set<string>(
    normalizeRightDockSkillNameList(
      normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(sourceProfile))
    )
  );
  if (Boolean(value)) {
    nextToolNameSet.add(targetName);
  } else {
    nextToolNameSet.delete(targetName);
  }
  const nextToolNames = Array.from(nextToolNameSet).sort((left, right) =>
    left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' })
  );
  const dependencyPayload = buildDeclaredDependencyPayload(
    nextToolNames,
    sourceProfile,
    (agentPromptToolSummary.value || {}) as Record<string, unknown>
  );
  rightDockSkillToggleSaving.value = true;
  try {
    const updated = (await agentStore.updateAgent(targetAgentId, {
      tool_names: dependencyPayload.tool_names,
      declared_tool_names: dependencyPayload.declared_tool_names,
      declared_skill_names: dependencyPayload.declared_skill_names
    })) as Record<string, unknown> | null;
    if (targetAgentId === DEFAULT_AGENT_KEY) {
      defaultAgentProfile.value = updated;
    } else if (targetAgentId === activeAgentId.value) {
      activeAgentDetailProfile.value = updated;
    }
    await loadAgentToolSummary({ force: true });
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    rightDockSkillToggleSaving.value = false;
  }
};

const isUserToolsScopeForAgentSummary = (scope: unknown): boolean => {
  const normalized = String(scope || '').trim().toLowerCase();
  if (!normalized || normalized === 'all') return true;
  return normalized === 'skills' || normalized === 'mcp' || normalized === 'knowledge';
};

const handleUserToolsUpdatedEvent = (event: CustomEvent<{ scope?: string; action?: string }>) => {
  const scope = event?.detail?.scope;
  if (!isUserToolsScopeForAgentSummary(scope)) {
    return;
  }
  agentToolSummaryPromise = null;
  agentToolSummaryLoading.value = false;
  invalidateUserToolsCatalogCache();
  invalidateUserToolsSummaryCache();
  invalidateUserSkillsCache();
  void loadAgentToolSummary({ force: true });
  void loadRightDockSkills({ force: true, silent: true });
  if (sessionHub.activeSection === 'tools') {
    void loadToolsCatalog({ silent: true });
  }
};

const handleChatPageRefresh = () => {
  if (isMessengerInteractionBlocked.value) {
    return;
  }
  window.location.reload();
};

const handleRightDockSkillArchiveUpload = async (file: File) => {
  if (!file || skillDockUploading.value) return;
  const filename = String(file.name || '').trim().toLowerCase();
  if (!filename.endsWith('.zip') && !filename.endsWith('.skill')) {
    ElMessage.warning(t('userTools.skills.upload.zipOnly'));
    return;
  }
  skillDockUploading.value = true;
  try {
    await uploadUserSkillZip(file);
    agentToolSummaryPromise = null;
    agentToolSummaryLoading.value = false;
    invalidateUserSkillsCache();
    invalidateUserToolsSummaryCache();
    invalidateUserToolsCatalogCache();
    await loadRightDockSkills({ force: true, silent: true });
    void loadAgentToolSummary({ force: true });
    emitUserToolsUpdated({ scope: 'skills', action: 'upload' });
    ElMessage.success(t('userTools.skills.upload.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.upload.failed'));
  } finally {
    skillDockUploading.value = false;
  }
};

const handleAgentAbilityTooltipShow = () => {
  agentAbilityTooltipVisible.value = true;
  void loadAgentToolSummary();
  void syncAgentPromptPreviewSelectedNames();
  void updateAgentAbilityTooltip();
};

const handleAgentAbilityTooltipHide = () => {
  agentAbilityTooltipVisible.value = false;
};

function warmMessengerUserToolsData(
  options: {
    catalog?: boolean;
    skills?: boolean;
    summary?: boolean;
  } = {}
) {
  if (options.catalog === true) {
    void loadUserToolsCatalogCache();
  }
  if (options.summary === true) {
    void loadAgentToolSummary();
  }
  if (options.skills === true) {
    void loadRightDockSkills({ silent: true });
  }
}

const openAgentPromptPreview = async () => {
  agentPromptPreviewVisible.value = true;
  agentPromptPreviewLoading.value = true;
  agentPromptPreviewContent.value = '';
  agentPromptPreviewMemoryMode.value = 'none';
  agentPromptPreviewToolingMode.value = '';
  agentPromptPreviewToolingContent.value = '';
  agentPromptPreviewToolingItems.value = [];
  const summaryPromise = loadAgentToolSummary();
  try {
    const promptPayload = await fetchActiveAgentPromptPreviewPayload();
    agentPromptPreviewSelectedNames.value = extractPromptPreviewSelectedAbilityNames(promptPayload);
    agentPromptPreviewContent.value = String(promptPayload.prompt || '').replace(
      /<<WUNDER_HISTORY_MEMORY>>/g,
      ''
    );
    const nextMode = String(promptPayload.memory_preview_mode || 'none').trim().toLowerCase();
    agentPromptPreviewMemoryMode.value =
      nextMode === 'frozen' || nextMode === 'pending' ? nextMode : 'none';
    const toolingPreview = extractPromptToolingPreview(promptPayload);
    agentPromptPreviewToolingMode.value = toolingPreview.mode;
    agentPromptPreviewToolingContent.value = toolingPreview.text;
    agentPromptPreviewToolingItems.value = toolingPreview.items;
    void summaryPromise.catch(() => null);
  } catch (error) {
    showApiError(error, t('chat.systemPromptFailed'));
    agentPromptPreviewSelectedNames.value = null;
    agentPromptPreviewContent.value = '';
    agentPromptPreviewMemoryMode.value = 'none';
    agentPromptPreviewToolingMode.value = '';
    agentPromptPreviewToolingContent.value = '';
    agentPromptPreviewToolingItems.value = [];
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
  const normalizedSessionId = String(sessionId || '').trim();
  if (!normalizedSessionId) return;
  const perfTrace = startMessengerPerfTrace('openAgentSession', { sessionId: normalizedSessionId, agentId });
  clearMiddlePaneOverlayHide();
  middlePaneOverlayVisible.value = false;
  const knownSession = chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
  const fallbackAgentId = agentId
    ? normalizeAgentId(agentId)
    : normalizeAgentId(knownSession?.agent_id ?? chatStore.draftAgentId);
  clearAgentConversationDismissed(fallbackAgentId);
  selectedAgentId.value = fallbackAgentId || DEFAULT_AGENT_KEY;
  sessionHub.setActiveConversation({
    kind: 'agent',
    id: normalizedSessionId,
    agentId: fallbackAgentId || DEFAULT_AGENT_KEY
  });
  const nextQuery = {
    ...route.query,
    section: 'messages',
    session_id: normalizedSessionId,
    agent_id: fallbackAgentId === DEFAULT_AGENT_KEY ? '' : fallbackAgentId
  } as Record<string, any>;
  delete nextQuery.conversation_id;
  router.replace({
    path: resolveChatShellPath(),
    query: nextQuery
  }).catch(() => undefined);
  const isForegroundSession = () =>
    String(chatStore.activeSessionId || '').trim() === normalizedSessionId;
  try {
    markMessengerPerfTrace(perfTrace, 'beforeLoadSessionDetail');
    let sessionDetail = null;
    let sessionDetailError: unknown = null;
    const sessionDetailTask = chatStore
      .loadSessionDetail(normalizedSessionId)
      .then((value) => {
        sessionDetail = value;
      })
      .catch((error) => {
        sessionDetailError = error;
      });
    markMessengerPerfTrace(perfTrace, 'loadSessionDetailScheduled');
    await scrollMessagesToBottom(true);
    markMessengerPerfTrace(perfTrace, 'uiReady');
    await sessionDetailTask;
    if (sessionDetailError) {
      throw sessionDetailError;
    }
    markMessengerPerfTrace(perfTrace, 'afterLoadSessionDetail');
    if (!isForegroundSession()) {
      finishMessengerPerfTrace(perfTrace, 'ok', { stale: true });
      return;
    }
    if (!sessionDetail) {
      await openAgentById(fallbackAgentId || DEFAULT_AGENT_KEY);
      finishMessengerPerfTrace(perfTrace, 'ok', { recovered: true });
      return;
    }
    const session = chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
    const targetAgentId = normalizeAgentId(session?.agent_id ?? fallbackAgentId);
    selectedAgentId.value = targetAgentId || DEFAULT_AGENT_KEY;
    sessionHub.setActiveConversation({
      kind: 'agent',
      id: normalizedSessionId,
      agentId: targetAgentId || DEFAULT_AGENT_KEY
    });
    const mainEntry = collectMainAgentSessionEntries().find((item) => item.agentId === targetAgentId);
    if (mainEntry?.sessionId === normalizedSessionId) {
      setAgentMainReadAt(targetAgentId, mainEntry.lastAt || Date.now());
      setAgentMainUnreadCount(targetAgentId, 0);
      persistAgentUnreadState();
    }
    finishMessengerPerfTrace(perfTrace, 'ok');
  } catch (error) {
    if (!isForegroundSession()) {
      finishMessengerPerfTrace(perfTrace, 'ok', { stale: true });
      return;
    }
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
  const targetId = String(sessionId || '').trim();
  if (!targetId) return false;
  const targetSession = chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
  if (targetSession?.is_main) {
    return true;
  }
  try {
    await chatStore.setMainSession(targetId);
    return true;
  } catch (error) {
    showApiError(error, t('chat.history.setMainFailed'));
    return false;
  }
};

const handleTimelineDialogActivateSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  timelineDialogVisible.value = false;
  await setTimelineSessionMain(targetId);
  await restoreTimelineSession(targetId);
};

const renameTimelineSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  const session = chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
  const currentTitle = String(session?.title || t('chat.newSession')).trim() || t('chat.newSession');
  try {
    const { value } = await ElMessageBox.prompt(
      t('chat.history.renamePrompt'),
      t('chat.history.rename'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        inputValue: currentTitle,
        inputPlaceholder: t('chat.history.renamePlaceholder'),
        inputValidator: (inputValue: string) =>
          String(inputValue || '').trim() ? true : t('chat.history.renameRequired')
      }
    );
    const nextTitle = String(value || '').trim();
    if (!nextTitle || nextTitle === currentTitle) {
      return;
    }
    await chatStore.renameSession(targetId, nextTitle);
    ElMessage.success(t('chat.history.renameSuccess'));
  } catch (error) {
    if (error === 'cancel' || error === 'close') {
      return;
    }
    showApiError(error, t('chat.history.renameFailed'));
  }
};

const archiveTimelineSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  const confirmed = await confirmWithFallback(
    t('chat.history.confirmArchive'),
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
    await chatStore.archiveSession(targetId);
    timelinePreviewMap.value.delete(targetId);
    triggerRealtimePulseRefresh?.('archive-session');
    ElMessage.success(t('chat.history.archiveSuccess'));
  } catch (error) {
    showApiError(error, t('chat.history.archiveFailed'));
  }
};

const handleArchivedSessionRemoved = (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  timelinePreviewMap.value.delete(targetId);
  if (timelineDetailSessionId.value === targetId) {
    timelineDetailDialogVisible.value = false;
  }
  triggerRealtimePulseRefresh?.('archived-session-removed');
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

function handleDesktopModelMetaChanged(): void {
  if (!desktopMode.value) return;
  agentVoiceModelSupportCheckedAt = 0;
  desktopDefaultModelMetaFetchPromise = null;
  void readDesktopDefaultModelMeta(true);
}

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

const loadToolsCatalog = async (options: { silent?: boolean } = {}) => {
  const loadVersion = ++toolsCatalogLoadVersion;
  const manageLoading = !options.silent || !toolsCatalogLoaded.value || toolsCatalogLoading.value;
  if (manageLoading) {
    toolsCatalogLoading.value = true;
  }
  try {
    const payload = ((await loadUserToolsCatalogCache()) || {}) as Record<string, unknown>;
    if (loadVersion !== toolsCatalogLoadVersion) {
      return;
    }
    builtinTools.value = (Array.isArray(payload.builtin_tools) ? payload.builtin_tools : [])
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
    toolsCatalogLoaded.value = true;
  } catch (error) {
    if (loadVersion !== toolsCatalogLoadVersion) {
      return;
    }
    showApiError(error, t('toolManager.loadFailed'));
  } finally {
    if (manageLoading && loadVersion === toolsCatalogLoadVersion) {
      toolsCatalogLoading.value = false;
    }
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

const selectToolCategory = (category: 'admin' | 'mcp' | 'skills' | 'knowledge') => {
  selectedToolCategory.value = category;
};

const toolCategoryLabel = (category: string) => {
  if (category === 'admin') return t('messenger.tools.adminTitle');
  if (category === 'mcp') return t('toolManager.system.mcp');
  if (category === 'skills') return t('toolManager.system.skills');
  if (category === 'knowledge') return t('toolManager.system.knowledge');
  return category;
};

const handleAgentSettingsSaved = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    loadDefaultAgentProfile(),
    loadRunningAgents({ force: true }),
    loadAgentUserRounds(),
    loadChannelBoundAgentIds({ force: true }),
    loadAgentToolSummary({ force: true })
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds({ force: true }));
  }
  await Promise.allSettled(tasks);
  const currentAgentId = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  if (currentAgentId && currentAgentId !== DEFAULT_AGENT_KEY) {
    const profile = await agentStore.getAgent(currentAgentId, { force: true }).catch(() => null);
    if (normalizeAgentId(activeAgentId.value || selectedAgentId.value) === currentAgentId) {
      activeAgentDetailProfile.value = (profile as Record<string, unknown> | null) || null;
    }
  } else {
    activeAgentDetailProfile.value = null;
  }
};

const handleAgentDeleteStart = () => {
  deletingAgentSelectionSnapshot.value = [...visibleAgentIdsForSelection.value];
};

const handleAgentDeleted = async (deletedAgentId: string) => {
  const normalizedDeletedAgentId = normalizeAgentId(deletedAgentId);
  const currentIdsWithoutDeleted = visibleAgentIdsForSelection.value.filter(
    (item) => normalizeAgentId(item) !== normalizedDeletedAgentId
  );
  selectedAgentId.value = resolveAgentSelectionAfterRemoval({
    removedId: normalizedDeletedAgentId,
    previousIds: deletingAgentSelectionSnapshot.value,
    currentIds: currentIdsWithoutDeleted,
    fallbackId: DEFAULT_AGENT_KEY
  });
  const currentIdentity = activeConversation.value;
  const activeConversationAgentId = currentIdentity?.kind === 'agent'
    ? normalizeAgentId(currentIdentity.agentId || String(currentIdentity.id || '').replace(/^draft:/, ''))
    : '';
  if (activeConversationAgentId && activeConversationAgentId === normalizedDeletedAgentId) {
    sessionHub.clearActiveConversation();
  }
  if (normalizeAgentId(chatStore.draftAgentId) === normalizedDeletedAgentId) {
    chatStore.draftAgentId = '';
    chatStore.draftToolOverrides = null;
  }
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (activeSessionId) {
    const activeSession = (Array.isArray(chatStore.sessions) ? chatStore.sessions : []).find(
      (item) => String(item?.id || '') === activeSessionId
    );
    const activeSessionAgentId = normalizeAgentId(
      activeSession?.agent_id || (activeSession?.is_default === true ? DEFAULT_AGENT_KEY : '')
    );
    if (activeSessionAgentId && activeSessionAgentId === normalizedDeletedAgentId) {
      chatStore.activeSessionId = null;
      chatStore.messages = [];
    }
  }
  activeAgentDetailProfile.value = null;
  deletingAgentSelectionSnapshot.value = [];
  const tasks: Promise<unknown>[] = [
    refreshAgentMutationState(),
    chatStore.loadSessions({ skipTransportRefresh: true }),
    loadRunningAgents({ force: true }),
    loadAgentUserRounds(),
    loadChannelBoundAgentIds({ force: true }),
    loadDefaultAgentProfile(),
    loadAgentToolSummary({ force: true })
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds({ force: true }));
  }
  await Promise.allSettled(tasks);
  ensureSectionSelection();
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
    const visibleAgentIds = visibleAgentIdsForSelection.value;
    if (!visibleAgentIds.length) {
      selectedAgentId.value = DEFAULT_AGENT_KEY;
      return;
    }
    if (!visibleAgentIds.includes(normalizeAgentId(selectedAgentId.value))) {
      selectedAgentId.value = visibleAgentIds[0] || DEFAULT_AGENT_KEY;
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
      const outcome = await runStartNewSession();
      if (outcome !== 'noop') {
        const sessionId = String(chatStore.activeSessionId || '').trim();
        const replyText =
          outcome === 'already_current' ? t('chat.newSessionAlreadyCurrent') : t('chat.command.newSuccess');
        chatStore.appendLocalMessage('assistant', replyText, { sessionId });
      }
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
  chatStore.appendLocalMessage('user', rawText, { sessionId });
  try {
    await chatStore.compactSession(sessionId);
  } catch {}
  await scrollMessagesToBottom();
};

const sendAgentMessage = async (payload: { content?: string; attachments?: unknown[] }) => {
  if (isMessengerInteractionBlocked.value) {
    chatDebugLog('messenger.send', 'blocked-send-during-interaction-lock', buildActiveSessionBusyDebugSnapshot());
    return;
  }
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
      approvalMode: normalizeAgentApprovalMode(
        composerApprovalMode.value || activeAgentApprovalMode.value
      )
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
  if (isMessengerInteractionBlocked.value) {
    chatDebugLog('messenger.send', 'blocked-stop-during-interaction-lock', buildActiveSessionBusyDebugSnapshot());
    return;
  }
  chatDebugLog('messenger.send', 'manual-stop-click', buildActiveSessionBusyDebugSnapshot());
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

const helperAppsOfflineItems = computed<HelperAppOfflineItem[]>(() => {
  const items: HelperAppOfflineItem[] = [
    {
      key: 'local-file-search',
      title: t('userWorld.helperApps.localFileSearch.cardTitle'),
      description: t('userWorld.helperApps.localFileSearch.cardDesc'),
      icon: 'fa-folder-tree'
    }
  ];
  if (!desktopMode.value) {
    items.push({
      key: 'globe',
      title: t('userWorld.helperApps.globe.cardTitle'),
      description: t('userWorld.helperApps.globe.cardDesc'),
      icon: 'fa-globe'
    });
  }
  return items;
});

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
  if (
    helperAppsActiveKind.value === 'offline' &&
    helperAppsOfflineItems.value.some((item) => item.key === helperAppsActiveKey.value)
  ) {
    return;
  }
  if (
    helperAppsActiveKind.value === 'online' &&
    helperAppsOnlineItems.value.some((item) => item.linkId === helperAppsActiveKey.value)
  ) {
    return;
  }
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
  if (isMessengerInteractionBlocked.value) return;
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
  if (messengerSendKey.value === 'ctrl_enter') {
    if (hasPrimaryModifier || hasBackupModifier) {
      event.preventDefault();
      await sendWorldMessage();
    }
    return;
  }
  if (event.shiftKey) {
    return;
  }
  if (hasPrimaryModifier || hasBackupModifier) {
    event.preventDefault();
    await sendWorldMessage();
    return;
  }
  event.preventDefault();
  await sendWorldMessage();
};

function resolveReusableFreshAgentSessionId(
  targetAgentId: string,
  options: { activeOnly?: boolean } = {}
): string {
  return chatStore.resolveReusableFreshSessionId(targetAgentId, options);
}

async function openOrReuseFreshAgentSession(
  targetAgentId: string,
  options: { reuseScope?: 'any' | 'active_only' | 'none' } = {}
): Promise<string> {
  const reuseScope = options.reuseScope || 'any';
  const reusableSessionId =
    reuseScope === 'none'
      ? ''
      : resolveReusableFreshAgentSessionId(targetAgentId, {
          activeOnly: reuseScope === 'active_only'
        });
  if (reusableSessionId) {
    void chatStore.setMainSession(reusableSessionId).catch(() => null);
    return reusableSessionId;
  }
  const payloadAgentId = targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId;
  const session = await chatStore.createSession(payloadAgentId ? { agent_id: payloadAgentId } : {});
  const sessionId = String((session as Record<string, unknown> | null)?.id || '').trim();
  if (!sessionId) return '';
  return sessionId;
}

type StartNewSessionOutcome = 'noop' | 'already_current' | 'opened';

async function runStartNewSession(options: { notify?: boolean } = {}): Promise<StartNewSessionOutcome> {
  if (!isAgentConversationActive.value || creatingAgentSession.value || isMessengerInteractionBlocked.value) {
    return 'noop';
  }
  const targetAgent = normalizeAgentId(activeAgentId.value || selectedAgentId.value);
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  const reusableSessionId = resolveReusableFreshAgentSessionId(targetAgent, {
    activeOnly: true
  });
  if (activeSessionId && reusableSessionId && activeSessionId === reusableSessionId) {
    if (options.notify === true) {
      ElMessage.info(t('chat.newSessionAlreadyCurrent'));
    }
    return 'already_current';
  }
  const runResult = await runWithMessengerInteractionBlock('new_session', async () => {
    creatingAgentSession.value = true;
    try {
      const sessionId = await openOrReuseFreshAgentSession(targetAgent, {
        reuseScope: 'active_only'
      });
      if (!sessionId) return 'noop';
      if (options.notify === true) {
        ElMessage.success(t('chat.newSessionOpened'));
      }
      // Keep "new thread" action responsive; detail hydration continues in background.
      void openAgentSession(sessionId, targetAgent);
      return 'opened';
    } finally {
      creatingAgentSession.value = false;
    }
  });
  return runResult || 'noop';
}

async function startNewSession() {
  try {
    await runStartNewSession({ notify: true });
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
}

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

const shouldReuseAgentMetaResult = (loadedAt: number, force = false): boolean =>
  !force && loadedAt > 0 && Date.now() - loadedAt < AGENT_META_REQUEST_CACHE_MS;

const loadRunningAgents = async (options: { force?: boolean } = {}) => {
  const force = options.force === true;
  if (!force && runningAgentsLoadPromise) {
    return runningAgentsLoadPromise;
  }
  if (shouldReuseAgentMetaResult(runningAgentsLoadedAt, force)) {
    return;
  }
  // Ignore stale responses when multiple refreshes race (manual refresh + pulse tick).
  const loadVersion = ++runningAgentsLoadVersion;
  const request = (async () => {
    try {
      const response = await listRunningAgents();
      if (loadVersion !== runningAgentsLoadVersion) {
        return;
      }
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
      runningAgentsLoadedAt = Date.now();
    } catch (error) {
      if (loadVersion !== runningAgentsLoadVersion) {
        return;
      }
      const status = resolveHttpStatus(error);
      if (isAuthDeniedStatus(status)) {
        agentRuntimeStateMap.value = new Map<string, AgentRuntimeState>();
        agentRuntimeStateSnapshot = new Map<string, AgentRuntimeState>();
        agentRuntimeStateHydrated = false;
      }
    }
  })().finally(() => {
    runningAgentsLoadPromise = null;
  });
  runningAgentsLoadPromise = request;
  return request;
};

const loadAgentUserRounds = async () => {
  const loadVersion = ++agentUserRoundsLoadVersion;
  try {
    const response = await listAgentUserRounds();
    if (loadVersion !== agentUserRoundsLoadVersion) {
      return;
    }
    const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
    const roundsMap = new Map<string, number>();
    items.forEach((item: Record<string, unknown>) => {
      const key = normalizeAgentUserRoundsKey(item?.agent_id);
      const raw = Number(item?.user_rounds ?? item?.rounds ?? 0);
      const value = Number.isFinite(raw) ? Math.max(0, Math.floor(raw)) : 0;
      roundsMap.set(key, value);
    });
    agentUserRoundsMap.value = roundsMap;
  } catch (error) {
    if (loadVersion !== agentUserRoundsLoadVersion) {
      return;
    }
    const status = resolveHttpStatus(error);
    if (isAuthDeniedStatus(status)) {
      agentUserRoundsMap.value = new Map<string, number>();
    }
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
  void loadCronAgentIds({ force: true });
};

const loadCronAgentIds = async (options: { force?: boolean } = {}) => {
  const force = options.force === true;
  if (!force && cronAgentIdsLoadPromise) {
    return cronAgentIdsLoadPromise;
  }
  if (shouldReuseAgentMetaResult(cronAgentIdsLoadedAt, force)) {
    return;
  }
  const loadVersion = ++cronAgentIdsLoadVersion;
  if (cronPermissionDenied.value) {
    if (loadVersion === cronAgentIdsLoadVersion) {
      cronAgentIds.value = new Set<string>();
    }
    return;
  }
  const request = (async () => {
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
      cronAgentIdsLoadedAt = Date.now();
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
  })().finally(() => {
    cronAgentIdsLoadPromise = null;
  });
  cronAgentIdsLoadPromise = request;
  return request;
};

const loadChannelBoundAgentIds = async (options: { force?: boolean } = {}) => {
  const force = options.force === true;
  if (!force && channelBoundAgentIdsLoadPromise) {
    return channelBoundAgentIdsLoadPromise;
  }
  if (shouldReuseAgentMetaResult(channelBoundAgentIdsLoadedAt, force)) {
    return;
  }
  const loadVersion = ++channelBoundAgentIdsLoadVersion;
  const request = (async () => {
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
      channelBoundAgentIdsLoadedAt = Date.now();
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
  })().finally(() => {
    channelBoundAgentIdsLoadPromise = null;
  });
  channelBoundAgentIdsLoadPromise = request;
  return request;
};

const refreshRealtimeChatSessions = async () => {
  await chatStore.loadSessions({
    // Realtime pulse only needs session list delta; skip transport profile requests on every tick.
    skipTransportRefresh: true
  });
};

const REALTIME_CONTACT_REFRESH_MIN_MS = 7_000;

const refreshRealtimeContacts = async () => {
  const lastRefreshedAt = Number(userWorldStore.lastContactRealtimeRefreshAt || 0);
  if (lastRefreshedAt > 0 && Date.now() - lastRefreshedAt < REALTIME_CONTACT_REFRESH_MIN_MS) {
    return;
  }
  await userWorldStore.refreshContacts('', {
    shouldApply: () =>
      sessionHub.activeSection === 'users' || sessionHub.activeSection === 'messages'
  });
};

const shouldRefreshRealtimeChatSessions = () => sessionHub.activeSection === 'messages';
const shouldRefreshAgentMeta = () =>
  sessionHub.activeSection === 'agents' || sessionHub.activeSection === 'tools';

const refreshAll = async () => {
  const tasks: Promise<unknown>[] = [
    agentStore.loadAgents(),
    beeroomStore.loadGroups(),
    chatStore.loadSessions(),
    userWorldStore.bootstrap(true),
    loadOrgUnits(),
    loadRunningAgents({ force: true }),
    loadAgentUserRounds(),
    loadToolsCatalog(),
    loadChannelBoundAgentIds({ force: true })
  ];
  if (!cronPermissionDenied.value) {
    tasks.push(loadCronAgentIds({ force: true }));
  }
  await Promise.allSettled(tasks);
  ensureSectionSelection();
  ElMessage.success(t('common.refreshSuccess'));
};

const syncMessageVirtualMetrics = () => {
  messageViewportRuntime?.syncMessageVirtualMetrics();
};

const pruneMessageVirtualHeightCache = () => {
  messageViewportRuntime?.pruneMessageVirtualHeightCache();
};

const scheduleMessageViewportRefresh = (
  options: { updateScrollState?: boolean; measure?: boolean; measureKeys?: string[] } = {}
) => {
  messageViewportRuntime?.scheduleMessageViewportRefresh(options);
};

const scheduleMessageVirtualMeasure = (measureKeys?: string[]) => {
  messageViewportRuntime?.scheduleMessageVirtualMeasure(measureKeys);
};

const handleMessageWorkflowLayoutChange = (messageKey?: string) => {
  messageViewportRuntime?.handleWorkflowLayoutChange(messageKey);
  if (
    autoStickToBottom.value &&
    messageKey &&
    String(messageKey).trim() === latestAgentRenderableMessageKey.value
  ) {
    void scrollMessagesToBottom();
  }
};

const updateMessageScrollState = () => {
  messageViewportRuntime?.updateMessageScrollState();
};

const handleMessageListScroll = () => {
  messageViewportRuntime?.handleMessageListScroll();
};

const scrollMessagesToBottom = async (force = false) => {
  return messageViewportRuntime?.scrollMessagesToBottom(force) ?? Promise.resolve();
};

const jumpToMessageBottom = async () => {
  return messageViewportRuntime?.jumpToMessageBottom() ?? Promise.resolve();
};

const jumpToMessageTop = async () => {
  return messageViewportRuntime?.jumpToMessageTop() ?? Promise.resolve();
};

const scrollVirtualMessageToIndex = (keys: string[], index: number, align: 'center' | 'start' = 'center') => {
  messageViewportRuntime?.scrollVirtualMessageToIndex(keys, index, align);
};

const scrollLatestAssistantToCenter = async () => {
  return messageViewportRuntime?.scrollLatestAssistantToCenter() ?? Promise.resolve();
};

const refreshLatestAssistantMessageLayout = (reason: string) => {
  if (!isAgentConversationActive.value) {
    return;
  }
  const latestMessage = chatStore.messages[chatStore.messages.length - 1] as
    | Record<string, unknown>
    | undefined;
  if (!latestMessage || String(latestMessage.role || '') !== 'assistant') {
    return;
  }
  const latestMessageKey = latestAgentRenderableMessageKey.value;
  if (isChatDebugEnabled()) {
    const workflowItems = Array.isArray(latestMessage.workflowItems)
      ? (latestMessage.workflowItems as unknown[])
      : [];
    chatDebugLog('messenger.viewport', 'latest-assistant-layout-refresh', {
      reason,
      activeSessionId: chatStore.activeSessionId,
      messageKey: latestMessageKey,
      shouldVirtualize: shouldVirtualizeMessages.value,
      autoStickToBottom: autoStickToBottom.value,
      workflowItemCount: workflowItems.length,
      workflowStreaming: Boolean(latestMessage.workflowStreaming),
      reasoningStreaming: Boolean(latestMessage.reasoningStreaming),
      streamIncomplete: Boolean(latestMessage.stream_incomplete),
      contentLength: String(latestMessage.content || '').length,
      reasoningLength: String(latestMessage.reasoning || '').length
    });
  }
  void nextTick(() => {
    scheduleMessageViewportRefresh({
      updateScrollState: true,
      measure: true,
      measureKeys: latestMessageKey ? [latestMessageKey] : undefined
    });
    if (autoStickToBottom.value) {
      void scrollMessagesToBottom();
    }
  });
};

messageViewportRuntime = createMessageViewportRuntime({
  messageListRef,
  showChatSettingsView,
  autoStickToBottom,
  showScrollTopButton,
  showScrollBottomButton,
  isAgentConversationActive,
  isWorldConversationActive,
  shouldVirtualizeMessages,
  agentRenderableMessages,
  worldRenderableMessages,
  messageVirtualHeightCache,
  messageVirtualLayoutVersion,
  messageVirtualScrollTop,
  messageVirtualViewportHeight,
  estimateVirtualOffsetTop,
  resolveVirtualMessageHeight
});

function normalizeAgentId(value: unknown): string {
  const text = String(value || '').trim();
  return text || DEFAULT_AGENT_KEY;
}

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
  const initialQuerySessionId = String(route.query.session_id || '').trim();
  const initialQueryConversationId = String(route.query.conversation_id || '').trim();
  const initialQueryAgentId = String(route.query.agent_id || '').trim();
  const initialQueryEntry = String(route.query.entry || '').trim().toLowerCase();
  const shouldPrioritizeWorldBootstrap =
    initialSection === 'messages' &&
    Boolean(initialQueryConversationId);
  const { critical, background } = splitMessengerBootstrapTasks(initialSection, [
    {
      sections: ['messages', 'agents', 'files', 'swarms'],
      run: () => agentStore.loadAgents()
    },
    {
      critical: true,
      sections: ['swarms'],
      run: () => beeroomStore.loadGroups()
    },
    {
      sections: ['messages'],
      run: () => chatStore.loadSessions()
    },
    {
      sections: shouldPrioritizeWorldBootstrap ? ['messages', 'users', 'groups'] : ['users', 'groups'],
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
    }
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
  () => [isEmbeddedChatRoute.value, isMiddlePaneOverlay.value, showMiddlePane.value] as const,
  ([embedded, overlay, visible]) => {
    if (embedded) {
      clearMiddlePanePrewarm();
      middlePaneMounted.value = false;
      return;
    }
    if (visible || !overlay) {
      clearMiddlePanePrewarm();
      middlePaneMounted.value = true;
      return;
    }
    scheduleMiddlePanePrewarm();
  },
  { immediate: true }
);

watch(
  () => isMiddlePaneOverlay.value,
  (overlay) => {
    if (!overlay) {
      clearMiddlePaneOverlayHide();
      middlePaneOverlayVisible.value = false;
      clearMiddlePaneOverlayPreview();
    }
  },
  { immediate: true }
);

watch(
  () => middlePaneOverlayVisible.value,
  (visible) => {
    if (visible) {
      middlePaneMounted.value = true;
      return;
    }
    if (!visible) {
      clearMiddlePaneOverlayPreview();
    }
  }
);

const syncRouteDrivenMessengerViewState = () => {
  settingsPanelMode.value = resolveRouteSettingsPanelMode(
    route.path,
    route.query.panel,
    desktopMode.value
  );
  const sectionHint = String(route.query.section || '').trim().toLowerCase();
  const helperWorkspaceEnabled = resolveRouteHelperWorkspaceEnabled(
    route.query.section,
    route.query.helper
  );
  helperAppsWorkspaceMode.value = helperWorkspaceEnabled;
  if (helperWorkspaceEnabled) {
    ensureHelperAppsSelection();
    void loadHelperExternalApps();
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
};

syncRouteDrivenMessengerViewState();

watch(
  () => [route.path, route.query.section, route.query.panel, route.query.helper],
  syncRouteDrivenMessengerViewState
);

watch(
  () => currentUserId.value,
  (value, previousValue) => {
    const changed = String(value || '') !== String(previousValue || '');
    const shouldClearConversationState = changed && currentUserContextInitialized && !bootLoading.value;
    currentUserContextInitialized = true;
    if (changed) {
      chatStore.resetState();
    }
    if (shouldClearConversationState) {
      sessionHub.clearActiveConversation();
      userWorldStore.activeConversationId = '';
      const nextQuery = { ...route.query } as Record<string, any>;
      delete nextQuery.session_id;
      delete nextQuery.conversation_id;
      router.replace({ path: route.path, query: nextQuery }).catch(() => undefined);
    }
    beeroomStore.resetState();
    beeroomGroupsLastRefreshAt = 0;
    selectedAgentHiveGroupId.value = '';
    void hydrateCurrentUserAppearance();
    cronPermissionDenied.value = false;
    cronAgentIds.value = new Set<string>();
    timelineDialogVisible.value = false;
    skillDockUploading.value = false;
    agentPromptToolSummary.value = null;
    agentToolSummaryLoading.value = false;
    rightDockSkillCatalog.value = [];
    rightDockSkillDialogVisible.value = false;
    rightDockSelectedSkillName.value = '';
    rightDockSkillContent.value = '';
    rightDockSkillContentPath.value = '';
    rightDockSkillCatalogLoading.value = false;
    rightDockSkillContentLoading.value = false;
    rightDockSkillToggleSaving.value = false;
    clearRightDockSkillAutoRetry();
    rightDockSkillCatalogLoadVersion += 1;
    rightDockSkillContentLoadVersion += 1;
    agentToolSummaryPromise = null;
    invalidateAllUserToolsCaches();
    clearWorkspaceResourceCache();
    ensureDismissedAgentConversationState(true);
    ensureAgentUnreadState(true);
    refreshAgentMainUnreadFromSessions();
    warmMessengerUserToolsData({
      catalog: sessionHub.activeSection === 'agents' || sessionHub.activeSection === 'tools',
      skills: showAgentRightDock.value,
      summary: sessionHub.activeSection === 'agents' || showAgentRightDock.value
    });
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
  () => [themeStore.palette],
  () => {
    if (appearanceHydrating.value) return;
    void persistCurrentUserAppearance();
  }
);

watch(
  () => sessionHub.activeSection,
  (section) => {
    closeFileContainerMenu();
    if (!isSearchableMiddlePaneSection(section) && (keywordInput.value || sessionHub.keyword)) {
      clearKeywordDebounce();
      keywordInput.value = '';
      sessionHub.setKeyword('');
    }
    if (section === 'swarms') {
      stopRealtimePulse?.();
      beeroomGroupsLastRefreshAt = 0;
      startBeeroomRealtimeSync?.();
      triggerBeeroomRealtimeSyncRefresh?.('enter-swarms');
    } else {
      stopBeeroomRealtimeSync?.();
      startRealtimePulse?.();
      triggerRealtimePulseRefresh?.(`enter-${section}`);
    }
    if (
      section === 'tools' &&
      !builtinTools.value.length &&
      !mcpTools.value.length &&
      !skillTools.value.length &&
      !knowledgeTools.value.length
    ) {
      loadToolsCatalog();
    }
    if (section === 'agents') {
      warmMessengerUserToolsData({
        catalog: true,
        summary: true
      });
      void loadChannelBoundAgentIds();
      if (!cronPermissionDenied.value) {
        void loadCronAgentIds();
      }
    }
    if (section === 'tools') {
      void loadChannelBoundAgentIds();
      if (!cronPermissionDenied.value) {
        void loadCronAgentIds();
      }
    }
    if (section === 'more') {
      void preloadMessengerSettingsPanels({ desktopMode: desktopMode.value });
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
  () => hasHotRuntimeState.value,
  (hot) => {
    if (!hot) return;
    if (sessionHub.activeSection === 'swarms') {
      triggerBeeroomRealtimeSyncRefresh?.('hot-runtime');
      return;
    }
    triggerRealtimePulseRefresh?.('hot-runtime');
  }
);

watch(
  () => hasHotBeeroomRuntimeState.value,
  (hot) => {
    if (!hot || sessionHub.activeSection !== 'swarms') return;
    triggerBeeroomRealtimeSyncRefresh?.('hot-beeroom');
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

watch(agentHiveTreeRows, (rows) => {
  if (!selectedAgentHiveGroupId.value) return;
  const exists = rows.some((row) => row.id === selectedAgentHiveGroupId.value);
  if (!exists) {
    selectedAgentHiveGroupId.value = '';
  }
});

watch(visibleAgentIdsForSelection, () => {
  if (sessionHub.activeSection !== 'agents') return;
  ensureSectionSelection();
});

watch(
  () => filteredGroups.value.map((item) => String(item?.group_id || '')).join('|'),
  () => {
    if (sessionHub.activeSection !== 'groups') return;
    ensureSectionSelection();
  }
);

watch(
  () =>
    filteredBeeroomGroups.value
      .map((item) => String(item?.group_id || item?.hive_id || ''))
      .join('|'),
  () => {
    if (sessionHub.activeSection !== 'swarms') return;
    ensureSectionSelection();
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
  () => [
    filteredContacts.value.length,
    filteredGroups.value.length,
    filteredOwnedAgents.value.length,
    filteredSharedAgents.value.length,
    showDefaultAgentEntry.value ? 1 : 0
  ],
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
  () => [timelineDialogVisible.value, rightPanelSessionHistory.value.map((item) => item.id).join('|')] as const,
  ([visible, value]) => {
    if (!visible || !value) {
      if (typeof window !== 'undefined' && timelinePrefetchTimer) {
        window.clearTimeout(timelinePrefetchTimer);
        timelinePrefetchTimer = null;
      }
      return;
    }
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
  () =>
    [
      sessionHub.activeSection,
      filteredMixedConversations.value
        .filter((item) => item.kind === 'agent')
        .slice(0, 4)
        .map((item) => `${item.agentId}:${String(item.sourceId || '').trim()}`)
        .join('|')
    ] as const,
  ([section, key]) => {
    if (section !== 'messages' || !key) {
      return;
    }
    filteredMixedConversations.value
      .filter((item) => item.kind === 'agent')
      .slice(0, 4)
      .forEach((item) => {
        preloadMixedConversation(item);
      });
  },
  { immediate: true }
);

watch(
  () => rightDockSkillDialogVisible.value,
  (visible) => {
    if (visible) return;
    rightDockSkillContentLoadVersion += 1;
    rightDockSkillContentLoading.value = false;
    rightDockSkillToggleSaving.value = false;
    rightDockSkillContent.value = '';
    rightDockSkillContentPath.value = '';
  }
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
    scheduleMessageViewportRefresh({
      updateScrollState: true,
      measure: true
    });
  }
);

watch(
  () => [chatStore.messages.length, userWorldStore.activeMessages.length, sessionHub.activeConversationKey],
  () => {
    pruneMessageVirtualHeightCache();
    void nextTick(() => {
      scheduleMessageViewportRefresh({
        measure: true
      });
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
  () => {
    const latestMessage = chatStore.messages[chatStore.messages.length - 1] as
      | Record<string, unknown>
      | undefined;
    return [
      chatStore.activeSessionId,
      latestAgentRenderableMessageKey.value,
      buildLatestAssistantLayoutSignature(latestMessage)
    ].join('::');
  },
  () => {
    scheduleWorkspaceResourceHydration();
    refreshLatestAssistantMessageLayout('latest-assistant-signature');
  },
  { flush: 'post' }
);

watch(
  () => userWorldStore.activeMessages[userWorldStore.activeMessages.length - 1]?.content,
  () => {
    scheduleWorkspaceResourceHydration();
    const latestMessageKey = latestWorldRenderableMessageKey.value;
    scheduleMessageViewportRefresh({
      measure: true,
      measureKeys: latestMessageKey ? [latestMessageKey] : undefined
    });
  }
);

watch(
  () => [agentRenderableMessages.value.length, worldRenderableMessages.value.length],
  () => {
    pruneMessageVirtualHeightCache();
    void nextTick(() => {
      scheduleMessageViewportRefresh({
        measure: true
      });
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
  () =>
    [
      isAgentConversationActive.value,
      desktopMode.value,
      activeAgentId.value,
      String(chatStore.activeSessionId || '').trim(),
      showChatSettingsView.value
    ] as const,
  ([active, desktop, _agentId, _sessionId, showingSettings], previous) => {
    if (!active) {
      void cancelAgentVoiceRecording();
      return;
    }
    const forceRefresh = Boolean(previous?.[4] && !showingSettings);
    if (desktop) {
      void readDesktopDefaultModelMeta(forceRefresh);
      return;
    }
    void readServerDefaultModelName(forceRefresh);
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
      if (viewportResizeFrame !== null) {
        return;
      }
      viewportResizeFrame = window.requestAnimationFrame(() => {
        viewportResizeFrame = null;
        refreshHostWidth();
        closeFileContainerMenu();
        syncContactVirtualMetrics();
        scheduleMessageViewportRefresh({
          updateScrollState: true,
          measure: true
        });
      });
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
  scheduleMessageViewportRefresh({
    updateScrollState: true,
    measure: true
  });
  scheduleWorkspaceResourceHydration();
  warmMessengerUserToolsData({
    catalog: sessionHub.activeSection === 'agents' || sessionHub.activeSection === 'tools',
    skills: showAgentRightDock.value,
    summary: sessionHub.activeSection === 'agents' || showAgentRightDock.value
  });
  stopWorkspaceRefreshListener = onWorkspaceRefresh(handleWorkspaceResourceRefresh);
  stopUserToolsUpdatedListener = onUserToolsUpdated(handleUserToolsUpdatedEvent);
  lifecycleTimer = window.setInterval(() => {
    fileLifecycleNowTick.value = Date.now();
  }, 60_000);
  const realtimePulse = createMessengerRealtimePulse({
    refreshRunningAgents: loadRunningAgents,
    refreshCronAgentIds: loadCronAgentIds,
    refreshChannelBoundAgentIds: loadChannelBoundAgentIds,
    refreshChatSessions: refreshRealtimeChatSessions,
    refreshContacts: refreshRealtimeContacts,
    isHotState: () => hasHotRuntimeState.value,
    shouldRefreshCron: () => !cronPermissionDenied.value,
    shouldRefreshChannelBoundAgentIds: shouldRefreshAgentMeta,
    shouldRefreshChatSessions: shouldRefreshRealtimeChatSessions,
    shouldRefreshContacts: () =>
      !userWorldPermissionDenied.value &&
      (sessionHub.activeSection === 'users' || sessionHub.activeSection === 'messages')
  });
  const beeroomRealtimeSync = createBeeroomRealtimeSync({
    refreshBeeroomGroups: refreshBeeroomRealtimeGroups,
    refreshBeeroomActiveGroup: refreshBeeroomRealtimeActiveGroup,
    isHotState: () => hasHotBeeroomRuntimeState.value,
    shouldSync: () => sessionHub.activeSection === 'swarms',
    refreshRunningAgents: loadRunningAgents
  });
  startRealtimePulse = () => realtimePulse.start();
  stopRealtimePulse = () => realtimePulse.stop();
  triggerRealtimePulseRefresh = (reason = '') => realtimePulse.trigger(reason);
  startBeeroomRealtimeSync = () => beeroomRealtimeSync.start();
  stopBeeroomRealtimeSync = () => beeroomRealtimeSync.stop();
  triggerBeeroomRealtimeSyncRefresh = (reason = '') => beeroomRealtimeSync.trigger(reason);
  if (sessionHub.activeSection === 'swarms') {
    beeroomRealtimeSync.start();
  } else {
    realtimePulse.start();
  }
});

onBeforeUnmount(() => {
  sectionRouteSyncToken += 1;
  if (typeof window !== 'undefined') {
    if (viewportResizeHandler) {
      window.removeEventListener('resize', viewportResizeHandler);
      viewportResizeHandler = null;
    }
    if (viewportResizeFrame !== null) {
      window.cancelAnimationFrame(viewportResizeFrame);
      viewportResizeFrame = null;
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
  clearRightDockSkillAutoRetry();
  closeFileContainerMenu();
  clearWorldQuickPanelClose();
  clearMiddlePaneOverlayHide();
  clearMiddlePanePrewarm();
  if (typeof window !== 'undefined' && rightDockEdgeHoverFrame !== null) {
    window.cancelAnimationFrame(rightDockEdgeHoverFrame);
    rightDockEdgeHoverFrame = null;
  }
  pendingRightDockPointerX = null;
  clearKeywordDebounce();
  closeImagePreview();
  stopWorldComposerResize();
  void cancelAgentVoiceRecording();
  void cancelWorldVoiceRecording();
  disposeWorldVoicePlayback();
  messageViewportRuntime?.dispose();
  if (typeof window !== 'undefined' && contactVirtualFrame !== null) {
    window.cancelAnimationFrame(contactVirtualFrame);
    contactVirtualFrame = null;
  }
  stopRealtimePulse?.();
  stopBeeroomRealtimeSync?.();
  startRealtimePulse = null;
  stopRealtimePulse = null;
  triggerRealtimePulseRefresh = null;
  startBeeroomRealtimeSync = null;
  stopBeeroomRealtimeSync = null;
  triggerBeeroomRealtimeSyncRefresh = null;
  if (lifecycleTimer) {
    window.clearInterval(lifecycleTimer);
    lifecycleTimer = null;
  }
  if (typeof window !== 'undefined' && timelinePrefetchTimer) {
    window.clearTimeout(timelinePrefetchTimer);
    timelinePrefetchTimer = null;
  }
  if (typeof window !== 'undefined' && sessionDetailPrefetchTimer !== null) {
    window.clearTimeout(sessionDetailPrefetchTimer);
    sessionDetailPrefetchTimer = null;
  }
  queuedSessionDetailPrefetchIds.clear();
  markdownCache.clear();
  messageVirtualHeightCache.clear();
  if (stopWorkspaceRefreshListener) {
    stopWorkspaceRefreshListener();
    stopWorkspaceRefreshListener = null;
  }
  if (stopUserToolsUpdatedListener) {
    stopUserToolsUpdatedListener();
    stopUserToolsUpdatedListener = null;
  }
  clearWorkspaceResourceCache();
  timelinePreviewMap.value.clear();
  timelinePreviewLoadingSet.value.clear();
  userWorldStore.stopAllWatchers();
});
</script>
