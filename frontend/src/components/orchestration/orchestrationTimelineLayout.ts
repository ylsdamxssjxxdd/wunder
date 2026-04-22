export type OrchestrationTimelineHistoryInput = {
  orchestrationId: string;
  runId: string;
  status: string;
  latestRoundIndex: number;
  enteredAt: number;
  updatedAt: number;
  exitedAt: number;
  restoredAt: number;
  parentOrchestrationId: string;
  branchRootOrchestrationId: string;
  branchFromRoundIndex: number;
  branchDepth: number;
  groupId?: string;
  motherAgentId?: string;
  motherAgentName?: string;
  motherSessionId?: string;
};

export type OrchestrationTimelineRoundInput = {
  id: string;
  index: number;
  orchestrationId?: string;
  userMessage?: string;
  finalizedAt?: number;
};

export type OrchestrationTimelineRunFallback = {
  runId: string;
  status: string;
  latestRoundIndex: number;
  groupId?: string;
  motherAgentId?: string;
  motherAgentName?: string;
  motherSessionId?: string;
  parentOrchestrationId?: string;
  branchRootOrchestrationId?: string;
  branchFromRoundIndex?: number;
  branchDepth?: number;
};

export type TimelineRunItem = {
  type: 'run';
  id: string;
  lane: number;
  column: number;
  title: string;
  latestRoundIndex: number;
  active: boolean;
  current: boolean;
  status: string;
  branchFromRoundIndex: number;
  branchDepth: number;
  parentOrchestrationId: string;
};

export type TimelineRoundItem = {
  type: 'round';
  id: string;
  lane: number;
  column: number;
  roundId: string;
  roundIndex: number;
  active: boolean;
  selected: boolean;
  pending: boolean;
  preview: boolean;
  orchestrationId: string;
  currentRun: boolean;
};

export type TimelineConnector = {
  id: string;
  className: string;
  style: Record<string, string>;
};

export type TimelineLayout = {
  items: Array<TimelineRunItem | TimelineRoundItem>;
  connectors: TimelineConnector[];
  laneCount: number;
  columnCount: number;
  debugRuns: OrchestrationTimelineHistoryInput[];
};

type TimelineRunLayoutMeta = {
  lane: number;
  runChipColumn: number;
  lastRoundColumn: number;
  branchFromRoundIndex: number;
  roundColumns: Map<number, number>;
};

import {
  roundHasCommittedContent,
  roundIsFinalized
} from '@/components/orchestration/orchestrationRoundStateStability';

const normalizeTimelineText = (value: unknown): string => String(value || '').trim();

const roundHasUserMessage = (round: { userMessage?: unknown } | null | undefined) =>
  Boolean(String(round?.userMessage || '').trim());

const sortTimelineHistoryItems = (items: OrchestrationTimelineHistoryInput[]) =>
  [...items].sort((left, right) => {
    const branchPointDiff =
      Math.max(0, Number(left.branchFromRoundIndex || 0)) -
      Math.max(0, Number(right.branchFromRoundIndex || 0));
    if (branchPointDiff !== 0) return branchPointDiff;
    const branchDepthDiff = (left.branchDepth || 0) - (right.branchDepth || 0);
    if (branchDepthDiff !== 0) return branchDepthDiff;
    const enteredDiff = (left.enteredAt || 0) - (right.enteredAt || 0);
    if (enteredDiff !== 0) return enteredDiff;
    return String(left.orchestrationId || '').localeCompare(String(right.orchestrationId || ''));
  });

const normalizeHistoryItem = (
  item: OrchestrationTimelineHistoryInput
): OrchestrationTimelineHistoryInput | null => {
  const orchestrationId = normalizeTimelineText(item?.orchestrationId);
  const runId = normalizeTimelineText(item?.runId);
  if (!orchestrationId || !runId) return null;
  return {
    orchestrationId,
    runId,
    status: normalizeTimelineText(item?.status),
    latestRoundIndex: Math.max(1, Number(item?.latestRoundIndex || 1)),
    enteredAt: Number(item?.enteredAt || 0),
    updatedAt: Number(item?.updatedAt || 0),
    exitedAt: Number(item?.exitedAt || 0),
    restoredAt: Number(item?.restoredAt || 0),
    parentOrchestrationId: normalizeTimelineText(item?.parentOrchestrationId),
    branchRootOrchestrationId:
      normalizeTimelineText(item?.branchRootOrchestrationId) || orchestrationId,
    branchFromRoundIndex: Math.max(0, Number(item?.branchFromRoundIndex || 0)),
    branchDepth: Math.max(0, Number(item?.branchDepth || 0)),
    groupId: normalizeTimelineText(item?.groupId),
    motherAgentId: normalizeTimelineText(item?.motherAgentId),
    motherAgentName: normalizeTimelineText(item?.motherAgentName),
    motherSessionId: normalizeTimelineText(item?.motherSessionId)
  };
};

