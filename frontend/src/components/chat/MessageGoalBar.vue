<template>
  <div v-if="goal" class="goal-bar" role="status">
    <span class="goal-bar-icon" aria-hidden="true">
      <i class="fa-solid fa-bullseye"></i>
    </span>
    <span class="goal-bar-title">{{ goalTitle }}</span>
    <span class="goal-bar-time">{{ t('chat.goalBar.rounds', { started: goal.rounds_started, max: goal.max_goal_rounds }) }}</span>
    <span v-if="objectiveLabel" class="goal-bar-objective" :title="objectiveLabel">{{ objectiveLabel }}</span>
    <span class="goal-bar-actions">
      <button
        class="goal-bar-action"
        type="button"
        :disabled="busy"
        :title="t('chat.goalBar.edit')"
        @click="handleEdit"
      >
        <i class="fa-solid fa-pen" aria-hidden="true"></i>
      </button>
      <button
        class="goal-bar-action"
        type="button"
        :disabled="busy"
        :title="t('chat.goalBar.remove')"
        @click="handleRemove"
      >
        <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
      </button>
      <button
        v-if="canPause"
        class="goal-bar-action"
        type="button"
        :disabled="busy"
        :title="t('chat.goalBar.pause')"
        @click="handlePause"
      >
        <i class="fa-solid fa-circle-pause" aria-hidden="true"></i>
      </button>
      <button
        v-else
        class="goal-bar-action is-primary"
        type="button"
        :disabled="busy"
        :title="t('chat.goalBar.resume')"
        @click="handleResume"
      >
        <i class="fa-solid fa-play" aria-hidden="true"></i>
      </button>
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { ElMessage } from 'element-plus';
import { mzConfirm, mzPrompt } from '@/utils/mzDialog';

import { useI18n } from '@/i18n';
import { clearSessionGoal } from '@/api/chat';
import { useChatStore } from '@/stores/chat';

/**
 * 目标条：时间线底部、输入区上方（参考样式：粉色圆角条 + 右侧图标操作）。
 *
 * 使用后端 phase / activation 和已受理目标轮次；完成的目标不再展示。
 * 暂停/恢复走 PUT 的 action: pause|resume。
 */
const props = withDefaults(defineProps<{
  sessionId?: string;
}>(), {
  sessionId: ''
});

const { t } = useI18n();
const chatStore = useChatStore();
const busy = ref(false);

const activeSessionId = computed(() => String(props.sessionId || chatStore.activeSessionId || '').trim());

const goal = computed(() => {
  const sessionId = activeSessionId.value;
  if (!sessionId) return null;
  const resolve = chatStore.sessionGoal;
  const value = typeof resolve === 'function' ? resolve(sessionId) : null;
  if (!value) return null;
  const status = value.phase;
  // complete 是已达成，不再打扰用户。
  if (status === 'complete') return null;
  if (!String(value.objective || '').trim()) return null;
  return value;
});

const goalStatus = computed(() => goal.value?.phase);
const canPause = computed(() => goalStatus.value === 'active' && goal.value?.activation === 'armed');

const objectiveLabel = computed(() => String(goal.value?.objective || '').trim());

const goalTitle = computed(() => {
  const status = goalStatus.value;
  if (status === 'blocked') return t('chat.goalBar.blocked');
  if (status === 'paused' || goal.value?.activation === 'disarmed') return t('chat.goalBar.paused');
  return t('chat.goal.timelineBadge');
});

const resolvedGoalSessionId = computed(
  () => String(goal.value?.session_id || activeSessionId.value).trim()
);

const handleEdit = async (): Promise<void> => {
  if (busy.value) return;
  const sessionId = resolvedGoalSessionId.value;
  if (!sessionId) return;
  const value = await mzPrompt({
    title: t('chat.goalBar.edit'),
    message: t('chat.goalBar.editHint'),
    inputValue: objectiveLabel.value,
    validator: (input: string) => Boolean(String(input || '').trim()) || t('chat.goal.objectiveRequired')
  });
  const nextObjective = String(value || '').trim();
  if (!nextObjective || nextObjective === objectiveLabel.value) return;
  busy.value = true;
  try {
    await chatStore.setSessionGoal(sessionId, { objective: nextObjective });
    ElMessage.success(t('chat.goalBar.saved'));
  } catch (error) {
    ElMessage.warning(t('chat.goalBar.failed'));
  } finally {
    busy.value = false;
  }
};

const handleRemove = async (): Promise<void> => {
  if (busy.value) return;
  const sessionId = resolvedGoalSessionId.value;
  if (!sessionId) return;
  const confirmed = await mzConfirm({
    title: t('chat.goalBar.remove'),
    message: t('chat.goalBar.removeConfirm'),
    tone: 'warning'
  });
  if (!confirmed) return;
  busy.value = true;
  try {
    // 删除走 DELETE 端点，随后同步本地 goal 状态。
    await clearSessionGoal(sessionId);
    chatStore.syncSessionGoal?.(sessionId, null);
    ElMessage.success(t('chat.goalBar.removed'));
  } catch (error) {
    ElMessage.warning(t('chat.goalBar.failed'));
  } finally {
    busy.value = false;
  }
};

const handlePause = async (): Promise<void> => {
  if (busy.value) return;
  const sessionId = resolvedGoalSessionId.value;
  if (!sessionId) return;
  busy.value = true;
  try {
    await chatStore.setSessionGoal(sessionId, { action: 'pause' });
    ElMessage.success(t('chat.goalBar.paused'));
  } catch (error) {
    ElMessage.warning(t('chat.goalBar.failed'));
  } finally {
    busy.value = false;
  }
};

const handleResume = async (): Promise<void> => {
  if (busy.value) return;
  const sessionId = resolvedGoalSessionId.value;
  if (!sessionId) return;
  busy.value = true;
  try {
    await chatStore.setSessionGoal(sessionId, { action: 'resume' });
    ElMessage.success(t('chat.command.goalResumed'));
  } catch (error) {
    ElMessage.warning(t('chat.goalBar.failed'));
  } finally {
    busy.value = false;
  }
};
</script>

<style scoped>
.goal-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 8px;
  padding: 7px 10px 7px 12px;
  border: 1px solid rgba(220, 96, 148, 0.24);
  border-radius: 12px;
  background: #fdf1f6;
  color: #9d2b5c;
  font-size: 13px;
  min-width: 0;
}

.goal-bar-icon {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  font-size: 13px;
  color: #d9437c;
}

.goal-bar-title {
  flex: 0 0 auto;
  font-weight: 600;
}

.goal-bar-time {
  flex: 0 0 auto;
  color: #b8557f;
  font-variant-numeric: tabular-nums;
}

.goal-bar-objective {
  flex: 1 1 auto;
  min-width: 0;
  color: #7a2949;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.goal-bar-actions {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.goal-bar-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  padding: 0;
  border: 1px solid rgba(220, 96, 148, 0.28);
  border-radius: 8px;
  background: #ffffff;
  color: #9d2b5c;
  font-size: 12px;
  cursor: pointer;
}

.goal-bar-action:hover:not(:disabled) {
  background: #ffe6ef;
}

.goal-bar-action:disabled {
  opacity: 0.6;
  cursor: default;
}

.goal-bar-action.is-primary {
  border-color: transparent;
  background: #d9437c;
  color: #ffffff;
}

.goal-bar-action.is-primary:hover:not(:disabled) {
  background: #c4366d;
}
</style>
