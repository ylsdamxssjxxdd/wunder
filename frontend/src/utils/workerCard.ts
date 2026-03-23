import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { normalizeDependencyNames } from '@/utils/agentDependencyStatus';

type WorkerCardSchemaVersion = 'wunder/worker-card@1' | 'wunder/worker-card@2';
type WorkerCardAbilityKind = 'tool' | 'skill';
type UnknownRecord = Record<string, unknown>;

export type WorkerCardAbilityItem = {
  id: string;
  name: string;
  runtime_name: string;
  display_name: string;
  description: string;
  kind: WorkerCardAbilityKind;
};

type AgentAbilityDescriptor = {
  id: string;
  name: string;
  runtime_name: string;
  display_name: string;
  description: string;
  input_schema: Record<string, unknown>;
  group: 'builtin' | 'skills';
  source: 'builtin' | 'skill';
  kind: WorkerCardAbilityKind;
  available: boolean;
  selected: boolean;
};

export type WorkerCardDocument = {
  schema_version: WorkerCardSchemaVersion;
  kind: 'WorkerCard';
  metadata: {
    id: string;
    name: string;
    description: string;
    icon: string;
    exported_at: string;
  };
  prompt?: {
    extra_prompt?: string;
    system_prompt?: string;
  };
  extra_prompt?: string;
  system_prompt?: string;
  abilities: {
    items?: WorkerCardAbilityItem[];
    tool_names: string[];
    skills: string[];
  };
  interaction: {
    preset_questions: string[];
  };
  runtime: {
    model_name?: string;
    approval_mode: 'suggest' | 'auto_edit' | 'full_auto';
    sandbox_container_id: number;
    is_shared: boolean;
  };
  hive: {
    id: string;
    name: string;
    description: string;
  };
  extensions: Record<string, unknown>;
};

export type WorkerCardBundleDocument = {
  schema_version: 'wunder/worker-card-bundle@1';
  kind: 'WorkerCardBundle';
  items: WorkerCardDocument[];
};

const WORKER_CARD_SCHEMA_VERSION = 'wunder/worker-card@2' as const;
const WORKER_CARD_BUNDLE_SCHEMA_VERSION = 'wunder/worker-card-bundle@1' as const;
const WORKER_CARD_SCHEMA_VERSIONS = new Set<WorkerCardSchemaVersion>([
  'wunder/worker-card@1',
  'wunder/worker-card@2'
]);
const APPROVAL_MODES = new Set(['suggest', 'auto_edit', 'full_auto']);

const trimString = (value: unknown) => String(value || '').trim();

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const hasOwn = (value: unknown, key: string) =>
  Object.prototype.hasOwnProperty.call(asRecord(value), key);

const normalizeStringList = (value: unknown): string[] => normalizeDependencyNames(value);

const normalizeApprovalMode = (value: unknown): 'suggest' | 'auto_edit' | 'full_auto' => {
  const normalized = trimString(value);
  return APPROVAL_MODES.has(normalized) ? (normalized as 'suggest' | 'auto_edit' | 'full_auto') : 'full_auto';
};

const normalizeOptionalModelName = (value: unknown): string | undefined => {
  const normalized = trimString(value);
  return normalized || undefined;
};

const normalizeSandboxContainerId = (value: unknown): number => {
  const numeric = Number(value);
  if (Number.isInteger(numeric) && numeric >= 1 && numeric <= 10) {
    return numeric;
  }
  return 1;
};

const sanitizeFilenamePart = (value: unknown): string => {
  const normalized = trimString(value).replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_');
  return normalized || 'worker-card';
};

const sanitizeOptionalFilenamePart = (value: unknown): string => {
  const raw = trimString(value);
  return raw ? sanitizeFilenamePart(raw) : '';
};

const buildWorkerCardFilename = (displayName: unknown, stableId: unknown): string => {
  const namePart = sanitizeOptionalFilenamePart(displayName);
  const idPart = sanitizeOptionalFilenamePart(stableId);
  const stem =
    namePart && idPart && namePart !== idPart
      ? `${namePart}--${idPart}`
      : namePart || idPart || 'worker-card';
  return `${stem}.worker-card.json`;
};

const joinPromptSections = (...parts: unknown[]): string =>
  parts
    .map((item) => trimString(item))
    .filter(Boolean)
    .join('\n\n');

const resolveWorkerCardPromptText = (value: unknown): string => {
  const source = asRecord(value);
  const prompt = asRecord(source.prompt);
  return joinPromptSections(
    source.system_prompt ?? prompt.system_prompt,
    source.extra_prompt ?? prompt.extra_prompt
  );
};

