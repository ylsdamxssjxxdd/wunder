import type { BeeroomGroup, BeeroomMember } from '@/stores/beeroom';
import {
  buildOrchestrationAgentArtifactDirName,
  buildOrchestrationRoundDirName,
  normalizeOrchestrationText
} from '@/components/orchestration/orchestrationShared';

export type OrchestrationPromptTemplates = {
  mother_runtime: string;
  round_artifacts: string;
  worker_first_dispatch: string;
  worker_round_artifacts: string;
  worker_guide: string;
  situation_context: string;
  user_message: string;
};

const normalizeText = normalizeOrchestrationText;

const buildRoundPromptDirectory = (roundIndex: number) => buildOrchestrationRoundDirName(roundIndex);

const buildPromptArtifactPath = (roundIndex: number, workerName: string, fallbackAgentId = '') =>
  [buildOrchestrationRoundDirName(roundIndex), buildOrchestrationAgentArtifactDirName(workerName, fallbackAgentId)]
    .filter(Boolean)
    .join('/');

const resolveMotherName = (group: BeeroomGroup | null, motherAgentId: string, agents: BeeroomMember[]) => {
  const member = agents.find((item) => normalizeText(item.agent_id) === normalizeText(motherAgentId));
  return normalizeText(member?.name || group?.mother_agent_name || motherAgentId) || motherAgentId;
};

const resolveWorkerMembers = (group: BeeroomGroup | null, agents: BeeroomMember[]) => {
  const motherAgentId = normalizeText(group?.mother_agent_id);
  return (Array.isArray(agents) ? agents : []).filter((item) => {
    const agentId = normalizeText(item.agent_id);
    return agentId && agentId !== motherAgentId;
  });
};

const renderTemplate = (template: string, values: Record<string, string>) =>
  String(template || '').replace(/\{\{\s*([a-zA-Z0-9_]+)\s*\}\}/g, (_match, key: string) =>
    String(values[key] ?? '')
  );

const resolveWorkerArtifactLines = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  runId: string;
  roundIndex: number;
}) =>
  resolveWorkerMembers(options.group, options.agents).map((worker, index) => {
    const agentId = normalizeText(worker.agent_id);
    const workerName = normalizeText(worker.name || agentId) || agentId;
    const artifactPath = buildPromptArtifactPath(options.roundIndex, workerName, agentId);
    return {
      agentId,
      workerName,
      artifactPath,
      nameLine: `${index + 1}. ${workerName}`,
      artifactLine: `${index + 1}. ${workerName}: ${artifactPath}`
    };
  });

export const buildMotherRoundArtifactInstructions = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  runId: string;
  roundIndex: number;
  templates: OrchestrationPromptTemplates;
}) => {
  const workers = resolveWorkerArtifactLines(options);
  return renderTemplate(options.templates.round_artifacts, {
    current_round_dir: buildRoundPromptDirectory(options.roundIndex),
    current_round_situation_file: `${buildOrchestrationRoundDirName(options.roundIndex)}/situation.txt`,
    worker_artifact_lines: workers.map((item) => item.artifactLine).join('\n')
  }).trim();
};

export const buildMotherOrchestrationPrimer = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  runId: string;
  roundIndex: number;
  templates: OrchestrationPromptTemplates;
}) => {
  const motherAgentId = normalizeText(options.group?.mother_agent_id);
  const motherName = resolveMotherName(options.group, motherAgentId, options.agents);
  const workers = resolveWorkerArtifactLines(options);
  return renderTemplate(options.templates.mother_runtime, {
    mother_name: motherName,
    run_id: normalizeText(options.runId),
    current_round_dir: buildRoundPromptDirectory(options.roundIndex),
    current_round_situation_file: `${buildOrchestrationRoundDirName(options.roundIndex)}/situation.txt`,
    worker_directory_lines: workers.map((item) => item.nameLine).join('\n')
  }).trim();
};

export const buildMotherDispatchEnvelope = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  runId: string;
  roundIndex: number;
  userMessage: string;
  situation: string;
  includePrimer: boolean;
  templates: OrchestrationPromptTemplates;
}) => {
  const blocks: string[] = [];
  const shouldIncludePrimer = options.includePrimer && Math.max(1, Number(options.roundIndex || 1)) === 1;
  if (shouldIncludePrimer) {
    blocks.push(
      buildMotherOrchestrationPrimer({
        group: options.group,
        agents: options.agents,
        runId: options.runId,
        roundIndex: options.roundIndex,
        templates: options.templates
      })
    );
  }
  if (normalizeText(options.situation)) {
    blocks.push(
      renderTemplate(options.templates.situation_context, {
        situation: normalizeText(options.situation)
      })
    );
  }
  return blocks.filter((item) => normalizeText(item)).join('\n\n');
};

export const buildWorkerFirstDispatchTemplate = (options: {
  workerName: string;
  roundArtifactPath: string;
  templates: OrchestrationPromptTemplates;
}) =>
  renderTemplate(options.templates.worker_first_dispatch, {
    worker_name: normalizeText(options.workerName),
    artifact_path: normalizeText(options.roundArtifactPath)
  }).trim();

export const buildMotherWorkerPrimerGuide = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  runId: string;
  roundIndex: number;
  templates: OrchestrationPromptTemplates;
}) => {
  const workers = resolveWorkerArtifactLines(options);
  if (!workers.length) {
    return '';
  }
  const workerFirstDispatchBlocks = workers
    .map((worker, index) =>
      [
        `${index + 1}. ${worker.workerName}`,
        buildWorkerFirstDispatchTemplate({
          workerName: worker.workerName,
          roundArtifactPath: worker.artifactPath,
          templates: options.templates
        })
      ].join('\n')
    )
    .join('\n\n');
  return renderTemplate(options.templates.worker_guide, {
    worker_first_dispatch_blocks: workerFirstDispatchBlocks
  }).trim();
};