const collectConnectedHistory = ({
  historyItems,
  currentRunId
}: {
  historyItems: OrchestrationTimelineHistoryInput[];
  currentRunId: string;
}) => {
  const historyById = new Map<string, OrchestrationTimelineHistoryInput>();
  historyItems.forEach((item) => {
    const normalized = normalizeHistoryItem(item);
    if (!normalized) return;
    historyById.set(normalized.orchestrationId, normalized);
  });
  if (!historyById.size) {
    return [] as OrchestrationTimelineHistoryInput[];
  }

  const childrenByParent = new Map<string, string[]>();
  historyById.forEach((item, orchestrationId) => {
    const parentId = normalizeTimelineText(item.parentOrchestrationId);
    const bucketKey = parentId && historyById.has(parentId) ? parentId : '';
    const bucket = childrenByParent.get(bucketKey) || [];
    bucket.push(orchestrationId);
    childrenByParent.set(bucketKey, bucket);
  });
  childrenByParent.forEach((ids, parentId) => {
    ids.sort((leftId, rightId) => {
      const left = historyById.get(leftId);
      const right = historyById.get(rightId);
      if (!left || !right) return leftId.localeCompare(rightId);
      return sortTimelineHistoryItems([left, right])[0]?.orchestrationId === leftId ? -1 : 1;
    });
    childrenByParent.set(parentId, ids);
  });

  const resolveTreeRootId = () => {
    if (currentRunId && historyById.has(currentRunId)) {
      const visited = new Set<string>();
      let cursorId = currentRunId;
      let rootId = currentRunId;
      while (cursorId && !visited.has(cursorId)) {
        visited.add(cursorId);
        const cursor = historyById.get(cursorId);
        if (!cursor) break;
        const parentId = normalizeTimelineText(cursor.parentOrchestrationId);
        if (!parentId || !historyById.has(parentId)) {
          rootId = cursorId;
          break;
        }
        rootId = parentId;
        cursorId = parentId;
      }
      return rootId;
    }
    const topLevel = childrenByParent.get('') || [];
    return topLevel[0] || [...historyById.keys()][0] || '';
  };

  const rootId = resolveTreeRootId();
  const orderedIds: string[] = [];
  const visited = new Set<string>();
  const visit = (runId: string) => {
    if (!runId || visited.has(runId) || !historyById.has(runId)) return;
    visited.add(runId);
    orderedIds.push(runId);
    (childrenByParent.get(runId) || []).forEach((childId) => visit(childId));
  };
  visit(rootId);
  if (currentRunId && historyById.has(currentRunId) && !visited.has(currentRunId)) {
    visit(currentRunId);
  }
  return orderedIds
    .map((id) => historyById.get(id) || null)
    .filter((item): item is OrchestrationTimelineHistoryInput => Boolean(item));
};

