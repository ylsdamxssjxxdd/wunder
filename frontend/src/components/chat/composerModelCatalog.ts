/**
 * Composer model catalog (B4 §8.3) — cache + request layer.
 *
 * `GET /wunder/agents/models` currently answers the legacy user view
 * (`items: string[]` + `default_model_name`). The frozen A3 contract widens it to
 * `items: [{ id, name, context, source, is_default }]` plus
 * `default_model_name` / `user_default_model_name`. Normalization (and every
 * graceful-degradation rule) lives in `composerModelCatalogModel.ts`.
 *
 * The catalog is a bounded module-level cache: one in-flight request at a time,
 * 5 minute TTL, at most `COMPOSER_MODEL_MAX_ITEMS` rows. Typing in the composer
 * never rebuilds it.
 */

import { ref } from 'vue';

import { listAgentModels } from '@/api/agents';
import {
  buildComposerModelSnapshot,
  emptyComposerModelCatalog,
  type ComposerModelCatalogSnapshot,
  type ComposerModelOption
} from '@/components/chat/composerModelCatalogModel';

export type { ComposerModelCatalogSnapshot, ComposerModelOption };
export { COMPOSER_MODEL_MAX_ITEMS } from '@/components/chat/composerModelCatalogModel';

const CATALOG_TTL_MS = 5 * 60 * 1000;

export const composerModelCatalog = ref<ComposerModelCatalogSnapshot>(
  emptyComposerModelCatalog()
);

let inflight: Promise<ComposerModelCatalogSnapshot> | null = null;

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};

export const invalidateComposerModelCatalog = (): void => {
  composerModelCatalog.value = emptyComposerModelCatalog();
};

/** Load the catalog once per TTL window; concurrent callers share one request. */
export const ensureComposerModelCatalog = async (
  options: { force?: boolean } = {}
): Promise<ComposerModelCatalogSnapshot> => {
  const current = composerModelCatalog.value;
  const isFresh = current.loaded && Date.now() - current.updatedAt < CATALOG_TTL_MS;
  if (!options.force && isFresh) return current;
  if (inflight) return inflight;
  composerModelCatalog.value = { ...current, loading: true };
  inflight = (async () => {
    try {
      const response = await listAgentModels();
      const payload = asRecord(asRecord(response?.data)?.data);
      const snapshot = buildComposerModelSnapshot(payload);
      composerModelCatalog.value = snapshot;
      return snapshot;
    } catch {
      const failed: ComposerModelCatalogSnapshot = {
        ...composerModelCatalog.value,
        loading: false,
        loaded: false,
        failed: true
      };
      composerModelCatalog.value = failed;
      return failed;
    } finally {
      inflight = null;
    }
  })();
  return inflight;
};
