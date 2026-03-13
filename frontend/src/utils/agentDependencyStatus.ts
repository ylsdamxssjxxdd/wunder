import { collectAbilityNames } from '@/utils/toolSummary';

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

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
  const explicit = record.declared_tool_names ?? record.declaredToolNames;
  if (Array.isArray(explicit)) {
    return normalizeDependencyNames(explicit);
  }
  return normalizeDependencyNames(record.tool_names ?? record.toolNames);
};

const readDeclaredSkillNames = (source: unknown): string[] => {
  const record = asRecord(source);
  return normalizeDependencyNames(record.declared_skill_names ?? record.declaredSkillNames ?? record.skills);
};

const collectAvailableNames = (catalog: unknown) => {
  const payload = asRecord(catalog);
  const abilityNames = collectAbilityNames(payload);
  return {
    availableToolNames: new Set<string>([...abilityNames.tools, ...abilityNames.skills]),
    availableSkillNames: new Set<string>(abilityNames.skills)
  };
};

export const buildDeclaredDependencyPayload = (
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

export const resolveAgentDependencyStatus = (
  source: unknown,
  catalog: unknown,
  selectedToolNames?: unknown
) => {
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

