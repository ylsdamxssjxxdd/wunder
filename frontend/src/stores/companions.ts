import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

import { resolveApiBase } from '@/config/runtime';
import {
  buildCompanionPackageBlob,
  buildCompanionPackageFilename,
  parseCompanionPackageFile,
  type CompanionPackageManifest
} from '@/utils/companionPackage';

export type CompanionSpriteStateId =
  | 'idle'
  | 'running-right'
  | 'running-left'
  | 'waving'
  | 'jumping'
  | 'failed'
  | 'waiting'
  | 'running'
  | 'review';

export type CompanionPosition = {
  x: number;
  y: number;
};

export type CompanionPackageRecord = CompanionPackageManifest & {
  spritesheetDataUrl: string;
  spritesheetMime: string;
  importedAt: number;
  updatedAt: number;
  scope?: 'private' | 'global';
};

export type CompanionSettings = {
  selectedId: string;
  enabled: boolean;
  position: CompanionPosition;
  scale: number;
  messageHintsEnabled: boolean;
};

export type CompanionMessage = {
  text: string;
  kind: 'info' | 'success' | 'warning';
  visibleUntil: number;
};

const DB_NAME = 'wunder-companions';
const DB_VERSION = 1;
const STORE_NAME = 'companions';
const SETTINGS_KEY = 'wunder_companion_settings';
const DEFAULT_POSITION: CompanionPosition = { x: 28, y: 28 };
const DEFAULT_SCALE = 1;

const normalizeNumber = (value: unknown, fallback: number): number => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : fallback;
};

const normalizePosition = (value: unknown): CompanionPosition => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  return {
    x: Math.max(0, Math.round(normalizeNumber(source.x, DEFAULT_POSITION.x))),
    y: Math.max(0, Math.round(normalizeNumber(source.y, DEFAULT_POSITION.y)))
  };
};

const normalizeSettings = (value: unknown): CompanionSettings => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const scale = normalizeNumber(source.scale, DEFAULT_SCALE);
  return {
    selectedId: String(source.selectedId || '').trim(),
    enabled: source.enabled === true,
    position: normalizePosition(source.position),
    scale: Math.min(1.6, Math.max(0.7, scale)),
    messageHintsEnabled: source.messageHintsEnabled !== false
  };
};

const normalizeCompanionMessage = (value: unknown): CompanionMessage | null => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const text = String(source.text || '').trim();
  if (!text) {
    return null;
  }
  const kind = String(source.kind || '').trim().toLowerCase();
  return {
    text,
    kind: kind === 'success' || kind === 'warning' ? kind : 'info',
    visibleUntil: Math.max(0, normalizeNumber(source.visibleUntil, Date.now()))
  };
};

const normalizeRecord = (value: unknown): CompanionPackageRecord | null => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const id = String(source.id || '').trim();
  const displayName = String(source.displayName || '').trim();
  const spritesheetPath = String(source.spritesheetPath || '').trim();
  const spritesheetDataUrl = String(source.spritesheetDataUrl || '').trim();
  if (!id || !displayName || !spritesheetPath || !spritesheetDataUrl.startsWith('data:image/')) {
    return null;
  }
  return {
    id,
    displayName,
    description: String(source.description || '').trim(),
    spritesheetPath,
    spritesheetDataUrl,
    spritesheetMime: String(source.spritesheetMime || 'image/webp').trim(),
    importedAt: Math.max(0, normalizeNumber(source.importedAt, Date.now())),
    updatedAt: Math.max(0, normalizeNumber(source.updatedAt, Date.now()))
  };
};

let databasePromise: Promise<IDBDatabase> | null = null;

const openDatabase = (): Promise<IDBDatabase> => {
  if (databasePromise) {
    return databasePromise;
  }
  databasePromise = new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('IndexedDB is unavailable'));
      return;
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        database.createObjectStore(STORE_NAME, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error('failed to open companion store'));
  });
  return databasePromise;
};

const runStoreRequest = async <T>(
  mode: IDBTransactionMode,
  executor: (store: IDBObjectStore) => IDBRequest<T>
): Promise<T> => {
  const database = await openDatabase();
  return new Promise((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, mode);
    const request = executor(transaction.objectStore(STORE_NAME));
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error('companion store request failed'));
    transaction.onerror = () => reject(transaction.error || new Error('companion transaction failed'));
  });
};

const listStoredCompanions = async (): Promise<CompanionPackageRecord[]> => {
  const records = await runStoreRequest<unknown[]>('readonly', (store) => store.getAll());
  return (Array.isArray(records) ? records : [])
    .map((item) => normalizeRecord(item))
    .filter((item): item is CompanionPackageRecord => Boolean(item))
    .sort((a, b) => b.updatedAt - a.updatedAt);
};

const saveStoredCompanion = async (record: CompanionPackageRecord): Promise<void> => {
  await runStoreRequest<IDBValidKey>('readwrite', (store) => store.put(record));
};

const removeStoredCompanion = async (id: string): Promise<void> => {
  await runStoreRequest<undefined>('readwrite', (store) => store.delete(id));
};

