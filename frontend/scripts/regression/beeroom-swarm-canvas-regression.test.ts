import test from 'node:test';
import assert from 'node:assert/strict';

import { normalizeBeeroomActorName } from '../../src/components/beeroom/beeroomActorIdentity';
import { resolveBeeroomDispatchPreviewStatus } from '../../src/components/beeroom/beeroomDispatchPreviewStatus';
import { buildNodeWorkflowPreviewLines } from '../../src/components/beeroom/beeroomTaskWorkflow';
import { shouldPollBeeroomTaskSubagents } from '../../src/components/beeroom/useBeeroomMissionSubagentPreview';
import {
  buildBeeroomRuntimeRelayMessageSignature,
  mergeBeeroomRuntimeRelayMessages
} from '../../src/components/beeroom/beeroomRuntimeRelayMessages';
import { resolveBeeroomProjectedSubagentAvatarImage } from '../../src/components/beeroom/canvas/beeroomSwarmAvatarIdentity';
import { resolveBeeroomSwarmNodeStatus } from '../../src/components/beeroom/canvas/beeroomSwarmNodeStatus';
import {
  isBeeroomSwarmWorkerShadowItem,
  buildBeeroomSwarmSubagentProjectionContext,
  isBeeroomTargetNotFoundGhostSubagent,
  resolveBeeroomSwarmWorkerShadowMatch,
  resolveProjectedWorkerSubagents,
  resolveBeeroomSwarmSubagentProjectionDecision,
  shouldProjectBeeroomSwarmSubagent,
  type BeeroomProjectedSubagentLike,
  type BeeroomProjectedTaskLike
} from '../../src/components/beeroom/canvas/beeroomSwarmSubagentProjection';

type TestSubagent = BeeroomProjectedSubagentLike & {
  key: string;
  sessionId: string;
  runId: string;
  runKind: string;
  requestedBy: string;
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
  parentTurnRef: string;
  parentUserRound: number | null;
  parentModelRound: number | null;
  spawnedBy: string;
  workflowItems: unknown[];
};

type TestTask = BeeroomProjectedTaskLike & {
  task_id: string;
  agent_id: string;
  target_session_id?: string;
  spawned_session_id?: string;
  session_run_id?: string;
  status?: string;
  updated_time?: number;
};

const buildSubagent = (partial: Partial<TestSubagent> = {}): TestSubagent => ({
  key: String(partial.key || partial.sessionId || partial.runId || 'subagent-key'),
  sessionId: String(partial.sessionId || 'sess_subagent'),
  runId: String(partial.runId || 'run_subagent'),
  runKind: String(partial.runKind || ''),
  requestedBy: String(partial.requestedBy || ''),
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
  parentTurnRef: String(partial.parentTurnRef || ''),
  parentUserRound: partial.parentUserRound ?? null,
  parentModelRound: partial.parentModelRound ?? null,
  spawnedBy: String(partial.spawnedBy || ''),
  workflowItems: Array.isArray(partial.workflowItems) ? partial.workflowItems : []
});

const DEFAULT_AGENT_LABEL = 'Default Agent Localized';
const t = (key: string) => (key === 'messenger.defaultAgent' ? DEFAULT_AGENT_LABEL : key);

test('beeroom actor naming normalizes default agent aliases to localized label', () => {
  assert.equal(normalizeBeeroomActorName('Default Agent', t), DEFAULT_AGENT_LABEL);
  assert.equal(normalizeBeeroomActorName('__default__', t), DEFAULT_AGENT_LABEL);
  assert.equal(normalizeBeeroomActorName(DEFAULT_AGENT_LABEL, t), DEFAULT_AGENT_LABEL);
});

