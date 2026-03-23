import { collectAbilityNames } from '@/utils/toolSummary';

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const hasOwn = (record: UnknownRecord, key: string) =>
  Object.prototype.hasOwnProperty.call(record, key);

const isPresetBoundSource = (source: unknown): boolean => {
  const record = asRecord(source);
  const binding = record.preset_binding ?? record.presetBinding;
  return Boolean(binding && typeof binding === 'object');
};

export const normalizeDependencyNames = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const output: string[] = [];
  value.forEach((item) => {
    const normalized = String(item || '').trim();
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    output.push(normalized);
  });
  return output;
};

const readDeclaredToolNames = (source: unknown): string[] => {
  const record = asRecord(source);
  return normalizeDependencyNames(record.declared_tool_names ?? record.declaredToolNames);
};

const readDeclaredSkillNames = (source: unknown): string[] => {
  const record = asRecord(source);
  return normalizeDependencyNames(record.declared_skill_names ?? record.declaredSkillNames ?? record.skills);
};

const hasExplicitDeclaredDependencies = (source: unknown): boolean => {
  if (isPresetBoundSource(source)) {
    return false;
  }
  const record = asRecord(source);
  if (
    !hasOwn(record, 'declared_tool_names') &&
    !hasOwn(record, 'declaredToolNames') &&
    !hasOwn(record, 'declared_skill_names') &&
    !hasOwn(record, 'declaredSkillNames')
  ) {
    return false;
  }
  const declaredToolNames = readDeclaredToolNames(source);
  const declaredSkillNames = readDeclaredSkillNames(source);
  if (declaredSkillNames.length > 0) {
    return true;
  }
  if (declaredToolNames.length === 0) {
    return false;
  }
  const selectedToolNames = normalizeDependencyNames(record.tool_names ?? record.toolNames);
  if (selectedToolNames.length === 0) {
    return true;
  }
  const selectedToolNameSet = new Set(selectedToolNames);
  return declaredToolNames.some((name) => !selectedToolNameSet.has(name));
};

const collectAvailableNames = (catalog: unknown) => {
  const payload = asRecord(catalog);
  const abilityNames = collectAbilityNames(payload);
  return {
    availableToolNames: new Set<string>([...abilityNames.tools, ...abilityNames.skills]),
    availableSkillNames: new Set<string>(abilityNames.skills)
  };
};

export const buildWorkerCardDependencyPayload = (
  selectedToolNames: unknown,
  source: unknown,
  catalog: unknown
) => {
  const selected = normalizeDependencyNames(selectedToolNames);
  const previousDeclaredToolNames = readDeclaredToolNames(source);
  const previousDeclaredSkillNames = readDeclaredSkillNames(source);
  const { availableToolNames, availableSkillNames } = collectAvailableNames(catalog);

  const selectedSkillNames = selected.filter((name) => availableSkillNames.has(name));
  const selectedNonSkillToolNames = selected.filter((name) => !availableSkillNames.has(name));
  const missingDeclaredToolNames = previousDeclaredToolNames.filter((name) => !availableToolNames.has(name));
  const missingDeclaredSkillNames = previousDeclaredSkillNames.filter((name) => !availableSkillNames.has(name));

  return {
    tool_names: selected,
    declared_tool_names: normalizeDependencyNames([
      ...selectedNonSkillToolNames,
      ...missingDeclaredToolNames
    ]),
    declared_skill_names: normalizeDependencyNames([
      ...selectedSkillNames,
      ...missingDeclaredSkillNames
    ])
  };
};

export const buildDeclaredDependencyPayload = (
  selectedToolNames: unknown,
  source: unknown,
  catalog: unknown
) => {
  const selected = normalizeDependencyNames(selectedToolNames);
  // Only worker-card style agents persist declared dependencies.
  // Regular agents should save their current selection without generating missing-dependency warnings.
  if (!hasExplicitDeclaredDependencies(source)) {
    return {
      tool_names: selected,
      declared_tool_names: [] as string[],
      declared_skill_names: [] as string[]
    };
  }
  return buildWorkerCardDependencyPayload(selectedToolNames, source, catalog);
};

export const resolveAgentDependencyStatus = (
  source: unknown,
  catalog: unknown,
  selectedToolNames?: unknown
) => {
  if (isPresetBoundSource(source)) {
    return {
      declaredToolNames: [] as string[],
      declaredSkillNames: [] as string[],
      missingToolNames: [] as string[],
      missingSkillNames: [] as string[]
    };
  }
  const effective = selectedToolNames === undefined
    ? {
        declared_tool_names: readDeclaredToolNames(source),
        declared_skill_names: readDeclaredSkillNames(source)
      }
    : buildDeclaredDependencyPayload(selectedToolNames, source, catalog);
  const { availableToolNames, availableSkillNames } = collectAvailableNames(catalog);

  return {
    declaredToolNames: effective.declared_tool_names,
    declaredSkillNames: effective.declared_skill_names,
    missingToolNames: effective.declared_tool_names.filter((name) => !availableToolNames.has(name)),
    missingSkillNames: effective.declared_skill_names.filter((name) => !availableSkillNames.has(name))
  };
};
