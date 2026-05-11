import { stopPendingAssistantMessage } from './chatPendingMessage';
import {
  isSubagentItemActive,
  normalizeSubagentRuntimeStatus
} from '@/utils/subagentRuntime';

const ACTIVE_WORKFLOW_ITEM_STATUS_SET = new Set(['loading', 'pending', 'running', 'streaming']);

const resolveTimestampIso = (value: number): string => new Date(value).toISOString();

export const settleTerminalAssistantArtifacts = (
  messages,
  options: { failed?: boolean } = {}
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const terminalWorkflowStatus = options.failed === true ? 'failed' : 'completed';
  const terminalSubagentStatus = options.failed === true ? 'failed' : 'completed';
  const nowMs = Date.now();
  const updatedAt = resolveTimestampIso(nowMs);
  let changed = false;

  messages.forEach((message) => {
    if (!message || message.role !== 'assistant') return;
    if (stopPendingAssistantMessage(message)) {
      changed = true;
    }

    if (Array.isArray(message.workflowItems)) {
      message.workflowItems.forEach((item) => {
        if (!item || typeof item !== 'object') return;
        const status = String(item.status || '').trim().toLowerCase();
        if (!ACTIVE_WORKFLOW_ITEM_STATUS_SET.has(status)) return;
        item.status = terminalWorkflowStatus;
        changed = true;
      });
    }

    if (!Array.isArray(message.subagents) || message.subagents.length === 0) return;
    let subagentsChanged = false;
    message.subagents = message.subagents.map((item) => {
      if (!item || typeof item !== 'object') return item;
      const active = isSubagentItemActive(item);
      const status = normalizeSubagentRuntimeStatus(item.status);
      const nextStatus =
        active || !status
          ? terminalSubagentStatus
          : status;
      const nextFailed = options.failed === true ? true : Boolean(item.failed);
      const nextTerminal = true;
      const nextCanTerminate = false;
      const nextUpdatedAtMs = Math.max(Number(item.updated_at_ms || 0), nowMs);
      const nextUpdatedAt = nextUpdatedAtMs === Number(item.updated_at_ms || 0)
        ? item.updated_at
        : updatedAt;

      if (
        nextStatus !== status ||
        nextFailed !== Boolean(item.failed) ||
        nextTerminal !== Boolean(item.terminal) ||
        nextCanTerminate !== Boolean(item.canTerminate) ||
        nextUpdatedAtMs !== Number(item.updated_at_ms || 0)
      ) {
        subagentsChanged = true;
      }

      return {
        ...item,
        status: nextStatus,
        failed: nextFailed,
        terminal: nextTerminal,
        canTerminate: nextCanTerminate,
        updated_at_ms: nextUpdatedAtMs,
        updated_at: nextUpdatedAt
      };
    });
    if (subagentsChanged) {
      changed = true;
    }
  });

  return changed;
};
