import { collectAbilityGroupDetails } from '@/utils/toolSummary';
import { resolveAbilityKind } from '@/utils/abilityVisuals';

const isSelectedAbilityDescriptor = (item: Record<string, unknown>) => item.selected !== false;

const normalizeAbilityNameList = (values: unknown): string[] => {
  if (!Array.isArray(values)) return [];
  const output: string[] = [];
  const seen = new Set<string>();
  values.forEach((item) => {
    const name = String(item || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push(name);
  });
  return output;
};

const normalizeAbilityDescriptors = (agent: Record<string, unknown> | null): Array<Record<string, unknown>> => {
  const source = agent || {};
  const abilities = source.abilities as Record<string, unknown> | null | undefined;
  const abilitySource = Array.isArray(source.ability_items)
    ? source.ability_items
    : Array.isArray(abilities?.items)
      ? abilities.items
      : [];
  return abilitySource.filter(
    (item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object' && !Array.isArray(item)
  );
};

const resolveSelectedAbilityNamesFromAgentProfile = (agent: Record<string, unknown> | null): string[] => {
  const abilitySource = normalizeAbilityDescriptors(agent);
  const output: string[] = [];
  const seen = new Set<string>();
  abilitySource.forEach((item) => {
    if (item.selected === false) return;
    const name = String(item.runtime_name || item.runtimeName || item.name || '').trim();
    if (!name || seen.has(name)) return;
    seen.add(name);
    output.push(name);
  });
  return output;
};

export const resolveAgentConfiguredAbilityNames = (agent: Record<string, unknown> | null): string[] => {
  const declared = normalizeAbilityNameList([
    ...normalizeAbilityNameList(agent?.declared_tool_names),
    ...normalizeAbilityNameList(agent?.declared_skill_names)
  ]);
  if (declared.length > 0) {
    return declared;
  }
  const selectedFromToolNames = normalizeAbilityNameList([
    ...normalizeAbilityNameList(agent?.tool_names),
    ...normalizeAbilityNameList(agent?.toolNames)
  ]);
  if (selectedFromToolNames.length > 0) {
    return selectedFromToolNames;
  }
  const selectedFromItems = resolveSelectedAbilityNamesFromAgentProfile(agent);
  if (selectedFromItems.length > 0) {
    return selectedFromItems;
  }
  return [];
};

const collectStructuredAbilityGroups = (agent: Record<string, unknown> | null) => {
  const items = normalizeAbilityDescriptors(agent).filter(isSelectedAbilityDescriptor);
  const normalizedItems = items.map((item) => {
    const name = String(item.runtime_name || item.runtimeName || item.name || '').trim();
    const description = String(item.description || item.desc || item.summary || '').trim();
    const group = String(item.group || '').trim();
    const source = String(item.source || '').trim();
    const kind = resolveAbilityKind(item.kind, group || source);
    return { name, description, group, source, kind };
  });
  return collectAbilityGroupDetails({
    items: normalizedItems
  });
};

export const resolveAgentOverviewAbilityCounts = (agent: Record<string, unknown> | null) => {
  const descriptors = normalizeAbilityDescriptors(agent);
  const hasStructuredDescriptors = descriptors.length > 0;
  const grouped = hasStructuredDescriptors
    ? collectStructuredAbilityGroups(agent)
    : collectAbilityGroupDetails((agent || {}) as Record<string, unknown>);
  const declaredSkillNames = normalizeAbilityNameList(agent?.declared_skill_names);
  return {
    skillCount: hasStructuredDescriptors || grouped.skills.length > 0 ? grouped.skills.length : declaredSkillNames.length,
    mcpCount: grouped.mcp.length
  };
};
