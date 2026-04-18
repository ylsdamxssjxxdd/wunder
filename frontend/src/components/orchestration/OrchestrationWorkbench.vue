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
        :initializing="initializing"
        :history-loading="historyLoading"
        :is-active="isActive"
        :is-busy="orchestrationStopBusy"
        :is-ready="isReady"
        :group-description="group.description || t('orchestration.empty.description')"
        :resolve-worker-outputs="resolveWorkerOutputs"
        :resolve-worker-thread-session-id="resolveWorkerThreadSessionId"
        :resolve-message-avatar-image="resolveMessageAvatarImage"
        :avatar-label="avatarLabel"
        @open-agent="emit('open-agent', $event)"
        @update:composer-text="composerText = $event"
        @send="handleSendToMother"
        @create-run="handleCreateRun"
        @start-run="handleStartRun"
        @exit-run="handleStopRun"
        @open-history="historyDialogVisible = true"
        @open-situation="situationDialogVisible = true"
        @select-round="selectRound($event)"
        @restore-run="handleRestoreHistoryAction($event)"
        @delete-round-tail="handleDeleteBranchAfterRound($event)"
      />

      <el-dialog
        v-model="historyDialogVisible"
        width="520px"
        append-to-body
        class="messenger-modal messenger-modal--beeroom"
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
              'is-deleting': deletingHistoryId === item.orchestrationId,
              'is-branching': branchingHistoryId === item.orchestrationId
            }"
          >
            <div class="orchestration-history-item-main">
              <button
                class="orchestration-history-item-open"
                type="button"
                @click="handleRestoreHistoryAction(item.orchestrationId)"
              >
              <div class="orchestration-history-item-head">
                <div class="orchestration-history-item-head-main">
                  <span class="orchestration-history-item-title">{{ item.runId }}</span>
                  <span
                    class="orchestration-history-item-status"
                    :class="resolveHistoryStatusClass(item)"
                  >
                    {{ resolveHistoryStatusLabel(item) }}
                  </span>
                </div>
                <span class="orchestration-history-item-time">
                  {{ formatHistoryPrimaryTime(item) }}
                </span>
              </div>
              <div class="orchestration-history-item-summary">
                <span class="orchestration-history-item-pill">
                  {{ t('orchestration.timeline.round', { round: item.latestRoundIndex }) }}
                </span>
                <span
                  v-if="item.motherAgentName"
                  class="orchestration-history-item-pill orchestration-history-item-pill--muted"
                >
                  {{ item.motherAgentName }}
                </span>
                <span
                  v-if="item.orchestrationId === currentOrchestrationId"
                  class="orchestration-history-item-pill orchestration-history-item-pill--current"
                >
                  {{ t('orchestration.dialog.historyCurrent') }}
                </span>
              </div>
              <div class="orchestration-history-item-timeline">
                <span>{{ t('orchestration.dialog.historyEnteredAt', { time: formatHistoryTime(item.enteredAt) }) }}</span>
                <span>{{ t('orchestration.dialog.historyUpdatedAt', { time: formatHistoryTime(item.updatedAt) }) }}</span>
                <span v-if="item.restoredAt">{{ t('orchestration.dialog.historyRestoredAt', { time: formatHistoryTime(item.restoredAt) }) }}</span>
                <span v-else-if="item.exitedAt">{{ t('orchestration.dialog.historyExitedAt', { time: formatHistoryTime(item.exitedAt) }) }}</span>
              </div>
              </button>
              <div class="orchestration-history-item-actions">
                <button
                  class="orchestration-history-item-branch"
                  type="button"
                  :disabled="branchingHistoryId === item.orchestrationId"
                  @click.stop="handleBranchFromHistory(item, getHistoryBranchTargetRound(item))"
                >
                  <i class="fa-solid fa-code-branch" aria-hidden="true"></i>
                  <span>{{ t('orchestration.dialog.historyBranchAction') }}</span>
                </button>
              </div>
            </div>
            <button
              class="orchestration-history-delete"
              type="button"
              :title="t('common.delete')"
              :aria-label="t('common.delete')"
              :disabled="cannotDeleteHistoryItem(item)"
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
        append-to-body
        class="messenger-modal messenger-modal--beeroom"
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
            <span class="orchestration-situation-toolbar-label">
              {{ t('orchestration.dialog.situationRoundLabel') }}
            </span>
            <el-input-number
              v-model="situationRoundInput"
              class="orchestration-situation-round-input"
              :min="1"
              :step="1"
              :precision="0"
              controls-position="right"
            />
            <el-button @click="handleSituationRoundJump">
              {{ t('orchestration.dialog.situationRoundGo') }}
            </el-button>
            <el-button @click="triggerSituationImport">
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
              rows="13"
            ></textarea>
          </div>
        </div>

        <template #footer>
          <div class="messenger-modal-footer">
            <el-button @click="handleCloseSituationDialog">{{ t('common.cancel') }}</el-button>
            <el-button type="primary" @click="handleSaveSituation">{{ t('common.confirm') }}</el-button>
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
import {
  clearBeeroomMissionChatState,
  getBeeroomMissionChatState,
  setBeeroomMissionChatState
} from '@/components/beeroom/beeroomMissionChatStateCache';
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
  normalizeOrchestrationStatus
} from '@/components/orchestration/orchestrationShared';
import { useOrchestrationRuntimeState } from '@/components/orchestration/orchestrationRuntimeState';
import { getCurrentLanguage, useI18n } from '@/i18n';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

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

