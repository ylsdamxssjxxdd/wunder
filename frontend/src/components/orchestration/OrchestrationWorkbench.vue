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
        :visible-chat-messages="visibleChatMessages"
        :mother-agent-id="motherAgentId"
        :mother-name="motherName"
        :mother-session-id="motherSessionId"
        :run-id="runId"
        :dispatch-preview="liveDispatchPreview"
        :composer-text="composerText"
        :composer-sending="composerSending"
        :can-send="canSend"
        :initializing="initializing"
        :refreshing="refreshing"
        :is-ready="isReady"
        :group-description="group.description || t('orchestration.empty.description')"
        :resolve-worker-outputs="resolveWorkerOutputs"
        :resolve-worker-thread-session-id="resolveWorkerThreadSessionId"
        :resolve-message-avatar-image="resolveMessageAvatarImage"
        :avatar-label="avatarLabel"
        @open-agent="emit('open-agent', $event)"
        @update:composer-text="composerText = $event"
        @clear-chat="handleClearChat"
        @send="handleSendToMother"
        @create-run="handleCreateRun"
        @open-situation="situationDialogVisible = true"
        @refresh="emit('refresh')"
        @select-round="selectRound($event)"
      />

      <el-dialog
        v-model="situationDialogVisible"
        width="640px"
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

        <div class="orchestration-situation-editor">
          <div
            v-for="entry in situationPlanRows"
            :key="entry.key"
            class="orchestration-situation-row"
          >
            <div class="orchestration-situation-row-head">
              <span class="orchestration-situation-row-title">
                {{ t('orchestration.timeline.round', { round: entry.round }) }}
              </span>
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
            <textarea
              v-model="situationPlanDraft[entry.key]"
              class="orchestration-situation-textarea"
              :placeholder="t('orchestration.dialog.situationPlaceholder')"
              rows="4"
            ></textarea>
          </div>
        </div>

        <template #footer>
          <div class="messenger-modal-footer">
            <el-button @click="situationDialogVisible = false">{{ t('common.cancel') }}</el-button>
            <el-button type="primary" @click="handleSaveSituation">{{ t('common.confirm') }}</el-button>
          </div>
        </template>
      </el-dialog>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue';
import { ElMessage } from 'element-plus';

import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import { clearBeeroomMissionChatState } from '@/components/beeroom/beeroomMissionChatStateCache';
import { clearBeeroomMissionCanvasState } from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';
import { useBeeroomMissionCanvasRuntime } from '@/components/beeroom/useBeeroomMissionCanvasRuntime';
import { fetchBeeroomOrchestrationPrompts } from '@/api/beeroom';
import OrchestrationMissionCanvas from '@/components/orchestration/OrchestrationMissionCanvas.vue';
import {
  type OrchestrationPromptTemplates,
  buildMotherDispatchEnvelope,
  buildMotherWorkerPrimerGuide
} from '@/components/orchestration/orchestrationPrompting';
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
const motherAgentId = computed(() => String(props.group?.mother_agent_id || '').trim());