const createDownload = (filename: string, payload: string) => {
  if (typeof window === 'undefined' || typeof document === 'undefined') return;
  const blob = new Blob(['\uFEFF', payload], { type: 'application/json;charset=utf-8' });
  const url = window.URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  window.URL.revokeObjectURL(url);
};

const normalizeAbilityKind = (value: unknown): WorkerCardAbilityKind =>
  trimString(value).toLowerCase() === 'skill' ? 'skill' : 'tool';

const normalizeWorkerCardAbilityItems = (value: unknown): WorkerCardAbilityItem[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  const items: WorkerCardAbilityItem[] = [];
  const seen = new Set<string>();
  value.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    const source = asRecord(item);
    const kind = normalizeAbilityKind(source.kind);
    const runtimeName = trimString(
      source.runtime_name ?? source.runtimeName ?? source.name ?? source.id
    );
    if (!runtimeName) return;
    const key = `${kind}:${runtimeName}`;
    if (seen.has(key)) return;
    seen.add(key);
    const displayName = trimString(source.display_name ?? source.displayName ?? runtimeName);
    const description = trimString(source.description);
    items.push({
      id: trimString(source.id) || key,
      name: trimString(source.name) || runtimeName,
      runtime_name: runtimeName,
      display_name: displayName || runtimeName,
      description,
      kind
    });
  });
  return items;
};

const buildWorkerCardAbilityItems = (
  toolNames: string[],
  skillNames: string[]
): WorkerCardAbilityItem[] => [
  ...toolNames.map((name) => ({
    id: `tool:${name}`,
    name,
    runtime_name: name,
    display_name: name,
    description: '',
    kind: 'tool' as const
  })),
  ...skillNames.map((name) => ({
    id: `skill:${name}`,
    name,
    runtime_name: name,
    display_name: name,
    description: '',
    kind: 'skill' as const
  }))
];

const buildMergedWorkerCardAbilityItems = (
  items: WorkerCardAbilityItem[],
  toolNames: string[],
  skillNames: string[]
): WorkerCardAbilityItem[] => {
  const expected = buildWorkerCardAbilityItems(toolNames, skillNames);
  if (!items.length) {
    return expected;
  }
  const byKey = new Map<string, WorkerCardAbilityItem>();
  items.forEach((item) => {
    const runtimeName = trimString(item.runtime_name || item.name);
    if (!runtimeName) return;
    byKey.set(`${item.kind}:${runtimeName}`, item);
  });
  return expected.map((item) => byKey.get(`${item.kind}:${item.runtime_name}`) || item);
};

const isLegacyEquivalentWorkerCardItem = (
  item: WorkerCardAbilityItem,
  expected: WorkerCardAbilityItem
): boolean => {
  const runtimeName = trimString(item.runtime_name || item.name);
  const name = trimString(item.name || runtimeName);
  const displayName = trimString(item.display_name || runtimeName);
  const description = trimString(item.description);
  return (
    runtimeName === expected.runtime_name &&
    normalizeAbilityKind(item.kind) === expected.kind &&
    name === expected.name &&
    displayName === expected.display_name &&
    !description
  );
};

const shouldEmitWorkerCardItems = (
  items: WorkerCardAbilityItem[],
  toolNames: string[],
  skillNames: string[]
): boolean => {
  if (!items.length) {
    return false;
  }
  const expected = buildWorkerCardAbilityItems(toolNames, skillNames);
  if (items.length !== expected.length) {
    return true;
  }
  return items.some((item, index) => !isLegacyEquivalentWorkerCardItem(item, expected[index]));
};

const buildWorkerCardAbilitiesPayload = (
  items: WorkerCardAbilityItem[],
  toolNames: string[],
  skillNames: string[]
) => ({
  items: shouldEmitWorkerCardItems(items, toolNames, skillNames) ? items : undefined,
  tool_names: toolNames,
  skills: skillNames
});

const normalizeWorkerCardAbilities = (value: unknown) => {
  const source = asRecord(value);
  const hasLegacyToolNames = hasOwn(source, 'tool_names') || hasOwn(source, 'toolNames');
  const hasLegacySkills = hasOwn(source, 'skills');
  const toolNames = normalizeStringList(source.tool_names ?? source.toolNames);
  const skillNames = normalizeStringList(source.skills);
  if (hasLegacyToolNames || hasLegacySkills) {
    return {
      tool_names: toolNames,
      skills: skillNames,
      items: buildWorkerCardAbilityItems(toolNames, skillNames)
    };
  }
  const items = normalizeWorkerCardAbilityItems(source.items);
  return {
    tool_names: items.filter((item) => item.kind !== 'skill').map((item) => item.runtime_name),
    skills: items.filter((item) => item.kind === 'skill').map((item) => item.runtime_name),
    items
  };
};