const groupRef = toRef(props, 'group');
const agentsRef = toRef(props, 'agents');
const missionsRef = toRef(props, 'missions');
const displayChatMessagesSeed = ref<MissionChatMessage[]>([]);
const orchestrationGroupId = computed(() => String(props.group?.group_id || props.group?.hive_id || '').trim());
const motherAgentId = computed(() => String(props.group?.mother_agent_id || '').trim());

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
  reserveUserRound,
  finalizePendingRound,
  discardPendingRound,
  resolveRoundSituation,
  markMotherPrimerInjected,
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
const branchingHistoryId = ref('');
const situationPlanDraft = ref<Record<string, string>>({});
const stagedSituationDraft = ref<Record<string, string>>({});
const stagedSituationDraftOrchestrationId = ref('');
const stagedSituationDraftActive = ref(false);
const selectedSituationRound = ref(1);
const situationRoundInput = ref(1);
const situationImportInputRef = ref<HTMLInputElement | null>(null);
const orchestrationPromptTemplates = ref<OrchestrationPromptTemplates | null>(null);
const orchestrationPromptLanguage = ref('');
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
const isViewingLatestRound = computed(() => {
  const latestId = String(latestRound.value?.id || '').trim();
  const activeId = String(activeRound.value?.id || '').trim();
  return Boolean(latestId && activeId && latestId === activeId);
});
const shouldStageSituationDraft = computed(() => Boolean(currentOrchestrationId.value) && !isViewingLatestRound.value);
const orchestrationCanSend = computed(
  () =>
    Boolean(
      isReady.value &&
        isActive.value &&
        String(composerText.value || '').trim()
    ) && !orchestrationStopBusy.value
);
const orchestrationComposerDisabled = computed(
  () => !isReady.value || !isActive.value
);

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
  if (targetId && stagedSituationDraftOrchestrationId.value !== targetId) {
    return;
  }
  stagedSituationDraft.value = {};
  stagedSituationDraftOrchestrationId.value = '';
  stagedSituationDraftActive.value = false;
};

const rememberStagedSituationDraft = (entries: Record<string, string>) => {
  stagedSituationDraft.value = normalizeSituationEntries(entries);
  stagedSituationDraftOrchestrationId.value = currentOrchestrationId.value;
  stagedSituationDraftActive.value = true;
};

const hasStagedSituationDraft = computed(
  () =>
    stagedSituationDraftActive.value &&
    Boolean(stagedSituationDraftOrchestrationId.value) &&
    stagedSituationDraftOrchestrationId.value === currentOrchestrationId.value
);

const plannedSituations = computed<Record<string, string>>(() => {
  const source = runtimeState.value?.plannedSituations;
  const persisted = source && typeof source === 'object' ? normalizeSituationEntries(source as Record<string, string>) : {};
  if (hasStagedSituationDraft.value) {
    return { ...stagedSituationDraft.value };
  }
  return persisted;
});
const latestRoundIndex = computed(() => Math.max(1, Number(latestRound.value?.index || 1)));
const normalizeSituationRound = (value: unknown) =>
  Math.max(1, Number.parseInt(String(value ?? '').trim(), 10) || 1);

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
    normalizeSituationRound(situationRoundInput.value),
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
  situationRoundInput.value = nextRound;
};

