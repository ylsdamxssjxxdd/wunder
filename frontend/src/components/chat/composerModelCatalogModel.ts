/**
 * Pure model-catalog normalization (B4 §8.3).
 *
 * Kept free of imports so the degradation rules can be unit-tested directly:
 * the legacy user view (`items: string[]`) and the frozen A3 contract
 * (`items: [{ id, name, context, source, is_default }]`) both normalize here,
 * and every field the server has not delivered yet degrades to "unknown"
 * instead of a fabricated value.
 */

export type ComposerModelOption = {
  /** Stable config key used for switching. */
  id: string;
  /** Display label; equals `id` when the server only sends names. */
  name: string;
  context: number | null;
  source: string;
  isDefault: boolean;
};

export type ComposerModelCatalogSnapshot = {
  loading: boolean;
  loaded: boolean;
  failed: boolean;
  items: ComposerModelOption[];
  defaultModelName: string;
  userDefaultModelName: string;
  /** True only when the server advertised `user_default_model_name`. */
  supportsUserDefault: boolean;
  updatedAt: number;
};

export const COMPOSER_MODEL_MAX_ITEMS = 200;

export const emptyComposerModelCatalog = (): ComposerModelCatalogSnapshot => ({
  loading: false,
  loaded: false,
  failed: false,
  items: [],
  defaultModelName: '',
  userDefaultModelName: '',
  supportsUserDefault: false,
  updatedAt: 0
});

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};

const normalizeContext = (value: unknown): number | null => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return Math.round(parsed);
};

const normalizeModelItem = (raw: unknown, defaultModelName: string): ComposerModelOption | null => {
  if (typeof raw === 'string') {
    const name = raw.trim();
    if (!name) return null;
    return {
      id: name,
      name,
      context: null,
      source: '',
      isDefault: Boolean(defaultModelName) && name.toLowerCase() === defaultModelName.toLowerCase()
    };
  }
  const record = asRecord(raw);
  const id = String(record.id ?? record.model_name ?? record.modelName ?? '').trim();
  const name = String(record.name ?? record.label ?? '').trim() || id;
  if (!id && !name) return null;
  const isDefaultFlag = record.is_default ?? record.isDefault;
  return {
    id: id || name,
    name: name || id,
    context: normalizeContext(record.context ?? record.max_context ?? record.maxContext),
    source: String(record.source ?? '').trim().toLowerCase(),
    isDefault:
      isDefaultFlag === undefined || isDefaultFlag === null
        ? Boolean(defaultModelName) && (id || name).toLowerCase() === defaultModelName.toLowerCase()
        : isDefaultFlag === true
  };
};

export const normalizeComposerModelItems = (
  value: unknown,
  defaultModelName: string
): ComposerModelOption[] => {
  if (!Array.isArray(value)) return [];
  const items: ComposerModelOption[] = [];
  const seen = new Set<string>();
  value.forEach((entry) => {
    if (items.length >= COMPOSER_MODEL_MAX_ITEMS) return;
    const option = normalizeModelItem(entry, defaultModelName);
    if (!option) return;
    const key = option.id.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    items.push(option);
  });
  return items;
};

/** Build a catalog snapshot from the `GET /wunder/agents/models` payload. */
export const buildComposerModelSnapshot = (
  payload: unknown
): ComposerModelCatalogSnapshot => {
  const record = asRecord(payload);
  const defaultModelName = String(record.default_model_name ?? '').trim();
  const supportsUserDefault = Object.prototype.hasOwnProperty.call(
    record,
    'user_default_model_name'
  );
  return {
    loading: false,
    loaded: true,
    failed: false,
    items: normalizeComposerModelItems(record.items, defaultModelName),
    defaultModelName,
    userDefaultModelName: supportsUserDefault
      ? String(record.user_default_model_name ?? '').trim()
      : '',
    supportsUserDefault,
    updatedAt: Date.now()
  };
};
