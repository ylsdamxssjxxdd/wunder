<template>
  <header class="messenger-middle-header">
    <div class="messenger-middle-title">{{ activeSectionTitle }}</div>
    <div v-if="activeSectionSubtitle" class="messenger-middle-subtitle">{{ activeSectionSubtitle }}</div>
  </header>

  <div v-if="showMiddlePaneSearch" class="messenger-search-row">
    <div v-if="activeSection === 'agents'" class="messenger-search-slot">
      <el-dropdown
        trigger="click"
        placement="bottom-start"
        @command="handleAgentFilterCommand"
      >
        <button class="messenger-search messenger-search-select" type="button">
          <span class="messenger-search-select-label">{{ selectedAgentHiveGroupLabel }}</span>
          <i class="fa-solid fa-chevron-down messenger-search-select-caret" aria-hidden="true"></i>
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="__all__">
              {{ t('messenger.agents.hiveAll') }}
            </el-dropdown-item>
            <el-dropdown-item
              v-for="row in agentHiveTreeRows"
              :key="`agent-filter-${row.id}`"
              :command="`hive:${row.id}`"
            >
              {{ row.label }}
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
    <label v-if="activeSection !== 'agents'" class="messenger-search">
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
    <el-dropdown
      v-if="activeSection === 'swarms'"
      trigger="click"
      placement="bottom-end"
      @command="handleSwarmPlusCommand"
    >
      <button
        class="messenger-plus-btn"
        type="button"
        :title="resolveSwarmPlusActionLabel()"
        :aria-label="resolveSwarmPlusActionLabel()"
      >
        <i class="fa-solid fa-plus" aria-hidden="true"></i>
      </button>
      <template #dropdown>
        <el-dropdown-menu>
          <el-dropdown-item
            command="import"
            :disabled="beeroomStore.packImportLoading || beeroomStore.packExportLoading"
          >
            {{ t('beeroom.pack.action.import') }}
          </el-dropdown-item>
          <el-dropdown-item command="create">
            {{ t('beeroom.dialog.createTitle') }}
          </el-dropdown-item>
        </el-dropdown-menu>
      </template>
    </el-dropdown>
    <div v-if="activeSection === 'agents'" class="messenger-search-actions">
      <el-dropdown
        trigger="click"
        placement="bottom-end"
        @command="handleAgentPlusCommand"
      >
        <button
          class="messenger-plus-btn"
          type="button"
          :title="resolvePlusActionLabel()"
          :aria-label="resolvePlusActionLabel()"
        >
          <i class="fa-solid fa-plus" aria-hidden="true"></i>
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="create">{{ t('messenger.action.newAgent') }}</el-dropdown-item>
            <el-dropdown-item command="import_worker_card">{{ t('portal.agent.importWorkerCard') }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <button
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
    <button
      v-else-if="activeSection === 'groups' && !userWorldPermissionDenied && !showHelperAppsWorkspace"
      class="messenger-plus-btn"
      type="button"
      :title="resolvePlusActionLabel()"
      :aria-label="resolvePlusActionLabel()"
      @click="handlePlusAction"
    >
      <i class="fa-solid fa-plus" aria-hidden="true"></i>
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
      <template v-for="(item, index) in displayedMixedConversations" :key="item.key">
        <div
        class="messenger-list-item messenger-conversation-item"
        :class="{
          active: isGuidedDefaultConversation(item)
            ? selectedAgentId === normalizeAgentId(defaultAgentKey)
            : isMixedConversationActive(item),
          'messenger-conversation-item--guided': isGuidedDefaultConversation(item),
          'is-running': item.kind === 'agent' && resolveAgentRuntimeState(item.agentId) === 'running',
          'is-dragging': dragState.section === 'messages' && dragState.key === resolveConversationItemKey(item),
          'is-drag-origin-hidden': isDragOriginHidden('messages', resolveConversationItemKey(item)),
          'is-drop-before': isDropBefore('messages', index),
          'is-drop-after': isDropAfter('messages', index, displayedMixedConversations.length)
        }"
        role="button"
        tabindex="0"
        draggable="true"
        :data-drag-key="resolveConversationItemKey(item)"
        @pointerenter="preloadMixedConversation(item)"
        @focus="preloadMixedConversation(item)"
        @click="openConversationFromList(item)"
        @keydown.enter.prevent="openConversationFromList(item)"
        @keydown.space.prevent="openConversationFromList(item)"
        @dragstart="handleConversationDragStart($event, item)"
        @dragover="handleConversationDragOver($event, index)"
        @drop="handleConversationDrop($event)"
        @dragend="handleDragEnd"
      >
        <AgentAvatar
          v-if="item.kind === 'agent'"
          size="md"
          :state="resolveAgentRuntimeState(item.agentId)"
          :icon="item.icon"
          :name="item.title"
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
          v-if="!isGuidedDefaultConversation(item) && canDeleteMixedConversation(item)"
          class="messenger-list-item-action"
          type="button"
          :title="t('chat.history.delete')"
          :aria-label="t('chat.history.delete')"
          @click.stop="deleteMixedConversation(item)"
        >
          <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
        </button>
        </div>
      </template>
      <div v-if="!displayedMixedConversations.length" class="messenger-list-empty">
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
      <template v-for="(group, index) in filteredBeeroomGroups" :key="group.group_id">
        <div
        class="messenger-list-item messenger-agent-item messenger-swarm-item"
        :class="{
          active: selectedBeeroomGroupId === String(group.group_id || ''),
          'is-running': isBeeroomGroupRunning(group),
          'is-dragging': dragState.section === 'swarms' && dragState.key === resolveSwarmDragKey(group),
          'is-drag-origin-hidden': isDragOriginHidden('swarms', resolveSwarmDragKey(group)),
          'is-drop-before': isDropBefore('swarms', index),
          'is-drop-after': isDropAfter('swarms', index, filteredBeeroomGroups.length)
        }"
        role="button"
        tabindex="0"
        draggable="true"
        :data-drag-key="resolveSwarmDragKey(group)"
        @click="selectBeeroomGroup(group)"
        @keydown.enter.prevent="selectBeeroomGroup(group)"
        @keydown.space.prevent="selectBeeroomGroup(group)"
        @dragstart="handleSwarmDragStart($event, group)"
        @dragover="handleSwarmDragOver($event, index)"
        @drop="handleSwarmDrop($event)"
        @dragend="handleDragEnd"
      >
        <div class="messenger-list-avatar">{{ avatarLabel(group.name || group.group_id) }}</div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ group.name || group.group_id }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">
              {{ group.description || group.mother_agent_name || t('beeroom.empty.description') }}
            </span>
          </div>
        </div>
        <div class="messenger-list-item-actions">
          <button
            class="messenger-list-item-action"
            type="button"
            :title="t('beeroom.pack.action.exportFull')"
            :aria-label="t('beeroom.pack.action.exportFull')"
            :disabled="beeroomStore.packImportLoading || beeroomStore.packExportLoading"
            @click.stop="handleSwarmExport(group)"
          >
            <i class="fa-solid fa-download" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-list-item-action"
            type="button"
            :title="t('common.edit')"
            :aria-label="t('common.edit')"
            @click.stop="openSwarmEditDialog(group)"
          >
            <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
          </button>
        </div>
        </div>
      </template>
      <div v-if="!filteredBeeroomGroups.length" class="messenger-list-empty">
        {{ t('messenger.empty.swarms') }}
      </div>
    </template>

    <template v-else-if="activeSection === 'plaza'">
      <button
        v-for="page in plazaBrowsePages"
        :key="page.kind"
        class="messenger-list-item messenger-plaza-page-item"
        :class="{ active: selectedPlazaBrowseKind === page.kind }"
        type="button"
        @click="selectPlazaBrowseKind(page.kind)"
      >
        <div class="messenger-list-avatar messenger-plaza-page-avatar" :class="`is-${page.kind}`">
          <img
            v-if="page.imageUrl"
            class="messenger-plaza-page-avatar-image"
            :src="page.imageUrl"
            :alt="page.label"
          />
          <i
            v-else
            class="fa-solid fa-wand-magic-sparkles"
            aria-hidden="true"
          ></i>
        </div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ page.label }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ page.description }}</span>
          </div>
        </div>
      </button>
    </template>

    <template v-else-if="activeSection === 'agents'">
      <div class="messenger-unit-structure messenger-unit-structure--agents">
        <div class="messenger-unit-structure-head">
          <span class="messenger-unit-structure-title">{{ t('messenger.agents.hiveTitle') }}</span>
          <span class="messenger-unit-structure-hint">{{ t('messenger.agents.hiveHint') }}</span>
        </div>
        <div class="messenger-unit-structure-actions">
          <button
            class="messenger-unit-all-btn"
            :class="{ active: !selectedAgentHiveGroupId }"
            type="button"
            @click="updateSelectedAgentHiveGroupId('')"
          >
            <span class="messenger-unit-tree-name">{{ t('messenger.agents.hiveAll') }}</span>
            <span class="messenger-unit-tree-count">{{ agentHiveTotalCount }}</span>
          </button>
        </div>
        <div v-if="agentHiveTreeRows.length" class="messenger-unit-tree">
          <div
            v-for="row in agentHiveTreeRows"
            :key="`agent-hive-tree-${row.id}`"
            class="messenger-unit-tree-row messenger-unit-tree-row--leaf"
            :class="{ active: selectedAgentHiveGroupId === row.id }"
            :style="resolveUnitTreeRowStyle(row)"
            role="button"
            tabindex="0"
            @click="updateSelectedAgentHiveGroupId(row.id)"
            @keydown.enter.prevent="updateSelectedAgentHiveGroupId(row.id)"
            @keydown.space.prevent="updateSelectedAgentHiveGroupId(row.id)"
          >
            <span class="messenger-unit-tree-toggle messenger-unit-tree-toggle--placeholder"></span>
            <span class="messenger-unit-tree-icon" aria-hidden="true">
              <i class="fa-solid fa-hexagon-nodes"></i>
            </span>
            <span class="messenger-unit-tree-name">{{ row.label }}</span>
            <span class="messenger-unit-tree-count">{{ row.count }}</span>
          </div>
        </div>
      </div>

      <template v-for="(agent, index) in orderedPrimaryAgents" :key="agent.renderKey">
        <button
        class="messenger-list-item messenger-agent-item"
        :class="{
          active: selectedAgentId === agent.agentId,
          selected: isAgentMultiSelected(agent.agentId),
          'is-running': resolveAgentRuntimeState(agent.agentId) === 'running',
          'is-dragging': dragState.section === 'agents-primary' && dragState.key === resolveAgentDragKey(agent.agentId),
          'is-drag-origin-hidden': isDragOriginHidden('agents-primary', resolveAgentDragKey(agent.agentId)),
          'is-drop-before': isDropBefore('agents-primary', index),
          'is-drop-after': isDropAfter('agents-primary', index, orderedPrimaryAgents.length)
        }"
        type="button"
        draggable="true"
        :data-drag-key="resolveAgentDragKey(agent.agentId)"
        @pointerenter="preloadAgentById(agent.agentId)"
        @focus="preloadAgentById(agent.agentId)"
        @click="handleAgentSelectionClick($event, agent.agentId)"
        @dblclick="handleAgentOpenById(agent.agentId)"
        @contextmenu.prevent.stop="openAgentContextMenu($event, agent.agentId)"
        @dragstart="handlePrimaryAgentDragStart($event, agent.agentId)"
        @dragover="handlePrimaryAgentDragOver($event, index)"
        @drop="handlePrimaryAgentDrop($event)"
        @dragend="handleDragEnd"
      >
        <AgentAvatar
          size="md"
          :state="resolveAgentRuntimeState(agent.agentId)"
          :icon="agent.icon"
          :name="agent.name"
        />
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ agent.name }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ agent.description }}</span>
          </div>
        </div>
        </button>
      </template>
      <div v-if="filteredSharedAgents.length" class="messenger-block-title">
        {{ t('messenger.agent.shared') }}
      </div>
      <template v-for="(agent, index) in filteredSharedAgents" :key="`shared-${agent.id}`">
        <button
        class="messenger-list-item messenger-agent-item"
        :class="{
          active: selectedAgentId === normalizeAgentId(agent.id),
          selected: isAgentMultiSelected(agent.id),
          'is-running': resolveAgentRuntimeState(agent.id) === 'running',
          'is-dragging': dragState.section === 'agents-shared' && dragState.key === resolveAgentDragKey(agent.id),
          'is-drag-origin-hidden': isDragOriginHidden('agents-shared', resolveAgentDragKey(agent.id)),
          'is-drop-before': isDropBefore('agents-shared', index),
          'is-drop-after': isDropAfter('agents-shared', index, filteredSharedAgents.length)
        }"
        type="button"
        draggable="true"
        :data-drag-key="resolveAgentDragKey(agent.id)"
        @pointerenter="preloadAgentById(agent.id)"
        @focus="preloadAgentById(agent.id)"
        @click="handleAgentSelectionClick($event, agent.id)"
        @dblclick="handleAgentOpenById(agent.id)"
        @contextmenu.prevent.stop="openAgentContextMenu($event, agent.id)"
        @dragstart="handleSharedAgentDragStart($event, agent.id)"
        @dragover="handleSharedAgentDragOver($event, index)"
        @drop="handleSharedAgentDrop($event)"
        @dragend="handleDragEnd"
      >
        <AgentAvatar
          size="md"
          :state="resolveAgentRuntimeState(agent.id)"
          :icon="agent.icon"
          :name="agent.name || agent.id"
        />
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
      <div v-if="!orderedPrimaryAgents.length && !filteredSharedAgents.length" class="messenger-list-empty">
        {{ t('messenger.empty.agents') }}
      </div>
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
        <div class="messenger-list-avatar"><i class="fa-solid fa-book" aria-hidden="true"></i></div>
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
        class="messenger-list-item"
        :class="{ active: settingsPanelMode === 'help-manual' }"
        type="button"
        @click="updateSettingsPanelMode('help-manual')"
      >
        <div class="messenger-list-avatar"><i class="fa-solid fa-book-open" aria-hidden="true"></i></div>
        <div class="messenger-list-main">
          <div class="messenger-list-row">
            <span class="messenger-list-name">{{ t('messenger.settings.helpManual') }}</span>
          </div>
          <div class="messenger-list-row">
            <span class="messenger-list-preview">{{ t('messenger.settings.helpManualHint') }}</span>
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

  <BeeroomCreateDialog
    v-model="swarmCreateVisible"
    :candidate-agents="swarmCreateCandidateAgents"
    :saving="swarmCreateSaving"
    @submit="handleSwarmCreateSubmit"
  />

  <BeeroomCreateDialog
    v-model="swarmEditVisible"
    mode="edit"
    :candidate-agents="swarmCreateCandidateAgents"
    :initial-group="swarmEditingGroup"
    :saving="swarmEditSaving"
    :deleting="swarmEditDeleting"
    @submit="handleSwarmEditSubmit"
    @delete="handleSwarmEditDelete"
  />

  <input
    ref="swarmPackInputRef"
    type="file"
    accept=".hivepack,.zip,application/zip,application/vnd.wunder.hivepack+zip"
    style="display: none"
    @change="handleSwarmPackFileChange"
  />

  <BeeroomPackWaitingOverlay
    :visible="packOverlayVisible"
    :mode="activePackMode"
    :phase="activePackJob?.phase"
    :progress="activePackJob?.progress"
    :summary="activePackJob?.summary"
    :target-name="packOverlayTargetName"
  />

  <Teleport to="body">
    <div
      v-if="agentContextMenuVisible"
      class="messenger-files-context-menu messenger-agent-context-menu"
      :style="agentContextMenuStyle"
      @contextmenu.prevent
    >
      <div class="messenger-agent-context-menu__meta">
        {{ t('messenger.agent.selection', { count: selectedAgentIds.length }) }}
      </div>
      <button class="messenger-files-menu-btn" type="button" @click="handleAgentContextExport">
        {{ t('messenger.agent.context.exportSelected') }}
      </button>
      <button
        class="messenger-files-menu-btn danger"
        type="button"
        :disabled="!deletableSelectedAgentIds.length"
        @click="handleAgentContextDelete"
      >
        {{ t('messenger.agent.context.deleteSelected') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="clearAgentSelection">
        {{ t('messenger.agent.context.clearSelection') }}
      </button>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, h, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { useI18n } from '@/i18n';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import BeeroomCreateDialog from '@/components/beeroom/BeeroomCreateDialog.vue';
import BeeroomPackWaitingOverlay from '@/components/beeroom/BeeroomPackWaitingOverlay.vue';
import avatar046Url from '@/assets/agent-avatars/avatar-046.png';
import avatar016Url from '@/assets/agent-avatars/avatar-016.png';
import type { PlazaBrowseKind } from '@/components/messenger/hivePlazaPanelState';
import { useBeeroomStore } from '@/stores/beeroom';
import { runUnsavedChangesGuards } from '@/utils/unsavedChangesGuard';

const { t } = useI18n();
const beeroomStore = useBeeroomStore();
const swarmPackInputRef = ref<HTMLInputElement | null>(null);
const swarmCreateVisible = ref(false);
const swarmCreateSaving = ref(false);
const swarmEditVisible = ref(false);
const swarmEditSaving = ref(false);
const swarmEditDeleting = ref(false);
const swarmEditingGroup = ref<Record<string, any> | null>(null);
const packOverlayMode = ref<'import' | 'export'>('export');
const packOverlayTargetName = ref('');
const selectedAgentIds = ref<string[]>([]);
const agentSelectionAnchorId = ref('');
const agentContextMenuVisible = ref(false);
const agentContextMenuStyle = ref<Record<string, string>>({});
const dragState = ref<{
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms' | '';
  key: string;
  sourceIndex: number;
}>({
  section: '',
  key: '',
  sourceIndex: -1
});
const dragOverState = ref<{
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms' | '';
  index: number;
}>({
  section: '',
  index: -1
});
const dragOriginHiddenState = ref<{
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms' | '';
  key: string;
}>({
  section: '',
  key: ''
});
let dragOriginHideFrame = 0;

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
  preloadMixedConversation,
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
  deleteBeeroomGroup,
  selectedPlazaBrowseKind,
  selectPlazaBrowseKind,
  filteredGroups,
  selectedGroupId,
  selectGroup,
  selectedAgentHiveGroupId,
  agentHiveTotalCount,
  agentHiveTreeRows,
  primaryAgentItems,
  filteredOwnedAgents,
  filteredSharedAgents,
  showDefaultAgentEntry,
  selectedAgentId,
  defaultAgentKey,
  defaultAgentIcon,
  selectAgentForSettings,
  openAgentById,
  preloadAgentById,
  normalizeAgentId,
  handleAgentBatchExport,
  handleAgentBatchDelete,
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
  handleSettingsLogout,
  moveMessageItem,
  moveAgentItem,
  moveSwarmItem,
  afterHivePackImported
} = defineProps<{
  activeSection: string;
  activeSectionTitle: string;
  activeSectionSubtitle: string;
  showHelperAppsWorkspace: boolean;
  keyword: string;
  searchPlaceholder: string;
  agentOverviewMode: 'detail' | 'grid';
  userWorldPermissionDenied: boolean;
  handleSearchCreateAction: (command?: string) => void | Promise<void>;
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
  preloadMixedConversation: (item: any) => void;
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
  deleteBeeroomGroup: (group: Record<string, any>) => void | Promise<void>;
  selectedPlazaBrowseKind: PlazaBrowseKind;
  selectPlazaBrowseKind: (kind: PlazaBrowseKind) => void;
  filteredGroups: Array<Record<string, any>>;
  selectedGroupId: string;
  selectGroup: (group: Record<string, any>) => void;
  selectedAgentHiveGroupId: string;
  agentHiveTotalCount: number;
  agentHiveTreeRows: Array<Record<string, any>>;
  primaryAgentItems: Array<Record<string, any>>;
  filteredOwnedAgents: Array<Record<string, any>>;
  filteredSharedAgents: Array<Record<string, any>>;
  showDefaultAgentEntry: boolean;
  selectedAgentId: string;
  defaultAgentKey: string;
  defaultAgentIcon?: unknown;
  selectAgentForSettings: (agentId: any) => void;
  openAgentById: (agentId: any) => void | Promise<void>;
  preloadAgentById: (agentId: any) => void;
  normalizeAgentId: (value: unknown) => string;
  handleAgentBatchExport: (agentIds: string[]) => void | Promise<void>;
  handleAgentBatchDelete: (agentIds: string[]) => void | Promise<void>;
  selectedToolEntryKey: string;
  selectToolCategory: (category: 'admin' | 'mcp' | 'skills' | 'knowledge') => void;
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
  moveMessageItem: (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => void;
  moveAgentItem: (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => void;
  moveSwarmItem: (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => void;
  afterHivePackImported?: (job: unknown) => void | Promise<void>;
}>();


type AgentSelectionEntry = {
  id: string;
  deletable: boolean;
};

const visibleSelectableAgentItems = computed<AgentSelectionEntry[]>(() => {
  const output: AgentSelectionEntry[] = [];
  (Array.isArray(primaryAgentItems) ? primaryAgentItems : []).forEach((agent) => {
    const id = normalizeAgentId(agent?.id || agent?.agentId);
    if (!id) return;
    output.push({ id, deletable: id !== normalizeAgentId(defaultAgentKey) });
  });
  (Array.isArray(filteredSharedAgents) ? filteredSharedAgents : []).forEach((agent) => {
    const id = normalizeAgentId(agent?.id);
    if (!id) return;
    output.push({ id, deletable: false });
  });
  return output;
});

const orderedPrimaryAgents = computed(() =>
  (Array.isArray(primaryAgentItems) ? primaryAgentItems : []).map((agent, index) => {
    const agentId = normalizeAgentId(agent?.id || agent?.agentId);
    return {
      renderKey: String(agent?.renderKey || `primary:${agentId || index}`),
      agentId,
      name: String(agent?.name || agent?.agentId || agent?.id || t('messenger.defaultAgent')),
      description: String(agent?.description || t('messenger.preview.empty')),
      icon: agent?.icon
    };
  })
);

const selectedAgentHiveGroupLabel = computed(() => {
  if (!selectedAgentHiveGroupId) {
    return t('messenger.agents.hiveAll');
  }
  const matched = (Array.isArray(agentHiveTreeRows) ? agentHiveTreeRows : []).find(
    (row) => String(row?.id || '').trim() === String(selectedAgentHiveGroupId || '').trim()
  );
  return String(matched?.label || t('messenger.agents.hiveAll')).trim();
});

const middlePaneSearchableSections = new Set(['messages', 'users', 'groups', 'swarms', 'agents']);
const showMiddlePaneSearch = computed(
  () => !showHelperAppsWorkspace && middlePaneSearchableSections.has(String(activeSection || '').trim())
);

const plazaHivePageIconUrl = `${import.meta.env.BASE_URL}beeroom.png`;
const plazaBrowsePages = computed(() => [
  {
    kind: 'hive_pack' as PlazaBrowseKind,
    label: t('plaza.kind.hive_pack'),
    description: t('plaza.page.hivePackDesc'),
    imageUrl: plazaHivePageIconUrl
  },
  {
    kind: 'worker_card' as PlazaBrowseKind,
    label: t('plaza.kind.worker_card'),
    description: t('plaza.page.workerCardDesc'),
    imageUrl: avatar046Url
  },
  {
    kind: 'skill_pack' as PlazaBrowseKind,
    label: t('plaza.kind.skill_pack'),
    description: t('plaza.page.skillPackDesc'),
    imageUrl: avatar016Url
  }
]);

const GUIDED_DEFAULT_CONVERSATION_KEY = '__guided_default_agent__';

const buildGuidedDefaultConversation = () => ({
  key: GUIDED_DEFAULT_CONVERSATION_KEY,
  kind: 'agent',
  agentId: normalizeAgentId(defaultAgentKey),
  icon: defaultAgentIcon,
  title: t('messenger.defaultAgent'),
  preview: t('messenger.empty.listStartDefault'),
  lastAt: null,
  unread: 0,
  guided: true
});

const isGuidedDefaultConversation = (item: Record<string, unknown> | null | undefined): boolean =>
  String(item?.key || '').trim() === GUIDED_DEFAULT_CONVERSATION_KEY;

const displayedMixedConversations = computed(() => {
  const items = Array.isArray(filteredMixedConversations) ? filteredMixedConversations : [];
  if (items.length > 0) {
    return items;
  }
  if (!showDefaultAgentEntry) {
    return [];
  }
  return [buildGuidedDefaultConversation()];
});

const resolveConversationItemKey = (item: Record<string, unknown> | null | undefined): string =>
  String(item?.key || '').trim();

const resolveSwarmDragKey = (group: Record<string, unknown> | null | undefined): string =>
  String(group?.group_id || group?.hive_id || '').trim();

const resolveAgentDragKey = (agentId: unknown): string => normalizeAgentId(agentId);

const clearDragOriginHiddenState = () => {
  if (typeof window !== 'undefined' && dragOriginHideFrame > 0) {
    window.cancelAnimationFrame(dragOriginHideFrame);
  }
  dragOriginHideFrame = 0;
  dragOriginHiddenState.value = { section: '', key: '' };
};

const isDragOriginHidden = (
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  key: string
): boolean => {
  const normalizedKey = String(key || '').trim();
  return (
    !!normalizedKey &&
    dragOriginHiddenState.value.section === section &&
    dragOriginHiddenState.value.key === normalizedKey
  );
};

const resetDragState = () => {
  clearDragOriginHiddenState();
  dragState.value = { section: '', key: '', sourceIndex: -1 };
  dragOverState.value = { section: '', index: -1 };
};

const resolveInsertIndex = (event: DragEvent, itemIndex: number): number => {
  const target = event.currentTarget as HTMLElement | null;
  if (!target) {
    return itemIndex;
  }
  const rect = target.getBoundingClientRect();
  return event.clientY > rect.top + rect.height / 2 ? itemIndex + 1 : itemIndex;
};

const beginDrag = (
  event: DragEvent,
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  key: string,
  sourceIndex: number
) => {
  const normalizedKey = String(key || '').trim();
  if (!normalizedKey) {
    event.preventDefault();
    return;
  }
  clearDragOriginHiddenState();
  dragState.value = { section, key: normalizedKey, sourceIndex };
  dragOverState.value = { section: '', index: -1 };
  const target = event.currentTarget as HTMLElement | null;
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', normalizedKey);
    const dragGhost = target?.cloneNode(true) as HTMLElement | null;
    if (dragGhost) {
      dragGhost.style.position = 'fixed';
      dragGhost.style.top = '-9999px';
      dragGhost.style.left = '-9999px';
      dragGhost.style.width = `${target?.getBoundingClientRect().width || 220}px`;
      dragGhost.style.opacity = '0.72';
      dragGhost.style.pointerEvents = 'none';
      dragGhost.style.zIndex = '9999';
      dragGhost.classList.add('is-drag-ghost');
      document.body.appendChild(dragGhost);
      event.dataTransfer.setDragImage(dragGhost, 24, 24);
      window.setTimeout(() => {
        dragGhost.remove();
      }, 0);
    }
  }
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    dragOriginHideFrame = window.requestAnimationFrame(() => {
      dragOriginHiddenState.value = { section, key: normalizedKey };
      dragOriginHideFrame = 0;
    });
  } else {
    dragOriginHiddenState.value = { section, key: normalizedKey };
  }
};

const updateDragOver = (
  event: DragEvent,
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  index: number
) => {
  if (dragState.value.section !== section || !dragState.value.key) {
    return;
  }
  event.preventDefault();
  const nextIndex = resolveInsertIndex(event, index);
  if (dragOverState.value.section === section && dragOverState.value.index === nextIndex) {
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
    return;
  }
  dragOverState.value = { section, index: nextIndex };
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move';
  }
};

const commitInsertMove = (
  event: DragEvent,
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  visibleKeys: string[],
  move: (draggedKey: string, key: string, position: 'before' | 'after', visibleKeys: string[]) => void
) => {
  if (dragState.value.section !== section || !dragState.value.key) {
    return;
  }
  const normalizedVisibleKeys = visibleKeys.map((key) => String(key || '').trim()).filter(Boolean);
  const rawInsertionIndex =
    dragOverState.value.section === section && dragOverState.value.index >= 0
      ? Math.min(dragOverState.value.index, normalizedVisibleKeys.length)
      : normalizedVisibleKeys.length;
  const sourceVisibleIndex = normalizedVisibleKeys.indexOf(dragState.value.key);
  const withoutDragged = normalizedVisibleKeys.filter((key) => key !== dragState.value.key);
  if (!withoutDragged.length) {
    resetDragState();
    return;
  }
  const normalizedInsertionIndex =
    sourceVisibleIndex >= 0 && rawInsertionIndex > sourceVisibleIndex ? rawInsertionIndex - 1 : rawInsertionIndex;
  const anchorIndex = Math.max(0, Math.min(normalizedInsertionIndex, withoutDragged.length));
  const previousKey = withoutDragged[anchorIndex - 1] || '';
  const nextKey = withoutDragged[anchorIndex] || '';
  event.preventDefault();
  if (nextKey) {
    move(dragState.value.key, nextKey, 'before', normalizedVisibleKeys);
  } else if (previousKey) {
    move(dragState.value.key, previousKey, 'after', normalizedVisibleKeys);
  }
  resetDragState();
};

const handleConversationDragStart = (event: DragEvent, item: Record<string, unknown>) =>
  beginDrag(
    event,
    'messages',
    resolveConversationItemKey(item),
    displayedMixedConversations.value.findIndex((entry) => resolveConversationItemKey(entry) === resolveConversationItemKey(item))
  );

const handleConversationDragOver = (event: DragEvent, index: number) => updateDragOver(event, 'messages', index);

const handleConversationDrop = (event: DragEvent) =>
  commitInsertMove(
    event,
    'messages',
    displayedMixedConversations.value.map((entry) => resolveConversationItemKey(entry)),
    moveMessageItem
  );

const handlePrimaryAgentDragStart = (event: DragEvent, agentId: unknown) =>
  beginDrag(
    event,
    'agents-primary',
    resolveAgentDragKey(agentId),
    orderedPrimaryAgents.value.findIndex((entry) => entry.agentId === resolveAgentDragKey(agentId))
  );

const handlePrimaryAgentDragOver = (event: DragEvent, index: number) =>
  updateDragOver(event, 'agents-primary', index);

const handlePrimaryAgentDrop = (event: DragEvent) =>
  commitInsertMove(
    event,
    'agents-primary',
    orderedPrimaryAgents.value.map((entry) => entry.agentId),
    moveAgentItem
  );

const handleSharedAgentDragStart = (event: DragEvent, agentId: unknown) =>
  beginDrag(
    event,
    'agents-shared',
    resolveAgentDragKey(agentId),
    filteredSharedAgents.findIndex((entry) => resolveAgentDragKey(entry.id) === resolveAgentDragKey(agentId))
  );

const handleSharedAgentDragOver = (event: DragEvent, index: number) =>
  updateDragOver(event, 'agents-shared', index);

const handleSharedAgentDrop = (event: DragEvent) =>
  commitInsertMove(
    event,
    'agents-shared',
    filteredSharedAgents.map((entry) => resolveAgentDragKey(entry.id)),
    moveAgentItem
  );

const handleSwarmDragStart = (event: DragEvent, group: Record<string, unknown>) =>
  beginDrag(
    event,
    'swarms',
    resolveSwarmDragKey(group),
    filteredBeeroomGroups.findIndex((entry) => resolveSwarmDragKey(entry) === resolveSwarmDragKey(group))
  );

const handleSwarmDragOver = (event: DragEvent, index: number) => updateDragOver(event, 'swarms', index);

const handleSwarmDrop = (event: DragEvent) =>
  commitInsertMove(
    event,
    'swarms',
    filteredBeeroomGroups.map((entry) => resolveSwarmDragKey(entry)),
    moveSwarmItem
  );

const isDropBefore = (
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  index: number
): boolean => dragOverState.value.section === section && dragOverState.value.index === index;

const isDropAfter = (
  section: 'messages' | 'agents-primary' | 'agents-shared' | 'swarms',
  index: number,
  total: number
): boolean =>
  total > 0 &&
  index === total - 1 &&
  dragOverState.value.section === section &&
  dragOverState.value.index === total;

const handleDragEnd = () => {
  resetDragState();
};

const HOT_BEEROOM_MISSION_STATUSES = new Set([
  'queued',
  'running',
  'awaiting_idle',
  'pending',
  'resuming',
  'merging'
]);

const isHotBeeroomMissionStatus = (value: unknown): boolean =>
  HOT_BEEROOM_MISSION_STATUSES.has(String(value || '').trim().toLowerCase());

function normalizeBeeroomGroupId(group: Record<string, any> | null | undefined): string {
  return String(group?.group_id || group?.hive_id || '').trim();
}

function isBeeroomGroupRunning(group: Record<string, any> | null | undefined): boolean {
  if (Number(group?.running_mission_total || 0) > 0) {
    return true;
  }
  if (isHotBeeroomMissionStatus(group?.latest_mission?.completion_status || group?.latest_mission?.status)) {
    return true;
  }
  const groupId = normalizeBeeroomGroupId(group);
  if (!groupId || groupId !== String(selectedBeeroomGroupId || '').trim()) {
    return false;
  }
  const activeMissions = Array.isArray(beeroomStore.activeMissions) ? beeroomStore.activeMissions : [];
  return activeMissions.some((mission) =>
    isHotBeeroomMissionStatus(mission?.completion_status || mission?.status)
  );
}

const selectedAgentIdSet = computed(() => new Set(selectedAgentIds.value));
const deletableSelectedAgentIds = computed(() =>
  visibleSelectableAgentItems.value
    .filter((item) => item.deletable && selectedAgentIdSet.value.has(item.id))
    .map((item) => item.id)
);

function closeAgentContextMenu() {
  agentContextMenuVisible.value = false;
}

const confirmUnsavedChangesBeforeAction = async (): Promise<boolean> => {
  return runUnsavedChangesGuards();
};

function applyAgentSelection(nextIds: string[], anchorId: string) {
  selectedAgentIds.value = Array.from(new Set(nextIds.map((item) => normalizeAgentId(item)).filter(Boolean)));
  agentSelectionAnchorId.value = normalizeAgentId(anchorId);
}

function clearAgentSelection() {
  selectedAgentIds.value = [];
  agentSelectionAnchorId.value = '';
  closeAgentContextMenu();
}

function isAgentMultiSelected(agentId: unknown) {
  return selectedAgentIdSet.value.has(normalizeAgentId(agentId));
}

function extendAgentSelection(targetId: string) {
  const items = visibleSelectableAgentItems.value;
  const anchorId = normalizeAgentId(agentSelectionAnchorId.value || targetId);
  const anchorIndex = items.findIndex((item) => item.id === anchorId);
  const targetIndex = items.findIndex((item) => item.id === targetId);
  if (anchorIndex < 0 || targetIndex < 0) {
    applyAgentSelection([targetId], targetId);
    return;
  }
  const [start, end] = anchorIndex <= targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
  applyAgentSelection(items.slice(start, end + 1).map((item) => item.id), anchorId);
}

function toggleAgentSelection(targetId: string) {
  if (selectedAgentIdSet.value.has(targetId)) {
    applyAgentSelection(
      selectedAgentIds.value.filter((item) => normalizeAgentId(item) != targetId),
      agentSelectionAnchorId.value || targetId
    );
    return;
  }
  applyAgentSelection([...selectedAgentIds.value, targetId], targetId);
}

async function handleAgentSelectionClick(event: MouseEvent, agentId: unknown) {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  const normalizedId = normalizeAgentId(agentId);
  if (!normalizedId) return;
  closeAgentContextMenu();
  if (event.shiftKey) {
    extendAgentSelection(normalizedId);
  } else if (event.ctrlKey || event.metaKey) {
    toggleAgentSelection(normalizedId);
  } else {
    applyAgentSelection([normalizedId], normalizedId);
  }
  selectAgentForSettings(normalizedId);
}

async function openAgentContextMenu(event: MouseEvent, agentId: unknown) {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  const normalizedId = normalizeAgentId(agentId);
  if (!normalizedId) return;
  if (!selectedAgentIdSet.value.has(normalizedId)) {
    applyAgentSelection([normalizedId], normalizedId);
  }
  selectAgentForSettings(normalizedId);
  agentContextMenuStyle.value = {
    left: `${event.clientX}px`,
    top: `${event.clientY}px`
  };
  agentContextMenuVisible.value = true;
}

async function handleAgentOpenById(agentId: unknown) {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  await Promise.resolve(openAgentById(agentId));
}

async function openConversationFromList(item: Record<string, unknown>) {
  if (isGuidedDefaultConversation(item)) {
    await handleAgentOpenById(defaultAgentKey);
    return;
  }
  await Promise.resolve(openMixedConversation(item));
}

function handleAgentContextExport() {
  const ids = selectedAgentIds.value.slice();
  closeAgentContextMenu();
  Promise.resolve(handleAgentBatchExport(ids)).catch(() => undefined);
}

function handleAgentContextDelete() {
  const ids = deletableSelectedAgentIds.value.slice();
  closeAgentContextMenu();
  if (!ids.length) {
    return;
  }
  Promise.resolve(handleAgentBatchDelete(ids))
    .then(() => {
      clearAgentSelection();
    })
    .catch(() => undefined);
}

function handleGlobalPointerDown(event: PointerEvent) {
  if (!agentContextMenuVisible.value) return;
  const target = event.target as HTMLElement | null;
  if (target?.closest('.messenger-agent-context-menu')) {
    return;
  }
  closeAgentContextMenu();
}

function handleGlobalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    clearAgentSelection();
  }
}

