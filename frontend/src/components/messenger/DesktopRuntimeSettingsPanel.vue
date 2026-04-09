<template>
  <div class="desktop-runtime-settings-shell">
    <DesktopRuntimePreferencesPanel v-if="desktopLocalMode" :desktop-local-mode="desktopLocalMode" />

    <section class="messenger-settings-card desktop-runtime-settings-danger">
      <div class="desktop-runtime-settings-danger-head">
        <div>
          <div class="messenger-settings-title">{{ t('desktop.system.resetWorkStateTitle') }}</div>
          <div class="messenger-settings-subtitle">
            {{ t('desktop.system.resetWorkStateDescription') }}
          </div>
        </div>
      </div>
      <div class="desktop-runtime-settings-danger-note">
        {{ t('desktop.system.resetWorkStateWarning') }}
      </div>
      <div class="desktop-runtime-settings-danger-actions">
        <el-button
          class="desktop-runtime-settings-danger-btn"
          :loading="resettingWorkState"
          @click="handleResetWorkState"
        >
          {{ t('desktop.system.resetWorkStateButton') }}
        </el-button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import DesktopRuntimePreferencesPanel from '@/components/messenger/DesktopRuntimePreferencesPanel.vue';
import {
  resetMyWorkState,
  type ResetWorkStateSummary
} from '@/api/auth';
import {
  resetDesktopWorkState
} from '@/api/desktop';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';

const props = withDefaults(
  defineProps<{
    desktopLocalMode?: boolean;
  }>(),
  {
    desktopLocalMode: true
  }
);

const { t } = useI18n();
const chatStore = useChatStore();
const resettingWorkState = ref(false);
const desktopLocalMode = computed(() => props.desktopLocalMode === true);

const resolveErrorMessage = (error: unknown, fallback: string): string => {
  const responseMessage = (error as { response?: { data?: { message?: string } } })?.response?.data
    ?.message;
  const detailMessage = (error as { response?: { data?: { detail?: string } } })?.response?.data
    ?.detail;
  const message = (error as { message?: string })?.message;
  return String(responseMessage || detailMessage || message || fallback);
};

const resolveCurrentResetAgentId = (): string => {
  const activeSessionId = String(chatStore.activeSessionId || '').trim();
  if (activeSessionId) {
    const activeSession = chatStore.sessions.find(
      (item) => String(item?.id || '').trim() === activeSessionId
    );
    const activeAgentId = String(activeSession?.agent_id || '').trim();
    if (activeAgentId || activeSession) {
      return activeAgentId;
    }
  }
  return String(chatStore.draftAgentId || '').trim();
};

const syncChatStateAfterReset = async (summary: ResetWorkStateSummary) => {
  const targetAgentId = resolveCurrentResetAgentId();
  await chatStore.loadSessions();

  const targetSessionId =
    chatStore.sessions.find((item) => {
      const itemAgentId = String(item?.agent_id || '').trim();
      return itemAgentId === targetAgentId && item?.is_main === true;
    })?.id ||
    summary.fresh_main_sessions.find(
      (item) => String(item.agent_id || '').trim() === targetAgentId
    )?.session_id ||
    '';

  if (targetSessionId) {
    await chatStore.loadSessionDetail(targetSessionId);
    return;
  }
  chatStore.openDraftSession({ agent_id: targetAgentId });
};

const handleResetWorkState = async () => {
  try {
    await ElMessageBox.confirm(
      t('desktop.system.resetWorkStateConfirmMessage'),
      t('desktop.system.resetWorkStateConfirmTitle'),
      {
        type: 'warning',
        confirmButtonText: t('desktop.system.resetWorkStateButton'),
        cancelButtonText: t('common.cancel'),
        confirmButtonClass: 'el-button--danger'
      }
    );
  } catch {
    return;
  }

  resettingWorkState.value = true;
  try {
    const response = desktopLocalMode.value
      ? await resetDesktopWorkState()
      : await resetMyWorkState();
    const summary = (response?.data?.data || {}) as ResetWorkStateSummary;
    await syncChatStateAfterReset(summary);
    ElMessage.success(
      t('desktop.system.resetWorkStateSuccess', {
        sessions: summary.cancelled_sessions ?? 0,
        tasks: summary.cancelled_tasks ?? 0,
        workspaces: summary.cleared_workspaces ?? 0
      })
    );
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveErrorMessage(error, t('desktop.system.resetWorkStateFailed')));
  } finally {
    resettingWorkState.value = false;
  }
};
</script>

<style scoped>
.desktop-runtime-settings-shell {
  display: grid;
  gap: 12px;
}

.desktop-runtime-settings-danger {
  display: grid;
  gap: 12px;
  border: 1px solid rgba(214, 77, 77, 0.28);
  background: linear-gradient(180deg, rgba(84, 18, 18, 0.9), rgba(36, 11, 11, 0.94));
}

.desktop-runtime-settings-danger-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.desktop-runtime-settings-danger-note {
  color: rgba(255, 226, 226, 0.88);
  line-height: 1.6;
  font-size: 13px;
}

.desktop-runtime-settings-danger-actions {
  display: flex;
  justify-content: flex-start;
}

.desktop-runtime-settings-danger-btn {
  border-radius: 10px;
  border: 1px solid rgba(248, 113, 113, 0.46);
  background: rgba(127, 29, 29, 0.88);
  color: #fff1f2;
  box-shadow: none;
}

.desktop-runtime-settings-danger-btn:hover:not(:disabled) {
  border-color: rgba(252, 165, 165, 0.8);
  background: rgba(153, 27, 27, 0.96);
  color: #ffffff;
}
</style>
