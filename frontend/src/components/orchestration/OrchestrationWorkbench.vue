<template>
  <section class="orchestration-workbench">
    <div v-if="loading && !group" class="orchestration-state orchestration-state--loading">{{ t('common.loading') }}</div>
    <div v-else-if="!group" class="orchestration-state">
      <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
      <span>{{ t('orchestration.empty.selectGroup') }}</span>
    </div>
    <template v-else>
      <OrchestrationMissionCanvas
        class="orchestration-shell"
        :group="group"
        :agents="agents"
        :rounds="rounds"
        :active-round="activeRound"
        :active-round-missions="activeRoundMissions"
        :artifact-cards="artifactCards"
        :visible-workers="visibleWorkers"
        :visible-chat-messages="activeRoundChatMessages"
        :mother-workflow-items="motherWorkflowItems"
        :workflow-items-by-task="workflowItemsByTask"
        :workflow-preview-by-task="workflowPreviewByTask"
        :history-items="historyItems"
        :current-orchestration-id="currentOrchestrationId"
        :mother-agent-id="motherAgentId"
        :mother-name="motherName"
        :mother-session-id="motherSessionId"
        :run-id="runId"
        :dispatch-preview="liveDispatchPreview"
        :composer-text="composerText"
        :composer-sending="orchestrationStopBusy"
        :can-send="orchestrationCanSend"
        :composer-disabled="orchestrationComposerDisabled"
        :current-situation="currentSituationText"
        :next-round-ready="orchestrationNextRoundReady"
        :initializing="initializing"
        :history-loading="historyLoading"
        :is-active="isActive"
        :is-busy="orchestrationStopBusy"
        :is-ready="isReady"
        :runtime-locked="orchestrationRuntimeLocked"
        :group-description="group.description || t('orchestration.empty.description')"
        :resolve-worker-outputs="resolveWorkerOutputs"
        :resolve-worker-thread-session-id="resolveWorkerThreadSessionId"
        :resolve-message-avatar-image="resolveMessageAvatarImage"
        :avatar-label="avatarLabel"
        @open-agent="emit('open-agent', $event)"
        @update:composer-text="composerText = $event"
        @update:current-situation="handleCurrentSituationInput"
        @commit:current-situation="handleCommitCurrentSituation"
        @send="handleSendToMother"
        @trigger-round="handleRunRoundAction"
        @branch-run="handleBranchRun"
        @create-run="handleCreateRun"
        @start-run="handleStartRun"
        @exit-run="handleStopRun"
        @open-history="handleOpenHistoryDialog"
        @open-situation="handleOpenSituationDialog"
        @select-round="selectRound($event)"
        @restore-run="handleRestoreHistoryAction($event)"
        @delete-round-tail="handleDeleteBranchAfterRound($event)"
      />

      <el-dialog
        v-model="historyDialogVisible"
        width="520px"
        append-to-body
        class="messenger-modal messenger-modal--beeroom orchestration-theme-dialog orchestration-history-dialog"
      >
        <template #header>
          <div class="messenger-modal-header">
            <div>
              <div class="messenger-modal-title">{{ t('orchestration.dialog.historyTitle') }}</div>
              <div class="messenger-modal-subtitle">{{ t('orchestration.dialog.historySubtitle') }}</div>
            </div>
          </div>
        </template>

        <div v-if="historyLoading" class="messenger-list-empty">
          {{ t('common.loading') }}
        </div>
        <div v-else-if="!historyItems.length" class="messenger-list-empty">
          {{ t('orchestration.dialog.historyEmpty') }}
        </div>
        <div v-else class="orchestration-history-list">
          <article
            v-for="item in historyItems"
            :key="item.orchestrationId"
            class="orchestration-history-item"
            :class="{
              'is-current': item.orchestrationId === currentOrchestrationId,
              'is-deleting': deletingHistoryId === item.orchestrationId
            }"
          >
            <button
              class="orchestration-history-item-main"
              type="button"
              :disabled="orchestrationRuntimeLocked"
              @click="handleRestoreHistoryAction(item.orchestrationId)"
            >
              <div class="orchestration-history-item-head">
                <span class="orchestration-history-item-title">{{ item.runId }}</span>
                <span class="orchestration-history-item-time">
                  {{ formatHistoryPrimaryTime(item) }}
                </span>
              </div>
              <div class="orchestration-history-item-meta">
                <span
                  class="orchestration-history-item-status"
                  :class="resolveHistoryStatusClass(item)"
                >
                  {{ resolveHistoryStatusLabel(item) }}
                </span>
                <span class="orchestration-history-item-meta-text">
                  {{ t('orchestration.timeline.round', { round: item.latestRoundIndex }) }}
                </span>
                <span
                  v-if="item.orchestrationId === currentOrchestrationId"
                  class="orchestration-history-item-status orchestration-history-item-status--current"
                >
                  {{ t('orchestration.dialog.historyCurrent') }}
                </span>
                <span
                  v-else
                  class="orchestration-history-item-meta-text"
                >
                  {{ resolveHistorySecondaryTime(item) }}
                </span>
              </div>
            </button>
            <button
              class="orchestration-history-delete"
              type="button"
              :title="t('common.delete')"
              :aria-label="t('common.delete')"
              :disabled="orchestrationRuntimeLocked || cannotDeleteHistoryItem(item)"
              @click.stop="handleDeleteHistory(item)"
            >
              <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
            </button>
          </article>
        </div>
      </el-dialog>

      <el-dialog
        v-model="situationDialogVisible"
        width="760px"
        top="calc(var(--desktop-window-chrome-height, 36px) + 12px)"
        append-to-body
        class="messenger-modal messenger-modal--beeroom orchestration-theme-dialog orchestration-situation-dialog"
      >
        <template #header>
          <div class="messenger-modal-header">
            <div>
              <div class="messenger-modal-title">{{ t('orchestration.dialog.situationTitle') }}</div>
              <div class="messenger-modal-subtitle">{{ t('orchestration.dialog.situationSubtitle') }}</div>
            </div>
          </div>
        </template>

        <div class="orchestration-situation-toolbar">
          <div class="orchestration-situation-toolbar-copy">
            <span class="orchestration-situation-toolbar-title">
              {{ t('orchestration.dialog.situationImportHint') }}
            </span>
          </div>
          <div class="orchestration-situation-toolbar-actions">
            <el-button type="primary" :disabled="orchestrationRuntimeLocked" @click="triggerSituationImport">
              <i class="fa-solid fa-file-import" aria-hidden="true"></i>
              <span>{{ t('common.import') }}</span>
            </el-button>
            <input
              ref="situationImportInputRef"
              type="file"
              accept=".txt,text/plain"
              hidden
              @change="handleSituationImportChange"
            />
          </div>
        </div>

        <div class="orchestration-situation-shell">
          <div class="orchestration-situation-round-list">
            <button
              v-for="entry in situationRoundRows"
              :key="entry.key"
              class="orchestration-situation-round-item"
              :class="{
                'is-selected': entry.isSelected,
                'is-active-round': entry.isActiveRound
              }"
              type="button"
              :disabled="orchestrationRuntimeLocked"
              @click="selectSituationRound(entry.round)"
            >
              <div class="orchestration-situation-round-item-head">
                <span class="orchestration-situation-round-item-title">
                  {{ t('orchestration.timeline.round', { round: entry.round }) }}
                </span>
                <span
                  v-if="entry.hasPreset"
                  class="orchestration-situation-round-item-state is-filled"
                >
                  {{ t('orchestration.dialog.situationPreset') }}
                </span>
                <span
                  v-else
                  class="orchestration-situation-round-item-state"
                >
                  {{ t('orchestration.dialog.situationEmptyRound') }}
                </span>
              </div>
              <div class="orchestration-situation-round-item-meta">
                <span
                  v-if="entry.isActiveRound"
                  class="orchestration-situation-row-badge"
                >
                  {{ t('orchestration.dialog.situationCurrentRound') }}
                </span>
                <span
                  v-else-if="entry.isUpcomingRound"
                  class="orchestration-situation-row-meta"
                >
                  {{ t('orchestration.dialog.situationPlannedRound') }}
                </span>
              </div>
            </button>
          </div>

          <div class="orchestration-situation-editor">
            <div class="orchestration-situation-editor-head">
              <div class="orchestration-situation-row-head">
                <span class="orchestration-situation-row-title">
                  {{ t('orchestration.timeline.round', { round: selectedSituationRound }) }}
                </span>
                <span
                  v-if="selectedSituationEntry?.isActiveRound"
                  class="orchestration-situation-row-badge"
                >
                  {{ t('orchestration.dialog.situationCurrentRound') }}
                </span>
                <span
                  v-else-if="selectedSituationEntry?.isUpcomingRound"
                  class="orchestration-situation-row-meta"
                >
                  {{ t('orchestration.dialog.situationPlannedRound') }}
                </span>
              </div>
            </div>
            <textarea
              v-model="selectedSituationDraft"
              class="orchestration-situation-textarea"
              :placeholder="t('orchestration.dialog.situationPlaceholder')"
              :disabled="orchestrationRuntimeLocked"
              rows="13"
            ></textarea>
          </div>
        </div>

        <template #footer>
          <div class="messenger-modal-footer">
            <el-button @click="handleCloseSituationDialog">{{ t('common.cancel') }}</el-button>
            <el-button type="primary" :disabled="orchestrationRuntimeLocked" @click="handleSaveSituation">{{ t('common.confirm') }}</el-button>
          </div>
        </template>
      </el-dialog>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, toRef, watch, type Ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import { clearBeeroomMissionChatState } from '@/components/beeroom/beeroomMissionChatStateCache';