test('canvas projection rejects swarm worker sessions from the generic subagent feed', () => {
  assert.equal(
    isBeeroomSwarmWorkerShadowItem(
      buildSubagent({
        sessionId: 'sess_worker_shadow',
        runId: 'run_worker_shadow',
        runKind: 'swarm',
        requestedBy: 'agent_swarm'
      })
    ),
    true
  );
  const swarmWorkerDecision = resolveBeeroomSwarmSubagentProjectionDecision(
    buildSubagent({
      sessionId: 'sess_worker_shadow',
      runId: 'run_worker_shadow',
      runKind: 'swarm',
      requestedBy: 'agent_swarm'
    })
  );
  assert.equal(swarmWorkerDecision.projectable, false);
  assert.equal(swarmWorkerDecision.reason, 'filtered:run_kind_swarm');
  assert.equal(
    shouldProjectBeeroomSwarmSubagent(
      buildSubagent({
        sessionId: 'sess_worker_shadow',
        runId: 'run_worker_shadow',
        runKind: 'swarm',
        requestedBy: 'agent_swarm'
      })
    ),
    false
  );
  const realSubagentDecision = resolveBeeroomSwarmSubagentProjectionDecision(
    buildSubagent({
      sessionId: 'sess_real_subagent',
      runId: 'run_real_subagent',
      runKind: 'subagent',
      requestedBy: 'subagent_control'
    })
  );
  assert.equal(realSubagentDecision.projectable, true);
  assert.equal(realSubagentDecision.reason, 'projectable');
  assert.equal(
    shouldProjectBeeroomSwarmSubagent(
      buildSubagent({
        sessionId: 'sess_real_subagent',
        runId: 'run_real_subagent',
        runKind: 'subagent',
        requestedBy: 'subagent_control'
      })
    ),
    true
  );
  assert.equal(
    isBeeroomSwarmWorkerShadowItem(
      buildSubagent({
        sessionId: 'sess_real_subagent',
        runId: 'run_real_subagent',
        runKind: 'subagent',
        requestedBy: 'subagent_control'
      })
    ),
    false
  );
});

test('canvas projection rejects target-not-found ghost children created by mistyped wait run ids', () => {
  const ghost = buildSubagent({
    key: 'run_ghost',
    sessionId: ' ',
    runId: 'run_ghost',
    runKind: ' ',
    requestedBy: ' ',
    status: 'not_found',
    failed: true,
    terminal: true
  });

  assert.equal(isBeeroomTargetNotFoundGhostSubagent(ghost), true);
  assert.deepEqual(resolveBeeroomSwarmSubagentProjectionDecision(ghost), {
    projectable: false,
    reason: 'filtered:target_not_found_ghost'
  });

  const projected = resolveProjectedWorkerSubagents({
    workerRole: 'worker',
    workerNodeId: 'agent:worker-1',
    runtimeTargetNodeId: '',
    runtimeSubagents: [],
    tasks: [
      {
        task_id: 'task-1',
        agent_id: 'worker-1',
        status: 'running',
        updated_time: 20
      }
    ],
    subagentsByTask: {
      'task-1': [
        buildSubagent({
          key: 'run_real',
          sessionId: 'sess_real',
          runId: 'run_real',
          runKind: 'subagent',
          requestedBy: 'subagent_control',
          status: 'success',
          terminal: true
        }),
        ghost
      ]
    }
  });

  assert.deepEqual(
    projected.map((item) => item.runId),
    ['run_real']
  );
});