const normalizeAgentAbilityItems = (value: unknown): AgentAbilityDescriptor[] =>
  normalizeWorkerCardAbilityItems(value).map((item) => ({
    id: item.id,
    name: item.name,
    runtime_name: item.runtime_name,
    display_name: item.display_name,
    description: item.description,
    input_schema: {},
    group: item.kind === 'skill' ? 'skills' : 'builtin',
    source: item.kind === 'skill' ? 'skill' : 'builtin',
    kind: item.kind,
    available: true,
    selected: true
  }));

export const buildWorkerCardDocument = (
  value: Record<string, unknown> | null | undefined
): WorkerCardDocument => {
  const source = value || {};
  const sourceAbilityItems =
    source.ability_items ?? source.abilityItems ?? asRecord(source.abilities).items;
  const explicitDeclaredToolNames = normalizeStringList(source.declared_tool_names ?? source.declaredToolNames);
  const explicitDeclaredSkillNames = normalizeStringList(
    source.declared_skill_names ?? source.declaredSkillNames ?? source.skills
  );
  const sourceAbilities = normalizeWorkerCardAbilities(
    hasOwn(source, 'abilities') ? source.abilities : { items: sourceAbilityItems }
  );
  const hasExplicitDeclaredDependencies =
    explicitDeclaredToolNames.length > 0 ||
    explicitDeclaredSkillNames.length > 0 ||
    hasOwn(source, 'declared_tool_names') ||
    hasOwn(source, 'declaredToolNames') ||
    hasOwn(source, 'declared_skill_names') ||
    hasOwn(source, 'declaredSkillNames');
  const declaredToolNames = hasExplicitDeclaredDependencies
    ? explicitDeclaredToolNames
    : sourceAbilities.tool_names.length > 0
      ? sourceAbilities.tool_names
      : normalizeStringList(source.tool_names);
  const declaredSkillNames = hasExplicitDeclaredDependencies
    ? explicitDeclaredSkillNames
    : sourceAbilities.skills;
  const abilityItems = buildMergedWorkerCardAbilityItems(
    sourceAbilities.items,
    declaredToolNames,
    declaredSkillNames
  );
  return {
    schema_version: WORKER_CARD_SCHEMA_VERSION,
    kind: 'WorkerCard',
    metadata: {
      id: trimString(source.id),
      name: trimString(source.name),
      description: trimString(source.description),
      icon: trimString(source.icon),
      exported_at: new Date().toISOString()
    },
    extra_prompt: resolveWorkerCardPromptText(source) || undefined,
    abilities: buildWorkerCardAbilitiesPayload(abilityItems, declaredToolNames, declaredSkillNames),
    interaction: {
      preset_questions: normalizeAgentPresetQuestions(source.preset_questions)
    },
    runtime: {
      model_name: normalizeOptionalModelName(source.model_name),
      approval_mode: normalizeApprovalMode(source.approval_mode),
      sandbox_container_id: normalizeSandboxContainerId(source.sandbox_container_id),
      is_shared: Boolean(source.is_shared)
    },
    hive: {
      id: trimString(source.hive_id),
      name: trimString(source.hive_name),
      description: trimString(source.hive_description)
    },
    extensions: {}
  };
};

export const downloadWorkerCard = (value: Record<string, unknown> | null | undefined) => {
  const document = buildWorkerCardDocument(value);
  return downloadWorkerCardDocument(document);
};

const downloadWorkerCardDocument = (document: WorkerCardDocument) => {
  const filename = buildWorkerCardFilename(document.metadata.name, document.metadata.id);
  createDownload(filename, JSON.stringify(document, null, 2));
  return filename;
};

export const downloadWorkerCardBundle = (items: Array<Record<string, unknown> | null | undefined>) => {
  const documents = items.map((item) => buildWorkerCardDocument(item));
  if (!documents.length) {
    throw new Error('worker card bundle is empty');
  }
  if (documents.length === 1) {
    return downloadWorkerCardDocument(documents[0]);
  }
  const bundle: WorkerCardBundleDocument = {
    schema_version: WORKER_CARD_BUNDLE_SCHEMA_VERSION,
    kind: 'WorkerCardBundle',
    items: documents
  };
  const filename = `worker-cards-${new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)}.json`;
  createDownload(filename, JSON.stringify(bundle, null, 2));
  return filename;
};

