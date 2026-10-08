<template>
  <header class="messenger-site-header">
    <div class="messenger-site-brand">
      <i class="fa-solid fa-hexagon-nodes" aria-hidden="true"></i>
      <span>{{ t('messenger.site.title') }}</span>
    </div>
    <div class="messenger-site-actions">
      <button
        class="messenger-site-action"
        type="button"
        :title="t('messenger.site.help')"
        :aria-label="t('messenger.site.help')"
        @click="emit('open-help')"
      >
        <i class="fa-regular fa-circle-question" aria-hidden="true"></i>
      </button>
      <el-dropdown
        trigger="click"
        :teleported="true"
        placement="bottom-end"
        popper-class="mz-thread-dropdown"
        @command="handleAccountCommand"
      >
        <button
          class="messenger-site-account"
          :style="currentUserAvatarStyle"
          type="button"
          :title="currentUsername"
          :aria-label="t('messenger.site.account')"
        >
          <img
            v-if="currentUserAvatarImageUrl"
            class="messenger-settings-profile-avatar-image"
            :src="currentUserAvatarImageUrl"
            alt=""
          />
          <span v-else class="messenger-avatar-text">{{ avatarLabel(currentUsername) }}</span>
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="profile">{{ t('messenger.site.profile') }}</el-dropdown-item>
            <el-dropdown-item command="settings">{{ t('messenger.site.settings') }}</el-dropdown-item>
            <el-dropdown-item command="logout" divided :disabled="Boolean(settingsLogoutDisabled?.value)">
              {{ t('messenger.site.logout') }}
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
  </header>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '../controller/messengerControllerContext';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ 'open-help': []; 'open-settings': []; 'open-profile': [] }>();

const t = props.controller.t;
const avatarLabel = props.controller.avatarLabel;
const currentUserAvatarImageUrl = props.controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = props.controller.currentUserAvatarStyle;
const currentUsername = props.controller.currentUsername;
const settingsLogoutDisabled = props.controller.settingsLogoutDisabled;
const handleSettingsLogout = props.controller.handleSettingsLogout;

const handleAccountCommand = (command: string) => {
  if (command === 'profile') {
    emit('open-profile');
    return;
  }
  if (command === 'settings') {
    emit('open-settings');
    return;
  }
  if (command === 'logout') {
    void handleSettingsLogout?.();
  }
};
</script>