const handleSituationRoundJump = () => {
  selectSituationRound(situationRoundInput.value);
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
  const missionIds = new Set(latestRound.value?.missionIds || []);
  return (Array.isArray(props.missions) ? props.missions : []).filter((item) =>
    missionIds.has(String(item?.mission_id || item?.team_run_id || '').trim())
  );
});

const latestRoundRunningMissions = computed(() =>
  latestRoundMissions.value.filter((item) => isOrchestrationMissionRunning(item))
);

const orchestrationStopBusy = computed(() => composerSending.value || latestRoundRunningMissions.value.length > 0);

const liveDispatchPreview = computed<BeeroomSwarmDispatchPreview | null>(() => {
  if (!orchestrationStopBusy.value) {
    return null;
  }
  const latestRound = rounds.value[rounds.value.length - 1] || null;
  if (!latestRound || latestRound.id !== activeRound.value?.id) {
    return null;
  }
  return dispatchPreview.value || null;
});

const visibleChatMessages = computed(() => activeRoundChatMessages.value);

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
  try {
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
    selectSituationRound(1);
    situationDialogVisible.value = false;
    const nextState = await initializeRun();
    await syncMotherSessionContextForState(nextState, 1);
    await loadHistory().catch(() => []);
    emit('create');
    ElMessage.success(t('orchestration.message.created'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleSaveSituation = () => {
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

const handleStopRun = async () => {
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

const handleRestoreHistoryAction = async (orchestrationId: string) => {
  try {
    const nextState = await restoreHistory(orchestrationId, { activate: isActive.value });
    clearStagedSituationDraft();
    if (nextState?.active) {
      await syncMotherSessionContextForState(
        nextState,
        Number(nextState.rounds[nextState.rounds.length - 1]?.index || 1)
      );
    }
    historyDialogVisible.value = false;
    emit('refresh');
    ElMessage.success(t('chat.history.restoreSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('chat.history.restoreFailed')));
  }
};

const handleBranchFromHistory = async (item: { orchestrationId: string; latestRoundIndex: number }, roundIndex?: number) => {
  const targetRoundIndex = Math.max(
    1,
    Number.parseInt(
      String(roundIndex || getHistoryBranchTargetRound(item) || item.latestRoundIndex || 1),
      10
    ) || 1
  );
  try {
    await ElMessageBox.confirm(
      t('orchestration.dialog.historyBranchConfirm', { round: targetRoundIndex }),
      t('common.notice'),
      {
        confirmButtonText: t('orchestration.dialog.historyBranchAction'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
  } catch {
    return;
  }
  branchingHistoryId.value = item.orchestrationId;
  try {
    const nextState = await branchHistory(item.orchestrationId, targetRoundIndex, { activate: isActive.value });
    clearStagedSituationDraft();
    if (nextState?.active) {
      await syncMotherSessionContextForState(
        nextState,
        Number(nextState.rounds[nextState.rounds.length - 1]?.index || targetRoundIndex)
      );
    }
    historyDialogVisible.value = false;
    emit('refresh');
    ElMessage.success(t('orchestration.dialog.historyBranchSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  } finally {
    branchingHistoryId.value = '';
  }
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

const getHistoryBranchTargetRound = (item: { orchestrationId: string; latestRoundIndex: number }) => {
  if (item.orchestrationId === currentOrchestrationId.value) {
    return Math.max(1, Number(activeRound.value?.index || latestRound.value?.index || item.latestRoundIndex || 1));
  }
  return Math.max(1, Number(item.latestRoundIndex || 1));
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
  item.orchestrationId === currentOrchestrationId.value;

const handleDeleteHistory = async (item: { orchestrationId: string; runId: string }) => {
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
    ElMessage.success(t('orchestration.dialog.historyDeleteSuccess'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.deleteFailed')));
  } finally {
    deletingHistoryId.value = '';
  }
};

const handleDeleteBranchAfterRound = async (payload: { orchestrationId: string; roundIndex: number }) => {
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
    clearStagedSituationDraft(payload.orchestrationId);
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
  if (orchestrationStopBusy.value) {
    const pending = pendingRound.value;
    await stopOrchestrationDispatch();
    if (pending?.id) {
      await discardPendingRound(pending.id);
    }
    return;
  }
  const content = String(composerText.value || '').trim();
  if (!content) return;
  if (!isActive.value) {
    ElMessage.warning(t('orchestration.message.startRunRequired'));
    return;
  }
  try {
    let state = await ensureRuntime();
    if (!String(state?.motherSessionId || '').trim()) {
      throw new Error(t('orchestration.message.createRunRequired'));
    }
    const currentOrchestrationIdValue = currentOrchestrationId.value;
    const stagedDraftEntries =
      hasStagedSituationDraft.value && stagedSituationDraftOrchestrationId.value === currentOrchestrationIdValue
        ? normalizeSituationEntries(stagedSituationDraft.value)
        : null;
    let workingRunId = runId.value;
    let nextRoundSource = latestRound.value;
    let nextActiveRound = activeRound.value;
    if (!isViewingLatestRound.value && currentOrchestrationId.value) {
      const branchBaseRoundIndex = Math.max(
        1,
        Number(nextActiveRound?.index || latestRound.value?.index || 1)
      );
      const branchedState = await branchHistory(currentOrchestrationId.value, branchBaseRoundIndex, {
        activate: true
      });
      if (branchedState?.active) {
        if (stagedDraftEntries) {
          await updatePlannedSituations(stagedDraftEntries);
        }
        await syncMotherSessionContextForState(
          branchedState,
          Number(branchedState.rounds[branchedState.rounds.length - 1]?.index || branchBaseRoundIndex)
        );
      }
      state = branchedState || state;
      clearStagedSituationDraft(currentOrchestrationIdValue);
      workingRunId = String(state?.runId || '').trim();
      nextRoundSource = state?.rounds?.[state.rounds.length - 1] || null;
      nextActiveRound = nextRoundSource;
      emit('refresh');
    }
    const targetRound =
      nextRoundSource && !String(nextRoundSource.userMessage || '').trim() ? nextRoundSource : null;
    const nextRoundIndex = targetRound ? targetRound.index : Math.max(1, Number(nextRoundSource?.index || 0)) + 1;
    const roundSituation = resolveDraftSituationByRoundIndex(nextRoundIndex) || await resolveRoundSituation(nextRoundIndex);
    const includePrimer = state?.motherPrimerInjected !== true;
    const templates = await ensureOrchestrationPromptTemplates();
    const dispatchContent = buildMotherDispatchEnvelope({
      group: props.group,
      agents: props.agents,
      runId: workingRunId,
      roundIndex: nextRoundIndex,
      userMessage: content,
      situation: roundSituation,
      includePrimer,
      templates
    });
    const reservedRound = await reserveUserRound({
      targetRoundId: targetRound?.id || '',
      situation: roundSituation,
      userMessage: content
    });
    await syncMotherSessionContext(nextRoundIndex);
    await handleComposerSend({
      content: dispatchContent,
      displayContent: content,
      displayCreatedAt: Number(reservedRound?.createdAt || 0) > 0
        ? Number(reservedRound?.createdAt || 0) / 1000
        : undefined
    });
    await finalizePendingRound(reservedRound?.id);
    if (includePrimer) {
      markMotherPrimerInjected();
    }
    clearStagedSituationDraft();
  } catch (error: any) {
    const pending = pendingRound.value;
    if (pending?.id) {
      await discardPendingRound(pending.id).catch(() => null);
    }
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

watch(
  () => orchestrationGroupId.value,
  () => {
    situationPlanDraft.value = {};
    clearStagedSituationDraft();
    selectSituationRound(1);
    historyDialogVisible.value = false;
    void loadHistory().catch(() => []);
    if (runtimeState.value?.runId) {
      void ensureRuntime().catch(() => null);
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

watch(
  [runtimeScopeKey, activeRoundChatMessages],
  ([scopeKey, messages]) => {
    const resolvedScopeKey = String(scopeKey || '').trim();
    if (!resolvedScopeKey) return;
    const cached = getBeeroomMissionChatState(resolvedScopeKey);
    setBeeroomMissionChatState(resolvedScopeKey, {
      ...(cached || { version: 2, manualMessages: [], runtimeRelayMessages: [], dispatch: null }),
      manualMessages: Array.isArray(messages) ? messages : []
    });
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

.orchestration-situation-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 14px;
  padding: 14px 16px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(12, 16, 24, 0.96), rgba(9, 12, 19, 0.94));
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
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.orchestration-situation-toolbar-label {
  font-size: 12px;
  color: rgba(191, 219, 254, 0.78);
}

.orchestration-situation-round-input {
  width: 120px;
}

.orchestration-situation-shell {
  display: grid;
  grid-template-columns: minmax(220px, 260px) minmax(0, 1fr);
  gap: 14px;
  min-height: min(68vh, 720px);
}

.orchestration-situation-round-list {
  display: grid;
  align-content: start;
  gap: 10px;
  max-height: min(68vh, 720px);
  padding-right: 4px;
  overflow: auto;
}

.orchestration-situation-round-item {
  display: grid;
  gap: 10px;
  width: 100%;
  padding: 14px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(12, 16, 24, 0.94), rgba(9, 12, 19, 0.92));
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
    radial-gradient(circle at top right, rgba(56, 189, 248, 0.12), transparent 38%),
    linear-gradient(180deg, rgba(15, 23, 42, 0.98), rgba(10, 14, 22, 0.96));
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
  min-height: 88px;
  padding: 12px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 14px;
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.92), rgba(15, 17, 23, 0.88));
  color: #f8fafc;
  line-height: 1.65;
  outline: none;
  box-sizing: border-box;
}

.orchestration-situation-editor {
  display: flex;
  flex-direction: column;
  min-height: 0;
  max-height: min(68vh, 720px);
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
  .orchestration-situation-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .orchestration-situation-toolbar-actions {
    justify-content: flex-start;
  }

  .orchestration-situation-shell {
    grid-template-columns: minmax(0, 1fr);
  }

  .orchestration-situation-round-list {
    max-height: 220px;
  }
}

.orchestration-history-list {
  display: grid;
  gap: 12px;
  max-height: min(68vh, 720px);
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
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  background:
    radial-gradient(circle at top right, rgba(56, 189, 248, 0.12), transparent 36%),
    linear-gradient(180deg, rgba(12, 16, 24, 0.96), rgba(9, 12, 19, 0.94));
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
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 12px;
  width: 100%;
}

.orchestration-history-item-open {
  display: grid;
  gap: 12px;
  width: 100%;
  padding: 0;
  border: none;
  background: transparent;
  color: #f8fafc;
  text-align: left;
  cursor: pointer;
}

.orchestration-history-item-actions {
  display: flex;
  align-items: flex-start;
}

.orchestration-history-item-branch {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid rgba(96, 165, 250, 0.24);
  border-radius: 12px;
  background: rgba(30, 41, 59, 0.64);
  color: rgba(219, 234, 254, 0.88);
  cursor: pointer;
  transition: border-color 0.18s ease, background 0.18s ease, color 0.18s ease;
}

.orchestration-history-item-branch:hover:not(:disabled),
.orchestration-history-item-branch:focus-visible:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.42);
  background: rgba(30, 41, 59, 0.92);
  color: #f8fafc;
  outline: none;
}

.orchestration-history-item-branch:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.orchestration-history-item-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.orchestration-history-item-head-main {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  flex-wrap: wrap;
}

.orchestration-history-item-title {
  font-size: 14px;
  font-weight: 700;
  line-height: 1.3;
  word-break: break-all;
}

.orchestration-history-item-status,
.orchestration-history-item-pill {
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

.orchestration-history-item-time {
  font-size: 12px;
  color: rgba(191, 219, 254, 0.64);
  white-space: nowrap;
}

.orchestration-history-item-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.orchestration-history-item-pill {
  color: rgba(226, 232, 240, 0.9);
  border: 1px solid rgba(148, 163, 184, 0.16);
  background: rgba(15, 23, 42, 0.58);
}

.orchestration-history-item-pill--muted {
  color: rgba(191, 219, 254, 0.72);
}

.orchestration-history-item-pill--current {
  color: #dbeafe;
  border-color: rgba(96, 165, 250, 0.34);
  background: rgba(30, 64, 175, 0.24);
}

.orchestration-history-item-timeline {
  display: grid;
  gap: 6px;
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
  border: 1px solid rgba(248, 113, 113, 0.18);
  border-radius: 12px;
  background: rgba(127, 29, 29, 0.16);
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
