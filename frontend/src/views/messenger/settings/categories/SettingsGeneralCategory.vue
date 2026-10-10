<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-general"
  >
    <!-- 外观 / 行为 / 版本：直接复用既有 MessengerSettingsPanel（不重写业务逻辑）。
         账号资料编辑已拆到独立的「账号」分类（SettingsAccountCategory）。 -->
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.appearanceGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.appearanceHint') }}</div>
      </div>
      <MessengerSettingsPanel
        mode="general"
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
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.stressGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.stressHint') }}</div>
      </div>
      <div class="messenger-settings-row messenger-settings-stress-row">
        <div class="messenger-settings-label">
          {{ t('messenger.settingsPage.general.stressUserRounds') }}
        </div>
        <input
          v-model.number="stressUserRounds"
          class="messenger-settings-number-input"
          type="number"
          min="1"
          max="2000"
          step="1"
          data-testid="settings-stress-user-rounds"
          :disabled="stressJobRunning"
        />
      </div>
      <div class="messenger-settings-row messenger-settings-stress-row">
        <div class="messenger-settings-label">
          {{ t('messenger.settingsPage.general.stressModelRounds') }}
        </div>
        <input
          v-model.number="stressModelRounds"
          class="messenger-settings-number-input"
          type="number"
          min="1"
          max="2000"
          step="1"
          data-testid="settings-stress-model-rounds"
          :disabled="stressJobRunning"
        />
      </div>
      <div class="messenger-settings-row messenger-settings-stress-row">
        <div class="messenger-settings-hint">
          {{ stressStatusText || t('messenger.settingsPage.general.stressScaleHint') }}
        </div>
        <div class="messenger-settings-page-row-actions">
          <button
            class="messenger-settings-action ghost"
            type="button"
            data-testid="settings-stress-generate"
            :disabled="stressJobRunning"
            @click="generateStressThread"
          >
            {{
              stressJobRunning
                ? t('messenger.settingsPage.general.stressGenerating')
                : t('messenger.settingsPage.general.stressGenerate')
            }}
          </button>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card messenger-settings-card--danger">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.sessionGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.sessionHint') }}</div>
      </div>
      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-right-from-bracket messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.site.logout') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.general.logoutHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <button
            class="messenger-settings-action danger"
            type="button"
            data-testid="settings-logout"
            :disabled="Boolean(settingsLogoutDisabled?.value)"
            @click="handleLogout"
          >
            {{ t('messenger.site.logout') }}
          </button>
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';

import { ElMessage } from 'element-plus';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';
import { confirmWithFallback } from '@/utils/confirm';
import {
  getStressThreadJob,
  listStressThreads,
  startStressThread,
  type StressThreadJobSnapshot
} from '@/api/stressThreads';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

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
const settingsLogoutDisabled = props.controller.settingsLogoutDisabled;
const handleSettingsLogout = props.controller.handleSettingsLogout;
const MessengerSettingsPanel = props.controller.MessengerSettingsPanel;

const handleLogout = async () => {
  const confirmed = await confirmWithFallback(
    t('messenger.settingsPage.general.logoutConfirm'),
    t('common.confirm'),
    {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    }
  );
  if (!confirmed) return;
  void handleSettingsLogout?.();
};

const stressUserRounds = ref<number>(1000);
const stressModelRounds = ref<number>(1000);
const stressJobRunning = ref(false);
const stressStatusText = ref('');
let stressJobTimer: number | null = null;

const stopStressJobPolling = () => {
  if (stressJobTimer !== null) {
    window.clearInterval(stressJobTimer);
    stressJobTimer = null;
  }
};

onBeforeUnmount(stopStressJobPolling);

const startStressJobPolling = (jobId: string) => {
  stopStressJobPolling();
  stressJobTimer = window.setInterval(() => {
    void refreshStressJob(jobId);
  }, 800);
};

// 统一套用任务快照：运行中恢复轮询，完成/失败展示结果并刷新会话列表。
// notify 仅在"本次进入页面后观察到状态变化"时弹全局提示，恢复展示保持安静。
const applyStressSnapshot = (job: StressThreadJobSnapshot, notify: boolean) => {
  const status = (job?.status || {}) as {
    state?: string;
    done_rounds?: number;
    items_written?: number;
    items?: number;
    error?: string;
  };
  if (status.state === 'running') {
    stressJobRunning.value = true;
    stressStatusText.value = t('messenger.settingsPage.general.stressProgress', {
      done: Number(status.done_rounds ?? 0),
      total: Number(job.total_rounds ?? 0)
    });
    startStressJobPolling(job.job_id);
  } else if (status.state === 'completed') {
    stopStressJobPolling();
    stressJobRunning.value = false;
    stressStatusText.value = t('messenger.settingsPage.general.stressDone', {
      items: Number(status.items ?? status.items_written ?? 0)
    });
    if (notify) {
      ElMessage.success(
        t('messenger.settingsPage.general.stressDone', {
          items: Number(status.items ?? status.items_written ?? 0)
        })
      );
    }
    void props.controller.chatStore?.loadSessions?.();
  } else if (status.state === 'failed') {
    stopStressJobPolling();
    stressJobRunning.value = false;
    stressStatusText.value = t('messenger.settingsPage.general.stressFailed', {
      message: String(status.error || '')
    });
    if (notify) {
      ElMessage.error(
        t('messenger.settingsPage.general.stressFailed', {
          message: String(status.error || '')
        })
      );
    }
  }
};

const refreshStressJob = async (jobId: string) => {
  try {
    const { data } = await getStressThreadJob(jobId);
    applyStressSnapshot(data as StressThreadJobSnapshot, true);
  } catch {
    // 轮询瞬时失败直接忽略，等待下一轮。
  }
};

// 重新进入设置页时恢复任务可见性：后端任务注册表仍在，避免"退出页面进度即丢"。
onMounted(async () => {
  try {
    const { data } = await listStressThreads();
    const jobs = (data?.jobs || []) as StressThreadJobSnapshot[];
    const running = jobs.find((job) => job?.status?.state === 'running');
    if (running) {
      applyStressSnapshot(running, false);
      return;
    }
    const latest = jobs[0];
    if (latest && (latest.status?.state === 'completed' || latest.status?.state === 'failed')) {
      applyStressSnapshot(latest, false);
    }
  } catch {
    // 任务接口不可用时静默跳过，不影响设置页其他能力。
  }
});

const generateStressThread = async () => {
  if (stressJobRunning.value) return;
  const userRounds = Math.floor(Number(stressUserRounds.value) || 0);
  const modelRounds = Math.floor(Number(stressModelRounds.value) || 0);
  if (userRounds < 1 || userRounds > 2000) {
    ElMessage.warning(t('messenger.settingsPage.general.stressInvalidUserRounds'));
    return;
  }
  if (modelRounds < 1 || modelRounds > 2000) {
    ElMessage.warning(t('messenger.settingsPage.general.stressInvalidModelRounds'));
    return;
  }
  try {
    const { data } = await startStressThread({
      user_rounds: userRounds,
      model_rounds: modelRounds
    });
    const jobId = String(data?.job_id || '');
    if (!jobId) {
      ElMessage.error(t('messenger.settingsPage.general.stressStartFailed'));
      return;
    }
    stressJobRunning.value = true;
    stressStatusText.value = t('messenger.settingsPage.general.stressProgress', {
      done: 0,
      total: userRounds
    });
    startStressJobPolling(jobId);
  } catch {
    ElMessage.error(t('messenger.settingsPage.general.stressStartFailed'));
  }
};
</script>
