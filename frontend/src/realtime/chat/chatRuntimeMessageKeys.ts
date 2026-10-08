type ChatMessageLike = Record<string, unknown>;

export type ChatRuntimeMessageKeyRole = 'user' | 'assistant' | 'message';

export const resolveStableChatRuntimeMessageId = (
  message: ChatMessageLike | null | undefined
): string => firstText(
  message?.__runtime_message_id,
  message?.message_id,
  message?.messageId,
  message?.id,
  message?.client_message_id,
  message?.clientMessageId,
  message?.request_id,
  message?.requestId
);

export const resolveChatRuntimeRenderableKey = (
  message: ChatMessageLike | null | undefined,
  fallbackIndex?: number
): string => {
  const runtimeKey = firstText(message?.__runtime_render_key);
  if (runtimeKey) return runtimeKey;
  const role = normalizeChatRuntimeMessageKeyRole(message?.role);
  const id = resolveStableChatRuntimeMessageId(message);
  if (id) return `runtime:${role}:${id}`;
  const safeIndex = Number.isFinite(fallbackIndex) ? Math.max(0, Math.trunc(Number(fallbackIndex))) : 0;
  return `legacy:${role}:${safeIndex}`;
};

export const normalizeChatRuntimeMessageKeyRole = (
  value: unknown
): ChatRuntimeMessageKeyRole => {
  const role = String(value || '').trim().toLowerCase();
  if (role === 'user' || role === 'assistant') return role;
  return 'message';
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};