import { clearBeeroomMissionCanvasState } from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';
import { useBeeroomMissionCanvasRuntime } from '@/components/beeroom/useBeeroomMissionCanvasRuntime';
import {
  fetchBeeroomOrchestrationPrompts,
  updateBeeroomOrchestrationSessionContext
} from '@/api/beeroom';
import { cancelTeamRun, listSessionTeamRuns } from '@/api/swarm';
import OrchestrationMissionCanvas from '@/components/orchestration/OrchestrationMissionCanvas.vue';
import {
  type OrchestrationPromptTemplates,
  buildMotherDispatchEnvelope
} from '@/components/orchestration/orchestrationPrompting';
import {
  isOrchestrationMissionRunning,
  normalizeOrchestrationStatus,
  normalizeOrchestrationText
} from '@/components/orchestration/orchestrationShared';
import { useOrchestrationRuntimeState } from '@/components/orchestration/orchestrationRuntimeState';
import { getCurrentLanguage, useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';
import { chatDebugLog } from '@/utils/chatDebug';

const props = defineProps<{
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  missions: BeeroomMission[];
  loading: boolean;
  refreshing: boolean;
  error: string;
}>();

const emit = defineEmits<{
  (event: 'refresh'): void;
  (event: 'create'): void;
  (event: 'edit-situation'): void;
  (event: 'open-agent', agentId: string): void;
}>();

const { t } = useI18n();
const authStore = useAuthStore();

const orchestrationWorkbenchDebug = (event: string, payload?: unknown) => {
  chatDebugLog('orchestration-workbench', event, payload);
};

const groupRef = toRef(props, 'group');
const agentsRef = toRef(props, 'agents');
const missionsRef = toRef(props, 'missions');
const displayChatMessagesSeed = ref<MissionChatMessage[]>([]);
const orchestrationGroupId = computed(() => String(props.group?.group_id || props.group?.hive_id || '').trim());
const motherAgentId = computed(() => String(props.group?.mother_agent_id || '').trim());
const currentUserId = computed(() => {
  const user = authStore.user as Record<string, unknown> | null;
  return String(user?.id || user?.user_id || user?.username || '').trim();
});

const {
  runtimeState,
  runtimeScopeKey,
  clearScopeKey,
  activeRound,
  latestRound,
  pendingRound,
  activeRoundChatMessages,
  visibleWorkers,
  artifactCards,
  historyLoading,
  historyItems,
  motherWorkflowItems,
  workflowItemsByTask,
  workflowPreviewByTask,
  initializing,
  isActive,
  isReady,
  ensureRuntime,
  initializeRun,
  startRun,
  exitRun,
  loadHistory,
  restoreHistory,
  branchHistory,
  deleteHistory,
  truncateHistoryFromRound,
  createRound,
  reserveUserRound,
  finalizePendingRound,
  discardPendingRound,
  resolveRoundSituation,
  markMotherPrimerInjected,
  updateSituation,
  updatePlannedSituations,
  selectRound,
  resolveWorkerOutputs,
  resolveWorkerThreadSessionId
} = useOrchestrationRuntimeState({
  group: groupRef,
  agents: agentsRef,
  missions: missionsRef,
  displayChatMessages: displayChatMessagesSeed
});

const {
  chatCollapsed: _chatCollapsed,
  composerText,
  composerSending,
  dispatchPreview,
  displayChatMessages,
  handleComposerSend,
  handleDispatchStop,
  resolveMessageAvatarImage,
  avatarLabel
} = useBeeroomMissionCanvasRuntime({
  group: groupRef,
  mission: computed(() => null),
  agents: agentsRef,
  t,
  onRefresh: () => emit('refresh'),
  runtimeOverrides: {
    runtimeScopeKey,
    clearScopeKey,
    fixedMotherDispatchSessionId: computed(() => String(runtimeState.value?.motherSessionId || '').trim()),
    lockedComposerTargetAgentId: motherAgentId,
    disableAutoMotherDispatchReconcile: true
  }
});

const situationDialogVisible = ref(false);
const historyDialogVisible = ref(false);
const deletingHistoryId = ref('');
const situationPlanDraft = ref<Record<string, string>>({});
const stagedSituationDrafts = ref<Record<string, Record<string, string>>>({});
const currentSituationDraft = ref('');
const selectedSituationRound = ref(1);
const situationImportInputRef = ref<HTMLInputElement | null>(null);
const orchestrationPromptTemplates = ref<OrchestrationPromptTemplates | null>(null);
const orchestrationPromptLanguage = ref('');
const orchestrationDispatchPreparing = ref(false);
const orchestrationDispatchStopRequested = ref(false);
const orchestrationDispatchFlowToken = ref(0);
let orchestrationPromptLoadTask: Promise<OrchestrationPromptTemplates> | null = null;

const ORCHESTRATION_PROMPT_TEMPLATE_KEYS = [
  'mother_runtime',
  'round_artifacts',
  'worker_first_dispatch',
  'worker_round_artifacts',
  'worker_guide',
  'situation_context',
  'user_message'
] as const satisfies readonly (keyof OrchestrationPromptTemplates)[];

const rounds = computed(() => runtimeState.value?.rounds || []);
const runId = computed(() => String(runtimeState.value?.runId || '').trim());
const currentOrchestrationId = computed(() => String(runtimeState.value?.orchestrationId || '').trim());
const motherSessionId = computed(() => String(runtimeState.value?.motherSessionId || '').trim());
const activeDispatchPreviewStatus = computed(() => String(dispatchPreview.value?.status || '').trim().toLowerCase());
const hasActiveDispatchPreview = computed(() =>
  ['queued', 'running', 'resuming', 'awaiting_approval', 'waiting', 'accepted', 'pending'].includes(
    activeDispatchPreviewStatus.value
  )
);
const roundHasUserMessage = (round: { userMessage?: unknown } | null | undefined) =>
  Boolean(String(round?.userMessage || '').trim());
const sortedRounds = computed(() =>
  [...rounds.value].sort(
    (left, right) =>
      Number(left.index || 0) - Number(right.index || 0) ||
      Number(left.createdAt || 0) - Number(right.createdAt || 0)
  )
);
const formalRounds = computed(() => sortedRounds.value.filter((round) => roundHasUserMessage(round)));
const latestFormalRound = computed(() => formalRounds.value[formalRounds.value.length - 1] || null);
const resolveFrontierRoundIndex = () =>
  latestFormalRound.value ? Math.max(1, Number(latestFormalRound.value.index || 0) + 1) : 1;
const findPreparedRoundByIndex = (roundIndex: number) =>
  sortedRounds.value.find(
    (round) => Number(round.index || 0) === Math.max(1, Number(roundIndex || 0)) && !roundHasUserMessage(round)
  ) || null;
const frontierPreparedRound = computed(() => findPreparedRoundByIndex(resolveFrontierRoundIndex()));
const latestInteractiveRound = computed(
  () => frontierPreparedRound.value || latestFormalRound.value || sortedRounds.value[sortedRounds.value.length - 1] || null
);
const findDirectSuccessorRound = (roundIndex: number) => {
  const targetIndex = Math.max(1, Number(roundIndex || 0)) + 1;
  const candidates = sortedRounds.value
    .filter((round) => Number(round.index || 0) === targetIndex)
    .sort((left, right) => {
      const completionDiff = Number(roundHasUserMessage(right)) - Number(roundHasUserMessage(left));
      if (completionDiff !== 0) return completionDiff;
      return Number(left.createdAt || 0) - Number(right.createdAt || 0);
    });
  return candidates[0] || null;
};
const resolveImmediateNextRoundIndex = () => resolveFrontierRoundIndex();
const isViewingLatestRound = computed(() => {
  const latestId = String(latestInteractiveRound.value?.id || '').trim();
  const activeId = String(activeRound.value?.id || '').trim();
  return Boolean(latestId && activeId && latestId === activeId);
});
const shouldStageSituationDraft = computed(() => Boolean(currentOrchestrationId.value) && !isViewingLatestRound.value);
const orchestrationCanSend = computed(
  () =>
    Boolean(
      isReady.value &&
        isActive.value
    ) && !orchestrationStopBusy.value
);
const orchestrationComposerDisabled = computed(
  () => !isReady.value || !isActive.value
);
const currentSituationText = computed(() => currentSituationDraft.value);
const orchestrationNextRoundReady = computed(() => {
  if (!isReady.value || !isActive.value || orchestrationRunning.value) {
    return false;
  }
  return roundHasUserMessage(activeRound.value);
});

const motherName = computed(() => {
  const motherId = motherAgentId.value;
  if (!motherId) return t('orchestration.canvas.unassignedMother');
  const member = (Array.isArray(props.agents) ? props.agents : []).find(
    (item) => String(item.agent_id || '').trim() === motherId
  );
  return String(member?.name || props.group?.mother_agent_name || motherId).trim() || motherId;
});

const normalizeSituationEntries = (value: Record<string, string>) =>
  Object.fromEntries(
    Object.entries(value || {})
      .map(([key, entry]) => [String(normalizeSituationRound(key)), String(entry || '').trim()])
      .filter(([, entry]) => Boolean(entry))
  );

const clearStagedSituationDraft = (orchestrationId?: string) => {
  const targetId = String(orchestrationId || '').trim();
  if (!targetId) {
    stagedSituationDrafts.value = {};
    return;
  }
  if (!stagedSituationDrafts.value[targetId]) {
    return;
  }
  const nextDrafts = { ...stagedSituationDrafts.value };
  delete nextDrafts[targetId];
  stagedSituationDrafts.value = nextDrafts;
};

const rememberStagedSituationDraft = (
  entries: Record<string, string>,
  orchestrationId: string = currentOrchestrationId.value
) => {
  const targetId = String(orchestrationId || '').trim();
  if (!targetId) return;
  stagedSituationDrafts.value = {
    ...stagedSituationDrafts.value,
    [targetId]: normalizeSituationEntries(entries)
  };
};

const hasStagedSituationDraft = computed(
  () => {
    const currentId = currentOrchestrationId.value;
    if (!currentId) return false;
    return Object.keys(stagedSituationDrafts.value[currentId] || {}).length > 0;
  }
);

const plannedSituations = computed<Record<string, string>>(() => {
  const source = runtimeState.value?.plannedSituations;
  const persisted = source && typeof source === 'object' ? normalizeSituationEntries(source as Record<string, string>) : {};
  const mergedFromRounds = { ...persisted };
  rounds.value.forEach((round) => {
    const roundKey = String(normalizeSituationRound(round.index));
    const situation = String(round.situation || '').trim();
    if (!situation) return;
    mergedFromRounds[roundKey] = situation;
  });
  const currentId = currentOrchestrationId.value;
  if (currentId && hasStagedSituationDraft.value) {
    return {
      ...mergedFromRounds,
      ...(stagedSituationDrafts.value[currentId] || {})
    };
  }
  return mergedFromRounds;
});
const latestRoundIndex = computed(() =>
  Math.max(1, Number(activeRound.value?.index || 0), Number(latestFormalRound.value?.index || 0) || 1)
);
const normalizeSituationRound = (value: unknown) =>
  Math.max(1, Number.parseInt(String(value ?? '').trim(), 10) || 1);

const resolveNextUserRoundIndex = (state: { rounds?: Array<{ index?: number; userMessage?: string }> } | null | undefined) => {
  const formalRoundIndex = Math.max(
    0,
    ...(Array.isArray(state?.rounds) ? state.rounds : [])
      .filter((round) => String(round?.userMessage || '').trim())
      .map((round) => Number(round?.index || 0))
  );
  const sentUserMessageCount = activeRoundChatMessages.value.filter((message) => message.tone === 'user').length;
  return Math.max(1, formalRoundIndex + 1, sentUserMessageCount + 1);
};

const selectedSituationKey = computed(() => String(normalizeSituationRound(selectedSituationRound.value)));
const selectedSituationDraft = computed({
  get: () => String(situationPlanDraft.value[selectedSituationKey.value] || ''),
  set: (value: string) => {
    situationPlanDraft.value = {
      ...situationPlanDraft.value,
      [selectedSituationKey.value]: String(value || '')
    };
  }
});

const situationRoundRows = computed(() => {
  const plannedRoundNumbers = Object.keys(situationPlanDraft.value || {})
    .map((key) => normalizeSituationRound(key))
    .filter((value, index, array) => array.indexOf(value) === index);
  const targetMax = Math.max(
    normalizeSituationRound(selectedSituationRound.value),
    latestRoundIndex.value + 2,
    ...plannedRoundNumbers
  );
  return Array.from({ length: Math.max(6, targetMax) }, (_, index) => {
    const round = index + 1;
    const key = String(round);
    const value = String(situationPlanDraft.value[key] || '').trim();
    return {
      key,
      round,
      hasPreset: Boolean(value),
      isSelected: round === normalizeSituationRound(selectedSituationRound.value),
      isActiveRound: round === normalizeSituationRound(activeRound.value?.index || 0),
      isUpcomingRound: round > latestRoundIndex.value
    };
  });
});

const selectedSituationEntry = computed(
  () => situationRoundRows.value.find((entry) => entry.round === normalizeSituationRound(selectedSituationRound.value)) || null
);

const selectSituationRound = (round: number) => {
  const nextRound = normalizeSituationRound(round);
  selectedSituationRound.value = nextRound;
};

const parseSituationImportText = (content: string) => {
  const text = String(content || '').replace(/\r\n/g, '\n');
  if (!text.trim()) return {} as Record<string, string>;
  const lines = text.split('\n');
  const entries = new Map<number, string[]>();
  let currentRound = 1;
  let buffer = entries.get(currentRound) || [];
  entries.set(currentRound, buffer);
  lines.forEach((line) => {
    const matched = line.match(/^\s*#\s*(\d+)\s*$/);
    if (matched) {
      currentRound = normalizeSituationRound(matched[1]);
      buffer = entries.get(currentRound) || [];
      entries.set(currentRound, buffer);
      return;
    }
    buffer.push(line);
  });
  return Object.fromEntries(
    Array.from(entries.entries())
      .map(([round, valueLines]) => [String(round), valueLines.join('\n').trim()])
      .filter(([, value]) => Boolean(String(value || '').trim()))
  );
};

const triggerSituationImport = () => {
  if (!situationImportInputRef.value) return;
  situationImportInputRef.value.value = '';
  situationImportInputRef.value.click();
};

const readSituationImportFile = (file: File) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ''));
    reader.onerror = () => reject(reader.error || new Error('situation_import_failed'));
    reader.readAsText(file, 'utf-8');
  });

