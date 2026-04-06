export type BeeroomGroupOption = {
  group_id: string;
  name?: string;
  description?: string;
  is_default?: boolean;
};

export type BeeroomGroupDraftMode = 'existing' | 'new';

export type BeeroomGroupDraft = {
  mode: BeeroomGroupDraftMode;
  hive_id: string;
  hive_name: string;
  hive_description: string;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeGroupOption = (value: unknown): BeeroomGroupOption | null => {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const source = value as Partial<BeeroomGroupOption>;
  const groupId = normalizeText(source.group_id);
  if (!groupId) {
    return null;
  }
  return {
    group_id: groupId,
    name: normalizeText(source.name || source.group_id),
    description: normalizeText(source.description),
    is_default: Boolean(source.is_default)
  };
};

const findBeeroomGroupOption = (
  groups: BeeroomGroupOption[] | null | undefined,
  hiveId: unknown
): BeeroomGroupOption | null => {
  const normalizedHiveId = normalizeText(hiveId);
  if (!normalizedHiveId) {
    return null;
  }
  const normalizedGroups = (Array.isArray(groups) ? groups : [])
    .map((group) => normalizeGroupOption(group))
    .filter((group): group is BeeroomGroupOption => Boolean(group));
  return normalizedGroups.find((group) => group.group_id === normalizedHiveId) || null;
};

export const createBeeroomGroupDraft = (defaultGroupId = ''): BeeroomGroupDraft => ({
  mode: 'existing',
  hive_id: normalizeText(defaultGroupId),
  hive_name: '',
  hive_description: ''
});

export const normalizeBeeroomGroupDraft = (
  input: Partial<BeeroomGroupDraft> | null | undefined,
  defaultGroupId = ''
): BeeroomGroupDraft => {
  const draft = input || {};
  const mode = draft.mode === 'new' ? 'new' : 'existing';
  const hiveId = normalizeText(draft.hive_id);
  const hiveName = normalizeText(draft.hive_name);
  const hiveDescription = normalizeText(draft.hive_description);
  return {
    mode,
    hive_id: hiveId,
    hive_name: hiveName,
    hive_description: hiveDescription
  };
};

export const buildBeeroomGroupPayload = (
  input: Partial<BeeroomGroupDraft> | null | undefined,
  groups: BeeroomGroupOption[] | null | undefined = []
) => {
  const draft = normalizeBeeroomGroupDraft(input);
  if (draft.mode === 'new' && draft.hive_name) {
    const payload: Record<string, string> = {
      hive_name: draft.hive_name,
      hive_description: draft.hive_description
    };
    const hiveId = normalizeText(draft.hive_id);
    if (hiveId) {
      payload.hive_id = hiveId;
    }
    return payload;
  }
  const hiveId = normalizeText(draft.hive_id);
  const matchedGroup = findBeeroomGroupOption(groups, hiveId);
  if (hiveId === 'default' || matchedGroup?.is_default) {
    return {
      hive_id: hiveId
    };
  }
  const hiveName = normalizeText(draft.hive_name || matchedGroup?.name);
  const hiveDescription = normalizeText(draft.hive_description || matchedGroup?.description);
  const payload: Record<string, string> = {
    hive_id: hiveId
  };
  if (hiveName) {
    payload.hive_name = hiveName;
  }
  if (hiveDescription) {
    payload.hive_description = hiveDescription;
  }
  return payload;
};

export const resolveBeeroomGroupDraftForAgent = (
  hiveId: unknown,
  groups: BeeroomGroupOption[] | null | undefined = [],
  defaultGroupId = ''
): BeeroomGroupDraft => {
  const resolvedHiveId = normalizeText(hiveId || defaultGroupId);
  const matchedGroup = findBeeroomGroupOption(groups, resolvedHiveId);
  return {
    mode: 'existing',
    hive_id: resolvedHiveId,
    hive_name: normalizeText(matchedGroup?.name),
    hive_description: normalizeText(matchedGroup?.description)
  };
};