const loadSettings = (): CompanionSettings => {
  try {
    return normalizeSettings(JSON.parse(localStorage.getItem(SETTINGS_KEY) || '{}'));
  } catch {
    return normalizeSettings({});
  }
};

const saveSettings = (settings: CompanionSettings): void => {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Ignore quota/private-mode failures; the current session still keeps state.
  }
};

const downloadBlob = (blob: Blob, filename: string): void => {
  const objectUrl = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = objectUrl;
  anchor.download = filename;
  anchor.rel = 'noopener';
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1200);
};

const normalizeGlobalRecord = (value: unknown): CompanionPackageRecord | null => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const id = String(source.id || '').trim();
  const displayName = String(source.display_name || source.displayName || source.name || '').trim();
  const spritesheetPath = String(source.spritesheet_path || source.spritesheetPath || '').trim();
  const spritesheetDataUrl = String(source.spritesheet_data_url || source.spritesheetDataUrl || '').trim();
  if (!id || !displayName || !spritesheetPath || !spritesheetDataUrl.startsWith('data:image/')) {
    return null;
  }
  return {
    id,
    displayName,
    description: String(source.description || '').trim(),
    spritesheetPath,
    spritesheetDataUrl,
    spritesheetMime: String(source.spritesheet_mime || source.spritesheetMime || 'image/webp').trim(),
    importedAt: Math.max(0, normalizeNumber(source.imported_at || source.importedAt, Date.now())),
    updatedAt: Math.max(0, normalizeNumber(source.updated_at || source.updatedAt, Date.now())),
    scope: 'global'
  };
};

const requestGlobalCompanions = async (): Promise<CompanionPackageRecord[]> => {
  const base = resolveApiBase().replace(/\/+$/, '') || '/wunder';
  const response = await fetch(`${base}/companions/global`, { cache: 'no-store' });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message =
      payload?.error?.message || payload?.detail?.message || payload?.detail || payload?.message || String(response.status);
    throw new Error(message);
  }
  const items = Array.isArray(payload?.data?.items) ? payload.data.items : [];
  return items
    .map((item: unknown) => normalizeGlobalRecord(item))
    .filter((item: CompanionPackageRecord | null): item is CompanionPackageRecord => Boolean(item));
};

const getDesktopBridge = (): Record<string, unknown> | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  const candidate = (window as Window & { wunderDesktop?: Record<string, unknown> }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
};

let desktopCompanionUnsubscribe: (() => void) | null = null;