export const buildOrchestrationTimelineLayout = ({
  historyItems,
  currentOrchestrationId,
  rounds,
  activeRoundId,
  isActive,
  isBusy,
  currentRunFallback
}: {
  historyItems: OrchestrationTimelineHistoryInput[];
  currentOrchestrationId: string;
  rounds: OrchestrationTimelineRoundInput[];
  activeRoundId?: string;
  isActive: boolean;
  isBusy: boolean;
  currentRunFallback?: OrchestrationTimelineRunFallback | null;
}): TimelineLayout => {
  const normalizedCurrentRunId = normalizeTimelineText(currentOrchestrationId);
  const normalizedRounds = (Array.isArray(rounds) ? rounds : [])
    .map((round) => ({
      id: normalizeTimelineText(round?.id),
      index: Math.max(1, Number(round?.index || 0)),
      orchestrationId:
        normalizeTimelineText(round?.orchestrationId) || normalizedCurrentRunId,
      userMessage: String(round?.userMessage || ''),
      finalizedAt: Number(round?.finalizedAt || 0)
    }))
    .filter((round) => round.id && round.index > 0)
    .sort(
      (left, right) =>
        left.index - right.index ||
        String(left.id || '').localeCompare(String(right.id || ''))
    );

  if (!historyItems.length && !normalizedRounds.length) {
    return {
      items: [],
      connectors: [],
      laneCount: 1,
      columnCount: 1,
      debugRuns: []
    };
  }

  const currentRounds = normalizedRounds.map((round) => ({
    ...round,
    orchestrationId: normalizeTimelineText(round.orchestrationId) || normalizedCurrentRunId
  }));
  const currentFormalRounds = currentRounds.filter(
    (round) => roundHasUserMessage(round) && roundIsFinalized({ finalizedAt: round.finalizedAt })
  );
  const currentFormalLatestRound = currentFormalRounds[currentFormalRounds.length - 1] || null;
  const normalizedHistoryItems = (Array.isArray(historyItems) ? historyItems : [])
    .map((item) => normalizeHistoryItem(item))
    .filter((item): item is OrchestrationTimelineHistoryInput => Boolean(item));
  const historyById = new Map(
    normalizedHistoryItems.map((item) => [item.orchestrationId, item] as const)
  );
  const currentHistoryItem =
    (normalizedCurrentRunId && historyById.get(normalizedCurrentRunId)) || null;
  const renderableRuns = normalizedHistoryItems.slice();

  if (
    normalizedCurrentRunId &&
    currentRounds.length &&
    !historyById.has(normalizedCurrentRunId)
  ) {
    renderableRuns.push({
      orchestrationId: normalizedCurrentRunId,
      runId:
        normalizeTimelineText(currentRunFallback?.runId) || normalizedCurrentRunId,
      groupId: normalizeTimelineText(currentRunFallback?.groupId),
      motherAgentId: normalizeTimelineText(currentRunFallback?.motherAgentId),
      motherAgentName: normalizeTimelineText(currentRunFallback?.motherAgentName),
      motherSessionId: normalizeTimelineText(currentRunFallback?.motherSessionId),
      status: normalizeTimelineText(currentRunFallback?.status) || (isActive ? 'active' : 'closed'),
      latestRoundIndex: Math.max(
        1,
        Number(currentRunFallback?.latestRoundIndex || currentFormalLatestRound?.index || 1)
      ),
      enteredAt: 0,
      updatedAt: 0,
      exitedAt: 0,
      restoredAt: 0,
      parentOrchestrationId: normalizeTimelineText(
        currentRunFallback?.parentOrchestrationId || currentHistoryItem?.parentOrchestrationId
      ),
      branchRootOrchestrationId:
        normalizeTimelineText(
          currentRunFallback?.branchRootOrchestrationId ||
            currentHistoryItem?.branchRootOrchestrationId
        ) || normalizedCurrentRunId,
      branchFromRoundIndex: Math.max(
        0,
        Number(
          currentRunFallback?.branchFromRoundIndex ??
            currentHistoryItem?.branchFromRoundIndex ??
            0
        )
      ),
      branchDepth: Math.max(
        0,
        Number(currentRunFallback?.branchDepth ?? currentHistoryItem?.branchDepth ?? 0)
      )
    });
  }

  const orderedRuns = collectConnectedHistory({
    historyItems: renderableRuns,
    currentRunId: normalizedCurrentRunId
  });

  const laneByRun = new Map<string, number>();
  const lanes: string[][] = [];

  const ensureRunLane = (runId: string, parentId = '') => {
    if (!runId || laneByRun.has(runId)) return;
    let lane = 0;
    if (parentId && laneByRun.has(parentId)) {
      lane = laneByRun.get(parentId) ?? 0;
      while (lanes[lane]?.length) {
        lane += 1;
      }
    } else {
      while (lanes[lane]?.length) {
        lane += 1;
      }
    }
    laneByRun.set(runId, lane);
    if (!lanes[lane]) lanes[lane] = [];
    lanes[lane].push(runId);
  };

  orderedRuns.forEach((item) => {
    ensureRunLane(item.orchestrationId, item.parentOrchestrationId);
  });

  if (normalizedCurrentRunId && currentRounds.length && !laneByRun.has(normalizedCurrentRunId)) {
    ensureRunLane(
      normalizedCurrentRunId,
      normalizeTimelineText(currentHistoryItem?.parentOrchestrationId)
    );
  }

  const items: Array<TimelineRunItem | TimelineRoundItem> = [];
  const connectors: TimelineConnector[] = [];
  const runLayoutById = new Map<string, TimelineRunLayoutMeta>();
  let maxColumn = 1;

  const resolveRunAnchorColumn = (runId: string, roundIndex: number) => {
    const meta = runLayoutById.get(runId);
    if (!meta) return 1;
    if (roundIndex <= meta.branchFromRoundIndex) {
      return meta.runChipColumn;
    }
    return meta.roundColumns.get(roundIndex) ?? meta.lastRoundColumn ?? meta.runChipColumn;
  };

  const buildSyntheticRounds = (runId: string, latestRoundIndex: number) =>
    Array.from({ length: Math.max(1, latestRoundIndex) }, (_, index) => ({
      id: `history:${runId}:round_${String(index + 1).padStart(2, '0')}`,
      index: index + 1,
      orchestrationId: runId,
      userMessage: '',
      finalizedAt: 0
    }));

  orderedRuns.forEach((item) => {
    const runId = normalizeTimelineText(item.orchestrationId);
    if (!runId) return;
    const lane = laneByRun.get(runId) ?? 0;
    const parentId = normalizeTimelineText(item.parentOrchestrationId);
    const branchFromRoundIndex = Math.max(0, Number(item.branchFromRoundIndex || 0));
    const parentAnchorColumn = parentId ? resolveRunAnchorColumn(parentId, branchFromRoundIndex) : 1;
    const runChipColumn = Math.max(1, parentAnchorColumn);
    maxColumn = Math.max(maxColumn, runChipColumn);
    items.push({
      type: 'run',
      id: `run:${runId}`,
      lane,
      column: runChipColumn,
      title: normalizeTimelineText(item.runId) || runId,
      latestRoundIndex: Math.max(1, Number(item.latestRoundIndex || 1)),
      active: runId === normalizedCurrentRunId && isActive,
      current: runId === normalizedCurrentRunId,
      status: normalizeTimelineText(item.status).toLowerCase(),
      branchFromRoundIndex,
      branchDepth: Math.max(0, Number(item.branchDepth || 0)),
      parentOrchestrationId: parentId
    });
    if (parentId && runLayoutById.has(parentId)) {
      const parentMeta = runLayoutById.get(parentId)!;
      if (parentMeta.lane !== lane) {
        connectors.push({
          id: `run-link-vertical:${parentId}:${runId}:${branchFromRoundIndex}`,
          className: 'orchestration-timeline-connector vertical orchestration-timeline-connector--branch',
          style: {
            '--lane-start': String(Math.min(parentMeta.lane, lane) + 1),
            '--lane-end': String(Math.max(parentMeta.lane, lane) + 1),
            '--column': String(runChipColumn)
          }
        });
      }
    }

    const isCurrentRun = runId === normalizedCurrentRunId;
    const sourceRounds = isCurrentRun
      ? currentRounds
      : buildSyntheticRounds(runId, Math.max(1, Number(item.latestRoundIndex || 1)));
    const scopedRounds = sourceRounds.filter((round) => {
      const roundIndex = Math.max(1, Number(round.index || 0));
      return branchFromRoundIndex > 0 ? roundIndex > branchFromRoundIndex : true;
    });
    const latestCommittedRoundIndex = isCurrentRun
      ? scopedRounds.reduce((latest, round) => {
          if (!roundHasCommittedContent(round)) {
            return latest;
          }
          return Math.max(latest, Math.max(1, Number(round.index || 0)));
        }, 0)
      : 0;
    const completedRounds = isCurrentRun
      ? scopedRounds.filter((round) => {
          const roundIndex = Math.max(1, Number(round.index || 0));
          if (latestCommittedRoundIndex > 0) {
            return roundIndex <= latestCommittedRoundIndex;
          }
          return roundHasCommittedContent(round);
        })
      : scopedRounds;
    const inFlightRound = isCurrentRun
      ? scopedRounds.find((round) => {
          const roundIndex = Math.max(1, Number(round.index || 0));
          if (roundIndex <= latestCommittedRoundIndex) {
            return false;
          }
          return roundHasUserMessage(round) && !roundIsFinalized({ finalizedAt: round.finalizedAt });
        }) || null
      : null;
    const lastCompletedRound = completedRounds[completedRounds.length - 1] || null;
    const branchBaseRound =
      branchFromRoundIndex > 0
        ? sourceRounds.find((round) => Math.max(1, Number(round.index || 0)) === branchFromRoundIndex) || null
        : null;
    const branchBaseRoundHasUserMessage = roundHasCommittedContent(branchBaseRound);
    const previewBaseRoundIndex = Math.max(
      branchFromRoundIndex,
      Number(lastCompletedRound?.index || 0) || Number(branchBaseRound?.index || 0) || 0
    );
    const nextFrontierRoundIndex = Math.max(1, previewBaseRoundIndex + 1);
    const existingPreparedNextRound =
      isCurrentRun
        ? scopedRounds.find(
            (round) =>
              Number(round.index || 0) === nextFrontierRoundIndex && !roundHasUserMessage(round)
          ) || null
        : null;
    const shouldAppendPreviewRound =
      isCurrentRun &&
      isActive &&
      !isBusy &&
      !inFlightRound &&
      !existingPreparedNextRound &&
      (completedRounds.length > 0 || branchBaseRoundHasUserMessage || nextFrontierRoundIndex === 1);
    const displayRounds: Array<{
      id: string;
      index: number;
      orchestrationId: string;
      userMessage?: string;
      finalizedAt?: number;
    }> = [
      ...completedRounds,
      ...(inFlightRound && !completedRounds.some((round) => round.id === inFlightRound.id)
        ? [inFlightRound]
        : []),
      ...(existingPreparedNextRound
        ? [existingPreparedNextRound]
        : shouldAppendPreviewRound
          ? [
              {
                id: `preview:${runId}:round_${String(nextFrontierRoundIndex).padStart(2, '0')}`,
                index: nextFrontierRoundIndex,
                orchestrationId: runId,
                userMessage: '',
                finalizedAt: 0
              }
            ]
          : [])
    ];
    const roundColumns = new Map<number, number>();
    let lastRoundColumn = runChipColumn;
    displayRounds.forEach((round) => {
      const roundIndex = Math.max(1, Number(round.index || 0));
      const hasCommittedContent = isCurrentRun ? roundHasCommittedContent(round) : true;
      const isSelectedRound = isCurrentRun && round.id === normalizeTimelineText(activeRoundId);
      const isPreviewRound = String(round.id || '').startsWith(`preview:${runId}:`);
      const hasCompletedLaterRound = completedRounds.some(
        (completedRound) => Math.max(1, Number(completedRound.index || 0)) > roundIndex
      );
      const isCompletedRound = hasCommittedContent || hasCompletedLaterRound;
      const column = runChipColumn + Math.max(1, roundIndex - branchFromRoundIndex);
      roundColumns.set(roundIndex, column);
      lastRoundColumn = Math.max(lastRoundColumn, column);
      maxColumn = Math.max(maxColumn, column);
      items.push({
        type: 'round',
        id: `round:${runId}:${round.id}`,
        lane,
        column,
        roundId: round.id,
        roundIndex,
        active: isSelectedRound && !isPreviewRound && isCompletedRound,
        selected: isSelectedRound,
        pending: !isCompletedRound || isPreviewRound,
        preview: isPreviewRound,
        orchestrationId: runId,
        currentRun: isCurrentRun
      });
    });
    if (displayRounds.length) {
      connectors.push({
        id: `run-round-link:${runId}`,
        className: 'orchestration-timeline-connector horizontal orchestration-timeline-connector--rounds',
        style: {
          '--lane': String(lane + 1),
          '--column-start': String(runChipColumn),
          '--column-end': String(lastRoundColumn)
        }
      });
    }
    runLayoutById.set(runId, {
      lane,
      runChipColumn,
      lastRoundColumn,
      branchFromRoundIndex,
      roundColumns
    });
  });

  return {
    items,
    connectors,
    laneCount: Math.max(1, lanes.length || 1),
    columnCount: Math.max(1, maxColumn + 1),
    debugRuns: orderedRuns
  };
};
