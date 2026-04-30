import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeSessionProjection,
  ChatRuntimeViolation
} from './chatRuntimeTypes';
import { isChatRuntimeBusyStatus } from './chatRuntimeReducer';
import { selectVisibleMessageProjections } from './chatRuntimeSelectors';

export const collectChatRuntimeInvariantViolations = (
  session: ChatRuntimeSessionProjection
): ChatRuntimeViolation[] => {
  const violations: ChatRuntimeViolation[] = [];
  const visible = selectVisibleMessageProjections(
    { activeSessionId: session.sessionId, sessions: { [session.sessionId]: session }, debugEvents: [] },
    session.sessionId
  );
  collectAssistantOrderingViolations(visible, violations);
  collectFinalAssistantViolations(session, violations);
  if (isChatRuntimeBusyStatus(session.runtimeStatus) && !session.busyReason) {
    violations.push({
      code: 'busy_without_reason',
      message: 'busy runtime status must expose a busy reason',
      eventSeq: session.appliedSeq || null,
      eventType: 'invariant_check'
    });
  }
  return violations;
};

const collectAssistantOrderingViolations = (
  visible: ChatRuntimeMessageProjection[],
  violations: ChatRuntimeViolation[]
): void => {
  const latestUserIndexByTurn = new Map<string, number>();
  visible.forEach((message, index) => {
    if (message.role === 'user' && message.userTurnId) {
      latestUserIndexByTurn.set(message.userTurnId, index);
    }
  });
  visible.forEach((message, index) => {
    if (message.role !== 'assistant' || !message.userTurnId) return;
    const userIndex = latestUserIndexByTurn.get(message.userTurnId);
    if (userIndex === undefined || userIndex <= index) return;
    violations.push({
      code: 'assistant_before_user',
      message: 'assistant message is rendered before its owning user turn',
      eventSeq: message.updatedSeq,
      eventType: 'invariant_check',
      messageId: message.id,
      userTurnId: message.userTurnId,
      modelTurnId: message.modelTurnId
    });
  });
};

const collectFinalAssistantViolations = (
  session: ChatRuntimeSessionProjection,
  violations: ChatRuntimeViolation[]
): void => {
  Object.values(session.modelTurnById).forEach((turn) => {
    const finalMessages = turn.messageIds
      .map((messageId) => session.messageById[messageId])
      .filter((message) => message?.final);
    if (finalMessages.length <= 1) return;
    violations.push({
      code: 'multiple_final_assistants',
      message: 'a model turn must not render multiple final assistant messages',
      eventSeq: session.appliedSeq || null,
      eventType: 'invariant_check',
      modelTurnId: turn.id
    });
  });
};