watch(
  visibleSelectableAgentItems,
  (items) => {
    const available = new Set(items.map((item) => item.id));
    selectedAgentIds.value = selectedAgentIds.value.filter((item) => available.has(normalizeAgentId(item)));
    if (agentSelectionAnchorId.value && !available.has(normalizeAgentId(agentSelectionAnchorId.value))) {
      agentSelectionAnchorId.value = '';
    }
    if (!selectedAgentIds.value.length) {
      closeAgentContextMenu();
    }
  },
  { deep: false }
);

watch(
  () => activeSection,
  (value) => {
    resetDragState();
    if (value !== 'agents') {
      clearAgentSelection();
    }
  }
);

if (typeof window !== 'undefined') {
  window.addEventListener('pointerdown', handleGlobalPointerDown);
  window.addEventListener('keydown', handleGlobalKeydown);
}

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') {
    window.removeEventListener('pointerdown', handleGlobalPointerDown);
    window.removeEventListener('keydown', handleGlobalKeydown);
  }
  resetDragState();
});

const emit = defineEmits<{
  (event: 'update:keyword', value: string): void;
  (event: 'update:selectedContactUnitId', value: string): void;
  (event: 'update:selectedAgentHiveGroupId', value: string): void;
  (event: 'update:settingsPanelMode', value: string): void;
  (event: 'activate-settings-panel', value: string): void;
}>();