const handleSituationImportChange = async (event: Event) => {
  const input = event.target as HTMLInputElement | null;
  const file = input?.files?.[0];
  if (!file) return;
  try {
    const content = await readSituationImportFile(file);
    const parsed = parseSituationImportText(content);
    const keys = Object.keys(parsed);
    if (!keys.length) {
      ElMessage.warning(t('orchestration.dialog.situationImportEmpty'));
      return;
    }
    situationPlanDraft.value = {
      ...situationPlanDraft.value,
      ...parsed
    };
    selectSituationRound(normalizeSituationRound(keys[0]));
    ElMessage.success(t('orchestration.dialog.situationImportSuccess', { count: keys.length }));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  } finally {
    if (input) {
      input.value = '';
    }
  }
};

const handleCloseSituationDialog = () => {
  situationDialogVisible.value = false;
  situationPlanDraft.value = { ...plannedSituations.value };
  selectSituationRound(activeRound.value?.index || latestRoundIndex.value);
};

const activeRoundMissions = computed(() => {
  const missionIds = new Set(activeRound.value?.missionIds || []);
  return (Array.isArray(props.missions) ? props.missions : []).filter((item) =>
    missionIds.has(String(item?.mission_id || item?.team_run_id || '').trim())
  );
});

const activeRoundRunningMissions = computed(() =>
  activeRoundMissions.value.filter((item) => isOrchestrationMissionRunning(item))
);

const latestRoundMissions = computed(() => {
  const missionIds = new Set(latestFormalRound.value?.missionIds || []);
  return (Array.isArray(props.missions) ? props.missions : []).filter((item) =>
    missionIds.has(String(item?.mission_id || item?.team_run_id || '').trim())
  );
});

