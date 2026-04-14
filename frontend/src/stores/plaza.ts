import { defineStore } from 'pinia';

import { deletePlazaItem, getPlazaItem, importPlazaItem, listPlazaItems, publishPlazaItem } from '@/api/plaza';

export type PlazaItem = {
  item_id: string;
  kind: 'hive_pack' | 'worker_card' | 'skill_pack' | string;
  title: string;
  summary?: string;
  icon?: unknown;
  owner_user_id: string;
  owner_username: string;
  source_key: string;
  artifact_filename?: string;
  artifact_size_bytes?: number;
  source_updated_at?: number;
  source_signature?: string;
  freshness_status?: 'current' | 'outdated' | 'source_missing' | string;
  tags?: string[];
  metadata?: Record<string, unknown> | null;
  created_at?: number;
  updated_at?: number;
  mine?: boolean;
};

const normalizeItem = (value: unknown): PlazaItem | null => {
  if (!value || typeof value !== 'object') return null;
  const source = value as Record<string, unknown>;
  const itemId = String(source.item_id || '').trim();
  if (!itemId) return null;
  return {
    item_id: itemId,
    kind: String(source.kind || '').trim() || 'worker_card',
    title: String(source.title || '').trim() || itemId,
    summary: String(source.summary || '').trim() || undefined,
    icon: source.icon,
    owner_user_id: String(source.owner_user_id || '').trim(),
    owner_username: String(source.owner_username || '').trim(),
    source_key: String(source.source_key || '').trim(),
    artifact_filename: String(source.artifact_filename || '').trim() || undefined,
    artifact_size_bytes: Number(source.artifact_size_bytes || 0) || 0,
    source_updated_at: Number(source.source_updated_at || 0) || undefined,
    source_signature: String(source.source_signature || '').trim() || undefined,
    freshness_status: String(source.freshness_status || '').trim() || 'current',
    tags: Array.isArray(source.tags)
      ? source.tags.map((item) => String(item || '').trim()).filter(Boolean)
      : [],
    metadata:
      source.metadata && typeof source.metadata === 'object' && !Array.isArray(source.metadata)
        ? (source.metadata as Record<string, unknown>)
        : null,
    created_at: Number(source.created_at || 0) || undefined,
    updated_at: Number(source.updated_at || 0) || undefined,
    mine: source.mine === true
  };
};

export const usePlazaStore = defineStore('plaza', {
  state: () => ({
    items: [] as PlazaItem[],
    loading: false,
    publishing: false,
    importingItemId: '',
    deletingItemId: '',
    error: '',
    loadedAt: 0
  }),
  actions: {
    async loadItems(params: { force?: boolean; mineOnly?: boolean; kind?: string } = {}) {
      if (this.loading && !params.force) {
        return this.items;
      }
      this.loading = true;
      this.error = '';
      try {
        const { data } = await listPlazaItems({
          mine_only: params.mineOnly ? '1' : undefined,
          kind: params.kind || undefined
        });
        const rawItems = Array.isArray(data?.data?.items) ? data.data.items : [];
        const items = rawItems
          .map(normalizeItem)
          .filter(Boolean)
          .sort((left, right) => {
            const leftAt = Number((left as PlazaItem)?.updated_at || (left as PlazaItem)?.created_at || 0);
            const rightAt = Number((right as PlazaItem)?.updated_at || (right as PlazaItem)?.created_at || 0);
            return rightAt - leftAt;
          }) as PlazaItem[];
        this.items = items;
        this.loadedAt = Date.now();
        return items;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'load plaza failed');
        throw error;
      } finally {
        this.loading = false;
      }
    },

    async getItem(itemId: string) {
      const cleaned = String(itemId || '').trim();
      if (!cleaned) return null;
      const existing = this.items.find((item) => item.item_id === cleaned);
      if (existing) return existing;
      const { data } = await getPlazaItem(cleaned);
      return normalizeItem(data?.data || data?.data?.data || null);
    },

    async publishItem(payload: Record<string, unknown>) {
      this.publishing = true;
      this.error = '';
      try {
        const { data } = await publishPlazaItem(payload);
        const item = normalizeItem(data?.data || null);
        await this.loadItems({ force: true });
        return item;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'publish plaza failed');
        throw error;
      } finally {
        this.publishing = false;
      }
    },

    async importItem(itemId: string) {
      const cleaned = String(itemId || '').trim();
      if (!cleaned) return null;
      this.importingItemId = cleaned;
      this.error = '';
      try {
        const { data } = await importPlazaItem(cleaned);
        return data?.data || null;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'import plaza failed');
        throw error;
      } finally {
        if (this.importingItemId === cleaned) {
          this.importingItemId = '';
        }
      }
    },

    async deleteItem(itemId: string) {
      const cleaned = String(itemId || '').trim();
      if (!cleaned) return null;
      this.deletingItemId = cleaned;
      this.error = '';
      try {
        const { data } = await deletePlazaItem(cleaned);
        await this.loadItems({ force: true });
        return data?.data || null;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'delete plaza failed');
        throw error;
      } finally {
        if (this.deletingItemId === cleaned) {
          this.deletingItemId = '';
        }
      }
    }
  }
});
