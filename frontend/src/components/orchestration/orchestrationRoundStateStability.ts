export type OrchestrationRoundSnapshot = {
  id: string;
  index: number;
  situation?: string;
  userMessage?: string;
  createdAt?: number;
  finalizedAt?: number;
  missionIds?: string[];
  branchParentRoundId?: string;
  branchFromRoundIndex?: number;
  branchRootOrchestrationId?: string;
  orchestrationId?: string;
};

const trimRoundText = (value: unknown) => String(value || '').trim();

const normalizeRoundIndex = (value: unknown) => Math.max(1, Number(value || 0) || 0);

const sortRounds = <T extends OrchestrationRoundSnapshot>(rounds: T[]) =>
  [...rounds].sort(
    (left, right) =>
      normalizeRoundIndex(left.index) - normalizeRoundIndex(right.index) ||
      Number(left.createdAt || 0) - Number(right.createdAt || 0) ||
      trimRoundText(left.id).localeCompare(trimRoundText(right.id))
  );

const mergeMissionIds = (remoteIds: unknown, existingIds: unknown) => {
  const merged: string[] = [];
  const seen = new Set<string>();
  [remoteIds, existingIds].forEach((value) => {
    (Array.isArray(value) ? value : []).forEach((entry) => {
      const normalized = trimRoundText(entry);
      if (!normalized || seen.has(normalized)) return;
      seen.add(normalized);
      merged.push(normalized);
    });
  });
  return merged;
};

export const roundHasCommittedUserMessage = (round: Pick<OrchestrationRoundSnapshot, 'userMessage'> | null | undefined) =>
  Boolean(trimRoundText(round?.userMessage));

export const roundIsFinalized = (
  round: Pick<OrchestrationRoundSnapshot, 'finalizedAt'> | null | undefined
) => Number(round?.finalizedAt || 0) > 0;

export const roundHasCommittedContent = (
  round: Pick<OrchestrationRoundSnapshot, 'userMessage' | 'finalizedAt'> | null | undefined
) => roundHasCommittedUserMessage(round) && roundIsFinalized(round);

const mergeRoundPair = <T extends OrchestrationRoundSnapshot>(
  remoteRound: T,
  existingRound: T,
  preserveExistingCommit: boolean
): T => {
  const remoteMessage = String(remoteRound.userMessage || '');
  const existingMessage = String(existingRound.userMessage || '');
  const remoteSituation = String(remoteRound.situation || '');
  const existingSituation = String(existingRound.situation || '');
  const remoteCreatedAt = Number(remoteRound.createdAt || 0);
  const existingCreatedAt = Number(existingRound.createdAt || 0);
  const remoteFinalizedAt = Number(remoteRound.finalizedAt || 0);
  const existingFinalizedAt = Number(existingRound.finalizedAt || 0);
  return {
    ...existingRound,
    ...remoteRound,
    id: trimRoundText(remoteRound.id) || trimRoundText(existingRound.id),
    index: normalizeRoundIndex(remoteRound.index || existingRound.index),
    situation: trimRoundText(remoteSituation) ? remoteSituation : existingSituation,
    userMessage: preserveExistingCommit && !trimRoundText(remoteMessage) ? existingMessage : remoteMessage,
    createdAt: remoteCreatedAt > 0 ? remoteCreatedAt : existingCreatedAt,
    finalizedAt: Math.max(remoteFinalizedAt, existingFinalizedAt),
    missionIds: mergeMissionIds(remoteRound.missionIds, existingRound.missionIds),
    branchParentRoundId:
      trimRoundText(remoteRound.branchParentRoundId) || trimRoundText(existingRound.branchParentRoundId),
    branchFromRoundIndex: Math.max(
      0,
      Number(remoteRound.branchFromRoundIndex || 0) || Number(existingRound.branchFromRoundIndex || 0) || 0
    ),
    branchRootOrchestrationId:
      trimRoundText(remoteRound.branchRootOrchestrationId) || trimRoundText(existingRound.branchRootOrchestrationId),
    orchestrationId: trimRoundText(remoteRound.orchestrationId) || trimRoundText(existingRound.orchestrationId)
  } as T;
};

export const stabilizeOrchestrationRoundSnapshots = <T extends OrchestrationRoundSnapshot>(
  existingRounds: T[] | null | undefined,
  remoteRounds: T[] | null | undefined
): T[] => {
  const existing = sortRounds(Array.isArray(existingRounds) ? existingRounds : []);
  const remote = sortRounds(Array.isArray(remoteRounds) ? remoteRounds : []);
  if (!remote.length) return existing;
  if (!existing.length) return remote;

  const existingByIndex = new Map(existing.map((round) => [normalizeRoundIndex(round.index), round] as const));
  const remoteByIndex = new Map(remote.map((round) => [normalizeRoundIndex(round.index), round] as const));
  const remoteLatestCommittedIndex = remote.reduce((latest, round) => {
    if (!roundHasCommittedContent(round)) {
      return latest;
    }
    return Math.max(latest, normalizeRoundIndex(round.index));
  }, 0);
  const remoteMaxIndex = remote.reduce((latest, round) => Math.max(latest, normalizeRoundIndex(round.index)), 0);
  const existingMaxIndex = existing.reduce((latest, round) => Math.max(latest, normalizeRoundIndex(round.index)), 0);
  const nextRounds: T[] = [];

  for (let index = 1; index <= Math.max(remoteMaxIndex, existingMaxIndex); index += 1) {
    const remoteRound = remoteByIndex.get(index) || null;
    const existingRound = existingByIndex.get(index) || null;

    if (remoteRound && existingRound) {
      const preserveExistingCommit =
        roundHasCommittedContent(existingRound) &&
        !roundHasCommittedContent(remoteRound) &&
        remoteLatestCommittedIndex > index;
      nextRounds.push(mergeRoundPair(remoteRound, existingRound, preserveExistingCommit));
      continue;
    }

    if (remoteRound) {
      nextRounds.push(remoteRound);
      continue;
    }

    if (!existingRound) {
      continue;
    }

    const preserveTrailingPreview = index > remoteMaxIndex && !roundHasCommittedContent(existingRound);
    const preserveMissingCommittedGap =
      roundHasCommittedContent(existingRound) && remoteLatestCommittedIndex > index;
    if (preserveTrailingPreview || preserveMissingCommittedGap) {
      nextRounds.push(existingRound);
    }
  }

  return sortRounds(nextRounds);
};
