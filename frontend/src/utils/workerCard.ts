import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { normalizeDependencyNames } from '@/utils/agentDependencyStatus';

export type WorkerCardDocument = {
  schema_version: 'wunder/worker-card@1';
  kind: 'WorkerCard';
  metadata: {
    id: string;
    name: string;
    description: string;
    icon: string;
    exported_at: string;
  };
  prompt: {
    system_prompt: string;
    extra_prompt: string;
  };
  abilities: {
    tool_names: string[];
    skills: string[];
  };
  interaction: {
    preset_questions: string[];
  };
  runtime: {
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

const WORKER_CARD_SCHEMA_VERSION = 'wunder/worker-card@1' as const;
const WORKER_CARD_BUNDLE_SCHEMA_VERSION = 'wunder/worker-card-bundle@1' as const;
const APPROVAL_MODES = new Set(['suggest', 'auto_edit', 'full_auto']);

const trimString = (value: unknown) => String(value || '').trim();

const normalizeStringList = (value: unknown): string[] => normalizeDependencyNames(value);

const normalizeApprovalMode = (value: unknown): 'suggest' | 'auto_edit' | 'full_auto' => {
  const normalized = trimString(value);
  return APPROVAL_MODES.has(normalized) ? (normalized as 'suggest' | 'auto_edit' | 'full_auto') : 'auto_edit';
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

const joinPromptSections = (...parts: unknown[]): string =>
  parts
    .map((item) => trimString(item))
    .filter(Boolean)
    .join('\n\n');

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

export const buildWorkerCardDocument = (value: Record<string, unknown> | null | undefined): WorkerCardDocument => {
  const source = value || {};
  const explicitDeclaredToolNames = normalizeStringList(source.declared_tool_names);
  const explicitDeclaredSkillNames = normalizeStringList(source.declared_skill_names ?? source.skills);
  const hasExplicitDeclaredDependencies =
    explicitDeclaredToolNames.length > 0 || explicitDeclaredSkillNames.length > 0;
  const declaredToolNames = hasExplicitDeclaredDependencies
    ? explicitDeclaredToolNames
    : normalizeStringList(source.tool_names);
  const declaredSkillNames = hasExplicitDeclaredDependencies ? explicitDeclaredSkillNames : [];
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
    prompt: {
      system_prompt: trimString(source.system_prompt),
      extra_prompt: trimString(source.extra_prompt)
    },
    abilities: {
      tool_names: declaredToolNames,
      skills: declaredSkillNames
    },
    interaction: {
      preset_questions: normalizeAgentPresetQuestions(source.preset_questions)
    },
    runtime: {
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
  const filename = `${sanitizeFilenamePart(document.metadata.name || document.metadata.id || 'worker-card')}.worker-card.json`;
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

const normalizeWorkerCardDocument = (value: Record<string, unknown> | null | undefined): WorkerCardDocument => {
  const source = value || {};
  const metadata = (source.metadata || {}) as Record<string, unknown>;
  const prompt = (source.prompt || {}) as Record<string, unknown>;
  const abilities = (source.abilities || {}) as Record<string, unknown>;
  const interaction = (source.interaction || {}) as Record<string, unknown>;
  const runtime = (source.runtime || {}) as Record<string, unknown>;
  const hive = (source.hive || {}) as Record<string, unknown>;
  const normalized = buildWorkerCardDocument({
    id: metadata.id,
    name: metadata.name,
    description: metadata.description,
    icon: metadata.icon,
    system_prompt: prompt.system_prompt,
    extra_prompt: prompt.extra_prompt,
    tool_names: abilities.tool_names,
    skills: abilities.skills,
    preset_questions: interaction.preset_questions,
    approval_mode: runtime.approval_mode,
    sandbox_container_id: runtime.sandbox_container_id,
    is_shared: runtime.is_shared,
    hive_id: hive.id,
    hive_name: hive.name,
    hive_description: hive.description
  });
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
  const declaredToolNames = normalizeStringList(document.abilities.tool_names);
  const declaredSkillNames = normalizeStringList(document.abilities.skills);
  const payload: Record<string, unknown> = {
    name: trimString(document.metadata.name),
    description: trimString(document.metadata.description),
    icon: trimString(document.metadata.icon),
    system_prompt: joinPromptSections(document.prompt.system_prompt, document.prompt.extra_prompt),
    tool_names: normalizeStringList([...declaredToolNames, ...declaredSkillNames]),
    declared_tool_names: declaredToolNames,
    declared_skill_names: declaredSkillNames,
    preset_questions: normalizeAgentPresetQuestions(document.interaction.preset_questions),
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


