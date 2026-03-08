<template>
  <header class="messenger-middle-header">
    <div class="messenger-middle-title">{{ activeSectionTitle }}</div>
    <div class="messenger-middle-subtitle">{{ activeSectionSubtitle }}</div>
  </header>

  <div v-if="activeSection !== 'more' && !showHelperAppsWorkspace" class="messenger-search-row">
    <label class="messenger-search">
      <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
      <input
        :value="keyword"
        type="text"
        :placeholder="searchPlaceholder"
        autocomplete="off"
        spellcheck="false"
        @input="updateKeyword(($event.target as HTMLInputElement).value)"
      />
    </label>
    <button
      v-if="
        activeSection === 'agents' ||
        activeSection === 'swarms' ||
        (activeSection === 'groups' && !userWorldPermissionDenied && !showHelperAppsWorkspace)
      "
      class="messenger-plus-btn"
      type="button"
      :title="
        activeSection === 'groups'
          ? t('userWorld.group.create')
          : activeSection === 'swarms'
            ? t('beeroom.dialog.createTitle')
            : t('messenger.action.newAgent')
      "
      :aria-label="
        activeSection === 'groups'
          ? t('userWorld.group.create')
          : activeSection === 'swarms'
            ? t('beeroom.dialog.createTitle')
            : t('messenger.action.newAgent')
      "
      @click="handleSearchCreateAction"
    >
      <i class="fa-solid fa-plus" aria-hidden="true"></i>
    </button>
    <button
      v-if="activeSection === 'agents'"
      class="messenger-plus-btn"
      :class="{ active: agentOverviewMode === 'grid' }"
      type="button"
      :title="
        agentOverviewMode === 'grid'
          ? t('messenger.agent.listView')
          : t('messenger.agent.gridView')
      "
      :aria-label="
        agentOverviewMode === 'grid'
          ? t('messenger.agent.listView')
          : t('messenger.agent.gridView')
      "
      @click="toggleAgentOverviewMode"
    >
      <i class="fa-solid fa-table-cells-large" aria-hidden="true"></i>
    </button>
  </div>

  <div
    class="messenger-middle-list"
    :class="{ 'messenger-middle-list--users': activeSection === 'users' && !userWorldPermissionDenied }"
  >
    <template v-if="showHelperAppsWorkspace">
      <div class="messenger-helper-list">
        <div class="messenger-helper-section">
          <div class="messenger-helper-section-title">{{ t('userWorld.helperApps.offlineTitle') }}</div>
          <button
            v-for="item in helperAppsOfflineItems"
            :key="item.key"
            class="messenger-list-item messenger-helper-list-item"
            :class="{ active: isHelperAppActive('offline', item.key) }"
            type="button"
            @click="selectHelperApp('offline', item.key)"
          >
            <div class="messenger-list-avatar">
              <i class="fa-solid" :class="item.icon" aria-hidden="true"></i>
            </div>
            <div class="messenger-list-main">
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ item.title }}</span>
              </div>
              <div class="messenger-list-row">
                <span class="messenger-list-preview">{{ item.description }}</span>
              </div>
            </div>
          </button>
        </div>

        <div class="messenger-helper-section">
          <div class="messenger-helper-section-title">{{ t('userWorld.helperApps.onlineTitle') }}</div>
          <div v-if="helperAppsOnlineLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
          <template v-else>
            <button
              v-for="item in helperAppsOnlineItems"
              :key="item.linkId"
              class="messenger-list-item messenger-helper-list-item"
              :class="{ active: isHelperAppActive('online', item.linkId) }"
              type="button"
              @click="selectHelperApp('online', item.linkId)"
            >
              <div class="messenger-list-avatar">
                <i
                  class="fa-solid"
                  :class="resolveExternalIcon(item.icon)"
                  :style="resolveExternalIconStyle(item.icon)"
                  aria-hidden="true"
                ></i>
              </div>
              <div class="messenger-list-main">
                <div class="messenger-list-row">
                  <span class="messenger-list-name">{{ item.title }}</span>
                  <span class="messenger-list-time">{{ resolveExternalHost(item.url) }}</span>
                </div>
                <div class="messenger-list-row">
                  <span class="messenger-list-preview">
                    {{ item.description || resolveExternalHost(item.url) }}
                  </span>
                </div>
              </div>
            </button>
            <div v-if="!helperAppsOnlineItems.length" class="messenger-list-empty">
              {{ t('userWorld.helperApps.onlineEmpty') }}
            </div>
          </template>
        </div>
      </div>
    </template>

    <template v-else-if="activeSection === 'messages'">
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

    <template v-else-if="activeSection === 'users'">
      <div class="messenger-users-pane">
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
              @click="updateSelectedContactUnitId('')"
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
              @click="updateSelectedContactUnitId(row.id)"
              @keydown.enter.prevent="updateSelectedContactUnitId(row.id)"
              @keydown.space.prevent="updateSelectedContactUnitId(row.id)"
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
        <div v-if="userWorldPermissionDenied" class="messenger-list-empty">{{ t('auth.login.noPermission') }}</div>
        <template v-else>
          <div
            v-if="filteredContacts.length"
            :ref="setContactVirtualListRef"
            class="messenger-contact-virtual-list"
            @scroll.passive="handleContactVirtualScroll"
          >
            <div
              class="messenger-contact-virtual-spacer"
              :style="{ height: `${contactVirtualTopPadding}px` }"
              aria-hidden="true"
            ></div>
            <button
              v-for="contact in visibleFilteredContacts"
              :key="contact.user_id"
              class="messenger-list-item messenger-contact-item"
              :class="{ active: selectedContactUserId === String(contact.user_id || '') }"
              type="button"
              @click="selectContact(contact)"
              @dblclick="openContactConversationFromList(contact)"
            >
              <div class="messenger-list-avatar">{{ avatarLabel(contact.username || contact.user_id) }}</div>
              <div class="messenger-list-main">
                <div class="messenger-list-row">
                  <span class="messenger-list-name">{{ contact.username || contact.user_id }}</span>
                  <span class="messenger-presence-tag" :class="{ online: isContactOnline(contact) }">
                    {{ formatContactPresence(contact) }}
                  </span>
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
            <div
              class="messenger-contact-virtual-spacer"
              :style="{ height: `${contactVirtualBottomPadding}px` }"
              aria-hidden="true"
            ></div>
          </div>
          <div v-else class="messenger-list-empty">{{ t('messenger.empty.users') }}</div>
        </template>
      </div>
    </template>

    <template v-else-if="activeSection === 'groups'">
      <div v-if="userWorldPermissionDenied" class="messenger-list-empty">{{ t('auth.login.noPermission') }}</div>
      <template v-else>
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
    </template>

    <template v-else-if="activeSection === 'swarms'">
      <button
        v-for="group in filteredBeeroomGroups"
        :key="group.group_id"
        class="messenger-list-item messenger-agent-item"
        :class="{ active: selectedBeeroomGroupId === String(group.group_id || '') }"
        type="button"
        @click="selectBeeroomGroup(group)"
      >
        <div class="messenger-list-avatar">{{ avatarLabel(group.name || group.group_id) }}</div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ group.name || group.group_id }}</span>
            <span class="messenger-list-time">{{ group.running_mission_total || 0 }} {{ t('beeroom.summary.runningTeams') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">
              {{ group.description || group.mother_agent_name || t('beeroom.empty.description') }}
            </span>
            <span class="messenger-list-unread">{{ group.active_agent_total || 0 }}</span>
          </div>
        </div>
      </button>
      <div v-if="!filteredBeeroomGroups.length" class="messenger-list-empty">
        {{ t('messenger.empty.swarms') }}
      </div>
    </template>

    <template v-else-if="activeSection === 'agents'">
      <div class="messenger-block-title">{{ t('messenger.agent.owned') }}</div>
      <button
        class="messenger-list-item messenger-agent-item"
        :class="{ active: selectedAgentId === defaultAgentKey }"
        type="button"
        @click="selectAgentForSettings(defaultAgentKey)"
        @dblclick="openAgentById(defaultAgentKey)"
      >
        <AgentAvatar size="md" :state="resolveAgentRuntimeState(defaultAgentKey)" />
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.defaultAgent') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.defaultAgentDesc') }}</span>
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
        @dblclick="openAgentById(agent.id)"
      >
        <AgentAvatar size="md" :state="resolveAgentRuntimeState(agent.id)" />
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ agent.name || agent.id }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ agent.description || t('messenger.preview.empty') }}</span>
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
        @dblclick="openAgentById(agent.id)"
      >
        <AgentAvatar size="md" :state="resolveAgentRuntimeState(agent.id)" />
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ agent.name || agent.id }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ agent.description || t('messenger.preview.empty') }}</span>
          </div>
        </div>
      </button>
    </template>

    <template v-else-if="activeSection === 'tools'">
      <div class="messenger-block-title">{{ t('messenger.tools.adminTitle') }}</div>
      <button
        class="messenger-list-item"
        :class="{ active: selectedToolEntryKey === 'category:admin' }"
        type="button"
        @click="selectToolCategory('admin')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-shield-halved" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.tools.adminTitle') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.tools.adminDesc') }}</span>
          </div>
        </div>
      </button>

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

      <template v-if="!desktopLocalMode">
        <div class="messenger-block-title">{{ t('messenger.tools.sharedTitle') }}</div>
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
      </template>
    </template>

    <template v-else-if="activeSection === 'files'">
      <div class="messenger-block-title messenger-block-title--tight">{{ t('messenger.files.userContainer') }}</div>
      <button
        class="messenger-list-item"
        :class="{ active: fileScope === 'user' }"
        type="button"
        @click="selectContainer('user')"
        @contextmenu.prevent.stop="openFileContainerMenu($event, 'user', userContainerId)"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-user" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.files.userContainer') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">
              {{ t('messenger.files.userContainerDesc', { id: userContainerId }) }}
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
        @contextmenu.prevent.stop="openFileContainerMenu($event, 'agent', container.id)"
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
        @contextmenu.prevent.stop="openFileContainerMenu($event, 'agent', container.id)"
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
        @click="updateSettingsPanelMode('general')"
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
        @click="updateSettingsPanelMode('profile')"
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
        class="messenger-list-item"
        :class="{ active: settingsPanelMode === 'prompts' }"
        type="button"
        @click="updateSettingsPanelMode('prompts')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-file-lines" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.prompt.title') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.prompt.desc') }}</span>
          </div>
        </div>
      </button>
      <button
        v-if="desktopMode"
        class="messenger-list-item"
        :class="{ active: settingsPanelMode === 'desktop-models' }"
        type="button"
        @click="updateSettingsPanelMode('desktop-models')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-desktop" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.settings.desktopModels') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.settings.desktopModelsHint') }}</span>
          </div>
        </div>
      </button>
      <button
        v-if="desktopMode"
        class="messenger-list-item"
        :class="{ active: settingsPanelMode === 'desktop-lan' }"
        type="button"
        @click="updateSettingsPanelMode('desktop-lan')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-network-wired" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.settings.desktopLan') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.settings.desktopLanHint') }}</span>
          </div>
        </div>
      </button>
      <button
        v-if="desktopMode"
        class="messenger-list-item"
        :class="{ active: settingsPanelMode === 'desktop-remote' }"
        type="button"
        @click="updateSettingsPanelMode('desktop-remote')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-server" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.settings.desktopRemote') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.settings.desktopRemoteHint') }}</span>
          </div>
        </div>
      </button>
    </template>
  </div>

  <div v-if="activeSection === 'more'" class="messenger-middle-footer">
    <button
      class="messenger-middle-logout-btn"
      type="button"
      :disabled="settingsLogoutDisabled"
      @click="handleSettingsLogout"
    >
      <i class="fa-solid fa-right-from-bracket" aria-hidden="true"></i>
      <span>{{ t('nav.logout') }}</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';