test('canvas projection rejects swarm worker shadow sessions by task identity before run metadata arrives', () => {
  const projectionContext = buildBeeroomSwarmSubagentProjectionContext([
    {
      task_id: 'task-1',
      agent_id: 'worker-1',
      target_session_id: 'sess_worker_shadow',
      spawned_session_id: 'sess_worker_shadow',
      session_run_id: 'run_worker_shadow'
    }
  ]);
  const workerShadowDecision = resolveBeeroomSwarmSubagentProjectionDecision(
    buildSubagent({
      sessionId: 'sess_worker_shadow',
      runId: '',
      runKind: '',
      requestedBy: ''
    }),
    projectionContext
  );
  assert.equal(workerShadowDecision.projectable, false);
  assert.equal(workerShadowDecision.reason, 'filtered:task_session_shadow');
  assert.equal(
    shouldProjectBeeroomSwarmSubagent(
      buildSubagent({
        sessionId: 'sess_worker_shadow',
        runId: '',
        runKind: '',
        requestedBy: ''
      }),
      projectionContext
    ),
    false
  );
  const realChildDecision = resolveBeeroomSwarmSubagentProjectionDecision(
    buildSubagent({
      sessionId: 'sess_real_child',
      runId: '',
      runKind: '',
      requestedBy: ''
    }),
    projectionContext
  );
  assert.equal(realChildDecision.projectable, true);
  assert.equal(realChildDecision.reason, 'projectable');
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

test('mother runtime projection ignores swarm worker sessions while keeping real subagents', () => {
  const subagents = resolveProjectedWorkerSubagents({
    workerRole: 'mother',
    workerNodeId: 'agent:mother-1',
    runtimeTargetNodeId: 'agent:mother-1',
    runtimeSubagents: [
      buildSubagent({
        key: 'worker-shadow',
        sessionId: 'sess_worker_shadow',
        runId: 'run_worker_shadow',
        runKind: 'swarm',
        requestedBy: 'agent_swarm',
        agentId: 'worker-1'
      }),
      buildSubagent({
        key: 'real-subagent',
        sessionId: 'sess_real_subagent',
        runId: 'run_real_subagent',
        runKind: 'subagent',
        requestedBy: 'subagent_control',
        agentId: 'subagent-1'
      })
    ],
    tasks: [],
    subagentsByTask: {}
  });

  assert.deepEqual(
    subagents.map((item) => item.sessionId),
    ['sess_real_subagent']
  );
});

test('worker runtime projection filters worker shadow sessions without hiding real child sessions', () => {
  const tasks: TestTask[] = [
    {
      task_id: 'task-worker-runtime',
      agent_id: 'worker-1',
      target_session_id: 'sess_worker_shadow',
      spawned_session_id: 'sess_worker_shadow',
      session_run_id: 'run_worker_shadow',
      status: 'running',
      updated_time: 20
    }
  ];
  const subagents = resolveProjectedWorkerSubagents({
    workerRole: 'worker',
    workerNodeId: 'agent:worker-1',
    runtimeTargetNodeId: 'agent:worker-1',
    runtimeSubagents: [
      buildSubagent({
        key: 'worker-shadow',
        sessionId: 'sess_worker_shadow',
        runId: '',
        runKind: '',
        requestedBy: '',
        agentId: 'worker-1'
      }),
      buildSubagent({
        key: 'real-child',
        sessionId: 'sess_real_child',
        runId: '',
        runKind: '',
        requestedBy: '',
        agentId: 'subagent-1'
      })
    ],
    tasks,
    subagentsByTask: {}
  });

  assert.deepEqual(
    subagents.map((item) => item.sessionId),
    ['sess_real_child']
  );
});

test('derived subagent avatars prefer external agent resolver and keep default-agent fallback', () => {
  assert.equal(
    resolveBeeroomProjectedSubagentAvatarImage({
      agentId: 'derived-agent-1',
      name: 'Default Agent',
      explicitAvatarImageUrl: '',
      resolveAgentAvatarImageByAgentId: (agentId: unknown) => {
        const normalized = String(agentId || '').trim();
        if (normalized === 'derived-agent-1') {
          return 'https://example.com/derived-agent-1.png';
        }
        if (normalized === '__default__') {
          return 'https://example.com/default-agent.png';
        }
        return '';
      },
      defaultAgentAvatarImageUrl: 'https://example.com/default-agent-fallback.png',
      fallbackAvatarImageUrl: 'https://example.com/subagent-fallback.png'
    }),
    'https://example.com/derived-agent-1.png'
  );

  assert.equal(
    resolveBeeroomProjectedSubagentAvatarImage({
      agentId: '',
      name: 'Default Agent',
      explicitAvatarImageUrl: '',
      resolveAgentAvatarImageByAgentId: (agentId: unknown) =>
        String(agentId || '').trim() === '__default__' ? 'https://example.com/default-agent.png' : '',
      defaultAgentAvatarImageUrl: 'https://example.com/default-agent-fallback.png',
      fallbackAvatarImageUrl: 'https://example.com/subagent-fallback.png'
    }),
    'https://example.com/default-agent.png'
  );
});

test('runtime relay messages stay visible when live preview temporarily goes empty', () => {
  const merged = mergeBeeroomRuntimeRelayMessages(
    [
      {
        key: 'subagent:run_worker_shadow:request',
        senderName: '默认智能体',
        senderAgentId: '__default__',
        avatarImageUrl: '',
        mention: '工蜂 A',
        body: '@工蜂 A 请处理任务',
        meta: '',
        time: 100,
        timeLabel: '10:00:00',
        tone: 'mother'
      }
    ],
    [],
    120
  );

  assert.equal(merged.length, 1);
  assert.equal(merged[0]?.key, 'subagent:run_worker_shadow:request');
});

test('runtime relay message signature changes when reply body changes under the same key', () => {
  const before = buildBeeroomRuntimeRelayMessageSignature([
    {
      key: 'subagent:run_worker_shadow:reply',
      senderName: '工蜂 A',
      senderAgentId: 'worker-a',
      avatarImageUrl: '',
      mention: '默认智能体',
      body: 'first reply',
      meta: '',
      time: 200,
      timeLabel: '10:00:00',
      tone: 'worker'
    }
  ]);
  const after = buildBeeroomRuntimeRelayMessageSignature([
    {
      key: 'subagent:run_worker_shadow:reply',
      senderName: '工蜂 A',
      senderAgentId: 'worker-a',
      avatarImageUrl: '',
      mention: '默认智能体',
      body: 'final reply',
      meta: '',
      time: 200,
      timeLabel: '10:00:00',
      tone: 'worker'
    }
  ]);
  assert.notEqual(before, after);
});

test('worker shadow matching prefers the runtime shadow session tied to the current worker task', () => {
  const matched = resolveBeeroomSwarmWorkerShadowMatch({
    workerAgentId: 'worker-1',
    tasks: [
      {
        task_id: 'task-worker-1',
        agent_id: 'worker-1',
        target_session_id: 'sess_worker_shadow',
        spawned_session_id: 'sess_worker_shadow',
        session_run_id: 'run_worker_shadow',
        status: 'awaiting_idle',
        updated_time: 100
      }
    ],
    runtimeSubagents: [
      buildSubagent({
        key: 'wrong-worker',
        sessionId: 'sess_other',
        runId: 'run_other',
        runKind: 'swarm',
        requestedBy: 'agent_swarm',
        agentId: 'worker-2',
        status: 'running',
        updatedTime: 140
      }),
      buildSubagent({
        key: 'worker-shadow',
        sessionId: 'sess_worker_shadow',
        runId: 'run_worker_shadow',
        runKind: 'swarm',
        requestedBy: 'agent_swarm',
        agentId: 'worker-1',
        status: 'running',
        updatedTime: 130,
        workflowItems: [
          {
            id: 'tool-call-1',
            title: 'Tool Call',
            detail: 'search_workspace',
            status: 'loading',
            eventType: 'tool_call',
            toolName: 'search_workspace'
          }
        ]
      })
    ]
  });

  assert.equal(matched?.sessionId, 'sess_worker_shadow');
  assert.equal(matched?.runId, 'run_worker_shadow');
  assert.equal(matched?.agentId, 'worker-1');
});

test('runtime relay message merge keeps worker reply without dropping the earlier dispatch request', () => {
  const merged = mergeBeeroomRuntimeRelayMessages(
    [
      {
        key: 'subagent:run_worker_shadow:request',
        senderName: '默认智能体',
        senderAgentId: '__default__',
        avatarImageUrl: '',
        mention: '工蜂 A',
        body: '@工蜂 A 请处理任务',
        meta: '',
        time: 100,
        timeLabel: '10:00:00',
        tone: 'mother'
      }
    ],
    [
      {
        key: 'subagent:run_worker_shadow:reply',
        senderName: '工蜂 A',
        senderAgentId: 'worker-a',
        avatarImageUrl: '',
        mention: '默认智能体',
        body: '@默认智能体 任务已完成',
        meta: '',
        time: 101,
        timeLabel: '10:00:01',
        tone: 'worker'
      }
    ],
    120
  );

  assert.deepEqual(
    merged.map((message) => message.key),
    ['subagent:run_worker_shadow:request', 'subagent:run_worker_shadow:reply']
  );
});

test('worker node stays in running state while workflow tail still shows live activity', () => {
  const status = resolveBeeroomSwarmNodeStatus({
    tasks: [
      {
        task_id: 'task-worker-runtime',
        agent_id: 'worker-1',
        status: 'awaiting_idle'
      } as never
    ],
    member: {
      agent_id: 'worker-1',
      name: '工蜂 A',
      idle: true,
      active_session_total: 0
    } as never,
    missionStatus: 'awaiting_idle',
    workflowTailTone: 'loading'
  });

  assert.equal(status, 'running');
});

test('node workflow preview collapses tool call and result pairs into a single row', () => {
  const lines = buildNodeWorkflowPreviewLines([
    {
      id: 'workflow:tool_call:1',
      title: '调用工具 search_workspace',
      detail: JSON.stringify({
        tool: 'search_workspace',
        query: 'beeroom'
      }),
      status: 'loading',
      isTool: true,
      eventType: 'tool_call',
      toolName: 'search_workspace',
      toolCallId: 'call-1'
    },
    {
      id: 'workflow:tool_result:2',
      title: '工具结果 search_workspace',
      detail: JSON.stringify({
        tool: 'search_workspace',
        query: 'beeroom',
        result_summary: 'ok'
      }),
      status: 'completed',
      isTool: true,
      eventType: 'tool_result',
      toolName: 'search_workspace',
      toolCallId: 'call-1'
    }
  ] as never);

  assert.equal(lines.length, 1);
  assert.equal(lines[0]?.main.includes('search'), true);
  assert.equal(lines[0]?.detail.includes('beeroom'), true);
});

test('node workflow preview keeps only tool call rows when result events arrive separately later', () => {
  const lines = buildNodeWorkflowPreviewLines([
    {
      id: 'workflow:tool_call:1',
      title: '调用工具 search_workspace',
      detail: JSON.stringify({
        tool: 'search_workspace',
        query: 'beeroom'
      }),
      status: 'loading',
      isTool: true,
      eventType: 'tool_call',
      toolName: 'search_workspace',
      toolCallId: 'call-2'
    },
    {
      id: 'workflow:progress:2',
      title: '进度',
      detail: 'searching',
      status: 'loading',
      eventType: 'progress'
    },
    {
      id: 'workflow:tool_result:3',
      title: '工具结果 search_workspace',
      detail: JSON.stringify({
        tool: 'search_workspace',
        result_summary: 'ok'
      }),
      status: 'completed',
      isTool: true,
      eventType: 'tool_result',
      toolName: 'search_workspace',
      toolCallId: 'call-2'
    }
  ] as never);

  assert.equal(lines.length, 1);
  assert.equal(lines[0]?.main.includes('search'), true);
});

test('recent worker tasks keep polling for subagents even after leaving the active task statuses', () => {
  assert.equal(
    shouldPollBeeroomTaskSubagents(
      {
        task_id: 'task-recent',
        agent_id: 'worker-1',
        status: 'completed',
        target_session_id: 'sess-worker',
        updated_time: 995
      } as never,
      [],
      1000
    ),
    true
  );
  assert.equal(
    shouldPollBeeroomTaskSubagents(
      {
        task_id: 'task-old',
        agent_id: 'worker-1',
        status: 'completed',
        target_session_id: 'sess-worker',
        updated_time: 900
      } as never,
      [],
      1000
    ),
    false
  );
});

test('subagent failure does not automatically mark the parent dispatch preview as failed', () => {
  const status = resolveBeeroomDispatchPreviewStatus({
    localStatus: 'completed',
    running: false,
    events: [],
    subagents: [
      buildSubagent({
        key: 'failed-child',
        sessionId: 'sess_failed_child',
        runId: 'run_failed_child',
        status: 'failed',
        failed: true
      })
    ] as never
  });

  assert.equal(status, 'completed');
});

test('parent dispatch preview still becomes failed when the parent session itself has terminal error events', () => {
  const status = resolveBeeroomDispatchPreviewStatus({
    localStatus: 'running',
    running: false,
    events: [
      {
        event: 'turn_terminal',
        data: {
          status: 'failed'
        }
      }
    ] as never,
    subagents: [
      buildSubagent({
        key: 'failed-child',
        sessionId: 'sess_failed_child',
        runId: 'run_failed_child',
        status: 'failed',
        failed: true
      })
    ] as never
  });

  assert.equal(status, 'failed');
});

test('terminal dispatch preview stays terminal instead of reviving old swarm replay state', () => {
  const status = resolveBeeroomDispatchPreviewStatus({
    localStatus: 'idle',
    running: false,
    events: [
      {
        event: 'final',
        data: {
          answer: 'done'
        }
      }
    ] as never,
    subagents: [
      buildSubagent({
        key: 'completed-child',
        sessionId: 'sess_completed_child',
        runId: 'run_completed_child',
        status: 'completed',
        terminal: true
      })
    ] as never
  });

  assert.equal(status, 'completed');
});