const latestRoundRunningMissions = computed(() =>
  latestRoundMissions.value.filter((item) => isOrchestrationMissionRunning(item))
);

const beginOrchestrationDispatchFlow = () => {
  const nextToken = orchestrationDispatchFlowToken.value + 1;
  orchestrationDispatchFlowToken.value = nextToken;
  orchestrationDispatchPreparing.value = true;
  orchestrationDispatchStopRequested.value = false;
  return nextToken;
};

const finishOrchestrationDispatchFlow = (token: number) => {
  if (orchestrationDispatchFlowToken.value !== token) return;
  orchestrationDispatchPreparing.value = false;
  orchestrationDispatchStopRequested.value = false;
};

const buildOrchestrationDispatchStoppedError = () => {
  const error = new Error('orchestration_dispatch_stopped');
  (error as Error & { code?: string }).code = 'ORCHESTRATION_DISPATCH_STOPPED';
  return error;
};

const ensureOrchestrationDispatchFlowActive = (token: number) => {
  if (orchestrationDispatchFlowToken.value !== token || orchestrationDispatchStopRequested.value) {
    throw buildOrchestrationDispatchStoppedError();
  }
};

const isOrchestrationDispatchStoppedError = (error: unknown) =>
  (error as { code?: string; message?: string } | null)?.code === 'ORCHESTRATION_DISPATCH_STOPPED' ||
  (error as { message?: string } | null)?.message === 'orchestration_dispatch_stopped';

const orchestrationPendingRoundActive = computed(() => {
  const pendingRoundId = normalizeOrchestrationText(runtimeState.value?.pendingRoundId);
  if (!pendingRoundId) return false;
  const pendingRound = rounds.value.find((item) => normalizeOrchestrationText(item.id) === pendingRoundId) || null;
  if (!pendingRound) return false;
  const pendingMessageStartedAt = Number(runtimeState.value?.pendingMessageStartedAt || 0);
  if (pendingMessageStartedAt > 0) return true;
  return Boolean(normalizeOrchestrationText(pendingRound.userMessage));
});

const orchestrationRunning = computed(
  () =>
    orchestrationDispatchPreparing.value ||
    composerSending.value ||
    hasActiveDispatchPreview.value ||
    latestRoundRunningMissions.value.length > 0 ||
    activeRoundRunningMissions.value.length > 0 ||
    orchestrationPendingRoundActive.value
);
const orchestrationStopBusy = computed(() => orchestrationRunning.value);
const orchestrationRuntimeLocked = computed(() => orchestrationRunning.value);

const liveDispatchPreview = computed<BeeroomSwarmDispatchPreview | null>(() => {
  if (!orchestrationRunning.value) {
    return null;
  }
  const latestRenderableRound = latestInteractiveRound.value;
  if (!latestRenderableRound || latestRenderableRound.id !== activeRound.value?.id) {
    return null;
  }
  return dispatchPreview.value || null;
});

const visibleChatMessages = computed(() => activeRoundChatMessages.value);

const handleCurrentSituationInput = (value: string) => {
  currentSituationDraft.value = String(value || '');
};

const handleCommitCurrentSituation = async () => {
  if (!isReady.value) {
    return;
  }
  const currentRound = activeRound.value;
  if (!currentRound) {
    return;
  }
  const draftValue = String(currentSituationDraft.value || '');
  if (draftValue === String(currentRound.situation || '')) {
    return;
  }
  if (!isViewingLatestRound.value) {
    const nextEntries = {
      ...plannedSituations.value,
      [String(normalizeSituationRound(currentRound.index))]: draftValue
    };
    rememberStagedSituationDraft(nextEntries);
    return;
  }
  if (orchestrationRuntimeLocked.value) {
    return;
  }
  await updateSituation(draftValue);
};

const buildOrchestrationRoundDispatchText = (situation: string) => String(situation || '').trim();

const updateSituationForRound = async (roundIndex: number, value: string) => {
  const targetRoundIndex = normalizeSituationRound(roundIndex);
  const normalizedValue = String(value || '');
  const nextEntries = { ...plannedSituations.value };
  if (String(normalizedValue).trim()) {
    nextEntries[String(targetRoundIndex)] = normalizedValue;
  } else {
    delete nextEntries[String(targetRoundIndex)];
  }
  const isEditingActiveRound = Number(activeRound.value?.index || 0) === targetRoundIndex;
  if (isEditingActiveRound && isViewingLatestRound.value && !orchestrationRuntimeLocked.value) {
    await updateSituation(normalizedValue);
    return;
  }
  if (shouldStageSituationDraft.value) {
    rememberStagedSituationDraft(nextEntries);
    return;
  }
  await updatePlannedSituations(nextEntries);
};

const createPreparedRoundAtIndex = async (roundIndex: number, preferredSituation = '') => {
  const nextRoundIndex = Math.max(1, Number(roundIndex || 0));
  const existingPreparedRound =
    rounds.value.find(
      (round) => Number(round.index || 0) === nextRoundIndex && !String(round.userMessage || '').trim()
    ) || null;
  if (existingPreparedRound?.id) {
    const nextSituation =
      String(preferredSituation || '').trim() ||
      resolveDraftSituationByRoundIndex(nextRoundIndex) ||
      String(existingPreparedRound.situation || '').trim() ||
      await resolveRoundSituation(nextRoundIndex);
    if (nextSituation !== String(existingPreparedRound.situation || '').trim()) {
      await updateSituationForRound(nextRoundIndex, nextSituation);
    }
    selectRound(existingPreparedRound.id);
    return existingPreparedRound;
  }
  const nextSituation =
    String(preferredSituation || '').trim() ||
    resolveDraftSituationByRoundIndex(nextRoundIndex) ||
    await resolveRoundSituation(nextRoundIndex);
  const created = await createRound(nextSituation, '', { roundIndex: nextRoundIndex });
  if (created?.id) {
    selectRound(created.id);
  }
  return created;
};

const createNextPreparedRound = async (preferredSituation = '') =>
  createPreparedRoundAtIndex(resolveImmediateNextRoundIndex(), preferredSituation);

const handleRunRoundAction = async () => {
  if (orchestrationRunning.value) {
    await handleSendToMother();
    return;
  }
  if (!isReady.value) {
    ElMessage.warning(t('orchestration.message.createRunRequired'));
    return;
  }
  if (!isActive.value) {
    ElMessage.warning(t('orchestration.message.startRunRequired'));
    return;
  }
  await handleCommitCurrentSituation();
  const currentRoundIndex = Math.max(1, Number(activeRound.value?.index || latestRound.value?.index || 1));
  const selectedRoundHasUserMessage = roundHasUserMessage(activeRound.value);
  const directSuccessorRound = findDirectSuccessorRound(currentRoundIndex);
  if (selectedRoundHasUserMessage) {
    if (directSuccessorRound?.id) {
      selectRound(directSuccessorRound.id);
      return;
    }
    await createPreparedRoundAtIndex(currentRoundIndex + 1);
    return;
  }
  const situation =
    resolveDraftSituationByRoundIndex(currentRoundIndex) ||
    String(currentSituationDraft.value || activeRound.value?.situation || '').trim() ||
    await resolveRoundSituation(currentRoundIndex);
  composerText.value = buildOrchestrationRoundDispatchText(situation);
  if (!composerText.value) {
    ElMessage.warning(t('orchestration.message.situationRequired'));
    return;
  }
  await handleSendToMother();
};

const resolveDraftSituationByRoundIndex = (roundIndex: number) => {
  const roundKey = String(normalizeSituationRound(roundIndex));
  const draftValue = String(plannedSituations.value[roundKey] || '').trim();
  if (draftValue) {
    return draftValue;
  }
  return String(rounds.value.find((item) => item.index === normalizeSituationRound(roundIndex))?.situation || '').trim();
};

const normalizePromptTemplates = (value: unknown): OrchestrationPromptTemplates => {
  const record = value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
  const templates = Object.fromEntries(
    ORCHESTRATION_PROMPT_TEMPLATE_KEYS.map((key) => [key, String(record[key] || '').trim()])
  ) as OrchestrationPromptTemplates;
  const missing = ORCHESTRATION_PROMPT_TEMPLATE_KEYS.filter((key) => !templates[key]);
  if (missing.length) {
    throw new Error(t('orchestration.prompt.loadFailed'));
  }
  return templates;
};