const {
  runtimeState,
  runtimeScopeKey,
  clearScopeKey,
  activeRound,
  latestRound,
  visibleWorkers,
  artifactCards,
  initializing,
  isReady,
  ensureRuntime,
  initializeRun,
  commitUserRound,
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
  clearManualChatHistory,
  handleComposerSend,
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
const situationPlanDraft = ref<Record<string, string>>({});
const orchestrationPromptTemplates = ref<OrchestrationPromptTemplates | null>(null);
const orchestrationPromptLanguage = ref('');
let orchestrationPromptLoadTask: Promise<OrchestrationPromptTemplates> | null = null;

const ORCHESTRATION_PROMPT_TEMPLATE_KEYS = [
  'mother_runtime',
  'round_artifacts',
  'worker_first_dispatch',
  'worker_guide',
  'situation_context',
  'user_message'
] as const satisfies readonly (keyof OrchestrationPromptTemplates)[];

const rounds = computed(() => runtimeState.value?.rounds || []);
const runId = computed(() => String(runtimeState.value?.runId || '').trim());
const motherSessionId = computed(() => String(runtimeState.value?.motherSessionId || '').trim());

const motherName = computed(() => {
  const motherId = motherAgentId.value;
  if (!motherId) return t('orchestration.canvas.unassignedMother');
  const member = (Array.isArray(props.agents) ? props.agents : []).find(
    (item) => String(item.agent_id || '').trim() === motherId
  );
  return String(member?.name || props.group?.mother_agent_name || motherId).trim() || motherId;
});

const plannedSituations = computed<Record<string, string>>(() => {
  const source = runtimeState.value?.plannedSituations;
  return source && typeof source === 'object' ? source : {};
});
const latestRoundIndex = computed(() => Math.max(1, Number(latestRound.value?.index || 1)));
const situationPlanRows = computed(() => {
  const roundsMax = Math.max(
    latestRoundIndex.value + 2,
    ...Object.keys(plannedSituations.value).map((key) => Number.parseInt(key, 10) || 0)
  );
  return Array.from({ length: Math.max(3, roundsMax) }, (_, index) => {
    const round = index + 1;
    return {
      key: String(round),
      round,
      isActiveRound: round === Number(activeRound.value?.index || 0),
      isUpcomingRound: round > latestRoundIndex.value
    };
  });
});

const activeRoundMissions = computed(() => {
  const missionIds = new Set(activeRound.value?.missionIds || []);
  return (Array.isArray(props.missions) ? props.missions : []).filter((item) =>
    missionIds.has(String(item?.mission_id || item?.team_run_id || '').trim())
  );
});

const liveDispatchPreview = computed<BeeroomSwarmDispatchPreview | null>(() => {
  const latestRound = rounds.value[rounds.value.length - 1] || null;
  if (!latestRound || latestRound.id !== activeRound.value?.id) {
    return null;
  }
  return dispatchPreview.value || null;
});

const visibleChatMessages = computed(() => displayChatMessages.value);

const canSend = computed(() => Boolean(isReady.value && String(composerText.value || '').trim()) && !composerSending.value);

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
    situationDialogVisible.value = false;
    await initializeRun();
    emit('create');
    ElMessage.success(t('orchestration.message.created'));
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleSaveSituation = () => {
  const nextDraft = Object.fromEntries(
    Object.entries(situationPlanDraft.value || {}).map(([key, value]) => [key, String(value || '').trim()]).filter(([, value]) => Boolean(value))
  );
  updatePlannedSituations(nextDraft);
  situationDialogVisible.value = false;
  emit('edit-situation');
};

const handleSendToMother = async () => {
  const content = String(composerText.value || '').trim();
  if (!content) return;
  try {
    const state = await ensureRuntime();
    if (!String(state?.motherSessionId || '').trim()) {
      throw new Error(t('orchestration.panel.pending'));
    }
    const latest = latestRound.value;
    const targetRound =
      latest && !String(latest.userMessage || '').trim() ? latest : null;
    const nextRoundIndex = targetRound ? targetRound.index : latestRoundIndex.value + 1;
    const roundSituation =
      String(plannedSituations.value[String(nextRoundIndex)] || '').trim() ||
      (targetRound ? String(targetRound.situation || '').trim() : '');
    const includePrimer = runtimeState.value?.motherPrimerInjected !== true;
    const templates = await ensureOrchestrationPromptTemplates();
    const dispatchContentBlocks = [
      buildMotherDispatchEnvelope({
        group: props.group,
        agents: props.agents,
        runId: runId.value,
        roundIndex: nextRoundIndex,
        userMessage: content,
        situation: roundSituation,
        includePrimer,
        templates
      }),
      includePrimer
        ? buildMotherWorkerPrimerGuide({
            group: props.group,
            agents: props.agents,
            runId: runId.value,
            roundIndex: nextRoundIndex,
            templates
          })
        : ''
    ].filter((item) => String(item || '').trim());
    const dispatchContent = dispatchContentBlocks.join('\n\n');
    await commitUserRound({
      targetRoundId: targetRound?.id || '',
      situation: roundSituation,
      userMessage: content
    });
    await handleComposerSend({
      content: dispatchContent,
      displayContent: content
    });
    if (includePrimer) {
      markMotherPrimerInjected();
    }
  } catch (error: any) {
    ElMessage.error(String(error?.message || t('common.requestFailed')));
  }
};

const handleClearChat = async () => {
  await clearManualChatHistory();
};

watch(
  () => props.group?.group_id,
  () => {
    situationPlanDraft.value = {};
  },
  { immediate: true }
);

watch(
  plannedSituations,
  (value) => {
    const next: Record<string, string> = {};
    Object.entries(value || {}).forEach(([key, situation]) => {
      next[String(key)] = String(situation || '');
    });
    situationPlanDraft.value = next;
  },
  { immediate: true }
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
  display: grid;
  gap: 14px;
  max-height: min(68vh, 720px);
  padding-right: 4px;
  overflow: auto;
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
</style>
