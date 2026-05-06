<template>
  <div
    ref="messengerRootRef"
    class="messenger-view"
    :class="{
      'messenger-view--embedded-chat': isEmbeddedChatRoute,
      'messenger-view--without-right': !showRightDock,
      'messenger-view--without-middle': !showMiddlePane,
      'messenger-view--right-collapsed': showRightDock && rightDockCollapsed,
      'messenger-view--right-resizing': isRightDockResizing,
      'messenger-view--nav-collapsed': navigationPaneCollapsed,
      'messenger-view--host-medium': isRightDockOverlay,
      'messenger-view--host-small': isMiddlePaneOverlay,
      'messenger-view--host-tight': viewportWidth <= MESSENGER_TIGHT_HOST_BREAKPOINT,
      'messenger-view--action-blocked': isMessengerInteractionBlocked
    }"
    :style="messengerViewStyle"
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
            <span class="messenger-left-nav-btn-label">{{ resolveLeftNavButtonLabel(item.key) }}</span>
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
          :selected-contact-unit-id="selectedContactUnitId"
          @update:selected-contact-unit-id="handleMiddlePaneContactUnitIdUpdate"
          :selected-agent-hive-group-id="selectedAgentHiveGroupId"
          @update:selected-agent-hive-group-id="handleMiddlePaneAgentHiveGroupIdUpdate"
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
          :select-helper-app="selectHelperAppFromMiddlePane"
          :resolve-external-icon="resolveExternalIcon"
          :resolve-external-icon-style="resolveExternalIconStyle"
          :resolve-external-host="resolveExternalHost"
          :filtered-mixed-conversations="filteredMixedConversations"
          :is-agent-orchestration-active="isAgentOrchestrationActive"
          :is-agent-goal-active="isAgentGoalActive"
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
          :select-contact="selectContactFromMiddlePane"
          :open-contact-conversation-from-list="openContactConversationFromList"
          :is-contact-online="isContactOnline"
          :format-contact-presence="formatContactPresence"
          :resolve-unread="resolveUnread"
          :filtered-beeroom-groups="filteredBeeroomGroupsOrdered"
          :selected-beeroom-group-id="beeroomStore.activeGroupId"
          :select-beeroom-group="selectBeeroomGroupFromMiddlePane"
          :delete-beeroom-group="handleDeleteBeeroomGroup"
          :selected-plaza-browse-kind="plazaBrowseKind"
          :select-plaza-browse-kind="selectPlazaBrowseKindFromMiddlePane"
          :filtered-groups="filteredGroups"
          :selected-group-id="selectedGroupId"
          :select-group="selectGroupFromMiddlePane"
          :agent-hive-total-count="agentHiveTotalCount"
          :agent-hive-tree-rows="agentHiveTreeRows"
          :owned-agents="ownedAgents"
          :primary-agent-items="orderedPrimaryAgents"
          :filtered-owned-agents="filteredOwnedAgentsOrdered"
          :filtered-shared-agents="filteredSharedAgentsOrdered"
          :show-default-agent-entry="showDefaultAgentEntry"
          :selected-agent-id="selectedAgentId"
          :default-agent-key="DEFAULT_AGENT_KEY"
          :default-agent-icon="defaultAgentProfile?.icon"
          :select-agent-for-settings="selectAgentForSettingsFromMiddlePane"
          :open-agent-by-id="openAgentById"
          :preload-agent-by-id="preloadAgentById"
          :normalize-agent-id="normalizeAgentId"
          :selected-tool-entry-key="selectedToolEntryKey"
          :select-tool-category="selectToolCategoryFromMiddlePane"
          :desktop-local-mode="desktopLocalMode"
          :file-scope="fileScope"
          :selected-file-container-id="selectedFileContainerId"
          :user-container-id="USER_CONTAINER_ID"
          :select-container="selectContainerFromMiddlePane"
          :open-file-container-menu="openFileContainerMenu"
          :bound-agent-file-containers="boundAgentFileContainers"
          :unbound-agent-file-containers="unboundAgentFileContainers"
          :desktop-mode="desktopMode"
          :current-username="currentUsername"
          :settings-logout-disabled="settingsLogoutDisabled"
          :handle-settings-logout="handleSettingsLogout"
          :move-message-item="moveMixedConversationItem"
          :move-agent-item="moveAgentListItem"
          :move-swarm-item="moveBeeroomGroupItem"
          :after-hive-pack-imported="handleHivePackImportedFromMiddlePane"
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
        v-if="showMessengerChatHeader"
        class="messenger-chat-header"
      >
        <div class="messenger-chat-heading">
          <div class="messenger-chat-title-row">
            <div class="messenger-chat-title">{{ chatPanelTitle }}</div>
          </div>
        </div>
        <div class="messenger-chat-header-actions">
          <button
            v-if="
              showChatSettingsView &&
              sessionHub.activeSection === 'agents' &&
              !showAgentGridOverview &&
              agentSettingMode === 'agent'
            "
            class="messenger-header-action-text messenger-header-action-text--danger"
            type="button"
            :disabled="!canDeleteSettingsAgent"
            @click="triggerAgentSettingsDelete"
          >
            <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
            <span>{{ t('portal.agent.delete') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'plaza'"
            class="messenger-header-action-text"
            type="button"
            @click="triggerPlazaPublish"
          >
            <i class="fa-solid fa-arrow-up-from-bracket" aria-hidden="true"></i>
            <span>{{ t('plaza.action.publish') }}</span>
          </button>
          <button
            v-if="showChatSettingsView && sessionHub.activeSection === 'plaza'"
            class="messenger-header-action-text"
            type="button"
            @click="triggerPlazaRefresh"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ t('common.refresh') }}</span>
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
            <span>{{ t('messenger.agent.openChat') }}</span>
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
            :class="{ 'is-orchestration-disabled': activeSessionOrchestrationLocked || activeSessionGoalLocked }"
            type="button"
            :disabled="creatingAgentSession || isMessengerInteractionBlocked || activeMessengerSessionBusy || activeSessionOrchestrationLocked || activeSessionGoalLocked"
            :title="t('chat.newConversation')"
            :aria-label="t('chat.newConversation')"
            @click="startNewSession"
          >
            <i class="fa-solid fa-plus" aria-hidden="true"></i>
            {{ t('chat.newConversation') }}
          </button>
          <button
            v-if="!showChatSettingsView && isAgentConversationActive"
            class="messenger-header-btn"
            :class="{ 'is-orchestration-disabled': activeSessionOrchestrationLocked || activeSessionGoalLocked }"
            type="button"
            :disabled="activeSessionGoalLocked"
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
          'is-beeroom': ['swarms', 'orchestrations'].includes(sessionHub.activeSection),
          'is-beeroom-canvas': ['swarms', 'orchestrations'].includes(sessionHub.activeSection),
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
              :active="sessionHub.activeSection === 'swarms'"
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

        <template v-else-if="sessionHub.activeSection === 'orchestrations'">
          <div
            class="messenger-chat-settings messenger-chat-settings--beeroom messenger-chat-settings--beeroom-canvas"
          >
            <OrchestrationWorkbench
              :group="selectedBeeroomGroup"
              :agents="beeroomStore.activeAgents"
              :missions="beeroomStore.activeMissions"
              :active="sessionHub.activeSection === 'orchestrations'"
              :loading="beeroomStore.detailLoading || beeroomStore.loading"
              :refreshing="beeroomStore.refreshing"
              :error="beeroomStore.error"
              @refresh="refreshActiveOrchestration"
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
                        <span
                          class="messenger-agent-grid-metric"
                          :title="t('toolManager.system.skills')"
                        >
                          <i class="fa-solid fa-book" aria-hidden="true"></i>
                          <span>{{ card.skillCount }}</span>
                        </span>
                        <span
                          class="messenger-agent-grid-metric"
                          :title="t('toolManager.system.mcp')"
                        >
                          <i class="fa-solid fa-plug" aria-hidden="true"></i>
                          <span>{{ card.mcpCount }}</span>
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

            <MessengerHivePlazaPanel
              v-else-if="sessionHub.activeSection === 'plaza'"
              ref="messengerHivePlazaPanelRef"
              :active="sessionHub.activeSection === 'plaza'"
              :items="filteredPlazaItems"
              :browse-kind="plazaBrowseKind"
              :selected-item-id="selectedPlazaItemId"
              :current-user-id="currentUserId"
              @update:selected-item-id="selectPlazaItem"
            />

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
                    settingsPanelMode === 'desktop-lan')
                "
                class="messenger-chat-settings-scroll messenger-chat-settings-scroll--desktop-system"
              >
                <DesktopSystemSettingsPanel
                  :panel="
                    settingsPanelMode === 'desktop-lan'
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
                    :session-busy="activeMessengerSessionBusy"
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
                  :session-busy="activeMessengerSessionBusy"
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
                    :items="Array.isArray(item.message.workflowItems) ? item.message.workflowItems : []"
                    :loading="Boolean(item.message.workflowStreaming)"
                    :render-version="buildMessageWorkflowRenderVersion(item.message)"
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
                        <i
                          :class="[
                            entry.iconClass || 'fa-solid fa-circle-info',
                            'messenger-message-stat-icon'
                          ]"
                          aria-hidden="true"
                        ></i>
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
            :class="{ 'messenger-agent-composer-lock': activeSessionOrchestrationLocked || activeSessionGoalLocked }"
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
            :context-total-tokens="activeAgentUsingDesktopDefaultModel ? desktopDefaultModelMaxContext : null"
            :goal-locked="activeSessionGoalLocked"
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

    <div
      v-if="showRightDock && rightDockResizable"
      class="messenger-right-dock-resizer"
      role="separator"
      aria-orientation="vertical"
      :aria-label="t('messenger.right.resize')"
      tabindex="0"
      @pointerdown="startRightDockResize"
      @dblclick.prevent="resetRightDockWidth"
      @keydown.left.prevent="nudgeRightDockWidth(-24)"
      @keydown.right.prevent="nudgeRightDockWidth(24)"
    ></div>

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
      :timeline-readonly="activeSessionOrchestrationLocked || activeSessionGoalLocked"
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
      top="5vh"
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
      :copy-from-agents="quickCreateCopyFromAgents"
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
    <CompanionFloatingLayer :desktop-mode="desktopMode" />
  </div>
</template>

<script setup lang="ts">
import CompanionFloatingLayer from '@/components/companions/CompanionFloatingLayer.vue';
import { useMessengerViewController } from '@/views/messenger/useMessengerViewController';