const updateKeyword = (value: string) => {
  emit('update:keyword', value);
};

const handleAgentFilterCommand = (command: string | number | Record<string, unknown>) => {
  const normalized = String(command || '').trim();
  if (normalized === '__all__') {
    updateSelectedAgentHiveGroupId('');
    return;
  }
  if (normalized.startsWith('hive:')) {
    updateSelectedAgentHiveGroupId(normalized.slice(5));
  }
};

const updateSelectedContactUnitId = (value: string) => {
  emit('update:selectedContactUnitId', value);
};

const updateSelectedAgentHiveGroupId = (value: string) => {
  emit('update:selectedAgentHiveGroupId', value);
};

const updateSettingsPanelMode = async (value: string) => {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  emit('update:settingsPanelMode', value);
  emit('activate-settings-panel', value);
};

const resolvePlusActionLabel = () => {
  if (activeSection === 'groups') {
    return t('userWorld.group.create');
  }
  return t('messenger.action.newAgent');
};

const resolveSwarmPlusActionLabel = () =>
  `${t('beeroom.pack.action.import')} / ${t('beeroom.dialog.createTitle')}`;

const triggerSearchCreateAction = (command?: string) => {
  Promise.resolve(handleSearchCreateAction(command)).catch(() => undefined);
};

const swarmCreateCandidateAgents = computed(() => {
  const list = [
    ...(Array.isArray(filteredOwnedAgents) ? filteredOwnedAgents : []),
    ...(Array.isArray(filteredSharedAgents) ? filteredSharedAgents : [])
  ];
  const unique = new Map<string, { id: string; name: string }>();
  list.forEach((item) => {
    const id = String(item?.id || '').trim();
    if (!id || unique.has(id)) return;
    const name = String(item?.name || id).trim() || id;
    unique.set(id, { id, name });
  });
  return Array.from(unique.values());
});

