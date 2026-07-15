import {
  selectChatRuntimeMessage,
  selectChatRuntimeSession,
  selectLatestAssistantForTurn
} from '@/realtime/chat/chatRuntimeSelectors';
import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeProjection
} from '@/realtime/chat/chatRuntimeTypes';

type MessageRecord = Record<string, unknown>;

export type RuntimeMessageContentSourceOptions = {
  projection: ChatRuntimeProjection | null | undefined;
  sessionId: string;
  runtimeMessageId?: unknown;
  runtimeUserTurnId?: unknown;
  runtimeModelTurnId?: unknown;
  message?: MessageRecord | null;
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};

const resolveRuntimeProjectedMessageByTurn = (
  options: RuntimeMessageContentSourceOptions
): ChatRuntimeMessageProjection | null => {
  const message = options.message || {};
  if (String(message.role || '').trim() !== 'assistant') return null;
  const modelTurnId = firstText(
    options.runtimeModelTurnId,
    message.__runtime_model_turn_id,
    message.model_turn_id,
    message.modelTurnId
  );
  if (modelTurnId) {
    const session = selectChatRuntimeSession(options.projection, options.sessionId);
    const modelTurn = session?.modelTurnById?.[modelTurnId];
    if (modelTurn) {
      for (let index = modelTurn.messageIds.length - 1; index >= 0; index -= 1) {
        const candidate = session.messageById[modelTurn.messageIds[index]];
        if (candidate?.role === 'assistant') return candidate;
      }
    }
  }

  const userTurnId = firstText(
    options.runtimeUserTurnId,
    message.__runtime_user_turn_id,
    message.user_turn_id,
    message.userTurnId
  );
  return userTurnId
    ? selectLatestAssistantForTurn(options.projection, options.sessionId, userTurnId)
    : null;
};

// A merged model turn may retain a stale turn id briefly. The rendered message
// id is authoritative, so never let a turn-level fallback replace it.
export const resolveRuntimeMessageContentSource = (
  options: RuntimeMessageContentSourceOptions
): ChatRuntimeMessageProjection | null => {
  const messageId = firstText(options.runtimeMessageId);
  const direct = messageId
    ? selectChatRuntimeMessage(options.projection, options.sessionId, messageId)
    : null;
  return direct || resolveRuntimeProjectedMessageByTurn(options);
};

export const resolveRuntimeMessageContentSubscriptionIds = (
  options: RuntimeMessageContentSourceOptions
): string[] => {
  const resolved = resolveRuntimeMessageContentSource(options);
  if (resolved?.id) return [resolved.id];
  const explicit = firstText(options.runtimeMessageId);
  return explicit ? [explicit] : [];
};