const { t } = useI18n();

type ContainerEntry = {
  id: number;
  preview: string;
  agentNames: string[];
};

const {
  activeSection,
  activeSectionTitle,
  activeSectionSubtitle,
  showHelperAppsWorkspace,
  keyword,
  searchPlaceholder,
  agentOverviewMode,
  userWorldPermissionDenied,
  handleSearchCreateAction,
  toggleAgentOverviewMode,
  helperAppsOfflineItems,
  helperAppsOnlineItems,
  helperAppsOnlineLoading,
  isHelperAppActive,
  selectHelperApp,
  resolveExternalIcon,
  resolveExternalIconStyle,
  resolveExternalHost,
  filteredMixedConversations,
  isMixedConversationActive,
  openMixedConversation,
  resolveAgentRuntimeState,
  avatarLabel,
  formatTime,
  canDeleteMixedConversation,
  deleteMixedConversation,
  selectedContactUnitId,
  contactTotalCount,
  contactUnitTreeRows,
  resolveUnitTreeRowStyle,
  toggleContactUnitExpanded,
  filteredContacts,
  setContactVirtualListRef,
  handleContactVirtualScroll,
  contactVirtualTopPadding,
  contactVirtualBottomPadding,
  visibleFilteredContacts,
  selectedContactUserId,
  selectContact,
  openContactConversationFromList,
  isContactOnline,
  formatContactPresence,
  resolveUnread,
  filteredBeeroomGroups,
  selectedBeeroomGroupId,
  selectBeeroomGroup,
  filteredGroups,
  selectedGroupId,
  selectGroup,
  filteredOwnedAgents,
  filteredSharedAgents,
  selectedAgentId,
  defaultAgentKey,
  selectAgentForSettings,
  openAgentById,
  normalizeAgentId,
  selectedToolEntryKey,
  selectToolCategory,
  desktopLocalMode,
  fileScope,
  selectedFileContainerId,
  userContainerId,
  selectContainer,
  openFileContainerMenu,
  boundAgentFileContainers,
  unboundAgentFileContainers,
  settingsPanelMode,
  desktopMode,
  currentUsername,
  settingsLogoutDisabled,
  handleSettingsLogout
} = defineProps<{
  activeSection: string;
  activeSectionTitle: string;
  activeSectionSubtitle: string;
  showHelperAppsWorkspace: boolean;
  keyword: string;
  searchPlaceholder: string;
  agentOverviewMode: 'detail' | 'grid';
  userWorldPermissionDenied: boolean;
  handleSearchCreateAction: () => void | Promise<void>;
  toggleAgentOverviewMode: () => void;
  helperAppsOfflineItems: Array<{ key: string; title: string; description: string; icon: string }>;
  helperAppsOnlineItems: Array<{ linkId: string; title: string; description: string; icon: string; url: string }>;
  helperAppsOnlineLoading: boolean;
  isHelperAppActive: (kind: 'offline' | 'online', key: string) => boolean;
  selectHelperApp: (kind: 'offline' | 'online', key: string) => void;
  resolveExternalIcon: (icon: string) => string;
  resolveExternalIconStyle: (icon: string) => Record<string, string> | string;
  resolveExternalHost: (url: string) => string;
  filteredMixedConversations: Array<Record<string, any>>;
  isMixedConversationActive: (item: any) => boolean;
  openMixedConversation: (item: any) => void | Promise<void>;
  resolveAgentRuntimeState: (agentId: any) => any;
  avatarLabel: (value: unknown) => string;
  formatTime: (value: unknown) => string;
  canDeleteMixedConversation: (item: any) => boolean;
  deleteMixedConversation: (item: any) => void | Promise<void>;
  selectedContactUnitId: string;
  contactTotalCount: number;
  contactUnitTreeRows: Array<Record<string, any>>;
  resolveUnitTreeRowStyle: (row: any) => Record<string, string> | string;
  toggleContactUnitExpanded: (unitId: string) => void;
  filteredContacts: Array<Record<string, any>>;
  setContactVirtualListRef: (element: HTMLElement | null) => void;
  handleContactVirtualScroll: () => void;
  contactVirtualTopPadding: number;
  contactVirtualBottomPadding: number;
  visibleFilteredContacts: Array<Record<string, any>>;
  selectedContactUserId: string;
  selectContact: (contact: Record<string, any>) => void;
  openContactConversationFromList: (contact: Record<string, any>) => void | Promise<void>;
  isContactOnline: (contact: Record<string, any>) => boolean;
  formatContactPresence: (contact: Record<string, any>) => string;
  resolveUnread: (value: unknown) => number;
  filteredBeeroomGroups: Array<Record<string, any>>;
  selectedBeeroomGroupId: string;
  selectBeeroomGroup: (group: Record<string, any>) => void;
  filteredGroups: Array<Record<string, any>>;
  selectedGroupId: string;
  selectGroup: (group: Record<string, any>) => void;
  filteredOwnedAgents: Array<Record<string, any>>;
  filteredSharedAgents: Array<Record<string, any>>;
  selectedAgentId: string;
  defaultAgentKey: string;
  selectAgentForSettings: (agentId: any) => void;
  openAgentById: (agentId: any) => void | Promise<void>;
  normalizeAgentId: (value: unknown) => string;
  selectedToolEntryKey: string;
  selectToolCategory: (category: 'admin' | 'mcp' | 'skills' | 'knowledge' | 'shared') => void;
  desktopLocalMode: boolean;
  fileScope: 'agent' | 'user';
  selectedFileContainerId: number;
  userContainerId: number;
  selectContainer: (containerId: number | 'user') => void;
  openFileContainerMenu: (event: MouseEvent, scope: 'agent' | 'user', containerId: number) => void;
  boundAgentFileContainers: ContainerEntry[];
  unboundAgentFileContainers: ContainerEntry[];
  settingsPanelMode: string;
  desktopMode: boolean;
  currentUsername: string;
  settingsLogoutDisabled: boolean;
  handleSettingsLogout: () => void;
}>();

const emit = defineEmits<{
  (event: 'update:keyword', value: string): void;
  (event: 'update:selectedContactUnitId', value: string): void;
  (event: 'update:settingsPanelMode', value: string): void;
}>();

const updateKeyword = (value: string) => {
  emit('update:keyword', value);
};

const updateSelectedContactUnitId = (value: string) => {
  emit('update:selectedContactUnitId', value);
};

const updateSettingsPanelMode = (value: string) => {
  emit('update:settingsPanelMode', value);
};
</script>