const packOverlayVisible = computed(
  () => beeroomStore.packImportLoading || beeroomStore.packExportLoading
);

const activePackMode = computed<'import' | 'export'>(() => {
  if (beeroomStore.packExportLoading) {
    return 'export';
  }
  if (beeroomStore.packImportLoading) {
    return 'import';
  }
  return packOverlayMode.value;
});

const activePackJob = computed(() => {
  if (activePackMode.value === 'export') {
    return beeroomStore.packExportJob;
  }
  return beeroomStore.packImportJob;
});

const beginPackWaiting = (mode: 'import' | 'export', targetName: unknown) => {
  packOverlayMode.value = mode;
  packOverlayTargetName.value = String(targetName || '').trim();
};

const clearPackWaiting = () => {
  packOverlayTargetName.value = '';
};

const resolvePackReportRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const resolvePositiveInt = (value: unknown): number => {
  const normalized = Math.floor(Number(value));
  return Number.isFinite(normalized) && normalized > 0 ? normalized : 0;
};

const resolvePackReportArray = (value: unknown): Record<string, unknown>[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((item): item is Record<string, unknown> => {
    return Boolean(item && typeof item === 'object' && !Array.isArray(item));
  });
};

const resolveImportRenameTotal = (report: unknown): number => {
  const reportRecord = resolvePackReportRecord(report);
  const conflicts = resolvePackReportRecord(reportRecord?.conflicts);
  if (!conflicts) return 0;
  // Prefer backend aggregate field, then fallback to per-section counters.
  const directTotal = resolvePositiveInt(conflicts.renamed_total);
  if (directTotal > 0) return directTotal;

  const hive = resolvePackReportRecord(conflicts.hive);
  const agents = resolvePackReportRecord(conflicts.agents);
  const skills = resolvePackReportRecord(conflicts.skills);
  const hiveRenamed = hive?.renamed === true ? 1 : 0;
  return (
    hiveRenamed +
    resolvePositiveInt(agents?.renamed_total) +
    resolvePositiveInt(skills?.renamed_total)
  );
};

