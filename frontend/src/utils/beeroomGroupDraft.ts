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
  if (mode === 'new') {
    return {
      mode,
      hive_id: hiveId,
      hive_name: hiveName,
      hive_description: hiveDescription
    };
  }
  return {
    mode,
    hive_id: hiveId,
    hive_name: '',
    hive_description: ''
  };
};

export const buildBeeroomGroupPayload = (input: Partial<BeeroomGroupDraft> | null | undefined) => {
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
  return {
    hive_id: normalizeText(draft.hive_id)
  };
};

export const resolveBeeroomGroupDraftForAgent = (
  hiveId: unknown,
  defaultGroupId = ''
): BeeroomGroupDraft => ({
  mode: 'existing',
  hive_id: normalizeText(hiveId || defaultGroupId),
  hive_name: '',
  hive_description: ''
});
