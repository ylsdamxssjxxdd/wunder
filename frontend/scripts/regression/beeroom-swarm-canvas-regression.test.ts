import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveProjectedWorkerSubagents,
  type BeeroomProjectedSubagentLike,
  type BeeroomProjectedTaskLike
} from '../../src/components/beeroom/canvas/beeroomSwarmSubagentProjection';

type TestSubagent = BeeroomProjectedSubagentLike & {
  key: string;
  sessionId: string;
  runId: string;
  agentId: string;
  title: string;
  label: string;
  status: string;
  summary: string;
  userMessage: string;
  assistantMessage: string;
  errorMessage: string;
  updatedTime: number;
  terminal: boolean;
  failed: boolean;
  depth: number | null;
  role: string;
  controlScope: string;
  spawnMode: string;
  strategy: string;
  dispatchLabel: string;
  controllerSessionId: string;
  parentSessionId: string;
  workflowItems: unknown[];
};

type TestTask = BeeroomProjectedTaskLike & {
  task_id: string;
  agent_id: string;
  status?: string;
  updated_time?: number;
};

const buildSubagent = (partial: Partial<TestSubagent> = {}): TestSubagent => ({
  key: String(partial.key || partial.sessionId || partial.runId || 'subagent-key'),
  sessionId: String(partial.sessionId || 'sess_subagent'),
  runId: String(partial.runId || 'run_subagent'),
  agentId: String(partial.agentId || 'subagent-agent'),
  title: String(partial.title || 'Subagent'),
  label: String(partial.label || ''),
  status: String(partial.status || 'running'),
  summary: String(partial.summary || ''),
  userMessage: String(partial.userMessage || ''),
  assistantMessage: String(partial.assistantMessage || ''),
  errorMessage: String(partial.errorMessage || ''),
  updatedTime: Number(partial.updatedTime || 0),
  terminal: partial.terminal === true,
  failed: partial.failed === true,
  depth: partial.depth ?? null,
  role: String(partial.role || ''),
  controlScope: String(partial.controlScope || ''),
  spawnMode: String(partial.spawnMode || ''),
  strategy: String(partial.strategy || ''),
  dispatchLabel: String(partial.dispatchLabel || ''),
  controllerSessionId: String(partial.controllerSessionId || ''),
  parentSessionId: String(partial.parentSessionId || ''),
  workflowItems: Array.isArray(partial.workflowItems) ? partial.workflowItems : []
});

test('runtime dispatch subagents stay projected even when mission already has worker tasks', () => {
  const tasks: TestTask[] = [
    {
      task_id: 'task-1',
      agent_id: 'worker-1',
      status: 'completed',
      updated_time: 20
    }
  ];
  const subagents = resolveProjectedWorkerSubagents({
    workerRole: 'worker',
    workerNodeId: 'agent:worker-1',
    runtimeTargetNodeId: 'agent:worker-1',
    runtimeSubagents: [
      buildSubagent({
        key: 'sub-runtime',
        sessionId: 'sess_sub_runtime',
        runId: 'run_sub_runtime',
        agentId: 'subagent-runtime',
        updatedTime: 50
      })
    ],
    tasks,
    subagentsByTask: {}
  });
  assert.equal(subagents.length, 1);
  assert.equal(subagents[0]?.sessionId, 'sess_sub_runtime');
});

test('worker projection keeps subagents from all tasks instead of only the latest task', () => {
  const tasks: TestTask[] = [
    {
      task_id: 'task-old',
      agent_id: 'worker-1',
      status: 'completed',
      updated_time: 10
    },
    {
      task_id: 'task-new',
      agent_id: 'worker-1',
      status: 'running',
      updated_time: 20
    }
  ];
  const subagents = resolveProjectedWorkerSubagents({
    workerRole: 'worker',
    workerNodeId: 'agent:worker-1',
    runtimeTargetNodeId: '',
    runtimeSubagents: [],
    tasks,
    subagentsByTask: {
      'task-old': [
        buildSubagent({
          key: 'sub-old',
          sessionId: 'sess_sub_old',
          runId: 'run_sub_old',
          agentId: 'subagent-old',
          updatedTime: 11
        })
      ],
      'task-new': [
        buildSubagent({
          key: 'sub-new',
          sessionId: 'sess_sub_new',
          runId: 'run_sub_new',
          agentId: 'subagent-new',
          updatedTime: 21
        })
      ]
    }
  });

  const subagentNodeIds = subagents
    .map((item) => item.sessionId)
    .sort();
  assert.deepEqual(subagentNodeIds, ['sess_sub_new', 'sess_sub_old']);
});

test('mother projection never mixes task subagents into the mother node itself', () => {
  const tasks: TestTask[] = [
    {
      task_id: 'task-mother',
      agent_id: 'mother-1',
      status: 'running',
      updated_time: 20
    }
  ];
  const subagents = resolveProjectedWorkerSubagents({
    workerRole: 'mother',
    workerNodeId: 'agent:mother-1',
    runtimeTargetNodeId: '',
    runtimeSubagents: [],
    tasks,
    subagentsByTask: {
      'task-mother': [buildSubagent({ sessionId: 'sess_should_ignore', runId: 'run_should_ignore' })]
    }
  });
  assert.deepEqual(subagents, []);
});