const normalizeWorkerCardDocument = (
  value: Record<string, unknown> | null | undefined
): WorkerCardDocument => {
  const source = value || {};
  const metadata = asRecord(source.metadata);
  const abilities = normalizeWorkerCardAbilities(source.abilities);
  const interaction = asRecord(source.interaction);
  const runtime = asRecord(source.runtime);
  const hive = asRecord(source.hive);
  const schemaVersion = trimString(source.schema_version || source.schemaVersion);
  if (schemaVersion && !WORKER_CARD_SCHEMA_VERSIONS.has(schemaVersion as WorkerCardSchemaVersion)) {
    throw new Error('unsupported worker card schema version');
  }
  const normalized: WorkerCardDocument = {
    schema_version: WORKER_CARD_SCHEMA_VERSION,
    kind: 'WorkerCard',
    metadata: {
      id: trimString(metadata.id),
      name: trimString(metadata.name),
      description: trimString(metadata.description),
      icon: trimString(metadata.icon),
      exported_at: trimString(metadata.exported_at) || new Date().toISOString()
    },
    extra_prompt: resolveWorkerCardPromptText(source) || undefined,
    abilities: buildWorkerCardAbilitiesPayload(
      buildMergedWorkerCardAbilityItems(abilities.items, abilities.tool_names, abilities.skills),
      abilities.tool_names,
      abilities.skills
    ),
    interaction: {
      preset_questions: normalizeAgentPresetQuestions(interaction.preset_questions)
    },
    runtime: {
      model_name: normalizeOptionalModelName(runtime.model_name),
      approval_mode: normalizeApprovalMode(runtime.approval_mode),
      sandbox_container_id: normalizeSandboxContainerId(runtime.sandbox_container_id),
      is_shared: Boolean(runtime.is_shared)
    },
    hive: {
      id: trimString(hive.id),
      name: trimString(hive.name),
      description: trimString(hive.description)
    },
    extensions: asRecord(source.extensions)
  };
  if (!normalized.metadata.name) {
    throw new Error('worker card name is required');
  }
  return normalized;
};

export const parseWorkerCardText = (raw: string): WorkerCardDocument[] => {
  const text = String(raw || '').trim();
  if (!text) {
    throw new Error('worker card file is empty');
  }
  const parsed = JSON.parse(text) as unknown;
  if (Array.isArray(parsed)) {
    return parsed.map((item) => normalizeWorkerCardDocument(item as Record<string, unknown>));
  }
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('invalid worker card payload');
  }
  const document = parsed as Record<string, unknown>;
  const kind = trimString(document.kind);
  if (kind === 'WorkerCardBundle') {
    const items = Array.isArray(document.items) ? document.items : [];
    const normalizedItems = items.map((item) => normalizeWorkerCardDocument(item as Record<string, unknown>));
    if (!normalizedItems.length) {
      throw new Error('worker card bundle is empty');
    }
    return normalizedItems;
  }
  return [normalizeWorkerCardDocument(document)];
};

export const workerCardToAgentPayload = (document: WorkerCardDocument): Record<string, unknown> => {
  const abilities = normalizeWorkerCardAbilities(document.abilities);
  const declaredToolNames = normalizeStringList(abilities.tool_names);
  const declaredSkillNames = normalizeStringList(abilities.skills);
  const abilityItems = normalizeAgentAbilityItems(
    buildMergedWorkerCardAbilityItems(abilities.items, declaredToolNames, declaredSkillNames)
  );
  const payload: Record<string, unknown> = {
    name: trimString(document.metadata.name),
    description: trimString(document.metadata.description),
    icon: trimString(document.metadata.icon),
    system_prompt: resolveWorkerCardPromptText(document),
    ability_items: abilityItems,
    abilities: {
      items: abilityItems
    },
    tool_names: normalizeStringList([...declaredToolNames, ...declaredSkillNames]),
    declared_tool_names: declaredToolNames,
    declared_skill_names: declaredSkillNames,
    preset_questions: normalizeAgentPresetQuestions(document.interaction.preset_questions),
    model_name: normalizeOptionalModelName(document.runtime.model_name),
    approval_mode: normalizeApprovalMode(document.runtime.approval_mode),
    sandbox_container_id: normalizeSandboxContainerId(document.runtime.sandbox_container_id),
    is_shared: Boolean(document.runtime.is_shared)
  };
  const hiveId = trimString(document.hive.id);
  const hiveName = trimString(document.hive.name);
  const hiveDescription = trimString(document.hive.description);
  if (hiveId) {
    payload.hive_id = hiveId;
  } else if (hiveName) {
    payload.hive_name = hiveName;
    if (hiveDescription) {
      payload.hive_description = hiveDescription;
    }
  }
  return payload;
};
