import { fetchMyPreferences, updateMyPreferences } from '@/api/auth';

export type MessengerOrderPreferences = {
  messages: string[];
  agentsOwned: string[];
  agentsShared: string[];
  swarms: string[];
  updatedAt: number;
};

const nowSeconds = () => Date.now() / 1000;

const normalizeStringList = (value: unknown): string[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  const output: string[] = [];
  const seen = new Set<string>();
  value.forEach((item) => {
    const normalized = String(item || '').trim();
    if (!normalized || seen.has(normalized)) {
      return;
    }
    seen.add(normalized);
    output.push(normalized);
  });
  return output;
};

export const defaultMessengerOrderPreferences = (): MessengerOrderPreferences => ({
  messages: [],
  agentsOwned: [],
  agentsShared: [],
  swarms: [],
  updatedAt: 0
});

export const normalizeMessengerOrderPreferences = (payload: unknown): MessengerOrderPreferences => {
  const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  const messengerOrder =
    source.messenger_order && typeof source.messenger_order === 'object'
      ? (source.messenger_order as Record<string, unknown>)
      : source;
  const updatedAt = Number(source.updated_at ?? source.updatedAt ?? messengerOrder.updated_at ?? 0);
  return {
    messages: normalizeStringList(messengerOrder.messages),
    agentsOwned: normalizeStringList(messengerOrder.agents_owned ?? messengerOrder.agentsOwned),
    agentsShared: normalizeStringList(messengerOrder.agents_shared ?? messengerOrder.agentsShared),
    swarms: normalizeStringList(messengerOrder.swarms),
    updatedAt: Number.isFinite(updatedAt) && updatedAt > 0 ? updatedAt : 0
  };
};

const toRemotePayload = (value: MessengerOrderPreferences) => ({
  messenger_order: {
    messages: normalizeStringList(value.messages),
    agents_owned: normalizeStringList(value.agentsOwned),
    agents_shared: normalizeStringList(value.agentsShared),
    swarms: normalizeStringList(value.swarms)
  }
});

export const loadMessengerOrderPreferences = async (): Promise<MessengerOrderPreferences> => {
  try {
    const { data } = await fetchMyPreferences();
    return normalizeMessengerOrderPreferences(data?.data);
  } catch {
    return defaultMessengerOrderPreferences();
  }
};

export const saveMessengerOrderPreferences = async (
  value: MessengerOrderPreferences
): Promise<MessengerOrderPreferences> => {
  const normalized: MessengerOrderPreferences = {
    ...defaultMessengerOrderPreferences(),
    ...value,
    messages: normalizeStringList(value.messages),
    agentsOwned: normalizeStringList(value.agentsOwned),
    agentsShared: normalizeStringList(value.agentsShared),
    swarms: normalizeStringList(value.swarms),
    updatedAt: nowSeconds()
  };
  try {
    const { data } = await updateMyPreferences(toRemotePayload(normalized));
    const remote = normalizeMessengerOrderPreferences(data?.data);
    return {
      ...normalized,
      ...remote,
      updatedAt: remote.updatedAt > 0 ? remote.updatedAt : normalized.updatedAt
    };
  } catch {
    return normalized;
  }
};