const resolvePackRenameLabel = (value: unknown, fallback = ''): string => {
  const text = String(value || '').trim();
  return text || fallback;
};

const showImportRenameDetails = async (report: unknown) => {
  const reportRecord = resolvePackReportRecord(report);
  const conflicts = resolvePackReportRecord(reportRecord?.conflicts);
  if (!conflicts) return;

  const lines: string[] = [];
  const hive = resolvePackReportRecord(conflicts.hive);
  if (hive?.renamed === true) {
    const fromRecord = resolvePackReportRecord(hive.from);
    const toRecord = resolvePackReportRecord(hive.to);
    const fromName = resolvePackRenameLabel(fromRecord?.name, t('beeroom.pack.rename.unknown'));
    const toName = resolvePackRenameLabel(toRecord?.name, t('beeroom.pack.rename.unknown'));
    const fromHiveId = resolvePackRenameLabel(fromRecord?.hive_id);
    const toHiveId = resolvePackRenameLabel(toRecord?.hive_id);
    lines.push(
      `${t('beeroom.pack.rename.kind.hive')}: ${fromName} [${fromHiveId || '-'}] → ${toName} [${toHiveId || '-'}]`
    );
  }

  const agentRenames = resolvePackReportArray(resolvePackReportRecord(conflicts.agents)?.renames);
  for (const item of agentRenames) {
    const from = resolvePackRenameLabel(item.from, t('beeroom.pack.rename.unknown'));
    const to = resolvePackRenameLabel(item.to, t('beeroom.pack.rename.unknown'));
    lines.push(`${t('beeroom.pack.rename.kind.agent')}: ${from} → ${to}`);
  }

  const skillRenames = resolvePackReportArray(resolvePackReportRecord(conflicts.skills)?.renames);
  for (const item of skillRenames) {
    const from = resolvePackRenameLabel(item.from, t('beeroom.pack.rename.unknown'));
    const to = resolvePackRenameLabel(item.to, t('beeroom.pack.rename.unknown'));
    lines.push(`${t('beeroom.pack.rename.kind.skill')}: ${from} → ${to}`);
  }

  if (!lines.length) {
    return;
  }

  const maxVisible = 12;
  const visibleLines = lines.slice(0, maxVisible);
  const hidden = lines.length - visibleLines.length;
  const messageChildren = [
    h('p', t('beeroom.pack.rename.dialogIntro')),
    h(
      'ul',
      { class: 'messenger-pack-rename-list' },
      visibleLines.map((line) => h('li', line))
    )
  ];
  if (hidden > 0) {
    messageChildren.push(h('p', t('beeroom.pack.rename.dialogMore', { count: hidden })));
  }

  await ElMessageBox.alert(
    h('div', { class: 'messenger-pack-rename-dialog-body' }, messageChildren),
    t('beeroom.pack.rename.dialogTitle'),
    {
      confirmButtonText: t('common.confirm'),
      closeOnClickModal: false,
      closeOnPressEscape: false,
      showClose: false
    }
  );
};