const ensureOrchestrationPromptTemplates = async () => {
  const language = getCurrentLanguage();
  if (orchestrationPromptTemplates.value && orchestrationPromptLanguage.value === language) {
    return orchestrationPromptTemplates.value;
  }
  if (!orchestrationPromptLoadTask) {
    orchestrationPromptLoadTask = fetchBeeroomOrchestrationPrompts()
      .then((response) => {
        const templates = normalizePromptTemplates(response?.data?.data?.prompts);
        orchestrationPromptTemplates.value = templates;
        orchestrationPromptLanguage.value = language;
        return templates;
      })
      .finally(() => {
        orchestrationPromptLoadTask = null;
      });
  }
  return orchestrationPromptLoadTask;
};

const handleCreateRun = async () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  try {
    let requestedRunName = '';
    try {
      const promptResult = await ElMessageBox.prompt(
        t('orchestration.dialog.createNameMessage'),
        t('orchestration.action.create'),
        {
          confirmButtonText: t('common.confirm'),
          cancelButtonText: t('common.cancel'),
          inputPlaceholder: t('orchestration.dialog.createNamePlaceholder'),
          inputValue: ''
        }
      );
      requestedRunName = String(promptResult.value || '').trim();
    } catch (error: any) {
      if (String(error || '') === 'cancel' || String(error || '') === 'close') {
        return;
      }
      throw error;
    }
    const previousRunId = runId.value;
    const previousRoundId = String(activeRound.value?.id || '').trim();
    const previousScopeKey =
      previousRunId && previousRoundId ? `orchestration:${previousRunId}:${previousRoundId}` : '';
    if (previousScopeKey) {
      clearBeeroomMissionCanvasState(previousScopeKey);
    }
    const previousRuntimeScopeKey = String(runtimeScopeKey.value || '').trim();
    if (previousRuntimeScopeKey) {
      clearBeeroomMissionChatState(previousRuntimeScopeKey);
    }
    composerText.value = '';
    situationPlanDraft.value = {};
    clearStagedSituationDraft();
    currentSituationDraft.value = '';
    selectSituationRound(1);
    situationDialogVisible.value = false;
    const nextState = await initializeRun({ runName: requestedRunName });
    await syncMotherSessionContextForState(nextState, 1);
    await loadHistory().catch(() => []);
    emit('create');
    ElMessage.success(t('orchestration.message.created'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleSaveSituation = () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  const nextDraft = normalizeSituationEntries(situationPlanDraft.value);
  const shouldStage = shouldStageSituationDraft.value;
  const task = shouldStage
    ? Promise.resolve(rememberStagedSituationDraft(nextDraft))
    : updatePlannedSituations(nextDraft);
  void task
    .then(() => {
      situationDialogVisible.value = false;
      emit('edit-situation');
    })
    .catch((error: any) => {
      ElMessage.error(String(error?.message || t('common.requestFailed')));
    });
};

const handleBranchRun = async () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  if (!isReady.value) {
    ElMessage.warning(t('orchestration.message.createRunRequired'));
    return;
  }
  if (!isActive.value) {
    ElMessage.warning(t('orchestration.message.startRunRequired'));
    return;
  }
  if (!currentOrchestrationId.value) {
    ElMessage.warning(t('orchestration.message.branchLatestBlocked'));
    return;
  }
  const branchSourceRound = activeRound.value || latestFormalRound.value || null;
  if (!branchSourceRound || !roundHasUserMessage(branchSourceRound)) {
    ElMessage.warning(t('orchestration.message.branchRequired'));
    return;
  }
  try {
    const sourceOrchestrationId = currentOrchestrationId.value;
    await handleCommitCurrentSituation();
    const branchBaseRoundIndex = Math.max(1, Number(branchSourceRound.index || 1));
    const draftSituation = String(currentSituationDraft.value || branchSourceRound.situation || '');
    const branchedState = await branchHistory(currentOrchestrationId.value, branchBaseRoundIndex, {
      activate: true
    });
    if (sourceOrchestrationId && branchedState?.orchestrationId) {
      const stagedDraft = stagedSituationDrafts.value[sourceOrchestrationId] || null;
      if (stagedDraft) {
        rememberStagedSituationDraft(stagedDraft, String(branchedState.orchestrationId || ''));
      }
    }
    if (branchedState?.active) {
    const nextStateRoundIndex = Math.max(
      1,
      Number(
        branchedState.rounds
          .filter((round) => roundHasUserMessage(round))
          .slice(-1)[0]?.index || branchBaseRoundIndex
      )
    );
      await syncMotherSessionContextForState(branchedState, nextStateRoundIndex);
      await createNextPreparedRound(draftSituation);
    }
    emit('refresh');
    ElMessage.success(t('orchestration.message.branched'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleStopRun = async () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  try {
    await exitRun();
    composerText.value = '';
    situationDialogVisible.value = false;
    historyDialogVisible.value = false;
    clearStagedSituationDraft();
    await loadHistory().catch(() => []);
    emit('refresh');
    ElMessage.success(t('orchestration.message.stopped'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleStartRun = async () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  try {
    const nextState = await startRun();
    clearStagedSituationDraft();
    await syncMotherSessionContextForState(
      nextState,
      Number(nextState?.rounds?.[nextState.rounds.length - 1]?.index || latestRound.value?.index || 1)
    );
    await loadHistory().catch(() => []);
    emit('refresh');
    ElMessage.success(t('orchestration.message.started'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleRestoreHistoryAction = async (
  payload:
    | string
    | {
        orchestrationId: string;
        roundIndex?: number;
        preview?: boolean;
      }
) => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  try {
    const normalizedPayload =
      typeof payload === 'string'
        ? { orchestrationId: payload, roundIndex: 0, preview: false }
        : {
            orchestrationId: String(payload?.orchestrationId || '').trim(),
            roundIndex: Math.max(0, Number(payload?.roundIndex || 0)),
            preview: payload?.preview === true
          };
    const currentRunSelected = normalizedPayload.orchestrationId === currentOrchestrationId.value;
    if (currentRunSelected) {
      if (normalizedPayload.preview && normalizedPayload.roundIndex > 0) {
        const previewRound =
          rounds.value.find(
            (round) =>
              Number(round.index || 0) === normalizedPayload.roundIndex &&
              !String(round.userMessage || '').trim()
          ) || null;
        if (previewRound?.id) {
          selectRound(previewRound.id);
          return;
        }
        const createdRound = await createPreparedRoundAtIndex(normalizedPayload.roundIndex);
        if (createdRound?.id) {
          selectRound(createdRound.id);
        }
        return;
      }
      if (normalizedPayload.roundIndex > 0) {
        const targetRound =
          rounds.value.find((round) => Number(round.index || 0) === normalizedPayload.roundIndex) || null;
        if (targetRound?.id) {
          selectRound(targetRound.id);
          return;
        }
      }
    }
    const nextState = await restoreHistory(normalizedPayload.orchestrationId, { activate: isActive.value });
    if (nextState && normalizedPayload.roundIndex > 0) {
      if (normalizedPayload.preview) {
        const previewRound =
          nextState.rounds.find(
            (round) =>
              Number(round.index || 0) === normalizedPayload.roundIndex &&
              !String(round.userMessage || '').trim()
          ) || null;
        if (previewRound?.id) {
          selectRound(previewRound.id);
        } else {
          const createdRound = await createRound(
            resolveDraftSituationByRoundIndex(normalizedPayload.roundIndex) ||
              await resolveRoundSituation(normalizedPayload.roundIndex),
            '',
            { roundIndex: normalizedPayload.roundIndex }
          );
          if (createdRound?.id) {
            selectRound(createdRound.id);
          }
        }
      } else {
        const targetRound =
          nextState.rounds.find((round) => Number(round.index || 0) === normalizedPayload.roundIndex) || null;
        if (targetRound?.id) {
          selectRound(targetRound.id);
        }
      }
    }
    if (nextState?.active) {
      await syncMotherSessionContextForState(
        nextState,
        normalizedPayload.roundIndex > 0
          ? normalizedPayload.roundIndex
          : Number(nextState.rounds[nextState.rounds.length - 1]?.index || 1)
      );
    }
    historyDialogVisible.value = false;
    emit('refresh');
    ElMessage.success(t('chat.history.restoreSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('chat.history.restoreFailed')));
  }
};

const handleOpenHistoryDialog = () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  historyDialogVisible.value = true;
};

const handleOpenSituationDialog = () => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  situationDialogVisible.value = true;
};

const formatHistoryTime = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return t('common.none');
  }
  return new Date(numeric).toLocaleString(getCurrentLanguage());
};

const formatHistoryPrimaryTime = (item: { updatedAt: number; restoredAt: number; exitedAt: number; enteredAt: number }) =>
  formatHistoryTime(item.updatedAt || item.restoredAt || item.exitedAt || item.enteredAt);

const resolveHistorySecondaryTime = (item: { restoredAt: number; exitedAt: number; enteredAt: number }) => {
  if (item.restoredAt) {
    return t('orchestration.dialog.historyRestoredAt', { time: formatHistoryTime(item.restoredAt) });
  }
  if (item.exitedAt) {
    return t('orchestration.dialog.historyExitedAt', { time: formatHistoryTime(item.exitedAt) });
  }
  return t('orchestration.dialog.historyEnteredAt', { time: formatHistoryTime(item.enteredAt) });
};

const resolveHistoryStatusLabel = (item: { orchestrationId: string; status: string }) => {
  if (item.orchestrationId === currentOrchestrationId.value && isActive.value) {
    return t('orchestration.dialog.historyStatusActive');
  }
  const status = normalizeOrchestrationStatus(item.status);
  if (status === 'active') {
    return t('orchestration.dialog.historyStatusActive');
  }
  return t('orchestration.dialog.historyStatusClosed');
};

const resolveHistoryStatusClass = (item: { orchestrationId: string; status: string }) => {
  if (item.orchestrationId === currentOrchestrationId.value && isActive.value) {
    return 'is-active';
  }
  return normalizeOrchestrationStatus(item.status) === 'active' ? 'is-active' : 'is-closed';
};

const syncMotherSessionContextForState = async (
  state: { motherSessionId?: string; runId?: string } | null | undefined,
  roundIndex: number
) => {
  const sessionId = String(state?.motherSessionId || '').trim();
  const nextRunId = String(state?.runId || '').trim();
  const groupId = orchestrationGroupId.value;
  if (!sessionId || !nextRunId || !groupId) return;
  await updateBeeroomOrchestrationSessionContext({
    session_id: sessionId,
    run_id: nextRunId,
    group_id: groupId,
    role: 'mother',
    round_index: Math.max(1, Number(roundIndex) || 1),
    mother_agent_id: String(motherAgentId.value || '').trim()
  });
};

const cannotDeleteHistoryItem = (item: { orchestrationId: string }) =>
  deletingHistoryId.value === item.orchestrationId ||
  (item.orchestrationId === currentOrchestrationId.value && isActive.value);

const handleDeleteHistory = async (item: { orchestrationId: string; runId: string }) => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  if (cannotDeleteHistoryItem(item)) return;
  try {
    await ElMessageBox.confirm(
      t('orchestration.dialog.historyDeleteConfirm', { runId: item.runId }),
      t('common.notice'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
  } catch {
    return;
  }
  deletingHistoryId.value = item.orchestrationId;
  try {
    await deleteHistory(item.orchestrationId);
    if (item.orchestrationId === currentOrchestrationId.value && !isActive.value) {
      historyDialogVisible.value = false;
    }
    ElMessage.success(t('orchestration.dialog.historyDeleteSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.deleteFailed')));
  } finally {
    deletingHistoryId.value = '';
  }
};

const handleDeleteBranchAfterRound = async (payload: { orchestrationId: string; roundIndex: number }) => {
  if (orchestrationRuntimeLocked.value) {
    ElMessage.warning(t('orchestration.message.busySwitchBlocked'));
    return;
  }
  const targetRoundIndex = Math.max(1, Number(payload.roundIndex || 1));
  try {
    await ElMessageBox.confirm(
      t('orchestration.timeline.deleteAfterConfirm', { round: targetRoundIndex }),
      t('common.notice'),
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
    await truncateHistoryFromRound(payload.orchestrationId, targetRoundIndex);
    const targetId = String(payload.orchestrationId || '').trim();
    const currentEntries = stagedSituationDrafts.value[targetId] || null;
    if (currentEntries) {
      const trimmedEntries = Object.fromEntries(
        Object.entries(currentEntries).filter(([key]) => normalizeSituationRound(key) <= targetRoundIndex)
      );
      if (Object.keys(trimmedEntries).length) {
        rememberStagedSituationDraft(trimmedEntries, targetId);
      } else {
        clearStagedSituationDraft(targetId);
      }
    }
    emit('refresh');
    ElMessage.success(t('orchestration.timeline.deleteAfterSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.deleteFailed')));
  }
};

const syncMotherSessionContext = async (roundIndex: number) =>
  syncMotherSessionContextForState(runtimeState.value, roundIndex);

const stopActiveRoundMissions = async () => {
  const sessionId = String(motherSessionId.value || '').trim();
  if (!sessionId) return;
  const missionIds = new Set(
    latestRoundRunningMissions.value
      .map((item) => String(item?.mission_id || item?.team_run_id || '').trim())
      .filter(Boolean)
  );
  if (!missionIds.size) {
    const response = await listSessionTeamRuns(sessionId, { limit: 100 }).catch(() => null);
    const runs = Array.isArray(response?.data?.data?.items) ? response?.data?.data?.items : [];
    runs.forEach((item: Record<string, unknown>) => {
      const teamRunId = String(item?.team_run_id || item?.teamRunId || '').trim();
      const status = String(item?.status || '').trim().toLowerCase();
      if (!teamRunId) return;
      if (['success', 'completed', 'failed', 'error', 'timeout', 'cancelled', 'canceled'].includes(status)) {
        return;
      }
      missionIds.add(teamRunId);
    });
  }
  if (!missionIds.size) return;
  await Promise.all(
    Array.from(missionIds).map((teamRunId) =>
      cancelTeamRun(teamRunId).catch(() => null)
    )
  );
};

const stopOrchestrationDispatch = async () => {
  const sessionId = String(motherSessionId.value || '').trim();
  await handleDispatchStop({
    force: true,
    sessionId
  }).catch(() => null);
  await stopActiveRoundMissions().catch(() => null);
};

const handleSendToMother = async () => {
  if (orchestrationRunning.value) {
    if (orchestrationDispatchPreparing.value) {
      orchestrationDispatchStopRequested.value = true;
    }
    const pending = pendingRound.value;
    await stopOrchestrationDispatch();
    if (pending?.id) {
      await discardPendingRound(pending.id, { clearSituation: true });
    }
    clearStagedSituationDraft();
    return;
  }
  const content = String(composerText.value || '').trim();
  if (!content) return;
  if (!isActive.value) {
    ElMessage.warning(t('orchestration.message.startRunRequired'));
    return;
  }
  if (!isViewingLatestRound.value) {
    ElMessage.warning(t('orchestration.message.branchRequired'));
    return;
  }
  await handleCommitCurrentSituation();
  const flowToken = beginOrchestrationDispatchFlow();
  let reservedRoundId = '';
  try {
    ensureOrchestrationDispatchFlowActive(flowToken);
    const state = await ensureRuntime();
    ensureOrchestrationDispatchFlowActive(flowToken);
    if (!String(state?.motherSessionId || '').trim()) {
      throw new Error(t('orchestration.message.createRunRequired'));
    }
    let workingRunId = runId.value;
    const inferredNextRoundIndex = resolveNextUserRoundIndex(state);
    orchestrationWorkbenchDebug('send:before-reserve', {
      runId: String(state?.runId || '').trim(),
      orchestrationId: String(state?.orchestrationId || '').trim(),
      activeRoundId: String(activeRound.value?.id || '').trim(),
      latestRoundId: String(latestRound.value?.id || '').trim(),
      inferredNextRoundIndex,
      rounds: (state?.rounds || []).map((round) => ({
        id: String(round?.id || '').trim(),
        index: Number(round?.index || 0),
        hasUserMessage: Boolean(String(round?.userMessage || '').trim())
      }))
    });
    const targetRound =
      (state?.rounds || []).find(
        (round) => Number(round?.index || 0) === inferredNextRoundIndex && !String(round?.userMessage || '').trim()
      ) || null;
    const initialRoundSituation =
      resolveDraftSituationByRoundIndex(inferredNextRoundIndex) ||
      await resolveRoundSituation(inferredNextRoundIndex);
    const reservedRound = await reserveUserRound({
      targetRoundId: targetRound?.id || '',
      situation: initialRoundSituation,
      userMessage: content
    });
    orchestrationWorkbenchDebug('send:after-reserve', {
      reservedRoundId: String(reservedRound?.id || '').trim(),
      reservedRoundIndex: Number(reservedRound?.index || 0),
      targetRoundId: String(targetRound?.id || '').trim(),
      inferredNextRoundIndex
    });
    reservedRoundId = String(reservedRound?.id || '').trim();
    if (reservedRoundId) {
      selectRound(reservedRoundId);
    }
    ensureOrchestrationDispatchFlowActive(flowToken);
    const actualRoundIndex = Math.max(1, Number(reservedRound?.index || inferredNextRoundIndex));
    const roundSituation =
      resolveDraftSituationByRoundIndex(actualRoundIndex) ||
      String(reservedRound?.situation || '').trim() ||
      await resolveRoundSituation(actualRoundIndex);
    await syncMotherSessionContext(actualRoundIndex);
    ensureOrchestrationDispatchFlowActive(flowToken);
    const includePrimer = state?.motherPrimerInjected !== true;
    const templates = await ensureOrchestrationPromptTemplates();
    ensureOrchestrationDispatchFlowActive(flowToken);
    const dispatchContent = buildMotherDispatchEnvelope({
      group: props.group,
      agents: props.agents,
      runId: workingRunId,
      roundIndex: actualRoundIndex,
      userMessage: content,
      situation: roundSituation,
      includePrimer,
      currentUserId: currentUserId.value,
      templates
    });
    ensureOrchestrationDispatchFlowActive(flowToken);
    const sendResult = await handleComposerSend({
      content: dispatchContent,
      displayContent: content,
      displayCreatedAt: Number(reservedRound?.createdAt || 0) > 0
        ? Number(reservedRound?.createdAt || 0) / 1000
        : undefined
    });
    if (sendResult?.status === 'completed') {
      const finalizedRound = await finalizePendingRound(reservedRound?.id, {
        situation: roundSituation,
        userMessage: content
      });
      orchestrationWorkbenchDebug('send:after-finalize', {
        actualRoundIndex,
        finalizedRoundId: String(finalizedRound?.id || '').trim(),
        finalizedRoundIndex: Number(finalizedRound?.index || 0),
        sendStatus: sendResult?.status || ''
      });
      if (finalizedRound?.id) {
        selectRound(finalizedRound.id);
      }
      reservedRoundId = '';
    } else {
      if (reservedRoundId) {
        await discardPendingRound(reservedRoundId).catch(() => null);
        reservedRoundId = '';
      }
      if (sendResult?.status === 'failed') {
        throw new Error(String(sendResult?.error || t('common.requestFailed')));
      }
      return;
    }
    if (includePrimer) {
      markMotherPrimerInjected();
    }
    clearStagedSituationDraft();
  } catch (error: any) {
    if (reservedRoundId) {
      await discardPendingRound(reservedRoundId, { clearSituation: true }).catch(() => null);
      reservedRoundId = '';
    } else {
      const pending = pendingRound.value;
      if (pending?.id) {
        await discardPendingRound(pending.id, { clearSituation: true }).catch(() => null);
      }
    }
    if (isOrchestrationDispatchStoppedError(error)) return;
    orchestrationWorkbenchDebug('send:error', {
      message: String(error?.message || ''),
      code: String(error?.code || '')
    });
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  } finally {
    finishOrchestrationDispatchFlow(flowToken);
  }
};

watch(
  () => activeRound.value?.id || '',
  () => {
    currentSituationDraft.value = String(activeRound.value?.situation || '');
  },
  { immediate: true }
);

watch(
  () => orchestrationGroupId.value,
  () => {
    situationPlanDraft.value = {};
    clearStagedSituationDraft();
    currentSituationDraft.value = '';
    selectSituationRound(1);
    historyDialogVisible.value = false;
    void loadHistory().catch(() => []);
    if (runtimeState.value?.runId) {
      void ensureRuntime({ forceRemote: true }).catch(() => null);
    }
  },
  { immediate: true }
);

watch(
  plannedSituations,
  (value) => {
    const next = { ...normalizeSituationEntries(value) };
    situationPlanDraft.value = next;
    if (!situationDialogVisible.value) {
      selectSituationRound(activeRound.value?.index || latestRoundIndex.value);
    }
  },
  { immediate: true }
);

watch(
  () => situationDialogVisible.value,
  (visible) => {
    if (!visible) return;
    situationPlanDraft.value = { ...plannedSituations.value };
    const nextRound = activeRound.value?.index || latestRoundIndex.value;
    selectSituationRound(nextRound);
  }
);

watch(
  displayChatMessages,
  (value) => {
    displayChatMessagesSeed.value = value;
  },
  { immediate: true }
);
</script>

<style scoped>
.orchestration-workbench {
  display: flex;
  flex: 1;
  min-height: 0;
  color: #e2e8f0;
}

.orchestration-state {
  display: flex;
  flex: 1;
  min-height: 360px;
  align-items: center;
  justify-content: center;
  gap: 12px;
  border: 1px dashed rgba(148, 163, 184, 0.24);
  border-radius: 24px;
  color: rgba(191, 219, 254, 0.78);
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.12), transparent 34%),
    linear-gradient(180deg, rgba(7, 10, 18, 0.96), rgba(8, 12, 21, 0.94));
}

.orchestration-shell {
  display: flex;
  flex: 1;
  min-height: 0;
}

.orchestration-theme-dialog :deep(.el-overlay-dialog) {
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 28px 20px 40px;
  box-sizing: border-box;
  overflow: auto;
  background:
    radial-gradient(circle at top, rgba(56, 189, 248, 0.14), transparent 32%),
    rgba(2, 6, 23, 0.68);
}

.orchestration-theme-dialog :deep(.el-dialog) {
  margin: 0;
  width: min(var(--el-dialog-width, 760px), calc(100vw - 40px));
  border: 1px solid rgba(96, 165, 250, 0.22);
  border-radius: 28px;
  background:
    radial-gradient(circle at top, rgba(56, 189, 248, 0.12), transparent 34%),
    linear-gradient(180deg, rgba(6, 10, 18, 0.98), rgba(8, 12, 22, 0.96));
  box-shadow:
    0 28px 80px rgba(2, 6, 23, 0.56),
    inset 0 1px 0 rgba(148, 163, 184, 0.12);
  overflow: hidden;
}

.orchestration-theme-dialog :deep(.el-dialog__header) {
  margin: 0;
  padding: 24px 28px 0;
}

.orchestration-theme-dialog :deep(.el-dialog__headerbtn) {
  top: 18px;
  right: 18px;
}

.orchestration-theme-dialog :deep(.el-dialog__headerbtn .el-dialog__close) {
  color: rgba(226, 232, 240, 0.72);
}

.orchestration-theme-dialog :deep(.el-dialog__body) {
  padding: 18px 28px 24px;
}

.orchestration-theme-dialog :deep(.el-dialog__footer) {
  padding: 0 28px 26px;
}

.orchestration-theme-dialog :deep(.messenger-modal-header) {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.orchestration-theme-dialog :deep(.messenger-modal-title) {
  color: #f8fafc;
  font-size: 22px;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.orchestration-theme-dialog :deep(.messenger-modal-subtitle) {
  margin-top: 8px;
  color: rgba(191, 219, 254, 0.74);
  font-size: 13px;
  line-height: 1.65;
}

.orchestration-theme-dialog :deep(.messenger-modal-footer) {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.orchestration-theme-dialog :deep(.messenger-modal-footer .el-button) {
  min-width: 90px;
  border-radius: 12px;
}

.orchestration-theme-dialog :deep(.messenger-modal-footer .el-button:not(.el-button--primary)) {
  border-color: rgba(148, 163, 184, 0.18);
  background: rgba(15, 23, 42, 0.68);
  color: rgba(226, 232, 240, 0.88);
}

.orchestration-theme-dialog :deep(.messenger-modal-footer .el-button--primary) {
  border-color: rgba(59, 130, 246, 0.46);
  background: linear-gradient(135deg, #2563eb, #38bdf8);
  color: #f8fafc;
}

.orchestration-history-dialog :deep(.el-dialog) {
  max-width: min(560px, calc(100vw - 40px));
}

.orchestration-history-dialog :deep(.el-dialog__body) {
  max-height: calc(100vh - 210px);
  overflow: auto;
}

.orchestration-situation-dialog :deep(.el-dialog) {
  max-width: min(900px, calc(100vw - 40px));
  max-height: calc(var(--app-viewport-height, 100vh) - 24px);
  box-sizing: border-box;
}

.orchestration-situation-dialog :deep(.el-dialog__body) {
  max-height: calc(var(--app-viewport-height, 100vh) - 210px);
  overflow: auto;
  box-sizing: border-box;
}

.orchestration-situation-toolbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 14px;
  padding: 14px 16px;
  border: 1px solid rgba(96, 165, 250, 0.16);
  border-radius: 20px;
  background:
    radial-gradient(circle at left top, rgba(56, 189, 248, 0.12), transparent 28%),
    linear-gradient(180deg, rgba(10, 15, 26, 0.96), rgba(8, 12, 22, 0.94));
  box-shadow: inset 0 1px 0 rgba(148, 163, 184, 0.08);
}

.orchestration-situation-toolbar-copy {
  min-width: 0;
  color: rgba(191, 219, 254, 0.74);
  font-size: 12px;
  line-height: 1.6;
}

.orchestration-situation-toolbar-title {
  display: block;
}

.orchestration-situation-toolbar-actions {
  display: flex;
  align-items: flex-start;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.orchestration-situation-shell {
  display: grid;
  grid-template-columns: minmax(220px, 260px) minmax(0, 1fr);
  gap: 14px;
  min-height: 0;
  max-height: calc(var(--app-viewport-height, 100vh) - 300px);
}

.orchestration-situation-round-list {
  display: grid;
  align-content: start;
  gap: 10px;
  min-height: 0;
  max-height: calc(var(--app-viewport-height, 100vh) - 300px);
  padding-right: 4px;
  overflow: auto;
}

.orchestration-situation-round-item {
  display: grid;
  gap: 10px;
  width: 100%;
  padding: 14px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 20px;
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.08), transparent 34%),
    linear-gradient(180deg, rgba(12, 16, 27, 0.94), rgba(9, 12, 20, 0.92));
  color: #f8fafc;
  text-align: left;
  cursor: pointer;
  transition: border-color 0.18s ease, transform 0.18s ease, box-shadow 0.18s ease;
}

.orchestration-situation-round-item:hover,
.orchestration-situation-round-item:focus-visible {
  border-color: rgba(96, 165, 250, 0.3);
  box-shadow: 0 14px 30px rgba(15, 23, 42, 0.2);
}

.orchestration-situation-round-item.is-selected {
  border-color: rgba(59, 130, 246, 0.5);
  background:
    radial-gradient(circle at top right, rgba(56, 189, 248, 0.16), transparent 40%),
    linear-gradient(180deg, rgba(15, 23, 42, 0.98), rgba(9, 14, 24, 0.96));
  box-shadow: 0 16px 34px rgba(2, 6, 23, 0.24);
}

.orchestration-situation-round-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.orchestration-situation-round-item-title {
  font-size: 13px;
  font-weight: 700;
  color: #f8fafc;
}

.orchestration-situation-round-item-state {
  display: inline-flex;
  align-items: center;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  background: rgba(15, 23, 42, 0.58);
  color: rgba(191, 219, 254, 0.68);
  font-size: 11px;
  line-height: 1;
}

.orchestration-situation-round-item-state.is-filled {
  color: #dcfce7;
  border-color: rgba(34, 197, 94, 0.28);
  background: rgba(22, 101, 52, 0.22);
}

.orchestration-situation-round-item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.orchestration-situation-textarea {
  width: 100%;
  resize: vertical;
  min-height: 320px;
  height: 100%;
  padding: 14px 16px;
  border: 1px solid rgba(96, 165, 250, 0.14);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(24, 35, 52, 0.96), rgba(16, 24, 39, 0.94));
  color: #f8fafc;
  line-height: 1.65;
  outline: none;
  box-sizing: border-box;
  box-shadow: inset 0 1px 0 rgba(148, 163, 184, 0.06);
}

.orchestration-situation-textarea::placeholder {
  color: rgba(226, 232, 240, 0.42);
}

.orchestration-situation-editor {
  display: flex;
  flex-direction: column;
  min-height: 0;
  max-height: calc(var(--app-viewport-height, 100vh) - 300px);
  padding: 16px;
  border: 1px solid rgba(96, 165, 250, 0.14);
  border-radius: 24px;
  background:
    radial-gradient(circle at top, rgba(56, 189, 248, 0.08), transparent 34%),
    linear-gradient(180deg, rgba(11, 15, 24, 0.98), rgba(7, 11, 20, 0.96));
  box-shadow: inset 0 1px 0 rgba(148, 163, 184, 0.08);
}

.orchestration-situation-editor-head {
  margin-bottom: 10px;
}

.orchestration-situation-row {
  display: grid;
  gap: 10px;
  padding: 14px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(12, 16, 24, 0.94), rgba(9, 12, 19, 0.92));
}

.orchestration-situation-row-head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.orchestration-situation-row-title {
  font-size: 13px;
  font-weight: 700;
  color: #f8fafc;
}

.orchestration-situation-row-badge,
.orchestration-situation-row-meta {
  display: inline-flex;
  align-items: center;
  padding: 3px 9px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 1;
}

.orchestration-situation-row-badge {
  color: #e0f2fe;
  background: rgba(2, 132, 199, 0.24);
  border: 1px solid rgba(56, 189, 248, 0.26);
}

.orchestration-situation-row-meta {
  color: rgba(191, 219, 254, 0.7);
  background: rgba(30, 41, 59, 0.72);
  border: 1px solid rgba(148, 163, 184, 0.18);
}

@media (max-width: 900px) {
  .orchestration-theme-dialog :deep(.el-overlay-dialog) {
    padding: 16px 12px 24px;
  }

  .orchestration-theme-dialog :deep(.el-dialog) {
    width: calc(100vw - 24px);
    border-radius: 22px;
  }

  .orchestration-theme-dialog :deep(.el-dialog__header) {
    padding: 20px 20px 0;
  }

  .orchestration-theme-dialog :deep(.el-dialog__body) {
    padding: 16px 20px 20px;
  }

  .orchestration-theme-dialog :deep(.el-dialog__footer) {
    padding: 0 20px 20px;
  }

  .orchestration-situation-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .orchestration-situation-toolbar-actions {
    justify-content: flex-start;
  }

  .orchestration-situation-shell {
    grid-template-columns: minmax(0, 1fr);
    max-height: none;
  }

  .orchestration-situation-round-list {
    max-height: 220px;
  }

  .orchestration-situation-editor {
    max-height: none;
  }

  .orchestration-situation-textarea {
    min-height: 260px;
  }
}

.orchestration-history-list {
  display: grid;
  gap: 12px;
  max-height: calc(100vh - 240px);
  padding-right: 4px;
  overflow: auto;
}

.orchestration-history-item {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 10px;
  padding: 14px 14px 14px 16px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 20px;
  background:
    radial-gradient(circle at top right, rgba(56, 189, 248, 0.14), transparent 38%),
    linear-gradient(180deg, rgba(11, 15, 24, 0.96), rgba(8, 12, 20, 0.94));
  box-shadow: inset 0 1px 0 rgba(148, 163, 184, 0.06);
}

.orchestration-history-item:hover,
.orchestration-history-item:focus-within {
  border-color: rgba(96, 165, 250, 0.38);
  box-shadow: 0 18px 38px rgba(15, 23, 42, 0.26);
}

.orchestration-history-item.is-current {
  border-color: rgba(96, 165, 250, 0.42);
}

.orchestration-history-item.is-deleting {
  opacity: 0.68;
}

.orchestration-history-item-main {
  display: grid;
  gap: 8px;
  width: 100%;
  padding: 0;
  border: none;
  background: transparent;
  color: #f8fafc;
  text-align: left;
  cursor: pointer;
}

.orchestration-history-item-main:focus-visible {
  outline: 2px solid rgba(96, 165, 250, 0.38);
  outline-offset: 4px;
}

.orchestration-history-item-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.orchestration-history-item-title {
  min-width: 0;
  font-size: 14px;
  font-weight: 700;
  line-height: 1.3;
  word-break: break-all;
}

.orchestration-history-item-time {
  flex-shrink: 0;
  font-size: 12px;
  color: rgba(191, 219, 254, 0.64);
  white-space: nowrap;
}

.orchestration-history-item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.orchestration-history-item-status {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  white-space: nowrap;
}

.orchestration-history-item-status {
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(30, 41, 59, 0.66);
}

.orchestration-history-item-status.is-active {
  color: #dcfce7;
  border-color: rgba(34, 197, 94, 0.34);
  background: rgba(22, 101, 52, 0.24);
}

.orchestration-history-item-status.is-closed {
  color: rgba(191, 219, 254, 0.72);
  border-color: rgba(148, 163, 184, 0.18);
  background: rgba(30, 41, 59, 0.66);
}

.orchestration-history-item-status--current {
  color: #dbeafe;
  border-color: rgba(96, 165, 250, 0.34);
  background: rgba(30, 64, 175, 0.24);
}

.orchestration-history-item-meta-text {
  font-size: 12px;
  color: rgba(191, 219, 254, 0.68);
}

.orchestration-history-delete {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  margin-top: 2px;
  border: 1px solid rgba(248, 113, 113, 0.22);
  border-radius: 12px;
  background: rgba(127, 29, 29, 0.18);
  color: rgba(254, 202, 202, 0.92);
  cursor: pointer;
  transition: border-color 0.18s ease, background 0.18s ease, color 0.18s ease;
}

.orchestration-history-delete:hover:not(:disabled),
.orchestration-history-delete:focus-visible:not(:disabled) {
  border-color: rgba(248, 113, 113, 0.34);
  background: rgba(127, 29, 29, 0.28);
}

.orchestration-history-delete:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}
</style>
