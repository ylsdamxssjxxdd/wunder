import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeModelTurnProjection,
  ChatRuntimeRawMessage,
  ChatRuntimeSessionProjection
} from './chatRuntimeTypes';

type TailRow = {
  message: ChatRuntimeMessageProjection;
  modelStatus?: ChatRuntimeModelTurnProjection['status'];
};
const id = (value: unknown): string => String(value ?? '').trim();
const key = (role: string, userTurn: string, modelTurn: string): string =>
  JSON.stringify([role, userTurn, role === 'assistant' ? modelTurn : '']);
const terminal = (status: string): boolean =>
  ['final', 'completed', 'failed', 'cancelled'].includes(status);

// Persisted history stops before the in-flight model turn. Replacing history
// must retain that tail: its stream events may already be in the dedupe index.
export const captureTranscriptTail = (
  session: ChatRuntimeSessionProjection,
  transcript: ChatRuntimeRawMessage[],
  preserveCurrent: boolean
): TailRow[] => {
  const covered = new Set(transcript.map((raw) => key(
    id(raw.role), id(raw.user_turn_id ?? raw.userTurnId), id(raw.model_turn_id ?? raw.modelTurnId)
  )));
  const lastTurn = session.userTurns[session.userTurns.length - 1];
  return session.messages.flatMap((messageId) => {
    const message = session.messageById[messageId];
    if (!message) return [];
    const represented = covered.has(key(message.role, message.userTurnId, message.modelTurnId));
    const turn = session.userTurnById[message.userTurnId];
    const keep = preserveCurrent || (!represented && (
      !terminal(message.status) || (turn && !terminal(turn.status)) || message.userTurnId === lastTurn
    ));
    return keep ? [{
      message: { ...message,
        workflowItems: message.workflowItems?.map(item => ({ ...item })),
        subagents: message.subagents?.map(item => ({ ...item }))
      },
      modelStatus: session.modelTurnById[message.modelTurnId]?.status
    }] : [];
  });
};

export const restoreTranscriptTail = (
  session: ChatRuntimeSessionProjection,
  tail: TailRow[],
  keepMessageIds: Set<string>,
  keepUserTurnIds: Set<string>,
  keepModelTurnIds: Set<string>,
  preserveCurrent: boolean
): string[] => {
  const restored: string[] = [];
  const canonicalByKey = new Map<string, ChatRuntimeMessageProjection>();
  let order = 0;
  for (const messageId of keepMessageIds) {
    const message = session.messageById[messageId];
    canonicalByKey.set(key(message.role, message.userTurnId, message.modelTurnId), message);
    order = Math.max(order, message.createdSeq);
  }
  for (const { message, modelStatus } of tail) {
    const canonical = canonicalByKey.get(key(message.role, message.userTurnId, message.modelTurnId));
    if (canonical) {
      if (preserveCurrent) {
        const identity = { id: canonical.id, createdSeq: canonical.createdSeq, raw: canonical.raw };
        Object.assign(canonical, message, identity);
        canonical.structureVersion = Number(canonical.structureVersion || 0) + 1;
        const model = session.modelTurnById[canonical.modelTurnId];
        if (model && modelStatus) model.status = modelStatus;
      }
      continue;
    }
    const userTurn = session.userTurnById[message.userTurnId];
    if (!userTurn) continue;
    message.createdSeq = ++order;
    session.messageById[message.id] = message;
    keepMessageIds.add(message.id);
    keepUserTurnIds.add(userTurn.id);
    if (message.modelTurnId) keepModelTurnIds.add(message.modelTurnId);
    restored.push(message.id);
  }
  return restored;
};