const isMessageBoxDismissAction = (value: unknown): boolean => {
  const action = String(value || '').trim().toLowerCase();
  return action === 'cancel' || action === 'close';
};

const resetSwarmPackInput = () => {
  if (swarmPackInputRef.value) {
    swarmPackInputRef.value.value = '';
  }
};

const handleAgentPlusCommand = async (command: string) => {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  triggerSearchCreateAction(command);
};

const handlePlusAction = async () => {
  if (!(await confirmUnsavedChangesBeforeAction())) {
    return;
  }
  triggerSearchCreateAction();
};

const openSwarmPackImportPicker = () => {
  if (beeroomStore.packImportLoading || beeroomStore.packExportLoading) {
    ElMessage.warning(t('beeroom.pack.message.busy'));
    return;
  }
  swarmPackInputRef.value?.click();
};

const openSwarmCreateDialog = () => {
  swarmCreateVisible.value = true;
};

const resolveSwarmGroupId = (group: Record<string, any> | null | undefined): string =>
  String(group?.group_id || group?.hive_id || '').trim();

const resolveSwarmGroupName = (group: Record<string, any> | null | undefined): string => {
  const groupId = resolveSwarmGroupId(group);
  return String(group?.name || groupId).trim() || groupId;
};

const openSwarmEditDialog = (group: Record<string, any>) => {
  swarmEditingGroup.value = { ...group };
  swarmEditVisible.value = true;
};

