<template>
  <div v-if="goal" class="goal-bar" role="status">
    <span class="goal-bar-icon" aria-hidden="true">
      <i class="fa-solid fa-bullseye"></i>
    </span>
    <span class="goal-bar-title">{{ goalTitle }}</span>
    <span v-if="elapsedLabel" class="goal-bar-time" :title="elapsedHint">{{ elapsedLabel }}</span>
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
import { ElMessage, ElMessageBox } from 'element-plus';

import { useI18n } from '@/i18n';
import { clearSessionGoal } from '@/api/chat';
import { useChatStore } from '@/stores/chat';
import { normalizeChatDurationSeconds } from '@/utils/chatTiming';

/**
 * 目标条：时间线底部、输入区上方（参考样式：粉色圆角条 + 右侧图标操作）。
 *
 * 降级策略（后端字段现状，已核实）：
 * - 后端 `services/goal` 运行期写入 active、budget_limited、complete；
 *   `paused` 在用户暂停或预算受限时出现。active/paused/budget_limited
 *   都在此呈现；complete 是已达成，不再打扰用户。
 * - 「已用时」优先取服务端 `time_used_seconds`，缺失时按 `created_at` 推算（tooltip 标注来源）。
 * - 暂停/恢复走 PUT 的 `action: pause|resume`（api/chat_goal.rs 的 GoalCommand）。
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
  const status = String(value.status || '').trim().toLowerCase();
  // complete 是已达成，不再打扰用户。
  if (status === 'complete') return null;
  if (!String(value.objective || '').trim()) return null;
  return value;
});

const goalStatus = computed(() => String(goal.value?.status || '').trim().toLowerCase());
const canPause = computed(() => goalStatus.value === 'active');

const objectiveLabel = computed(() => String(goal.value?.objective || '').trim());

const goalTitle = computed(() => {
  const status = goalStatus.value;
  if (status === 'budget_limited') return t('chat.goalBar.budgetLimited');
  if (status === 'paused') return t('chat.goalBar.paused');
  return t('chat.goal.timelineBadge');
});

const formatDuration = (totalSeconds: number): string => {
  const seconds = Math.max(0, Math.round(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  if (hours > 0) return `${hours}小时${minutes}分${rest}秒`;
  if (minutes > 0) return `${minutes}分${rest}秒`;
  return `${rest}秒`;
};

const elapsedSeconds = computed<number | null>(() => {
  const value = goal.value;
  if (!value) return null;
  // 优先使用服务端累计用时；为 0 时退回按创建时间推算（并可能偏低，因为
  // 暂停期间不计入服务端累计）。
  const reported = normalizeChatDurationSeconds(value.time_used_seconds);
  if (reported !== null && reported > 0) return reported;
  const createdAt = Number(value.created_at || 0);
  if (Number.isFinite(createdAt) && createdAt > 0) {
    // 服务端 now_ts() 为「秒 + 毫秒小数」，这里同时兼容秒/毫秒时间戳。
    const startedAtMs = createdAt < 1e11 ? createdAt * 1000 : createdAt;
    const delta = (Date.now() - startedAtMs) / 1000;
    if (delta > 0) return delta;
  }
  return reported;
});

const elapsedLabel = computed(() => {
  const seconds = elapsedSeconds.value;
  if (seconds === null) return '';
  return formatDuration(seconds);
});

// 说明用时的来源，避免把推算值当成服务端累计值。
const elapsedHint = computed(() =>
  elapsedSeconds.value === null ? t('chat.goalBar.elapsedUnknown') : t('chat.goalBar.elapsedHint')
);

const resolvedGoalSessionId = computed(
  () => String(goal.value?.session_id || activeSessionId.value).trim()
);

const handleEdit = async (): Promise<void> => {
  if (busy.value) return;
  const sessionId = resolvedGoalSessionId.value;
  if (!sessionId) return;
  let nextObjective = '';
  try {
    const result = await ElMessageBox.prompt(
      t('chat.goalBar.editHint'),
      t('chat.goalBar.edit'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        inputValue: objectiveLabel.value,
        inputValidator: (value: string) => Boolean(String(value || '').trim()) || t('chat.goal.objectiveRequired')
      }
    );
    nextObjective = String(result.value || '').trim();
  } catch (error) {
    return;
  }
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
  try {
    await ElMessageBox.confirm(t('chat.goalBar.removeConfirm'), t('chat.goalBar.remove'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  busy.value = true;
  try {
    // 后端不支持「清空目标」的 PUT（`GoalCommand::Clear` 会被拒绝），
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
