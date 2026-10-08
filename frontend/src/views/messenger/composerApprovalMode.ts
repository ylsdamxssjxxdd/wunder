import { computed, ref, watch, type Ref } from 'vue';

export type AgentApprovalMode = 'suggest' | 'auto_edit' | 'full_auto';

export type AgentApprovalOption = {
  value: AgentApprovalMode;
  label: string;
};

type PersistTarget = {
  agentId: string;
  mode: AgentApprovalMode;
};

type UseComposerApprovalModeOptions = {
  isAgentConversationActive: Readonly<Ref<boolean>>;
  activeAgentId: Readonly<Ref<string>>;
  activeAgentApprovalMode: Readonly<Ref<AgentApprovalMode>>;
  resolvePersistAgentId: () => string;
  persistApprovalMode: (agentId: string, mode: AgentApprovalMode) => Promise<void>;
  onPersistError?: (error: unknown) => void;
};

const AGENT_APPROVAL_MODES: AgentApprovalMode[] = ['suggest', 'auto_edit', 'full_auto'];

export const normalizeAgentApprovalMode = (value: unknown): AgentApprovalMode => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase();
  if (normalized === 'suggest') return 'suggest';
  if (normalized === 'auto_edit' || normalized === 'auto-edit') return 'auto_edit';
  if (normalized === 'full_auto' || normalized === 'full-auto') return 'full_auto';
  return 'full_auto';
};

export const buildAgentApprovalOptions = (
  resolveLabel: (mode: AgentApprovalMode) => string
): AgentApprovalOption[] =>
  AGENT_APPROVAL_MODES.map((mode) => {
    const label = String(resolveLabel(mode) || '').trim();
    return {
      value: mode,
      label: label || mode
    };
  });

export const useComposerApprovalMode = (options: UseComposerApprovalModeOptions) => {
  const composerApprovalMode = ref<AgentApprovalMode>(
    normalizeAgentApprovalMode(options.activeAgentApprovalMode.value)
  );
  const composerApprovalModeSyncing = ref(false);
  const localDirty = ref(false);
  const pendingPersistTarget = ref<PersistTarget | null>(null);
  let persistLoopRunning = false;

  const resetFromSource = () => {
    pendingPersistTarget.value = null;
    localDirty.value = false;
    composerApprovalMode.value = normalizeAgentApprovalMode(options.activeAgentApprovalMode.value);
  };

  watch(
    () => options.activeAgentId.value,
    () => {
      // Agent switch should always reset local overrides to avoid cross-agent leakage.
      resetFromSource();
    },
    { immediate: true }
  );

  watch(
    () => options.activeAgentApprovalMode.value,
    (nextModeRaw) => {
      const nextMode = normalizeAgentApprovalMode(nextModeRaw);
      if (!localDirty.value || !composerApprovalModeSyncing.value) {
        composerApprovalMode.value = nextMode;
      }
      if (!composerApprovalModeSyncing.value && composerApprovalMode.value === nextMode) {
        localDirty.value = false;
      }
    }
  );

  const flushPersistQueue = async () => {
    if (persistLoopRunning) return;
    persistLoopRunning = true;
    composerApprovalModeSyncing.value = true;
    try {
      while (pendingPersistTarget.value) {
        const target = pendingPersistTarget.value;
        pendingPersistTarget.value = null;
        try {
          await options.persistApprovalMode(target.agentId, target.mode);
          if (
            options.activeAgentId.value === target.agentId &&
            composerApprovalMode.value === target.mode
          ) {
            localDirty.value = false;
          }
        } catch (error) {
          const hasNewerTarget = Boolean(pendingPersistTarget.value);
          if (hasNewerTarget) {
            continue;
          }
          if (options.activeAgentId.value === target.agentId) {
            composerApprovalMode.value = normalizeAgentApprovalMode(
              options.activeAgentApprovalMode.value
            );
          }
          localDirty.value = false;
          options.onPersistError?.(error);
        }
      }
    } finally {
      persistLoopRunning = false;
      composerApprovalModeSyncing.value = false;
      if (pendingPersistTarget.value) {
        void flushPersistQueue();
      }
    }
  };

  const updateComposerApprovalMode = (value: unknown) => {
    const nextMode = normalizeAgentApprovalMode(value);
    composerApprovalMode.value = nextMode;

    if (!options.isAgentConversationActive.value) {
      localDirty.value = false;
      return;
    }

    const sourceMode = normalizeAgentApprovalMode(options.activeAgentApprovalMode.value);
    if (nextMode === sourceMode && !composerApprovalModeSyncing.value) {
      pendingPersistTarget.value = null;
      localDirty.value = false;
      return;
    }

    const persistAgentId = String(options.resolvePersistAgentId() || '').trim();
    if (!persistAgentId) {
      localDirty.value = false;
      return;
    }

    localDirty.value = true;
    pendingPersistTarget.value = { agentId: persistAgentId, mode: nextMode };
    void flushPersistQueue();
  };

  const effectiveComposerApprovalMode = computed<AgentApprovalMode>(() =>
    normalizeAgentApprovalMode(composerApprovalMode.value || options.activeAgentApprovalMode.value)
  );

  return {
    composerApprovalMode: effectiveComposerApprovalMode,
    composerApprovalModeSyncing,
    updateComposerApprovalMode
  };
};
