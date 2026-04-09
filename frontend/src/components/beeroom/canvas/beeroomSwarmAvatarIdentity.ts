import { isBeeroomDefaultAgentLike } from '../beeroomActorIdentity';

export const resolveBeeroomProjectedSubagentAvatarImage = (options: {
  agentId: unknown;
  name: unknown;
  explicitAvatarImageUrl?: unknown;
  resolveAgentAvatarImageByAgentId?: ((agentId: unknown) => string) | undefined;
  defaultAgentAvatarImageUrl?: unknown;
  fallbackAvatarImageUrl?: unknown;
}): string => {
  const normalizedAgentId = String(options.agentId || '').trim();
  const resolvedByAgentId =
    normalizedAgentId && typeof options.resolveAgentAvatarImageByAgentId === 'function'
      ? String(options.resolveAgentAvatarImageByAgentId(normalizedAgentId) || '').trim()
      : '';
  if (resolvedByAgentId) {
    return resolvedByAgentId;
  }
  const explicitAvatarImageUrl = String(options.explicitAvatarImageUrl || '').trim();
  if (explicitAvatarImageUrl) {
    return explicitAvatarImageUrl;
  }
  if (isBeeroomDefaultAgentLike(options.name)) {
    const defaultAgentAvatarImageUrl =
      String(options.resolveAgentAvatarImageByAgentId?.('__default__') || '').trim() ||
      String(options.defaultAgentAvatarImageUrl || '').trim();
    if (defaultAgentAvatarImageUrl) {
      return defaultAgentAvatarImageUrl;
    }
  }
  return String(options.fallbackAvatarImageUrl || '').trim();
};
