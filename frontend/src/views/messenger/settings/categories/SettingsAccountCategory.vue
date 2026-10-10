<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-account"
  >
    <!-- 账号：编辑资料（昵称 / 头像 / 密码），复用 MessengerSettingsPanel 的资料页分支。 -->
    <MessengerSettingsPanel
      mode="profile"
      :username="currentUsername"
      :user-id="currentUserId"
      :language-label="currentLanguageLabel"
      :send-key="messengerSendKey"
      :theme-palette="themeStore.palette"
      :ui-font-size="uiFontSize"
      :username-saving="usernameSaving"
      :profile-avatar-icon="currentUserAvatarIcon"
      :profile-avatar-color="currentUserAvatarColor"
      :profile-avatar-options="profileAvatarOptions"
      :profile-avatar-colors="profileAvatarColors"
      @toggle-language="toggleLanguage"
      @update:send-key="updateSendKey"
      @update:theme-palette="updateThemePalette"
      @update:ui-font-size="updateUiFontSize"
      @update:username="updateCurrentUsername"
      @update:profile-avatar-icon="updateCurrentUserAvatarIcon"
      @update:profile-avatar-color="updateCurrentUserAvatarColor"
    />
  </div>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';

const props = defineProps<{ controller: MessengerControllerContext }>();

const currentUsername = props.controller.currentUsername;
const currentUserId = props.controller.currentUserId;
const currentLanguageLabel = props.controller.currentLanguageLabel;
const messengerSendKey = props.controller.messengerSendKey;
const themeStore = props.controller.themeStore;
const uiFontSize = props.controller.uiFontSize;
const usernameSaving = props.controller.usernameSaving;
const currentUserAvatarIcon = props.controller.currentUserAvatarIcon;
const currentUserAvatarColor = props.controller.currentUserAvatarColor;
const profileAvatarOptions = props.controller.profileAvatarOptions;
const profileAvatarColors = props.controller.profileAvatarColors;
const toggleLanguage = props.controller.toggleLanguage;
const updateSendKey = props.controller.updateSendKey;
const updateThemePalette = props.controller.updateThemePalette;
const updateUiFontSize = props.controller.updateUiFontSize;
const updateCurrentUsername = props.controller.updateCurrentUsername;
const updateCurrentUserAvatarIcon = props.controller.updateCurrentUserAvatarIcon;
const updateCurrentUserAvatarColor = props.controller.updateCurrentUserAvatarColor;
const MessengerSettingsPanel = props.controller.MessengerSettingsPanel;
</script>
