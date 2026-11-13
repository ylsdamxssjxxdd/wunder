import { defineStore } from 'pinia';

import { fetchSystemAnnouncement, type SystemAnnouncement } from '@/api/announcement';

const DISMISSED_STORAGE_KEY = 'wunder:system-announcement:dismissed';

let loadInFlight: Promise<void> | null = null;
let loadGeneration = 0;

const readDismissedRevision = (): string => {
  if (typeof window === 'undefined') return '';
  try {
    return String(window.localStorage.getItem(DISMISSED_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
};

const normalizeAnnouncement = (value: unknown): SystemAnnouncement | null => {
  const payload = value as Partial<SystemAnnouncement> | null | undefined;
  if (!payload || typeof payload !== 'object') return null;
  const revision = String(payload.revision || '').trim();
  const content = String(payload.content || '');
  if (!revision || !content.trim()) return null;
  return {
    revision,
    title: String(payload.title || ''),
    content,
    updated_at: String(payload.updated_at || '')
  };
};

export const useSystemAnnouncementStore = defineStore('systemAnnouncement', {
  state: () => ({
    record: null as SystemAnnouncement | null,
    loading: false,
    loaded: false,
    dismissedRevision: readDismissedRevision(),
    promptedForUserId: ''
  }),
  getters: {
    unread(state): boolean {
      return Boolean(state.record) && state.record?.revision !== state.dismissedRevision;
    }
  },
  actions: {
    async load(force = false) {
      if (this.loaded && !force) return;
      if (loadInFlight) return loadInFlight;
      this.loading = true;
      const generation = loadGeneration;
      const task = (async () => {
        try {
          const { data } = await fetchSystemAnnouncement();
          if (generation !== loadGeneration) return;
          this.record = normalizeAnnouncement(data?.announcement);
          this.loaded = true;
        } catch {
          // 公告是增强信息，拉取失败不打扰用户；下次上线或点铃铛时再试。
        } finally {
          if (generation === loadGeneration) {
            this.loading = false;
            loadInFlight = null;
          }
        }
      })();
      loadInFlight = task;
      return task;
    },
    dismiss() {
      const revision = String(this.record?.revision || '').trim();
      if (!revision) return;
      this.dismissedRevision = revision;
      if (typeof window === 'undefined') return;
      try {
        window.localStorage.setItem(DISMISSED_STORAGE_KEY, revision);
      } catch {
        // 本地存储不可用时只影响跨会话去重。
      }
    },
    resetForLogout() {
      loadGeneration += 1;
      loadInFlight = null;
      this.record = null;
      this.loading = false;
      this.loaded = false;
      this.promptedForUserId = '';
      this.dismissedRevision = readDismissedRevision();
    }
  }
});