const handleSwarmPlusCommand = (command: string | number | Record<string, unknown>) => {
  const normalized = String(command || '').trim().toLowerCase();
  if (normalized === 'import') {
    openSwarmPackImportPicker();
    return;
  }
  if (normalized === 'create') {
    openSwarmCreateDialog();
  }
};

const handleSwarmCreateSubmit = async (payload: Record<string, unknown>) => {
  if (swarmCreateSaving.value) return;
  swarmCreateSaving.value = true;
  try {
    await beeroomStore.createGroup(payload);
    swarmCreateVisible.value = false;
    ElMessage.success(t('beeroom.message.hiveCreated'));
  } catch (error: any) {
    const detail = String(error?.response?.data?.detail || error?.message || '').trim();
    ElMessage.error(detail || t('common.requestFailed'));
  } finally {
    swarmCreateSaving.value = false;
  }
};

const handleSwarmEditSubmit = async (payload: Record<string, unknown>) => {
  const groupId = resolveSwarmGroupId(swarmEditingGroup.value);
  if (swarmEditSaving.value || !groupId) return;
  swarmEditSaving.value = true;
  try {
    const updated = await beeroomStore.updateGroup(groupId, payload);
    swarmEditingGroup.value = updated ? { ...updated } : swarmEditingGroup.value;
    swarmEditVisible.value = false;
    ElMessage.success(t('common.saved'));
  } catch (error: any) {
    const detail = String(error?.response?.data?.detail || error?.message || '').trim();
    ElMessage.error(detail || t('common.saveFailed'));
  } finally {
    swarmEditSaving.value = false;
  }
};