export const useCompanionStore = defineStore('companions', () => {
  const companions = ref<CompanionPackageRecord[]>([]);
  const globalCompanions = ref<CompanionPackageRecord[]>([]);
  const settings = ref<CompanionSettings>(loadSettings());
  const message = ref<CompanionMessage | null>(null);
  const hydrated = ref(false);
  const loading = ref(false);
  const saving = ref(false);
  const lastError = ref('');

  const selectedCompanion = computed(
    () => companions.value.find((item) => item.id === settings.value.selectedId) || companions.value[0] || null
  );

  const enabled = computed(() => settings.value.enabled && Boolean(selectedCompanion.value));
  const featureEnabled = computed(() => settings.value.enabled);

  const persistSettings = () => {
    saveSettings(settings.value);
  };

  const applyDesktopState = (value: unknown): void => {
    const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
    const selectedId = String(source.selectedId || source.selected_id || '').trim();
    if (selectedId && companions.value.some((item) => item.id === selectedId)) {
      settings.value.selectedId = selectedId;
    }
    if ('enabled' in source) {
      settings.value.enabled = source.enabled === true;
    }
    if ('position' in source || 'x' in source || 'y' in source) {
      settings.value.position = 'position' in source
        ? normalizePosition(source.position)
        : normalizePosition({ x: source.x, y: source.y });
    }
    if ('scale' in source) {
      settings.value.scale = Math.min(1.6, Math.max(0.7, normalizeNumber(source.scale, DEFAULT_SCALE)));
    }
    if ('messageHintsEnabled' in source || 'message_hints_enabled' in source) {
      settings.value.messageHintsEnabled = source.messageHintsEnabled !== false && source.message_hints_enabled !== false;
    }
    persistSettings();
  };

  const watchDesktopState = (): void => {
    if (desktopCompanionUnsubscribe) {
      return;
    }
    const bridge = getDesktopBridge();
    const listener = bridge?.onCompanionStateChanged;
    if (typeof listener !== 'function') {
      return;
    }
    const unsubscribe = listener.call(bridge, (payload: unknown) => {
      applyDesktopState(payload);
    });
    if (typeof unsubscribe === 'function') {
      desktopCompanionUnsubscribe = unsubscribe;
    }
  };

  const hydrate = async () => {
    if (hydrated.value || loading.value) {
      return;
    }
    loading.value = true;
    lastError.value = '';
    try {
      companions.value = await listStoredCompanions();
      void loadGlobalCompanions().catch(() => undefined);
      if (settings.value.selectedId && !companions.value.some((item) => item.id === settings.value.selectedId)) {
        settings.value.selectedId = '';
      }
      if (!settings.value.selectedId && companions.value.length) {
        settings.value.selectedId = companions.value[0].id;
      }
      const bridge = getDesktopBridge();
      const getCompanionState = bridge?.getCompanionState;
      if (typeof getCompanionState === 'function') {
        applyDesktopState(await Promise.resolve(getCompanionState.call(bridge)));
      }
      watchDesktopState();
      persistSettings();
      hydrated.value = true;
    } catch (error) {
      lastError.value = String((error as { message?: string })?.message || error || '');
      throw error;
    } finally {
      loading.value = false;
    }
  };

  const loadGlobalCompanions = async (options: { force?: boolean } = {}): Promise<CompanionPackageRecord[]> => {
    if (!options.force && globalCompanions.value.length) {
      return globalCompanions.value;
    }
    const items = await requestGlobalCompanions();
    globalCompanions.value = items;
    return items;
  };

  const findCompanion = (
    scope: 'private' | 'global',
    id: string
  ): CompanionPackageRecord | null => {
    const cleaned = String(id || '').trim();
    if (!cleaned) return null;
    const list = scope === 'global' ? globalCompanions.value : companions.value;
    return list.find((item) => item.id === cleaned) || null;
  };

  const importPackage = async (file: File): Promise<CompanionPackageRecord> => {
    saving.value = true;
    lastError.value = '';
    try {
      const parsed = await parseCompanionPackageFile(file);
      const now = Date.now();
      const existing = companions.value.find((item) => item.id === parsed.id);
      const record: CompanionPackageRecord = {
        ...parsed,
        importedAt: existing?.importedAt || now,
        updatedAt: now,
        scope: 'private'
      };
      await saveStoredCompanion(record);
      companions.value = [record, ...companions.value.filter((item) => item.id !== record.id)];
      settings.value.selectedId = record.id;
      settings.value.enabled = true;
      persistSettings();
      return record;
    } catch (error) {
      lastError.value = String((error as { message?: string })?.message || error || '');
      throw error;
    } finally {
      saving.value = false;
    }
  };

  const updateCompanion = async (
    id: string,
    patch: Pick<CompanionPackageManifest, 'displayName' | 'description'>
  ): Promise<void> => {
    const target = companions.value.find((item) => item.id === id);
    if (!target) {
      return;
    }
    const record: CompanionPackageRecord = {
      ...target,
      displayName: String(patch.displayName || '').trim() || target.displayName,
      description: String(patch.description || '').trim(),
      updatedAt: Date.now()
    };
    await saveStoredCompanion(record);
    companions.value = companions.value.map((item) => (item.id === id ? record : item));
  };

  const exportPackage = async (id: string): Promise<void> => {
    const target = companions.value.find((item) => item.id === id);
    if (!target) {
      return;
    }
    const blob = buildCompanionPackageBlob(target, target.spritesheetDataUrl);
    downloadBlob(blob, buildCompanionPackageFilename(target.id));
  };

  const removeCompanion = async (id: string): Promise<void> => {
    await removeStoredCompanion(id);
    companions.value = companions.value.filter((item) => item.id !== id);
    if (settings.value.selectedId === id) {
      settings.value.selectedId = companions.value[0]?.id || '';
      settings.value.enabled = Boolean(settings.value.selectedId) && settings.value.enabled;
    }
    persistSettings();
  };

  const selectCompanion = (id: string): void => {
    if (!companions.value.some((item) => item.id === id)) {
      return;
    }
    settings.value.selectedId = id;
    persistSettings();
  };

  const setEnabled = (value: boolean): void => {
    settings.value.enabled = value === true;
    persistSettings();
  };

  const setPosition = (position: CompanionPosition): void => {
    settings.value.position = normalizePosition(position);
    persistSettings();
  };

  const setScale = (value: number): void => {
    settings.value.scale = Math.min(1.6, Math.max(0.7, normalizeNumber(value, DEFAULT_SCALE)));
    persistSettings();
  };

  const setMessageHintsEnabled = (value: boolean): void => {
    settings.value.messageHintsEnabled = value === true;
    persistSettings();
  };

  const showMessage = (
    text: string,
    options: { kind?: 'info' | 'success' | 'warning'; durationMs?: number } = {}
  ): void => {
    const cleaned = String(text || '').trim();
    if (!cleaned) {
      return;
    }
    message.value = {
      text: cleaned,
      kind: options.kind === 'success' || options.kind === 'warning' ? options.kind : 'info',
      visibleUntil: Date.now() + Math.max(1200, Math.min(8000, Number(options.durationMs || 2600)))
    };
  };

  const clearMessage = (): void => {
    message.value = null;
  };

  return {
    companions,
    enabled,
    featureEnabled,
    globalCompanions,
    hydrated,
    lastError,
    loading,
    message,
    saving,
    selectedCompanion,
    settings,
    clearMessage,
    exportPackage,
    hydrate,
    importPackage,
    findCompanion,
    loadGlobalCompanions,
    removeCompanion,
    selectCompanion,
    setEnabled,
    setMessageHintsEnabled,
    setPosition,
    setScale,
    showMessage,
    updateCompanion
  };
});
