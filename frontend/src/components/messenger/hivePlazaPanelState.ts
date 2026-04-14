import type { BeeroomGroup } from '@/stores/beeroom';
import type { PlazaItem } from '@/stores/plaza';

export type PlazaBrowseKind = 'hive_pack' | 'worker_card' | 'skill_pack';

const DEFAULT_AGENT_ALIASES = new Set(['default', '__default__']);
const DEFAULT_HIVE_ID = 'default';

const normalizeText = (value: unknown): string => String(value || '').trim();
const normalizeLower = (value: unknown): string => normalizeText(value).toLowerCase();

export const normalizePlazaBrowseKind = (value: unknown): PlazaBrowseKind => {
  const normalized = normalizeLower(value);
  if (normalized === 'worker_card' || normalized === 'skill_pack') {
    return normalized;
  }
  return 'hive_pack';
};

export const isDefaultAgentAlias = (value: unknown): boolean => DEFAULT_AGENT_ALIASES.has(normalizeLower(value));

export const isPublishableOwnedAgent = (value: { id?: unknown } | null | undefined): boolean => {
  const agentId = normalizeText(value?.id);
  return Boolean(agentId) && !isDefaultAgentAlias(agentId);
};

export const isDefaultBeeroomGroup = (
  value: Pick<BeeroomGroup, 'group_id' | 'hive_id' | 'is_default'> | null | undefined
): boolean => {
  if (!value) return false;
  if (value.is_default === true) return true;
  const groupId = normalizeLower(value.group_id || value.hive_id);
  return groupId === DEFAULT_HIVE_ID;
};

export const isPublishableBeeroomGroup = (value: BeeroomGroup | null | undefined): boolean => {
  const groupId = normalizeText(value?.group_id || value?.hive_id);
  return Boolean(groupId) && !isDefaultBeeroomGroup(value);
};

export const filterPlazaItemsByKeyword = (
  items: PlazaItem[] | null | undefined,
  keyword: string
): PlazaItem[] => {
  const text = normalizeLower(keyword);
  return (Array.isArray(items) ? items : []).filter((item) => {
    if (!text) {
      return true;
    }
    const title = normalizeLower(item?.title);
    const summary = normalizeLower(item?.summary);
    const owner = normalizeLower(item?.owner_username || item?.owner_user_id);
    const sourceKey = normalizeLower(item?.source_key);
    const tags = Array.isArray(item?.tags) ? item.tags.map((tag) => normalizeText(tag)).join(' ').toLowerCase() : '';
    return (
      title.includes(text) ||
      summary.includes(text) ||
      owner.includes(text) ||
      sourceKey.includes(text) ||
      tags.includes(text)
    );
  });
};

export const filterPlazaItemsByKindAndKeyword = (
  items: PlazaItem[] | null | undefined,
  kind: PlazaBrowseKind,
  keyword: string
): PlazaItem[] => {
  const normalizedKind = normalizePlazaBrowseKind(kind);
  return filterPlazaItemsByKeyword(
    (Array.isArray(items) ? items : []).filter((item) => normalizePlazaBrowseKind(item?.kind) === normalizedKind),
    keyword
  );
};

export const resolvePlazaPageCount = (totalItems: number, pageSize: number): number => {
  const normalizedSize = Math.max(1, Math.floor(Number(pageSize) || 0));
  const total = Math.max(0, Math.floor(Number(totalItems) || 0));
  return Math.max(1, Math.ceil(total / normalizedSize));
};

export const clampPlazaPage = (page: number, totalItems: number, pageSize: number): number => {
  const totalPages = resolvePlazaPageCount(totalItems, pageSize);
  const normalizedPage = Math.max(1, Math.floor(Number(page) || 0));
  return Math.min(normalizedPage, totalPages);
};

export const paginatePlazaItems = (
  items: PlazaItem[] | null | undefined,
  page: number,
  pageSize: number
): PlazaItem[] => {
  const list = Array.isArray(items) ? items : [];
  const normalizedSize = Math.max(1, Math.floor(Number(pageSize) || 0));
  const normalizedPage = clampPlazaPage(page, list.length, normalizedSize);
  const start = (normalizedPage - 1) * normalizedSize;
  return list.slice(start, start + normalizedSize);
};

export const resolveRetainedSelectedPlazaItemId = (
  items: PlazaItem[] | null | undefined,
  selectedItemId: unknown
): string => {
  const normalizedSelectedId = normalizeText(selectedItemId);
  if (!normalizedSelectedId) {
    return '';
  }
  return (Array.isArray(items) ? items : []).some((item) => normalizeText(item?.item_id) === normalizedSelectedId)
    ? normalizedSelectedId
    : '';
};