const handleSwarmEditDelete = async () => {
  const group = swarmEditingGroup.value;
  const groupId = resolveSwarmGroupId(group);
  if (!groupId || swarmEditDeleting.value) {
    return;
  }
  try {
    await ElMessageBox.confirm(
      t('beeroom.message.deleteConfirm', { name: resolveSwarmGroupName(group) }),
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

  swarmEditDeleting.value = true;
  try {
    await deleteBeeroomGroup(group);
    swarmEditVisible.value = false;
    swarmEditingGroup.value = null;
  } finally {
    swarmEditDeleting.value = false;
  }
};

const handleSwarmPackFileChange = async (event: Event) => {
  const source = event.target as HTMLInputElement | null;
  const file = source?.files?.[0];
  if (!file) {
    resetSwarmPackInput();
    return;
  }
  beginPackWaiting('import', file.name);
  try {
    const job = await beeroomStore.importHivePack(file);
    const status = String(job?.status || '').trim().toLowerCase();
    const importedName = String(job?.report?.hive_name || '').trim() || file.name;
    const renamedTotal = resolveImportRenameTotal(job?.report);
    if (status === 'completed') {
      await Promise.resolve(afterHivePackImported?.(job));
      if (renamedTotal > 0) {
        ElMessage.success(
          t('beeroom.pack.message.importSuccessRenamed', {
            name: importedName,
            count: renamedTotal
          })
        );
        try {
          await showImportRenameDetails(job?.report);
        } catch (dialogError) {
          if (!isMessageBoxDismissAction(dialogError)) {
            throw dialogError;
          }
        }
        return;
      }
      ElMessage.success(t('beeroom.pack.message.importSuccess', { name: importedName }));
      return;
    }
    if (status === 'failed' || status === 'error' || status === 'cancelled' || status === 'canceled') {
      const detail = String(job?.detail?.error || '').trim();
      ElMessage.error(detail || beeroomStore.packError || t('beeroom.pack.message.importFailed'));
      return;
    }
    ElMessage.warning(t('beeroom.pack.message.importPending'));
  } catch (error: any) {
    const detail = String(error?.response?.data?.detail || '').trim();
    const message = detail || beeroomStore.packError || t('beeroom.pack.message.importFailed');
    ElMessage.error(message);
  } finally {
    resetSwarmPackInput();
    clearPackWaiting();
  }
};

const handleSwarmExport = async (group: Record<string, any>) => {
  const groupId = String(group?.group_id || group?.hive_id || '').trim();
  if (!groupId) {
    return;
  }
  if (beeroomStore.packImportLoading || beeroomStore.packExportLoading) {
    ElMessage.warning(t('beeroom.pack.message.busy'));
    return;
  }
  beginPackWaiting('export', group?.name || groupId);
  try {
    const job = await beeroomStore.exportHivePack(groupId, 'full');
    const status = String(job?.status || '').trim().toLowerCase();
    if (status === 'completed') {
      await beeroomStore.downloadExportPack(job?.job_id || '');
      const filename = String(job?.artifact?.filename || '').trim() || `${groupId}.hivepack`;
      ElMessage.success(t('beeroom.pack.message.exportSuccess', { name: filename }));
      return;
    }
    if (status === 'failed' || status === 'error') {
      ElMessage.error(beeroomStore.packError || t('beeroom.pack.message.exportFailed'));
      return;
    }
    ElMessage.warning(t('beeroom.pack.message.exportPending'));
  } catch (error: any) {
    const detail = String(error?.response?.data?.detail || '').trim();
    const message = detail || beeroomStore.packError || t('beeroom.pack.message.exportFailed');
    ElMessage.error(message);
  } finally {
    clearPackWaiting();
  }
};
</script>

<style scoped>
.messenger-plaza-page-item {
  align-items: center;
}

.messenger-plaza-page-avatar {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  overflow: hidden;
  background: linear-gradient(135deg, rgba(255, 214, 120, 0.22), rgba(237, 157, 49, 0.12));
  color: #ab6a10;
  box-shadow: inset 0 0 0 1px rgba(223, 161, 72, 0.18);
}

.messenger-plaza-page-avatar.is-hive_pack {
  background: linear-gradient(135deg, rgba(255, 216, 128, 0.28), rgba(236, 165, 79, 0.14));
}

.messenger-plaza-page-avatar.is-worker_card {
  background: linear-gradient(135deg, rgba(252, 221, 168, 0.24), rgba(214, 160, 122, 0.14));
}

.messenger-plaza-page-avatar.is-skill_pack {
  background: linear-gradient(135deg, rgba(255, 236, 184, 0.28), rgba(244, 194, 86, 0.12));
}

.messenger-plaza-page-avatar-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.messenger-search-select {
  min-width: 0;
  display: block;
  position: relative;
  width: 100%;
  height: 36px;
  box-sizing: border-box;
  padding: 0 30px 0 10px;
  text-align: left;
  cursor: pointer;
  line-height: 34px;
  overflow: hidden;
}

.messenger-search-slot {
  display: flex;
  flex: 1 1 0;
  min-width: 0;
}

.messenger-search-slot :deep(.el-dropdown) {
  display: flex;
  flex: 1 1 0;
  width: 100%;
  min-width: 0;
}

.messenger-search-select-label {
  display: block;
  min-width: 0;
  width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.messenger-search-select-caret {
  position: absolute;
  top: 50%;
  right: 10px;
  transform: translateY(-50%);
  pointer-events: none;
  color: rgba(100, 116, 139, 0.88);
  font-size: 12px;
}

.messenger-search-actions {
  flex: 0 0 68px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  width: 68px;
}

.messenger-search-actions :deep(.el-dropdown) {
  display: flex;
  flex: 0 0 auto;
}

.messenger-search-actions .messenger-plus-btn {
  width: 32px;
  height: 32px;
  border-radius: 7px;
}
</style>


