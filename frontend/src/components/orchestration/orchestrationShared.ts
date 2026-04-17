import type { BeeroomMission } from '@/stores/beeroom';

export const normalizeOrchestrationText = (value: unknown): string => String(value || '').trim();

export const buildOrchestrationRoundId = (index: number) =>
  `round_${String(Math.max(1, Math.trunc(Number(index) || 0))).padStart(4, '0')}`;

export const buildOrchestrationRoundDirName = (index: number) => buildOrchestrationRoundId(index);

export const buildOrchestrationAgentArtifactPath = (runId: string, roundIndex: number, agentId: string) =>
  ['orchestration', normalizeOrchestrationText(runId), buildOrchestrationRoundDirName(roundIndex), normalizeOrchestrationText(agentId)]
    .filter(Boolean)
    .join('/');

export const buildOrchestrationRoundSituationPath = (runId: string, roundIndex: number) =>
  ['orchestration', normalizeOrchestrationText(runId), buildOrchestrationRoundDirName(roundIndex), 'situation.txt']
    .filter(Boolean)
    .join('/');

const ACTIVE_MISSION_STATUSES = new Set(['queued', 'pending', 'running', 'awaiting_idle', 'resuming', 'merging']);
const ACTIVE_TASK_STATUSES = new Set([
  'queued',
  'pending',
  'running',
  'awaiting_idle',
  'resuming',
  'merging',
  'waiting',
  'accepted',
  'cancelling'
]);
const TERMINAL_MISSION_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled', 'canceled']);
const TERMINAL_TASK_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled', 'canceled']);

export const normalizeOrchestrationStatus = (value: unknown) =>
  normalizeOrchestrationText(value).toLowerCase();

export const isOrchestrationMissionTerminalStatus = (value: unknown) =>
  TERMINAL_MISSION_STATUSES.has(normalizeOrchestrationStatus(value));

export const isOrchestrationTaskTerminalStatus = (value: unknown) =>
  TERMINAL_TASK_STATUSES.has(normalizeOrchestrationStatus(value));

export const isOrchestrationTaskActiveStatus = (value: unknown) =>
  ACTIVE_TASK_STATUSES.has(normalizeOrchestrationStatus(value));

export const isOrchestrationMissionRunning = (mission: BeeroomMission) => {
  const completionStatus = normalizeOrchestrationStatus(mission?.completion_status);
  const missionStatus = normalizeOrchestrationStatus(mission?.status);
  if (
    isOrchestrationMissionTerminalStatus(completionStatus) ||
    isOrchestrationMissionTerminalStatus(missionStatus) ||
    Number(mission?.finished_time || 0) > 0
  ) {
    return false;
  }
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  if (
    tasks.length &&
    tasks.every(
      (task) =>
        isOrchestrationTaskTerminalStatus(task?.status) || Number(task?.finished_time || 0) > 0
    )
  ) {
    return false;
  }
  if (
    tasks.some((task) => {
      const taskStatus = normalizeOrchestrationStatus(task?.status);
      if (isOrchestrationTaskActiveStatus(taskStatus)) return true;
      return !taskStatus && Number(task?.finished_time || 0) <= 0;
    })
  ) {
    return true;
  }
  return ACTIVE_MISSION_STATUSES.has(completionStatus || missionStatus);
};