const controller = useMessengerViewController();
const AbilityTooltipListItem = controller.AbilityTooltipListItem;
const activateSettingsPanel = controller.activateSettingsPanel;
const activeAgent = controller.activeAgent;
const activeAgentApprovalMode = controller.activeAgentApprovalMode;
const activeAgentConfiguredModelName = controller.activeAgentConfiguredModelName;
const activeAgentDetailProfile = controller.activeAgentDetailProfile;
const activeAgentDirectConfiguredModelName = controller.activeAgentDirectConfiguredModelName;
const activeAgentGreetingOverride = controller.activeAgentGreetingOverride;
const activeAgentIcon = controller.activeAgentIcon;
const activeAgentId = controller.activeAgentId;
const activeAgentIdForApi = controller.activeAgentIdForApi;
const activeAgentInquiryPanel = controller.activeAgentInquiryPanel;
const activeAgentName = controller.activeAgentName;
const activeAgentPlan = controller.activeAgentPlan;
const activeAgentPlanMessage = controller.activeAgentPlanMessage;
const activeAgentPresetQuestions = controller.activeAgentPresetQuestions;
const activeAgentProfileForModelResolution = controller.activeAgentProfileForModelResolution;
const activeAgentPromptPreviewHtml = controller.activeAgentPromptPreviewHtml;
const activeAgentPromptPreviewText = controller.activeAgentPromptPreviewText;
const activeAgentRuntimeModelName = controller.activeAgentRuntimeModelName;
const activeAgentSession = controller.activeAgentSession;
const activeAgentSessionModelName = controller.activeAgentSessionModelName;
const activeAgentUsingDesktopDefaultModel = controller.activeAgentUsingDesktopDefaultModel;
const activeConversation = controller.activeConversation;
const activeConversationKindLabel = controller.activeConversationKindLabel;
const activeConversationSubtitle = controller.activeConversationSubtitle;
const activeConversationTitle = controller.activeConversationTitle;
const activeMessengerSessionBusy = controller.activeMessengerSessionBusy;
const activeSectionSubtitle = controller.activeSectionSubtitle;
const activeSectionTitle = controller.activeSectionTitle;
const activeSessionApproval = controller.activeSessionApproval;
const activeSessionOrchestrationLock = controller.activeSessionOrchestrationLock;
const activeSessionOrchestrationLocked = controller.activeSessionOrchestrationLocked;
const activeSessionGoalLocked = controller.activeSessionGoalLocked;
const activeSessionRecord = controller.activeSessionRecord;
const activeWorldConversationId = controller.activeWorldConversationId;
const activeWorldGroupId = controller.activeWorldGroupId;
const adminToolGroups = controller.adminToolGroups;
const AGENT_CONTAINER_IDS = controller.AGENT_CONTAINER_IDS;
const AGENT_MAIN_READ_AT_STORAGE_PREFIX = controller.AGENT_MAIN_READ_AT_STORAGE_PREFIX;
const AGENT_MAIN_UNREAD_STORAGE_PREFIX = controller.AGENT_MAIN_UNREAD_STORAGE_PREFIX;
const AGENT_META_REQUEST_CACHE_MS = controller.AGENT_META_REQUEST_CACHE_MS;
const AGENT_PROMPT_PREVIEW_CACHE_MS = controller.AGENT_PROMPT_PREVIEW_CACHE_MS;
const AGENT_TOOL_OVERRIDE_NONE = controller.AGENT_TOOL_OVERRIDE_NONE;
const AGENT_VOICE_MODEL_SUPPORT_CACHE_MS = controller.AGENT_VOICE_MODEL_SUPPORT_CACHE_MS;
const agentAbilitySections = controller.agentAbilitySections;
const agentAbilityTooltipOptions = controller.agentAbilityTooltipOptions;
const agentAbilityTooltipRef = controller.agentAbilityTooltipRef;
const agentAbilityTooltipVisible = controller.agentAbilityTooltipVisible;
const AgentAvatar = controller.AgentAvatar;
const agentComposerApprovalHintLabel = controller.agentComposerApprovalHintLabel;
const agentComposerApprovalHintMode = controller.agentComposerApprovalHintMode;
const agentComposerApprovalModeOptions = controller.agentComposerApprovalModeOptions;
const agentComposerDraftKey = controller.agentComposerDraftKey;
const AgentCronPanel = controller.AgentCronPanel;
const agentFileContainers = controller.agentFileContainers;
const agentHeaderModelDisplayName = controller.agentHeaderModelDisplayName;
const agentHeaderModelJumpEnabled = controller.agentHeaderModelJumpEnabled;
const agentHiveEntries = controller.agentHiveEntries;
const agentHiveLabelMap = controller.agentHiveLabelMap;
const agentHiveTotalCount = controller.agentHiveTotalCount;
const agentHiveTreeRows = controller.agentHiveTreeRows;
const agentInquirySelection = controller.agentInquirySelection;
const agentMainReadAtMap = controller.agentMainReadAtMap;
const agentMainUnreadCountMap = controller.agentMainUnreadCountMap;
const agentMap = controller.agentMap;
const AgentMemoryPanel = controller.AgentMemoryPanel;
const agentOverviewCards = controller.agentOverviewCards;
const agentOverviewMode = controller.agentOverviewMode;
const agentPlanExpanded = controller.agentPlanExpanded;
const agentPromptPreviewContent = controller.agentPromptPreviewContent;
const agentPromptPreviewLoading = controller.agentPromptPreviewLoading;
const agentPromptPreviewMemoryMode = controller.agentPromptPreviewMemoryMode;
const agentPromptPreviewPayloadCache = controller.agentPromptPreviewPayloadCache;
const agentPromptPreviewPayloadPromise = controller.agentPromptPreviewPayloadPromise;
const agentPromptPreviewPayloadPromiseKey = controller.agentPromptPreviewPayloadPromiseKey;
const agentPromptPreviewSelectedNames = controller.agentPromptPreviewSelectedNames;
const agentPromptPreviewToolingContent = controller.agentPromptPreviewToolingContent;
const agentPromptPreviewToolingItems = controller.agentPromptPreviewToolingItems;
const agentPromptPreviewToolingMode = controller.agentPromptPreviewToolingMode;
const agentPromptPreviewVisible = controller.agentPromptPreviewVisible;
const agentPromptToolSummary = controller.agentPromptToolSummary;
const AgentQuickCreateDialog = controller.AgentQuickCreateDialog;
const agentQuickCreateVisible = controller.agentQuickCreateVisible;
const agentRenderableMessages = controller.agentRenderableMessages;
const AgentRuntimeRecordsPanel = controller.AgentRuntimeRecordsPanel;
const agentRuntimeStateHydrated = controller.agentRuntimeStateHydrated;
const agentRuntimeStateMap = controller.agentRuntimeStateMap;
const agentRuntimeStateSnapshot = controller.agentRuntimeStateSnapshot;
const agentSessionLoading = controller.agentSessionLoading;
const agentSettingMode = controller.agentSettingMode;
const agentSettingsFocusTarget = controller.agentSettingsFocusTarget;
const agentSettingsFocusToken = controller.agentSettingsFocusToken;
const AgentSettingsPanel = controller.AgentSettingsPanel;
const agentSettingsPanelRef = controller.agentSettingsPanelRef;
const agentStore = controller.agentStore;
const agentToolSummaryError = controller.agentToolSummaryError;
const agentToolSummaryLoading = controller.agentToolSummaryLoading;
const agentToolSummaryPromise = controller.agentToolSummaryPromise;
const agentUnreadRefreshInFlight = controller.agentUnreadRefreshInFlight;
const agentUnreadStorageKeys = controller.agentUnreadStorageKeys;
const agentUserRoundsLoadVersion = controller.agentUserRoundsLoadVersion;
const agentUserRoundsMap = controller.agentUserRoundsMap;
const agentVoiceDurationMs = controller.agentVoiceDurationMs;
const agentVoiceModelHearingSupported = controller.agentVoiceModelHearingSupported;
const agentVoiceModelSupportCheckedAt = controller.agentVoiceModelSupportCheckedAt;
const agentVoiceRecording = controller.agentVoiceRecording;
const agentVoiceRecordingRuntime = controller.agentVoiceRecordingRuntime;
const agentVoiceSupported = controller.agentVoiceSupported;
const allowNavigationCollapse = controller.allowNavigationCollapse;
const appearanceHydrating = controller.appearanceHydrating;
const appendAgentLocalCommandMessages = controller.appendAgentLocalCommandMessages;
const appendScreenshotFileNameSuffix = controller.appendScreenshotFileNameSuffix;
const appendWorldAttachmentTokens = controller.appendWorldAttachmentTokens;
const applyCurrentUserAppearance = controller.applyCurrentUserAppearance;
const applyInitialBeeroomSectionSelection = controller.applyInitialBeeroomSectionSelection;
const applyMessengerOrderPreferences = controller.applyMessengerOrderPreferences;
const applyUiFontSize = controller.applyUiFontSize;
const approvalResponding = controller.approvalResponding;
const ArchivedThreadManager = controller.ArchivedThreadManager;
const archiveTimelineSession = controller.archiveTimelineSession;
const asObjectRecord = controller.asObjectRecord;
const AUDIO_ATTACHMENT_EXTENSIONS = controller.AUDIO_ATTACHMENT_EXTENSIONS;
const audioRecordingSupported = controller.audioRecordingSupported;
const audioRecordingSupportHandler = controller.audioRecordingSupportHandler;
const audioRecordingSupportRetryTimer = controller.audioRecordingSupportRetryTimer;
const authStore = controller.authStore;
const autoStickToBottom = controller.autoStickToBottom;
const avatarLabel = controller.avatarLabel;
const basePrefix = controller.basePrefix;
const BEEROOM_GROUPS_REFRESH_MIN_MS_HOT = controller.BEEROOM_GROUPS_REFRESH_MIN_MS_HOT;
const BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE = controller.BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE;
const beeroomCandidateAgents = controller.beeroomCandidateAgents;
const beeroomDispatchSessionIdsByGroup = controller.beeroomDispatchSessionIdsByGroup;
const beeroomFirstEntryAutoSelectionPending = controller.beeroomFirstEntryAutoSelectionPending;
const beeroomGroupOptions = controller.beeroomGroupOptions;
const beeroomGroupsLastRefreshAt = controller.beeroomGroupsLastRefreshAt;
const beeroomStore = controller.beeroomStore;
const BeeroomWorkbench = controller.BeeroomWorkbench;
const beginWorkerCardImportOverlay = controller.beginWorkerCardImportOverlay;
const bootLoading = controller.bootLoading;
const bootstrap = controller.bootstrap;
const boundAgentFileContainers = controller.boundAgentFileContainers;
const buildAbilityAllowedNameSet = controller.buildAbilityAllowedNameSet;
const buildActiveSessionBusyDebugSnapshot = controller.buildActiveSessionBusyDebugSnapshot;
const buildAgentApprovalOptions = controller.buildAgentApprovalOptions;
const buildAgentInquiryReply = controller.buildAgentInquiryReply;
const buildAgentVoiceFileName = controller.buildAgentVoiceFileName;
const buildAssistantDisplayContent = controller.buildAssistantDisplayContent;
const buildAssistantMessageStatsEntries = controller.buildAssistantMessageStatsEntries;
const buildCurrentUserFallbackUnitTree = controller.buildCurrentUserFallbackUnitTree;
const buildDeclaredDependencyPayload = controller.buildDeclaredDependencyPayload;
const buildDesktopUpdateStatusText = controller.buildDesktopUpdateStatusText;
const buildLatestAssistantLayoutSignature = controller.buildLatestAssistantLayoutSignature;
const buildMessageStatsEntries = controller.buildMessageStatsEntries;
const buildMessageWorkflowRenderVersion = controller.buildMessageWorkflowRenderVersion;
const buildProfileAvatarOptionLabel = controller.buildProfileAvatarOptionLabel;
const buildQuickAgentName = controller.buildQuickAgentName;
const buildRouteQuerySignature = controller.buildRouteQuerySignature;
const buildSessionAgentMap = controller.buildSessionAgentMap;
const buildUnitTreeFromFlat = controller.buildUnitTreeFromFlat;
const buildUnitTreeRows = controller.buildUnitTreeRows;
const buildWorkflowSurfaceDebugSnapshot = controller.buildWorkflowSurfaceDebugSnapshot;
const buildWorkspacePublicPath = controller.buildWorkspacePublicPath;
const buildWorldAttachmentToken = controller.buildWorldAttachmentToken;
const buildWorldDraftKey = controller.buildWorldDraftKey;
const buildWorldVoiceFileName = controller.buildWorldVoiceFileName;
const buildWorldVoicePayloadContent = controller.buildWorldVoicePayloadContent;
const buildWorldVoiceResourceKey = controller.buildWorldVoiceResourceKey;
const builtinTools = controller.builtinTools;
const cachedMessengerRootRight = controller.cachedMessengerRootRight;
const cachedMessengerRootWidth = controller.cachedMessengerRootWidth;
const cancelAgentVoiceRecording = controller.cancelAgentVoiceRecording;
const cancelMiddlePaneOverlayHide = controller.cancelMiddlePaneOverlayHide;
const cancelWorldVoiceRecording = controller.cancelWorldVoiceRecording;
const canDeleteMixedConversation = controller.canDeleteMixedConversation;
const canDeleteSettingsAgent = controller.canDeleteSettingsAgent;
const canSendWorldMessage = controller.canSendWorldMessage;
const captureMessengerOrderPreferences = controller.captureMessengerOrderPreferences;
const captureWorldScreenshotData = controller.captureWorldScreenshotData;
const channelBoundAgentIds = controller.channelBoundAgentIds;
const channelBoundAgentIdsLoadedAt = controller.channelBoundAgentIdsLoadedAt;
const channelBoundAgentIdsLoadPromise = controller.channelBoundAgentIdsLoadPromise;
const channelBoundAgentIdsLoadVersion = controller.channelBoundAgentIdsLoadVersion;
const ChatComposer = controller.ChatComposer;
const chatDebugLog = controller.chatDebugLog;
const chatFooterRef = controller.chatFooterRef;
const chatPanelKindLabel = controller.chatPanelKindLabel;
const chatPanelSubtitle = controller.chatPanelSubtitle;
const chatPanelTitle = controller.chatPanelTitle;
const chatStore = controller.chatStore;
const checkClientUpdate = controller.checkClientUpdate;
const clampWorldComposerHeight = controller.clampWorldComposerHeight;
const classifyWorldHistoryMessage = controller.classifyWorldHistoryMessage;
const clearAgentConversationDismissed = controller.clearAgentConversationDismissed;
const clearAgentVoiceRecordingTimer = controller.clearAgentVoiceRecordingTimer;
const clearBeeroomMissionCanvasState = controller.clearBeeroomMissionCanvasState;
const clearBeeroomMissionChatState = controller.clearBeeroomMissionChatState;
const clearBeeroomRuntimeCachesByGroup = controller.clearBeeroomRuntimeCachesByGroup;
const clearCachedDispatchPreview = controller.clearCachedDispatchPreview;
const clearKeywordDebounce = controller.clearKeywordDebounce;
const clearMessagePanelWhenConversationEmpty = controller.clearMessagePanelWhenConversationEmpty;
const clearMiddlePaneOverlayHide = controller.clearMiddlePaneOverlayHide;
const clearMiddlePaneOverlayPreview = controller.clearMiddlePaneOverlayPreview;
const clearMiddlePanePrewarm = controller.clearMiddlePanePrewarm;
const clearRightDockSkillAutoRetry = controller.clearRightDockSkillAutoRetry;
const clearWorkspaceLoadingLabelTimer = controller.clearWorkspaceLoadingLabelTimer;
const clearWorkspaceResourceCache = controller.clearWorkspaceResourceCache;
const clearWorkspaceResourceCacheByPaths = controller.clearWorkspaceResourceCacheByPaths;
const clearWorldQuickPanelClose = controller.clearWorldQuickPanelClose;
const clearWorldVoiceRecordingTimer = controller.clearWorldVoiceRecordingTimer;
const closeFileContainerMenu = controller.closeFileContainerMenu;
const closeImagePreview = controller.closeImagePreview;
const closeLeftRailMoreMenu = controller.closeLeftRailMoreMenu;
const closeWorldAttachmentPanels = controller.closeWorldAttachmentPanels;
const closeWorldQuickPanelWhenOutside = controller.closeWorldQuickPanelWhenOutside;
const collectAbilityDetails = controller.collectAbilityDetails;
const collectAbilityGroupDetails = controller.collectAbilityGroupDetails;
const collectAbilityNames = controller.collectAbilityNames;
const collectMainAgentSessionEntries = controller.collectMainAgentSessionEntries;
const collectUnitNodeIds = controller.collectUnitNodeIds;
const composerApprovalMode = controller.composerApprovalMode;
const composerApprovalModeSyncing = controller.composerApprovalModeSyncing;
const computed = controller.computed;
const confirmWithFallback = controller.confirmWithFallback;
const CONTACT_VIRTUAL_ITEM_HEIGHT = controller.CONTACT_VIRTUAL_ITEM_HEIGHT;
const CONTACT_VIRTUAL_OVERSCAN = controller.CONTACT_VIRTUAL_OVERSCAN;
const contactTotalCount = controller.contactTotalCount;
const contactUnitDescendantMap = controller.contactUnitDescendantMap;
const contactUnitDirectCountMap = controller.contactUnitDirectCountMap;
const contactUnitExpandedIds = controller.contactUnitExpandedIds;
const contactUnitKnownIdSet = controller.contactUnitKnownIdSet;
const contactUnitLabelMap = controller.contactUnitLabelMap;
const contactUnitTreeRows = controller.contactUnitTreeRows;
const contactVirtualBottomPadding = controller.contactVirtualBottomPadding;
const contactVirtualFrame = controller.contactVirtualFrame;
const contactVirtualListRef = controller.contactVirtualListRef;
const contactVirtualRange = controller.contactVirtualRange;
const contactVirtualScrollTop = controller.contactVirtualScrollTop;
const contactVirtualTopPadding = controller.contactVirtualTopPadding;
const contactVirtualViewportHeight = controller.contactVirtualViewportHeight;
const copyMessageContent = controller.copyMessageContent;
const copyText = controller.copyText;
const isMessageTtsLoading = controller.isMessageTtsLoading;
const isMessageTtsPlaying = controller.isMessageTtsPlaying;
const resolveMessageTtsActionLabel = controller.resolveMessageTtsActionLabel;
const toggleMessageTtsPlayback = controller.toggleMessageTtsPlayback;
const createAgentApi = controller.createAgentApi;
const createAgentQuickly = controller.createAgentQuickly;
const createBeeroomRealtimeSync = controller.createBeeroomRealtimeSync;
const createMessageViewportRuntime = controller.createMessageViewportRuntime;
const createMessengerRealtimePulse = controller.createMessengerRealtimePulse;
const createScopedStorageKeys = controller.createScopedStorageKeys;
const creatingAgentSession = controller.creatingAgentSession;
const cronAgentIds = controller.cronAgentIds;
const cronAgentIdsLoadedAt = controller.cronAgentIdsLoadedAt;
const cronAgentIdsLoadPromise = controller.cronAgentIdsLoadPromise;
const cronAgentIdsLoadVersion = controller.cronAgentIdsLoadVersion;
const cronPermissionDenied = controller.cronPermissionDenied;
const currentContainerId = controller.currentContainerId;
const currentLanguageLabel = controller.currentLanguageLabel;
const currentUserAvatarColor = controller.currentUserAvatarColor;
const currentUserAvatarIcon = controller.currentUserAvatarIcon;
const currentUserAvatarImageUrl = controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = controller.currentUserAvatarStyle;
const currentUserContextInitialized = controller.currentUserContextInitialized;
const currentUserId = controller.currentUserId;
const currentUsername = controller.currentUsername;
const debugToolsAvailable = controller.debugToolsAvailable;
const decodeWorldAtPathToken = controller.decodeWorldAtPathToken;
const DEFAULT_AGENT_KEY = controller.DEFAULT_AGENT_KEY;
const DEFAULT_BEEROOM_GROUP_ID = controller.DEFAULT_BEEROOM_GROUP_ID;
const defaultAgentApprovalMode = controller.defaultAgentApprovalMode;
const defaultAgentMatchesKeyword = controller.defaultAgentMatchesKeyword;
const defaultAgentProfile = controller.defaultAgentProfile;
const defaultBeeroomGroupId = controller.defaultBeeroomGroupId;
const defaultMessengerOrderPreferences = controller.defaultMessengerOrderPreferences;
const deleteAgentApi = controller.deleteAgentApi;
const deleteMixedConversation = controller.deleteMixedConversation;
const deletingAgentSelectionSnapshot = controller.deletingAgentSelectionSnapshot;
const DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY = controller.DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY;
const DesktopContainerManagerPanel = controller.DesktopContainerManagerPanel;
const desktopContainerManagerPanelRef = controller.desktopContainerManagerPanelRef;
const desktopContainerRootMap = controller.desktopContainerRootMap;
const desktopDefaultModelDisplayName = controller.desktopDefaultModelDisplayName;
const desktopDefaultModelMaxContext = controller.desktopDefaultModelMaxContext;
const desktopDefaultModelMetaFetchPromise = controller.desktopDefaultModelMetaFetchPromise;
const desktopFirstLaunchDefaultAgentHintAt = controller.desktopFirstLaunchDefaultAgentHintAt;
const desktopInitialSectionPinned = controller.desktopInitialSectionPinned;
const desktopLocalMode = controller.desktopLocalMode;
const desktopMode = controller.desktopMode;
const desktopShowFirstLaunchDefaultAgentHint = controller.desktopShowFirstLaunchDefaultAgentHint;
const DesktopSystemSettingsPanel = controller.DesktopSystemSettingsPanel;
const desktopUpdateAvailable = controller.desktopUpdateAvailable;
const detectAudioRecordingSupport = controller.detectAudioRecordingSupport;
const detectMessengerLayoutAnomaly = controller.detectMessengerLayoutAnomaly;
const dismissActiveAgentPlan = controller.dismissActiveAgentPlan;
const DISMISSED_AGENT_STORAGE_PREFIX = controller.DISMISSED_AGENT_STORAGE_PREFIX;
const dismissedAgentConversationMap = controller.dismissedAgentConversationMap;
const dismissedAgentStorageKey = controller.dismissedAgentStorageKey;
const dismissedPlanMessages = controller.dismissedPlanMessages;
const dismissedPlanVersion = controller.dismissedPlanVersion;
const disposeWorldVoicePlayback = controller.disposeWorldVoicePlayback;
const downloadExternalImage = controller.downloadExternalImage;
const downloadUserWorldFile = controller.downloadUserWorldFile;
const downloadWorkerCardBundle = controller.downloadWorkerCardBundle;
const downloadWorkspaceResource = controller.downloadWorkspaceResource;
const downloadWunderWorkspaceFile = controller.downloadWunderWorkspaceFile;
const effectiveAgentToolSummary = controller.effectiveAgentToolSummary;
const ElLoading = controller.ElLoading;
const ElMessage = controller.ElMessage;
const ElMessageBox = controller.ElMessageBox;
const emitUserToolsUpdated = controller.emitUserToolsUpdated;
const emitWorkspaceRefresh = controller.emitWorkspaceRefresh;
const ensureAgentUnreadState = controller.ensureAgentUnreadState;
const ensureDismissedAgentConversationState = controller.ensureDismissedAgentConversationState;
const ensureHelperAppsSelection = controller.ensureHelperAppsSelection;
const ensureMiddlePaneSection = controller.ensureMiddlePaneSection;
const ensureSectionSelection = controller.ensureSectionSelection;
const ensureUserAttachmentResource = controller.ensureUserAttachmentResource;
const ensureWorldVoicePlaybackRuntime = controller.ensureWorldVoicePlaybackRuntime;
const enterSelectedAgentConversation = controller.enterSelectedAgentConversation;
const estimateVirtualOffsetTop = controller.estimateVirtualOffsetTop;
const extractLatestConversationPreview = controller.extractLatestConversationPreview;
const extractLatestUserPreview = controller.extractLatestUserPreview;
const extractLatestVisibleMessagePreview = controller.extractLatestVisibleMessagePreview;
const extractPromptPreviewSelectedAbilityNames = controller.extractPromptPreviewSelectedAbilityNames;
const extractPromptToolingPreview = controller.extractPromptToolingPreview;
const extractWorkspaceRefreshPaths = controller.extractWorkspaceRefreshPaths;
const fetchActiveAgentPromptPreviewPayload = controller.fetchActiveAgentPromptPreviewPayload;
const fetchCronJobs = controller.fetchCronJobs;
const fetchDesktopSettings = controller.fetchDesktopSettings;
const fetchExternalLinks = controller.fetchExternalLinks;
const fetchOrgUnits = controller.fetchOrgUnits;
const fetchRealtimeSystemPrompt = controller.fetchRealtimeSystemPrompt;
const fetchSessionSystemPrompt = controller.fetchSessionSystemPrompt;
const fetchUserSkillContent = controller.fetchUserSkillContent;
const fetchWorkspaceResource = controller.fetchWorkspaceResource;
const fetchWorldVoiceObjectUrl = controller.fetchWorldVoiceObjectUrl;
const fetchWunderWorkspaceContent = controller.fetchWunderWorkspaceContent;
const fileContainerCloudLocation = controller.fileContainerCloudLocation;
const fileContainerContextMenu = controller.fileContainerContextMenu;
const fileContainerContextMenuStyle = controller.fileContainerContextMenuStyle;
const fileContainerEntryCount = controller.fileContainerEntryCount;
const fileContainerLatestUpdatedAt = controller.fileContainerLatestUpdatedAt;
const fileContainerLifecycleText = controller.fileContainerLifecycleText;
const fileContainerLocalLocation = controller.fileContainerLocalLocation;
const fileContainerMenuViewRef = controller.fileContainerMenuViewRef;
const fileLifecycleNowTick = controller.fileLifecycleNowTick;
const fileScope = controller.fileScope;
const filterAbilitySummaryByNames = controller.filterAbilitySummaryByNames;
const filteredBeeroomGroupIdSet = controller.filteredBeeroomGroupIdSet;
const filteredBeeroomGroups = controller.filteredBeeroomGroups;
const filteredBeeroomGroupsOrdered = controller.filteredBeeroomGroupsOrdered;
const filteredContacts = controller.filteredContacts;
const filteredGroupCreateContacts = controller.filteredGroupCreateContacts;
const filteredGroups = controller.filteredGroups;
const filteredMixedConversations = controller.filteredMixedConversations;
const filteredOwnedAgentIdSet = controller.filteredOwnedAgentIdSet;
const filteredOwnedAgents = controller.filteredOwnedAgents;
const filteredOwnedAgentsOrdered = controller.filteredOwnedAgentsOrdered;
const filteredPlazaItems = controller.filteredPlazaItems;
const filteredSharedAgentIdSet = controller.filteredSharedAgentIdSet;
const filteredSharedAgents = controller.filteredSharedAgents;
const filteredSharedAgentsOrdered = controller.filteredSharedAgentsOrdered;
const filteredWorldHistoryRecords = controller.filteredWorldHistoryRecords;
const filterPlazaItemsByKindAndKeyword = controller.filterPlazaItemsByKindAndKeyword;
const findWorldOversizedFile = controller.findWorldOversizedFile;
const finishMessengerPerfTrace = controller.finishMessengerPerfTrace;
const flattenUnitNodes = controller.flattenUnitNodes;
const flushSessionDetailPrefetchQueue = controller.flushSessionDetailPrefetchQueue;
const focusWorldTextareaToEnd = controller.focusWorldTextareaToEnd;
const formatAgentRuntimeState = controller.formatAgentRuntimeState;
const formatContactPresence = controller.formatContactPresence;
const formatTime = controller.formatTime;
const formatUserRounds = controller.formatUserRounds;
const formatWorldVoiceDuration = controller.formatWorldVoiceDuration;
const fullPrimaryAgentList = controller.fullPrimaryAgentList;
const generalSettingsPanelMode = controller.generalSettingsPanelMode;
const getChatSessionApi = controller.getChatSessionApi;
const getCurrentLanguage = controller.getCurrentLanguage;
const getDesktopBridge = controller.getDesktopBridge;
const getFilenameFromHeaders = controller.getFilenameFromHeaders;
const getRuntimeConfig = controller.getRuntimeConfig;
const getUserAttachmentResourceState = controller.getUserAttachmentResourceState;
const GlobeAppPanel = controller.GlobeAppPanel;
const groupCreateKeyword = controller.groupCreateKeyword;
const groupCreateMemberIds = controller.groupCreateMemberIds;
const groupCreateName = controller.groupCreateName;
const groupCreateVisible = controller.groupCreateVisible;
const groupCreating = controller.groupCreating;
const handleAgentAbilityTooltipHide = controller.handleAgentAbilityTooltipHide;
const handleAgentAbilityTooltipShow = controller.handleAgentAbilityTooltipShow;
const handleAgentBatchDelete = controller.handleAgentBatchDelete;
const handleAgentBatchExport = controller.handleAgentBatchExport;
const handleAgentDeleted = controller.handleAgentDeleted;
const handleAgentDeleteStart = controller.handleAgentDeleteStart;
const handleAgentInquirySelection = controller.handleAgentInquirySelection;
const handleAgentLocalCommand = controller.handleAgentLocalCommand;
const handleAgentRuntimeStateUpdate = controller.handleAgentRuntimeStateUpdate;
const handleAgentSettingsFocusConsumed = controller.handleAgentSettingsFocusConsumed;
const handleAgentSettingsSaved = controller.handleAgentSettingsSaved;
const handleArchivedSessionRemoved = controller.handleArchivedSessionRemoved;
const handleBeeroomMoveAgents = controller.handleBeeroomMoveAgents;
const handleChatPageRefresh = controller.handleChatPageRefresh;
const handleContactVirtualScroll = controller.handleContactVirtualScroll;
const handleCronPanelChanged = controller.handleCronPanelChanged;
const handleDeleteBeeroomGroup = controller.handleDeleteBeeroomGroup;
const handleDesktopContainerRootsChange = controller.handleDesktopContainerRootsChange;
const handleDesktopModelMetaChanged = controller.handleDesktopModelMetaChanged;
const handleFileContainerMenuCopyId = controller.handleFileContainerMenuCopyId;
const handleFileContainerMenuOpen = controller.handleFileContainerMenuOpen;
const handleFileContainerMenuSettings = controller.handleFileContainerMenuSettings;
const handleFileWorkspaceStats = controller.handleFileWorkspaceStats;
const handleHelpManualLoadingChange = controller.handleHelpManualLoadingChange;
const handleHivePackImportedFromMiddlePane = controller.handleHivePackImportedFromMiddlePane;
const handleImagePreviewDownload = controller.handleImagePreviewDownload;
const handleMessageContentClick = controller.handleMessageContentClick;
const handleMessageListScroll = controller.handleMessageListScroll;
const handleMessageWorkflowLayoutChange = controller.handleMessageWorkflowLayoutChange;
const handleMessengerRootPointerLeave = controller.handleMessengerRootPointerLeave;
const handleMessengerRootPointerMove = controller.handleMessengerRootPointerMove;
const handleMiddlePaneAgentHiveGroupIdUpdate = controller.handleMiddlePaneAgentHiveGroupIdUpdate;
const handleMiddlePaneContactUnitIdUpdate = controller.handleMiddlePaneContactUnitIdUpdate;
const handleRightDockSkillArchiveUpload = controller.handleRightDockSkillArchiveUpload;
const handleRightDockSkillEnabledToggle = controller.handleRightDockSkillEnabledToggle;
const handleSearchCreateAction = controller.handleSearchCreateAction;
const handleSessionApprovalDecision = controller.handleSessionApprovalDecision;
const handleSettingsLogout = controller.handleSettingsLogout;
const handleTimelineDialogActivateSession = controller.handleTimelineDialogActivateSession;
const handleUserToolsUpdatedEvent = controller.handleUserToolsUpdatedEvent;
const handleWorkerCardImportInput = controller.handleWorkerCardImportInput;
const handleWorkspaceResourceRefresh = controller.handleWorkspaceResourceRefresh;
const handleWorldComposerEnterKeydown = controller.handleWorldComposerEnterKeydown;
const handleWorldComposerResizeMove = controller.handleWorldComposerResizeMove;
const handleWorldContainerPickerEntry = controller.handleWorldContainerPickerEntry;
const handleWorldUploadInput = controller.handleWorldUploadInput;
const hasActiveSubagentItems = controller.hasActiveSubagentItems;
const hasActiveSubagentsAfterLatestUser = controller.hasActiveSubagentsAfterLatestUser;
const hasAgentAbilitySummary = controller.hasAgentAbilitySummary;
const hasAnyMixedConversations = controller.hasAnyMixedConversations;
const hasAssistantWaitingForCurrentOutput = controller.hasAssistantWaitingForCurrentOutput;
const hasCronTask = controller.hasCronTask;
const hasHotBeeroomRuntimeState = controller.hasHotBeeroomRuntimeState;
const hasHotRuntimeState = controller.hasHotRuntimeState;
const hasMessageContent = controller.hasMessageContent;
const hasMessengerOrderEntries = controller.hasMessengerOrderEntries;
const hasPlanSteps = controller.hasPlanSteps;
const hasRunningAssistantMessage = controller.hasRunningAssistantMessage;
const hasStreamingAssistantMessage = controller.hasStreamingAssistantMessage;
const hasUserAudioAttachments = controller.hasUserAudioAttachments;
const hasUserImageAttachments = controller.hasUserImageAttachments;
const hasWorkflowOrThinking = controller.hasWorkflowOrThinking;
const helperAppsActiveDescription = controller.helperAppsActiveDescription;
const helperAppsActiveExternalItem = controller.helperAppsActiveExternalItem;
const helperAppsActiveKey = controller.helperAppsActiveKey;
const helperAppsActiveKind = controller.helperAppsActiveKind;
const helperAppsActiveOfflineItem = controller.helperAppsActiveOfflineItem;
const helperAppsActiveTitle = controller.helperAppsActiveTitle;
const helperAppsOfflineItems = controller.helperAppsOfflineItems;
const helperAppsOnlineItems = controller.helperAppsOnlineItems;
const helperAppsOnlineLoaded = controller.helperAppsOnlineLoaded;
const helperAppsOnlineLoading = controller.helperAppsOnlineLoading;
const helperAppsWorkspaceMode = controller.helperAppsWorkspaceMode;
const helpManualLoading = controller.helpManualLoading;
const HoneycombWaitingOverlay = controller.HoneycombWaitingOverlay;
const hydrateCurrentUserAppearance = controller.hydrateCurrentUserAppearance;
const hydrateExternalMarkdownImages = controller.hydrateExternalMarkdownImages;
const hydrateMessengerOrderPreferences = controller.hydrateMessengerOrderPreferences;
const hydrateWorkspaceResourceCard = controller.hydrateWorkspaceResourceCard;
const hydrateWorkspaceResources = controller.hydrateWorkspaceResources;
const imagePreviewTitle = controller.imagePreviewTitle;
const imagePreviewUrl = controller.imagePreviewUrl;
const imagePreviewVisible = controller.imagePreviewVisible;
const imagePreviewWorkspacePath = controller.imagePreviewWorkspacePath;
const initDesktopLaunchBehavior = controller.initDesktopLaunchBehavior;
const InquiryPanel = controller.InquiryPanel;
const insertWorldEmoji = controller.insertWorldEmoji;
const invalidateAllUserToolsCaches = controller.invalidateAllUserToolsCaches;
const invalidateUserSkillsCache = controller.invalidateUserSkillsCache;
const invalidateUserToolsCatalogCache = controller.invalidateUserToolsCatalogCache;
const invalidateUserToolsSummaryCache = controller.invalidateUserToolsSummaryCache;
const isAdminUser = controller.isAdminUser;
const isAgentConversationActive = controller.isAgentConversationActive;
const isAgentOrchestrationActive = controller.isAgentOrchestrationActive;
const isAgentGoalActive = controller.isAgentGoalActive;
const isAudioPath = controller.isAudioPath;
const isAudioRecordingSupported = controller.isAudioRecordingSupported;
const isAuthDeniedStatus = controller.isAuthDeniedStatus;
const isChatDebugEnabled = controller.isChatDebugEnabled;
const isCompactionMarkerMessage = controller.isCompactionMarkerMessage;
const isCompactionOnlyWorkflowItems = controller.isCompactionOnlyWorkflowItems;
const isCompactionRunningFromWorkflowItems = controller.isCompactionRunningFromWorkflowItems;
const isContactOnline = controller.isContactOnline;
const isContactUnitExpanded = controller.isContactUnitExpanded;
const isDefaultModelSelectorValue = controller.isDefaultModelSelectorValue;
const isDesktopModeEnabled = controller.isDesktopModeEnabled;
const isDesktopUpdatePending = controller.isDesktopUpdatePending;
const isDesktopUpdateTerminal = controller.isDesktopUpdateTerminal;
const isEmbeddedChatRoute = controller.isEmbeddedChatRoute;
const isGreetingMessage = controller.isGreetingMessage;
const isHelperAppActive = controller.isHelperAppActive;
const isHelperAppsMiddlePaneActive = controller.isHelperAppsMiddlePaneActive;
const isHiddenInternalMessage = controller.isHiddenInternalMessage;
const isHotBeeroomMissionStatus = controller.isHotBeeroomMissionStatus;
const isImagePath = controller.isImagePath;
const isLeftNavSectionActive = controller.isLeftNavSectionActive;
const isLeftRailMoreActive = controller.isLeftRailMoreActive;
const isMessengerInteractionBlocked = controller.isMessengerInteractionBlocked;
const isMiddlePaneOverlay = controller.isMiddlePaneOverlay;
const isMixedConversationActive = controller.isMixedConversationActive;
const isOwnMessage = controller.isOwnMessage;
const isPlanMessageDismissed = controller.isPlanMessageDismissed;
const isRightDockOverlay = controller.isRightDockOverlay;
const isRightDockResizing = controller.isRightDockResizing;
const isSameModelName = controller.isSameModelName;
const isSameRouteLocation = controller.isSameRouteLocation;
const isSearchableMiddlePaneSection = controller.isSearchableMiddlePaneSection;
const isSectionButtonActive = controller.isSectionButtonActive;
const isSessionBusy = controller.isSessionBusy;
const isSettingsDefaultAgentReadonly = controller.isSettingsDefaultAgentReadonly;
const isSilentAgent = controller.isSilentAgent;
const isUserToolsScopeForAgentSummary = controller.isUserToolsScopeForAgentSummary;
const isVisibleAgentAssistantMessage = controller.isVisibleAgentAssistantMessage;
const isWorkspacePathAffected = controller.isWorkspacePathAffected;
const isWorkspaceResourceMissing = controller.isWorkspaceResourceMissing;
const isWorldConversationActive = controller.isWorldConversationActive;
const isWorldVoiceContentType = controller.isWorldVoiceContentType;
const isWorldVoiceLoading = controller.isWorldVoiceLoading;
const isWorldVoiceMessage = controller.isWorldVoiceMessage;
const isWorldVoicePlaying = controller.isWorldVoicePlaying;
const jumpToMessageBottom = controller.jumpToMessageBottom;
const jumpToMessageTop = controller.jumpToMessageTop;
const keyword = controller.keyword;
const KEYWORD_INPUT_DEBOUNCE_MS = controller.KEYWORD_INPUT_DEBOUNCE_MS;
const keywordDebounceTimer = controller.keywordDebounceTimer;
const keywordInput = controller.keywordInput;
const knowledgeTools = controller.knowledgeTools;
const lastMessengerLayoutDebugSignature = controller.lastMessengerLayoutDebugSignature;
const latestAgentRenderableMessageKey = controller.latestAgentRenderableMessageKey;
const latestRenderableAssistantMessage = controller.latestRenderableAssistantMessage;
const latestVisibleAgentAssistantMessage = controller.latestVisibleAgentAssistantMessage;
const latestWorldRenderableMessageKey = controller.latestWorldRenderableMessageKey;
const leftRailMainSectionOptions = controller.leftRailMainSectionOptions;
const leftRailMoreExpanded = controller.leftRailMoreExpanded;
const leftRailMoreToggleTitle = controller.leftRailMoreToggleTitle;
const leftRailRef = controller.leftRailRef;
const leftRailSocialSectionOptions = controller.leftRailSocialSectionOptions;
const lifecycleTimer = controller.lifecycleTimer;
const listAgentUserRounds = controller.listAgentUserRounds;
const listChannelBindings = controller.listChannelBindings;
const listRunningAgents = controller.listRunningAgents;
const loadAgentToolSummary = controller.loadAgentToolSummary;
const loadAgentUserRounds = controller.loadAgentUserRounds;
const loadChannelBoundAgentIds = controller.loadChannelBoundAgentIds;
const loadCronAgentIds = controller.loadCronAgentIds;
const loadDefaultAgentProfile = controller.loadDefaultAgentProfile;
const loadHelperExternalApps = controller.loadHelperExternalApps;
const loadMessengerOrderPreferences = controller.loadMessengerOrderPreferences;
const loadOrgUnits = controller.loadOrgUnits;
const loadRightDockSkills = controller.loadRightDockSkills;
const loadRunningAgents = controller.loadRunningAgents;
const loadStoredStringArray = controller.loadStoredStringArray;
const loadToolsCatalog = controller.loadToolsCatalog;
const loadUserAppearance = controller.loadUserAppearance;
const loadUserSkillsCache = controller.loadUserSkillsCache;
const loadUserToolsCatalogCache = controller.loadUserToolsCatalogCache;
const loadUserToolsSummaryCache = controller.loadUserToolsSummaryCache;
const loadWorldContainerPickerEntries = controller.loadWorldContainerPickerEntries;
const locateWorldHistoryMessage = controller.locateWorldHistoryMessage;
const markAgentConversationDismissed = controller.markAgentConversationDismissed;
const MARKDOWN_CACHE_LIMIT = controller.MARKDOWN_CACHE_LIMIT;
const MARKDOWN_STREAM_THROTTLE_MS = controller.MARKDOWN_STREAM_THROTTLE_MS;
const markdownCache = controller.markdownCache;
const markMessengerPerfTrace = controller.markMessengerPerfTrace;
const markPlanMessageDismissed = controller.markPlanMessageDismissed;
const matchesAgentHiveSelection = controller.matchesAgentHiveSelection;
const matchesAgentKeyword = controller.matchesAgentKeyword;
const mcpTools = controller.mcpTools;
const measureMessengerLayoutElement = controller.measureMessengerLayoutElement;
const MESSAGE_VIRTUAL_ESTIMATED_HEIGHT = controller.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
const MessageCompactionDivider = controller.MessageCompactionDivider;
const MessageFeedbackActions = controller.MessageFeedbackActions;
const MessageKnowledgeCitation = controller.MessageKnowledgeCitation;
const messageListRef = controller.messageListRef;
const messageStatsNowTick = controller.messageStatsNowTick;
const messageStatsTimer = controller.messageStatsTimer;
const MessageSubagentPanel = controller.MessageSubagentPanel;
const MessageThinking = controller.MessageThinking;
const MessageToolWorkflow = controller.MessageToolWorkflow;
const messageViewportRuntime = controller.messageViewportRuntime;
const messageVirtualHeightCache = controller.messageVirtualHeightCache;
const messageVirtualLayoutVersion = controller.messageVirtualLayoutVersion;
const messageVirtualScrollTop = controller.messageVirtualScrollTop;
const messageVirtualViewportHeight = controller.messageVirtualViewportHeight;
const MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT = controller.MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT;
const MESSENGER_EMBEDDED_RIGHT_DOCK_OVERLAY_BREAKPOINT = controller.MESSENGER_EMBEDDED_RIGHT_DOCK_OVERLAY_BREAKPOINT;
const MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT = controller.MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT;
const MESSENGER_PERF_TRACE_ENABLED = controller.MESSENGER_PERF_TRACE_ENABLED;
const MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT = controller.MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT;
const MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY = controller.MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY;
const MESSENGER_SEND_KEY_STORAGE_KEY = controller.MESSENGER_SEND_KEY_STORAGE_KEY;
const MESSENGER_TIGHT_HOST_BREAKPOINT = controller.MESSENGER_TIGHT_HOST_BREAKPOINT;
const MESSENGER_UI_FONT_SIZE_STORAGE_KEY = controller.MESSENGER_UI_FONT_SIZE_STORAGE_KEY;
const MessengerDialogsHost = controller.MessengerDialogsHost;
const MessengerFileContainerMenu = controller.MessengerFileContainerMenu;
const MessengerGroupDock = controller.MessengerGroupDock;
const MessengerHelpManualPanel = controller.MessengerHelpManualPanel;
const MessengerHivePlazaPanel = controller.MessengerHivePlazaPanel;
const messengerHivePlazaPanelRef = controller.messengerHivePlazaPanelRef;
const messengerInteractionBlockingLabel = controller.messengerInteractionBlockingLabel;
const messengerInteractionBlockReason = controller.messengerInteractionBlockReason;
const MessengerLocalFileSearchPanel = controller.MessengerLocalFileSearchPanel;
const MessengerMiddlePane = controller.MessengerMiddlePane;
const messengerOrderHydrating = controller.messengerOrderHydrating;
const messengerOrderReady = controller.messengerOrderReady;
const messengerOrderSaveTimer = controller.messengerOrderSaveTimer;
const messengerOrderSnapshot = controller.messengerOrderSnapshot;
const messengerPageWaitingState = controller.messengerPageWaitingState;
const MessengerRightDock = controller.MessengerRightDock;
const messengerRootRef = controller.messengerRootRef;
const messengerSendKey = controller.messengerSendKey;
const MessengerSettingsPanel = controller.MessengerSettingsPanel;
const MessengerTimelineDialog = controller.MessengerTimelineDialog;
const MessengerToolsSection = controller.MessengerToolsSection;
const messengerViewStyle = controller.messengerViewStyle;
const MessengerWorldComposer = controller.MessengerWorldComposer;
const middlePaneActiveSection = controller.middlePaneActiveSection;
const middlePaneActiveSectionSubtitle = controller.middlePaneActiveSectionSubtitle;
const middlePaneActiveSectionTitle = controller.middlePaneActiveSectionTitle;
const middlePaneMounted = controller.middlePaneMounted;
const middlePaneOverlayHideTimer = controller.middlePaneOverlayHideTimer;
const middlePaneOverlayVisible = controller.middlePaneOverlayVisible;
const middlePanePrewarmTimer = controller.middlePanePrewarmTimer;
const middlePaneRef = controller.middlePaneRef;
const middlePaneSearchPlaceholder = controller.middlePaneSearchPlaceholder;
const middlePaneTransitionName = controller.middlePaneTransitionName;
const mixedConversations = controller.mixedConversations;
const mountedAgentSettingModes = controller.mountedAgentSettingModes;
const moveAgentListItem = controller.moveAgentListItem;
const moveBeeroomGroupItem = controller.moveBeeroomGroupItem;
const moveMixedConversationItem = controller.moveMixedConversationItem;
const moveOwnedAgentsToFront = controller.moveOwnedAgentsToFront;
const navigationPaneCollapsed = controller.navigationPaneCollapsed;
const navigationPaneToggleTitle = controller.navigationPaneToggleTitle;
const nextTick = controller.nextTick;
const normalizeAbilityItemName = controller.normalizeAbilityItemName;
const normalizeAbilityNameList = controller.normalizeAbilityNameList;
const normalizeAgentApprovalMode = controller.normalizeAgentApprovalMode;
const normalizeAgentHiveGroupId = controller.normalizeAgentHiveGroupId;
const normalizeAgentId = controller.normalizeAgentId;
const normalizeAgentPresetQuestions = controller.normalizeAgentPresetQuestions;
const normalizeAgentUserRoundsKey = controller.normalizeAgentUserRoundsKey;
const normalizeAssistantMessageRuntimeState = controller.normalizeAssistantMessageRuntimeState;
const normalizeAvatarColor = controller.normalizeAvatarColor;
const normalizeAvatarIcon = controller.normalizeAvatarIcon;
const normalizeConversationPreviewText = controller.normalizeConversationPreviewText;
const normalizeDesktopUpdatePhase = controller.normalizeDesktopUpdatePhase;
const normalizeDismissedAgentConversationMap = controller.normalizeDismissedAgentConversationMap;
const normalizeExternalLink = controller.normalizeExternalLink;
const normalizeHexColor = controller.normalizeHexColor;
const normalizeMessengerSendKey = controller.normalizeMessengerSendKey;
const normalizeNumericMap = controller.normalizeNumericMap;
const normalizePlazaBrowseKind = controller.normalizePlazaBrowseKind;
const normalizeRightDockSkillCatalog = controller.normalizeRightDockSkillCatalog;
const normalizeRightDockSkillNameList = controller.normalizeRightDockSkillNameList;
const normalizeRightDockSkillRuntimeName = controller.normalizeRightDockSkillRuntimeName;
const normalizeRightDockSkillSummaryItems = controller.normalizeRightDockSkillSummaryItems;
const normalizeRouteQueryValue = controller.normalizeRouteQueryValue;
const normalizeRuntimeState = controller.normalizeRuntimeState;
const normalizeSandboxContainerId = controller.normalizeSandboxContainerId;
const normalizeSettingsPanelMode = controller.normalizeSettingsPanelMode;
const normalizeStringListUnique = controller.normalizeStringListUnique;
const normalizeThemePalette = controller.normalizeThemePalette;
const normalizeTimestamp = controller.normalizeTimestamp;
const normalizeToolEntry = controller.normalizeToolEntry;
const normalizeUiFontSize = controller.normalizeUiFontSize;
const normalizeUnitNode = controller.normalizeUnitNode;
const normalizeUnitShortLabel = controller.normalizeUnitShortLabel;
const normalizeUnitText = controller.normalizeUnitText;
const normalizeUploadPath = controller.normalizeUploadPath;
const normalizeWorkerCardImportProgress = controller.normalizeWorkerCardImportProgress;
const normalizeWorkspaceImageBlob = controller.normalizeWorkspaceImageBlob;
const normalizeWorkspaceOwnerId = controller.normalizeWorkspaceOwnerId;
const normalizeWorldContainerPickerEntry = controller.normalizeWorldContainerPickerEntry;
const normalizeWorldHistoryText = controller.normalizeWorldHistoryText;
const normalizeWorldMessageTimestamp = controller.normalizeWorldMessageTimestamp;
const notifyAgentTaskCompleted = controller.notifyAgentTaskCompleted;
const nudgeRightDockWidth = controller.nudgeRightDockWidth;
const onAgentRuntimeRefresh = controller.onAgentRuntimeRefresh;
const onBeforeUnmount = controller.onBeforeUnmount;
const onMounted = controller.onMounted;
const onUpdated = controller.onUpdated;
const onUserToolsUpdated = controller.onUserToolsUpdated;
const onWorkspaceRefresh = controller.onWorkspaceRefresh;
const openActiveAgentSettings = controller.openActiveAgentSettings;
const openAgentById = controller.openAgentById;
const openAgentDraftSession = controller.openAgentDraftSession;
const openAgentDraftSessionWithScroll = controller.openAgentDraftSessionWithScroll;
const openAgentPromptPreview = controller.openAgentPromptPreview;
const openAgentSession = controller.openAgentSession;
const openContactConversation = controller.openContactConversation;
const openContactConversationFromList = controller.openContactConversationFromList;
const openContainerFromRightDock = controller.openContainerFromRightDock;
const openContainerSettingsFromRightDock = controller.openContainerSettingsFromRightDock;
const openCreatedAgentSettings = controller.openCreatedAgentSettings;
const openDebugTools = controller.openDebugTools;
const openDesktopContainerSettings = controller.openDesktopContainerSettings;
const openDesktopModelSettingsFromHeader = controller.openDesktopModelSettingsFromHeader;
const openFileContainerMenu = controller.openFileContainerMenu;
const openHelperAppsDialog = controller.openHelperAppsDialog;
const openImagePreview = controller.openImagePreview;
const openMiddlePaneOverlay = controller.openMiddlePaneOverlay;
const openMixedConversation = controller.openMixedConversation;
const openMoreRailSection = controller.openMoreRailSection;
const openOrReuseFreshAgentSession = controller.openOrReuseFreshAgentSession;
const openProfilePage = controller.openProfilePage;
const openRightDockSkillDetail = controller.openRightDockSkillDetail;
const openSelectedContactConversation = controller.openSelectedContactConversation;
const openSelectedGroupConversation = controller.openSelectedGroupConversation;
const openSettingsPage = controller.openSettingsPage;
const openTimelineSessionDetail = controller.openTimelineSessionDetail;
const openWorkerCardImportPicker = controller.openWorkerCardImportPicker;
const openWorldContainerPicker = controller.openWorldContainerPicker;
const openWorldContainerPickerParent = controller.openWorldContainerPickerParent;
const openWorldContainerPickerPath = controller.openWorldContainerPickerPath;
const openWorldConversation = controller.openWorldConversation;
const openWorldHistoryDialog = controller.openWorldHistoryDialog;
const openWorldQuickPanel = controller.openWorldQuickPanel;
const OrchestrationWorkbench = controller.OrchestrationWorkbench;
const orderedBeeroomGroupsState = controller.orderedBeeroomGroupsState;
const orderedMixedConversationsState = controller.orderedMixedConversationsState;
const orderedOwnedAgentsState = controller.orderedOwnedAgentsState;
const orderedPrimaryAgents = controller.orderedPrimaryAgents;
const orderedSharedAgentsState = controller.orderedSharedAgentsState;
const orgUnitPathMap = controller.orgUnitPathMap;
const orgUnitTree = controller.orgUnitTree;
const ownedAgents = controller.ownedAgents;
const parseAgentLocalCommand = controller.parseAgentLocalCommand;
const parseWorkerCardText = controller.parseWorkerCardText;
const parseWorkspaceRefreshContainerId = controller.parseWorkspaceRefreshContainerId;
const parseWorkspaceResourceUrl = controller.parseWorkspaceResourceUrl;
const parseWorldVoicePayload = controller.parseWorldVoicePayload;
const pendingApprovalAgentIdSet = controller.pendingApprovalAgentIdSet;
const pendingAssistantCenter = controller.pendingAssistantCenter;
const pendingAssistantCenterCount = controller.pendingAssistantCenterCount;
const pendingRightDockPointerX = controller.pendingRightDockPointerX;
const persistAgentUnreadState = controller.persistAgentUnreadState;
const persistCurrentUserAppearance = controller.persistCurrentUserAppearance;
const persistDismissedAgentConversationState = controller.persistDismissedAgentConversationState;
const persistMessengerOrderPreferences = controller.persistMessengerOrderPreferences;
const persistWorldComposerHeight = controller.persistWorldComposerHeight;
const PlanPanel = controller.PlanPanel;
const plazaBrowseKind = controller.plazaBrowseKind;
const plazaStore = controller.plazaStore;
const pollDesktopUpdateState = controller.pollDesktopUpdateState;
const preferredBeeroomGroupId = controller.preferredBeeroomGroupId;
const preloadAgentById = controller.preloadAgentById;
const preloadAgentSettingsPanels = controller.preloadAgentSettingsPanels;
const preloadMessengerSettingsPanels = controller.preloadMessengerSettingsPanels;
const preloadMixedConversation = controller.preloadMixedConversation;
const preloadTimelinePreview = controller.preloadTimelinePreview;
const prepareMessageMarkdownContent = controller.prepareMessageMarkdownContent;
const previewMiddlePaneSection = controller.previewMiddlePaneSection;
const prioritizeImportedBeeroomAgents = controller.prioritizeImportedBeeroomAgents;
const PROFILE_AVATAR_COLORS = controller.PROFILE_AVATAR_COLORS;
const PROFILE_AVATAR_IMAGE_KEYS = controller.PROFILE_AVATAR_IMAGE_KEYS;
const PROFILE_AVATAR_IMAGE_MAP = controller.PROFILE_AVATAR_IMAGE_MAP;
const PROFILE_AVATAR_OPTION_KEYS = controller.PROFILE_AVATAR_OPTION_KEYS;
const profileAvatarColors = controller.profileAvatarColors;
const profileAvatarOptions = controller.profileAvatarOptions;
const pruneMessageVirtualHeightCache = controller.pruneMessageVirtualHeightCache;
const queuedSessionDetailPrefetchIds = controller.queuedSessionDetailPrefetchIds;
const queuePreviewMiddlePaneSection = controller.queuePreviewMiddlePaneSection;
const queueSessionDetailPrefetch = controller.queueSessionDetailPrefetch;
const quickCreateCopyFromAgents = controller.quickCreateCopyFromAgents;
const quickCreatingAgent = controller.quickCreatingAgent;
const readAgentVoiceModelSupport = controller.readAgentVoiceModelSupport;
const readDesktopDefaultModelMeta = controller.readDesktopDefaultModelMeta;
const readServerDefaultModelName = controller.readServerDefaultModelName;
const readWorldDraft = controller.readWorldDraft;
const REALTIME_CONTACT_REFRESH_MIN_MS = controller.REALTIME_CONTACT_REFRESH_MIN_MS;
const redirectToLoginAfterLogout = controller.redirectToLoginAfterLogout;
const ref = controller.ref;
const refreshActiveAgentConversation = controller.refreshActiveAgentConversation;
const refreshActiveBeeroom = controller.refreshActiveBeeroom;
const refreshActiveOrchestration = controller.refreshActiveOrchestration;
const refreshAgentMainUnreadCount = controller.refreshAgentMainUnreadCount;
const refreshAgentMainUnreadFromSessions = controller.refreshAgentMainUnreadFromSessions;
const refreshAgentMutationState = controller.refreshAgentMutationState;
const refreshAll = controller.refreshAll;
const refreshAudioRecordingSupport = controller.refreshAudioRecordingSupport;
const refreshBeeroomRealtimeActiveGroup = controller.refreshBeeroomRealtimeActiveGroup;
const refreshBeeroomRealtimeGroups = controller.refreshBeeroomRealtimeGroups;
const refreshHostWidth = controller.refreshHostWidth;
const refreshLatestAssistantMessageLayout = controller.refreshLatestAssistantMessageLayout;
const refreshMessengerRootBounds = controller.refreshMessengerRootBounds;
const refreshRealtimeChatSessions = controller.refreshRealtimeChatSessions;
const refreshRealtimeContacts = controller.refreshRealtimeContacts;
const refreshSessionPreviewCache = controller.refreshSessionPreviewCache;
const refreshWorldContainerPicker = controller.refreshWorldContainerPicker;
const rememberBeeroomDispatchSessionIds = controller.rememberBeeroomDispatchSessionIds;
const rememberWorldEmoji = controller.rememberWorldEmoji;
const renameTimelineSession = controller.renameTimelineSession;
const renderAgentMarkdown = controller.renderAgentMarkdown;
const renderMarkdown = controller.renderMarkdown;
const renderMessageMarkdown = controller.renderMessageMarkdown;
const renderSystemPromptHighlight = controller.renderSystemPromptHighlight;
const renderWorldMarkdown = controller.renderWorldMarkdown;
const replaceWorldAtPathTokens = controller.replaceWorldAtPathTokens;
const reportMessengerLayoutAnomaly = controller.reportMessengerLayoutAnomaly;
const requestAgentSettingsFocus = controller.requestAgentSettingsFocus;
const requestSystemNotificationPermission = controller.requestSystemNotificationPermission;
const resetAgentVoiceRecordingState = controller.resetAgentVoiceRecordingState;
const resetContactVirtualScroll = controller.resetContactVirtualScroll;
const resetRightDockWidth = controller.resetRightDockWidth;
const resetWorkerCardImportOverlay = controller.resetWorkerCardImportOverlay;
const resetWorkspaceImageCardState = controller.resetWorkspaceImageCardState;
const resetWorkspaceResourceCards = controller.resetWorkspaceResourceCards;
const resetWorldVoicePlaybackProgress = controller.resetWorldVoicePlaybackProgress;
const resetWorldVoiceRecordingState = controller.resetWorldVoiceRecordingState;
const resolveActiveAgentPromptPreviewKey = controller.resolveActiveAgentPromptPreviewKey;
const resolveAdminToolDetail = controller.resolveAdminToolDetail;
const resolveAgentConfiguredAbilityNames = controller.resolveAgentConfiguredAbilityNames;
const resolveAgentDependencyStatus = controller.resolveAgentDependencyStatus;
const resolveAgentDisplayName = controller.resolveAgentDisplayName;
const resolveAgentDraftIdentity = controller.resolveAgentDraftIdentity;
const resolveAgentHiveGroupId = controller.resolveAgentHiveGroupId;
const resolveAgentIconForDisplay = controller.resolveAgentIconForDisplay;
const resolveAgentInquirySelectionRoutes = controller.resolveAgentInquirySelectionRoutes;
const resolveAgentMarkdownWorkspacePath = controller.resolveAgentMarkdownWorkspacePath;
const resolveAgentMessageKey = controller.resolveAgentMessageKey;
const resolveAgentOverviewAbilityCounts = controller.resolveAgentOverviewAbilityCounts;
const resolveAgentRuntimeState = controller.resolveAgentRuntimeState;
const resolveAgentSelectionAfterRemoval = controller.resolveAgentSelectionAfterRemoval;
const resolveAgentUnreadStorageKeys = controller.resolveAgentUnreadStorageKeys;
const resolveAgentUserRounds = controller.resolveAgentUserRounds;
const resolveAssistantFailureNotice = controller.resolveAssistantFailureNotice;
const resolveAssistantMessageRuntimeState = controller.resolveAssistantMessageRuntimeState;
const resolveAttachmentContentType = controller.resolveAttachmentContentType;
const resolveAttachmentPublicPath = controller.resolveAttachmentPublicPath;
const resolveChatShellPath = controller.resolveChatShellPath;
const resolveCommandErrorMessage = controller.resolveCommandErrorMessage;
const resolveCompactApprovalOptionLabel = controller.resolveCompactApprovalOptionLabel;
const resolveComposerApprovalPersistAgentId = controller.resolveComposerApprovalPersistAgentId;
const resolveCurrentUserAppearance = controller.resolveCurrentUserAppearance;
const resolveCurrentUserScope = controller.resolveCurrentUserScope;
const resolveCurrentUserScopeAliases = controller.resolveCurrentUserScopeAliases;
const resolveDesktopContainerRoot = controller.resolveDesktopContainerRoot;
const resolveDesktopDefaultModelMeta = controller.resolveDesktopDefaultModelMeta;
const resolveDesktopUpdateProgress = controller.resolveDesktopUpdateProgress;
const resolveDesktopWorkspaceRoot = controller.resolveDesktopWorkspaceRoot;
const resolveDismissedAgentStorageKey = controller.resolveDismissedAgentStorageKey;
const resolvedMessageConversationKind = controller.resolvedMessageConversationKind;
const resolveEffectiveSessionBusy = controller.resolveEffectiveSessionBusy;
const resolveExplicitAgentModelName = controller.resolveExplicitAgentModelName;
const resolveExternalHost = controller.resolveExternalHost;
const resolveExternalIcon = controller.resolveExternalIcon;
const resolveExternalIconConfig = controller.resolveExternalIconConfig;
const resolveExternalIconStyle = controller.resolveExternalIconStyle;
const resolveFileContainerLifecycleText = controller.resolveFileContainerLifecycleText;
const resolveFileWorkspaceEmptyText = controller.resolveFileWorkspaceEmptyText;
const resolveFirstVisibleBeeroomGroupId = controller.resolveFirstVisibleBeeroomGroupId;
const resolveHttpStatus = controller.resolveHttpStatus;
const resolveLatestCompactionSnapshot = controller.resolveLatestCompactionSnapshot;
const resolveLatestConversationMessageTimestamp = controller.resolveLatestConversationMessageTimestamp;
const resolveLeftNavButtonLabel = controller.resolveLeftNavButtonLabel;
const resolveMarkdownWorkspacePath = controller.resolveMarkdownWorkspacePath;
const resolveMessageAgentAvatarState = controller.resolveMessageAgentAvatarState;
const resolveMessageModelName = controller.resolveMessageModelName;
const resolveMessengerPageWaitingSummary = controller.resolveMessengerPageWaitingSummary;
const resolveMessengerPageWaitingTarget = controller.resolveMessengerPageWaitingTarget;
const resolveMessengerRootElement = controller.resolveMessengerRootElement;
const resolveModelNameFromRecord = controller.resolveModelNameFromRecord;
const resolveOnlineFlag = controller.resolveOnlineFlag;
const resolvePreferredAgentSessionId = controller.resolvePreferredAgentSessionId;
const resolveRetainedSelectedPlazaItemId = controller.resolveRetainedSelectedPlazaItemId;
const resolveReusableFreshAgentSessionId = controller.resolveReusableFreshAgentSessionId;
const resolveRouteHelperWorkspaceEnabled = controller.resolveRouteHelperWorkspaceEnabled;
const resolveRouteSettingsPanelMode = controller.resolveRouteSettingsPanelMode;
const resolveSectionFromRoute = controller.resolveSectionFromRoute;
const resolveSessionActivityTimestamp = controller.resolveSessionActivityTimestamp;
const resolveSessionBusyRecoveryMessage = controller.resolveSessionBusyRecoveryMessage;
const resolveSessionLoadingFlag = controller.resolveSessionLoadingFlag;
const resolveSessionPreviewFromFields = controller.resolveSessionPreviewFromFields;
const resolveSessionRuntimeStatus = controller.resolveSessionRuntimeStatus;
const resolveSessionTimelinePreview = controller.resolveSessionTimelinePreview;
const resolveUnitIdKey = controller.resolveUnitIdKey;
const resolveUnitLabel = controller.resolveUnitLabel;
const resolveUnitTreeRowStyle = controller.resolveUnitTreeRowStyle;
const resolveUnread = controller.resolveUnread;
const resolveUploadedWorldPath = controller.resolveUploadedWorldPath;
const resolveUserAudioAttachments = controller.resolveUserAudioAttachments;
const resolveUserImageAttachments = controller.resolveUserImageAttachments;
const resolveVirtualMessageHeight = controller.resolveVirtualMessageHeight;
const resolveVoiceRecordingErrorText = controller.resolveVoiceRecordingErrorText;
const resolveWorkspaceResource = controller.resolveWorkspaceResource;
const resolveWorkspaceRootPrefix = controller.resolveWorkspaceRootPrefix;
const resolveWorkspaceScopeSuffix = controller.resolveWorkspaceScopeSuffix;
const resolveWorldContainerPickerParent = controller.resolveWorldContainerPickerParent;
const resolveWorldHistoryIcon = controller.resolveWorldHistoryIcon;
const resolveWorldMarkdownWorkspacePath = controller.resolveWorldMarkdownWorkspacePath;
const resolveWorldMessageDomId = controller.resolveWorldMessageDomId;
const resolveWorldMessageKey = controller.resolveWorldMessageKey;
const resolveWorldMessageSender = controller.resolveWorldMessageSender;
const resolveWorldRenderKey = controller.resolveWorldRenderKey;
const resolveWorldVoiceActionLabel = controller.resolveWorldVoiceActionLabel;
const resolveWorldVoiceContainerId = controller.resolveWorldVoiceContainerId;
const resolveWorldVoiceDurationLabel = controller.resolveWorldVoiceDurationLabel;
const resolveWorldVoicePayloadFromMessage = controller.resolveWorldVoicePayloadFromMessage;
const resolveWorldVoiceTotalDurationMs = controller.resolveWorldVoiceTotalDurationMs;
const restoreConversationFromRoute = controller.restoreConversationFromRoute;
const restoreTimelineSession = controller.restoreTimelineSession;
const resumeAgentMessage = controller.resumeAgentMessage;
const RIGHT_DOCK_EDGE_HOVER_THRESHOLD = controller.RIGHT_DOCK_EDGE_HOVER_THRESHOLD;
const RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS = controller.RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS;
const rightDockCollapsed = controller.rightDockCollapsed;
const rightDockDisabledSkills = controller.rightDockDisabledSkills;
const rightDockEdgeHover = controller.rightDockEdgeHover;
const rightDockEdgeHoverFrame = controller.rightDockEdgeHoverFrame;
const rightDockEnabledSkills = controller.rightDockEnabledSkills;
const rightDockRef = controller.rightDockRef;
const rightDockResizable = controller.rightDockResizable;
const rightDockSelectedSkill = controller.rightDockSelectedSkill;
const rightDockSelectedSkillEnabled = controller.rightDockSelectedSkillEnabled;
const rightDockSelectedSkillName = controller.rightDockSelectedSkillName;
const rightDockSkillAutoRetryTimer = controller.rightDockSkillAutoRetryTimer;
const rightDockSkillCatalog = controller.rightDockSkillCatalog;
const rightDockSkillCatalogLoading = controller.rightDockSkillCatalogLoading;
const rightDockSkillCatalogLoadVersion = controller.rightDockSkillCatalogLoadVersion;
const rightDockSkillContent = controller.rightDockSkillContent;
const rightDockSkillContentLoading = controller.rightDockSkillContentLoading;
const rightDockSkillContentLoadVersion = controller.rightDockSkillContentLoadVersion;
const rightDockSkillContentPath = controller.rightDockSkillContentPath;
const rightDockSkillDialogPath = controller.rightDockSkillDialogPath;
const rightDockSkillDialogTitle = controller.rightDockSkillDialogTitle;
const rightDockSkillDialogVisible = controller.rightDockSkillDialogVisible;
const rightDockSkillEnabledNameSet = controller.rightDockSkillEnabledNameSet;
const rightDockSkillItems = controller.rightDockSkillItems;
const rightDockSkillsLoading = controller.rightDockSkillsLoading;
const rightDockSkillToggleSaving = controller.rightDockSkillToggleSaving;
const rightDockStyle = controller.rightDockStyle;
const rightPanelAgentId = controller.rightPanelAgentId;
const rightPanelAgentIdForApi = controller.rightPanelAgentIdForApi;
const rightPanelContainerId = controller.rightPanelContainerId;
const rightPanelSessionHistory = controller.rightPanelSessionHistory;
const route = controller.route;
const router = controller.router;
const routeSectionIntent = controller.routeSectionIntent;
const routeSettingsPanelModeIntent = controller.routeSettingsPanelModeIntent;
const runningAgentsLoadedAt = controller.runningAgentsLoadedAt;
const runningAgentsLoadPromise = controller.runningAgentsLoadPromise;
const runningAgentsLoadVersion = controller.runningAgentsLoadVersion;
const runStartNewSession = controller.runStartNewSession;
const runtimeStateOverrides = controller.runtimeStateOverrides;
const runWithMessengerInteractionBlock = controller.runWithMessengerInteractionBlock;
const saveMessengerOrderPreferences = controller.saveMessengerOrderPreferences;
const saveObjectUrlAsFile = controller.saveObjectUrlAsFile;
const saveStoredStringArray = controller.saveStoredStringArray;
const saveUserAppearance = controller.saveUserAppearance;
const scheduleMessageViewportRefresh = controller.scheduleMessageViewportRefresh;
const scheduleMessageVirtualMeasure = controller.scheduleMessageVirtualMeasure;
const scheduleMessengerBootstrapBackgroundTasks = controller.scheduleMessengerBootstrapBackgroundTasks;
const scheduleMessengerOrderPersist = controller.scheduleMessengerOrderPersist;
const scheduleMiddlePaneOverlayHide = controller.scheduleMiddlePaneOverlayHide;
const scheduleMiddlePanePrewarm = controller.scheduleMiddlePanePrewarm;
const scheduleRightDockSkillAutoRetry = controller.scheduleRightDockSkillAutoRetry;
const scheduleSectionRouteSync = controller.scheduleSectionRouteSync;
const scheduleWorkspaceLoadingLabel = controller.scheduleWorkspaceLoadingLabel;
const scheduleWorkspaceResourceHydration = controller.scheduleWorkspaceResourceHydration;
const scheduleWorldQuickPanelClose = controller.scheduleWorldQuickPanelClose;
const screenshotDataUrlToFile = controller.screenshotDataUrlToFile;
const scrollLatestAssistantToCenter = controller.scrollLatestAssistantToCenter;
const scrollMessagesToBottom = controller.scrollMessagesToBottom;
const scrollVirtualMessageToIndex = controller.scrollVirtualMessageToIndex;
const searchableMiddlePaneSections = controller.searchableMiddlePaneSections;
const searchPlaceholder = controller.searchPlaceholder;
const sectionOptions = controller.sectionOptions;
const sectionRouteMap = controller.sectionRouteMap;
const sectionRouteSyncToken = controller.sectionRouteSyncToken;
const selectAgentForSettings = controller.selectAgentForSettings;
const selectAgentForSettingsFromMiddlePane = controller.selectAgentForSettingsFromMiddlePane;
const selectBeeroomGroup = controller.selectBeeroomGroup;
const selectBeeroomGroupFromMiddlePane = controller.selectBeeroomGroupFromMiddlePane;
const selectContact = controller.selectContact;
const selectContactFromMiddlePane = controller.selectContactFromMiddlePane;
const selectContainer = controller.selectContainer;
const selectContainerFromMiddlePane = controller.selectContainerFromMiddlePane;
const selectedAgentFileContainer = controller.selectedAgentFileContainer;
const selectedAgentHiveGroupId = controller.selectedAgentHiveGroupId;
const selectedAgentId = controller.selectedAgentId;
const selectedBeeroomGroup = controller.selectedBeeroomGroup;
const selectedContact = controller.selectedContact;
const selectedContactUnitId = controller.selectedContactUnitId;
const selectedContactUnitScope = controller.selectedContactUnitScope;
const selectedContactUserId = controller.selectedContactUserId;
const selectedFileAgentIdForApi = controller.selectedFileAgentIdForApi;
const selectedFileContainerAgentLabel = controller.selectedFileContainerAgentLabel;
const selectedFileContainerId = controller.selectedFileContainerId;
const selectedGroup = controller.selectedGroup;
const selectedGroupId = controller.selectedGroupId;
const selectedPlazaItemId = controller.selectedPlazaItemId;
const selectedToolCategory = controller.selectedToolCategory;
const selectedToolEntryKey = controller.selectedToolEntryKey;
const selectGroup = controller.selectGroup;
const selectGroupFromMiddlePane = controller.selectGroupFromMiddlePane;
const selectHelperApp = controller.selectHelperApp;
const selectHelperAppFromMiddlePane = controller.selectHelperAppFromMiddlePane;
const selectPlazaBrowseKindFromMiddlePane = controller.selectPlazaBrowseKindFromMiddlePane;
const selectPlazaItem = controller.selectPlazaItem;
const selectToolCategory = controller.selectToolCategory;
const selectToolCategoryFromMiddlePane = controller.selectToolCategoryFromMiddlePane;
const sendAgentMessage = controller.sendAgentMessage;
const sendDesktopNotification = controller.sendDesktopNotification;
const sendSystemNotification = controller.sendSystemNotification;
const sendWorldMessage = controller.sendWorldMessage;
const SERVER_DEFAULT_MODEL_CACHE_MS = controller.SERVER_DEFAULT_MODEL_CACHE_MS;
const serverDefaultModelCheckedAt = controller.serverDefaultModelCheckedAt;
const serverDefaultModelDisplayName = controller.serverDefaultModelDisplayName;
const serverDefaultModelFetchPromise = controller.serverDefaultModelFetchPromise;
const SESSION_DETAIL_PREFETCH_DELAY_MS = controller.SESSION_DETAIL_PREFETCH_DELAY_MS;
const SESSION_OPEN_RECOVERY_ATTEMPTS = controller.SESSION_OPEN_RECOVERY_ATTEMPTS;
const sessionDetailPrefetchTimer = controller.sessionDetailPrefetchTimer;
const sessionHub = controller.sessionHub;
const setAgentMainReadAt = controller.setAgentMainReadAt;
const setAgentMainUnreadCount = controller.setAgentMainUnreadCount;
const setContactVirtualListRef = controller.setContactVirtualListRef;
const setLanguage = controller.setLanguage;
const setNavigationPaneCollapsed = controller.setNavigationPaneCollapsed;
const setRightDockEdgeHover = controller.setRightDockEdgeHover;
const setRuntimeStateOverride = controller.setRuntimeStateOverride;
const setTimelineSessionMain = controller.setTimelineSessionMain;
const settingsAgentId = controller.settingsAgentId;
const settingsAgentIdForApi = controller.settingsAgentIdForApi;
const settingsAgentIdForPanel = controller.settingsAgentIdForPanel;
const settingsLogoutDisabled = controller.settingsLogoutDisabled;
const settingsPanelMode = controller.settingsPanelMode;
const settingsPanelRenderKey = controller.settingsPanelRenderKey;
const settingsRuntimeAgentIdForApi = controller.settingsRuntimeAgentIdForApi;
const settleAgentSessionBusyAfterRefresh = controller.settleAgentSessionBusyAfterRefresh;
const settleMessengerBootstrapTasks = controller.settleMessengerBootstrapTasks;
const setUserAttachmentResourceState = controller.setUserAttachmentResourceState;
const setWorkerCardImportCreatingOverlay = controller.setWorkerCardImportCreatingOverlay;
const setWorkerCardImportRefreshingOverlay = controller.setWorkerCardImportRefreshingOverlay;
const sharedAgents = controller.sharedAgents;
const shouldHandleWorkspaceResourceRefresh = controller.shouldHandleWorkspaceResourceRefresh;
const shouldHideAgentSettingsRightDock = controller.shouldHideAgentSettingsRightDock;
const shouldNotifyAgentCompletion = controller.shouldNotifyAgentCompletion;
const shouldRefreshAgentMeta = controller.shouldRefreshAgentMeta;
const shouldRefreshRealtimeChatSessions = controller.shouldRefreshRealtimeChatSessions;
const shouldRenderAgentMessage = controller.shouldRenderAgentMessage;
const shouldReuseAgentMetaResult = controller.shouldReuseAgentMetaResult;
const shouldShowAgentMessageBubble = controller.shouldShowAgentMessageBubble;
const shouldShowAgentResumeButton = controller.shouldShowAgentResumeButton;
const shouldShowCompactionDivider = controller.shouldShowCompactionDivider;
const shouldShowMessageStats = controller.shouldShowMessageStats;
const shouldVirtualizeMessages = controller.shouldVirtualizeMessages;
const showAgentComposerApprovalHint = controller.showAgentComposerApprovalHint;
const showAgentComposerApprovalSelector = controller.showAgentComposerApprovalSelector;
const showAgentGridOverview = controller.showAgentGridOverview;
const showAgentRightDock = controller.showAgentRightDock;
const showAgentSettingsPanel = controller.showAgentSettingsPanel;
const showApiError = controller.showApiError;
const showChatComposerFooter = controller.showChatComposerFooter;
const showChatSettingsView = controller.showChatSettingsView;
const showDefaultAgentEntry = controller.showDefaultAgentEntry;
const showGroupRightDock = controller.showGroupRightDock;
const showHelperAppsWorkspace = controller.showHelperAppsWorkspace;
const showHelpManualWaitingOverlay = controller.showHelpManualWaitingOverlay;
const showMessengerChatHeader = controller.showMessengerChatHeader;
const showMiddlePane = controller.showMiddlePane;
const showMiddlePaneHelperAppsWorkspace = controller.showMiddlePaneHelperAppsWorkspace;
const showNavigationCollapseToggle = controller.showNavigationCollapseToggle;
const showRightAgentPanels = controller.showRightAgentPanels;
const showRightDock = controller.showRightDock;
const showScrollBottomButton = controller.showScrollBottomButton;
const showScrollTopButton = controller.showScrollTopButton;
const skillDockUploading = controller.skillDockUploading;
const skillTools = controller.skillTools;
const sortedMixedConversations = controller.sortedMixedConversations;
const sortWorldContainerPickerEntries = controller.sortWorldContainerPickerEntries;
const splitMessengerBootstrapTasks = controller.splitMessengerBootstrapTasks;
const standardNavigationCollapsed = controller.standardNavigationCollapsed;
const startAgentVoiceRecording = controller.startAgentVoiceRecording;
const startAudioRecording = controller.startAudioRecording;
const startBeeroomRealtimeSync = controller.startBeeroomRealtimeSync;
const startMessengerPerfTrace = controller.startMessengerPerfTrace;
const startNewSession = controller.startNewSession;
const startRealtimePulse = controller.startRealtimePulse;
const startRightDockResize = controller.startRightDockResize;
const startWorldComposerResize = controller.startWorldComposerResize;
const startWorldVoiceRecording = controller.startWorldVoiceRecording;
const stopAgentMessage = controller.stopAgentMessage;
const stopAgentRuntimeRefreshListener = controller.stopAgentRuntimeRefreshListener;
const stopAgentVoiceRecordingAndSend = controller.stopAgentVoiceRecordingAndSend;
const stopBeeroomRealtimeSync = controller.stopBeeroomRealtimeSync;
const stopRealtimePulse = controller.stopRealtimePulse;
const stopUserToolsUpdatedListener = controller.stopUserToolsUpdatedListener;
const stopWorkspaceRefreshListener = controller.stopWorkspaceRefreshListener;
const stopWorldComposerResize = controller.stopWorldComposerResize;
const stopWorldVoicePlayback = controller.stopWorldVoicePlayback;
const stopWorldVoiceRecordingAndSend = controller.stopWorldVoiceRecordingAndSend;
const streamingAgentIdSet = controller.streamingAgentIdSet;
const submitAgentCreate = controller.submitAgentCreate;
const submitAgentQuickCreate = controller.submitAgentQuickCreate;
const submitGroupCreate = controller.submitGroupCreate;
const SUPPORTED_SKILL_ARCHIVE_SUFFIXES = controller.SUPPORTED_SKILL_ARCHIVE_SUFFIXES;
const suppressMessengerPageWaitingOverlay = controller.suppressMessengerPageWaitingOverlay;
const switchSection = controller.switchSection;
const syncAgentConversationFallback = controller.syncAgentConversationFallback;
const syncAgentPromptPreviewSelectedNames = controller.syncAgentPromptPreviewSelectedNames;
const syncContactVirtualMetrics = controller.syncContactVirtualMetrics;
const syncMessageVirtualMetrics = controller.syncMessageVirtualMetrics;
const syncRouteDrivenMessengerViewState = controller.syncRouteDrivenMessengerViewState;
const syncWorldVoicePlaybackProgress = controller.syncWorldVoicePlaybackProgress;
const systemNotificationPermissionRequested = controller.systemNotificationPermissionRequested;
const t = controller.t;
const TERMINAL_RUNTIME_STATUS_SET = controller.TERMINAL_RUNTIME_STATUS_SET;
const themeStore = controller.themeStore;
const timelineDetailDialogVisible = controller.timelineDetailDialogVisible;
const timelineDetailSessionId = controller.timelineDetailSessionId;
const timelineDialogVisible = controller.timelineDialogVisible;
const timelinePrefetchTimer = controller.timelinePrefetchTimer;
const timelinePreviewLoadingSet = controller.timelinePreviewLoadingSet;
const timelinePreviewMap = controller.timelinePreviewMap;
const toggleAgentOverviewMode = controller.toggleAgentOverviewMode;
const toggleAgentVoiceRecord = controller.toggleAgentVoiceRecord;
const toggleContactUnitExpanded = controller.toggleContactUnitExpanded;
const toggleLanguage = controller.toggleLanguage;
const toggleLeftRailMoreMenu = controller.toggleLeftRailMoreMenu;
const toggleNavigationPaneCollapsed = controller.toggleNavigationPaneCollapsed;
const toggleWorldQuickPanel = controller.toggleWorldQuickPanel;
const toggleWorldVoicePlayback = controller.toggleWorldVoicePlayback;
const toggleWorldVoiceRecord = controller.toggleWorldVoiceRecord;
const ToolApprovalComposer = controller.ToolApprovalComposer;
const toolCategoryLabel = controller.toolCategoryLabel;
const toolsCatalogLoaded = controller.toolsCatalogLoaded;
const toolsCatalogLoading = controller.toolsCatalogLoading;
const toolsCatalogLoadVersion = controller.toolsCatalogLoadVersion;
const triggerAgentSettingsDelete = controller.triggerAgentSettingsDelete;
const triggerAgentSettingsExport = controller.triggerAgentSettingsExport;
const triggerAgentSettingsReload = controller.triggerAgentSettingsReload;
const triggerAgentSettingsSave = controller.triggerAgentSettingsSave;
const triggerBeeroomRealtimeSyncRefresh = controller.triggerBeeroomRealtimeSyncRefresh;
const triggerPlazaPublish = controller.triggerPlazaPublish;
const triggerPlazaRefresh = controller.triggerPlazaRefresh;
const triggerRealtimePulseRefresh = controller.triggerRealtimePulseRefresh;
const triggerWorldScreenshot = controller.triggerWorldScreenshot;
const triggerWorldUpload = controller.triggerWorldUpload;
const trimAgentMainUnreadState = controller.trimAgentMainUnreadState;
const trimMarkdownCache = controller.trimMarkdownCache;
const tryParseJsonRecord = controller.tryParseJsonRecord;
const uiFontSize = controller.uiFontSize;
const unboundAgentFileContainers = controller.unboundAgentFileContainers;
const UNIT_UNGROUPED_ID = controller.UNIT_UNGROUPED_ID;
const updateAgentAbilityTooltip = controller.updateAgentAbilityTooltip;
const updateComposerApprovalMode = controller.updateComposerApprovalMode;
const updateCurrentUserAvatarColor = controller.updateCurrentUserAvatarColor;
const updateCurrentUserAvatarIcon = controller.updateCurrentUserAvatarIcon;
const updateCurrentUsername = controller.updateCurrentUsername;
const updateMessageScrollState = controller.updateMessageScrollState;
const updateProfile = controller.updateProfile;
const updateSendKey = controller.updateSendKey;
const updateThemePalette = controller.updateThemePalette;
const updateUiFontSize = controller.updateUiFontSize;
const uploadUserSkillZip = controller.uploadUserSkillZip;
const uploadWorldFilesToUserContainer = controller.uploadWorldFilesToUserContainer;
const uploadWunderWorkspace = controller.uploadWunderWorkspace;
const useAgentStore = controller.useAgentStore;
const useAuthStore = controller.useAuthStore;
const useBeeroomStore = controller.useBeeroomStore;
const useChatStore = controller.useChatStore;
const useComposerApprovalMode = controller.useComposerApprovalMode;
const useI18n = controller.useI18n;
const useMessengerHostWidth = controller.useMessengerHostWidth;
const useMessengerInteractionBlocker = controller.useMessengerInteractionBlocker;
const useMessengerRightDockResize = controller.useMessengerRightDockResize;
const useMiddlePaneOverlayPreview = controller.useMiddlePaneOverlayPreview;
const usePersistentStableListOrder = controller.usePersistentStableListOrder;
const usePlazaStore = controller.usePlazaStore;
const USER_CONTAINER_ID = controller.USER_CONTAINER_ID;
const USER_WORLD_UPLOAD_BASE = controller.USER_WORLD_UPLOAD_BASE;
const userAttachmentResourceCache = controller.userAttachmentResourceCache;
const userAttachmentWorkspacePaths = controller.userAttachmentWorkspacePaths;
const UserChannelSettingsPanel = controller.UserChannelSettingsPanel;
const usernameSaving = controller.usernameSaving;
const useRoute = controller.useRoute;
const useRouter = controller.useRouter;
const UserPromptSettingsPanel = controller.UserPromptSettingsPanel;
const userWorldPermissionDenied = controller.userWorldPermissionDenied;
const userWorldStore = controller.userWorldStore;
const useSessionHubStore = controller.useSessionHubStore;
const useStableMixedConversationOrder = controller.useStableMixedConversationOrder;
const useThemeStore = controller.useThemeStore;
const useUserWorldStore = controller.useUserWorldStore;
const viewportResizeFrame = controller.viewportResizeFrame;
const viewportResizeHandler = controller.viewportResizeHandler;
const viewportWidth = controller.viewportWidth;
const visibleAgentIdsForSelection = controller.visibleAgentIdsForSelection;
const visibleFilteredContacts = controller.visibleFilteredContacts;
const wait = controller.wait;
const warmMessengerUserToolsData = controller.warmMessengerUserToolsData;
const watch = controller.watch;
const withTrailingSeparator = controller.withTrailingSeparator;
const workerCardImporting = controller.workerCardImporting;
const workerCardImportInputRef = controller.workerCardImportInputRef;
const workerCardImportOverlayCurrent = controller.workerCardImportOverlayCurrent;
const workerCardImportOverlayPhase = controller.workerCardImportOverlayPhase;
const workerCardImportOverlayProgress = controller.workerCardImportOverlayProgress;
const workerCardImportOverlayTargetName = controller.workerCardImportOverlayTargetName;
const workerCardImportOverlayTotal = controller.workerCardImportOverlayTotal;
const workerCardImportOverlayVisible = controller.workerCardImportOverlayVisible;
const WorkerCardImportWaitingOverlay = controller.WorkerCardImportWaitingOverlay;
const workerCardToAgentPayload = controller.workerCardToAgentPayload;
const WorkspacePanel = controller.WorkspacePanel;
const workspacePanelKey = controller.workspacePanelKey;
const workspaceResourceCache = controller.workspaceResourceCache;
const workspaceResourceHydrationFrame = controller.workspaceResourceHydrationFrame;
const workspaceResourceHydrationPending = controller.workspaceResourceHydrationPending;
const WORLD_AT_PATH_RE = controller.WORLD_AT_PATH_RE;
const WORLD_AT_PATH_SUFFIX_RE = controller.WORLD_AT_PATH_SUFFIX_RE;
const WORLD_COMPOSER_HEIGHT_STORAGE_KEY = controller.WORLD_COMPOSER_HEIGHT_STORAGE_KEY;
const WORLD_EMOJI_CATALOG = controller.WORLD_EMOJI_CATALOG;
const WORLD_QUICK_EMOJI_STORAGE_KEY = controller.WORLD_QUICK_EMOJI_STORAGE_KEY;
const WORLD_UPLOAD_SIZE_LIMIT = controller.WORLD_UPLOAD_SIZE_LIMIT;
const WORLD_VOICE_RECORDING_TICK_MS = controller.WORLD_VOICE_RECORDING_TICK_MS;
const worldComposerHeight = controller.worldComposerHeight;
const worldComposerResizeRuntime = controller.worldComposerResizeRuntime;
const worldComposerStyle = controller.worldComposerStyle;
const worldComposerViewRef = controller.worldComposerViewRef;
const worldContainerPickerDisplayEntries = controller.worldContainerPickerDisplayEntries;
const worldContainerPickerEntries = controller.worldContainerPickerEntries;
const worldContainerPickerKeyword = controller.worldContainerPickerKeyword;
const worldContainerPickerLoading = controller.worldContainerPickerLoading;
const worldContainerPickerPath = controller.worldContainerPickerPath;
const worldContainerPickerPathLabel = controller.worldContainerPickerPathLabel;
const worldContainerPickerVisible = controller.worldContainerPickerVisible;
const worldDesktopScreenshotSupported = controller.worldDesktopScreenshotSupported;
const worldDraft = controller.worldDraft;
const worldDraftMap = controller.worldDraftMap;
const worldEmojiCatalog = controller.worldEmojiCatalog;
const worldHistoryActiveTab = controller.worldHistoryActiveTab;
const worldHistoryDateRange = controller.worldHistoryDateRange;
const worldHistoryDialogVisible = controller.worldHistoryDialogVisible;
const worldHistoryKeyword = controller.worldHistoryKeyword;
const worldHistoryRecords = controller.worldHistoryRecords;
const worldHistoryTabOptions = controller.worldHistoryTabOptions;
const worldQuickPanelCloseTimer = controller.worldQuickPanelCloseTimer;
const worldQuickPanelMode = controller.worldQuickPanelMode;
const worldRecentEmojis = controller.worldRecentEmojis;
const worldRenderableMessages = controller.worldRenderableMessages;
const worldUploading = controller.worldUploading;
const worldVoiceDurationMs = controller.worldVoiceDurationMs;
const worldVoiceLoadingMessageKey = controller.worldVoiceLoadingMessageKey;
const worldVoicePlaybackCurrentMs = controller.worldVoicePlaybackCurrentMs;
const worldVoicePlaybackDurationMs = controller.worldVoicePlaybackDurationMs;
const worldVoicePlaybackRuntime = controller.worldVoicePlaybackRuntime;
const worldVoicePlayingMessageKey = controller.worldVoicePlayingMessageKey;
const worldVoiceRecording = controller.worldVoiceRecording;
const worldVoiceRecordingRuntime = controller.worldVoiceRecordingRuntime;
const worldVoiceSupported = controller.worldVoiceSupported;
const writeWorldDraft = controller.writeWorldDraft;
</script>
