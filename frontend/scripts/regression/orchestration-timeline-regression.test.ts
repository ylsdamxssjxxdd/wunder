import test from 'node:test';
import assert from 'node:assert/strict';

import { buildOrchestrationTimelineLayout } from '../../src/components/orchestration/orchestrationTimelineLayout';
import { stabilizeOrchestrationRoundSnapshots } from '../../src/components/orchestration/orchestrationRoundStateStability';

test('round stabilization preserves a completed middle round when a remote snapshot develops an internal gap', () => {
  const merged = stabilizeOrchestrationRoundSnapshots(
    [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'mainline round 1',
        situation: 'situation 1',
        createdAt: 100,
        finalizedAt: 110,
        missionIds: ['mission-1']
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: 'mainline round 2',
        situation: 'situation 2',
        createdAt: 120,
        finalizedAt: 130,
        missionIds: ['mission-2']
      },
      {
        id: 'round_03',
        index: 3,
        userMessage: '',
        situation: 'preview situation',
        createdAt: 140,
        finalizedAt: 0
      }
    ],
    [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'mainline round 1',
        situation: 'situation 1',
        createdAt: 100,
        finalizedAt: 110
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: '',
        situation: '',
        createdAt: 120,
        finalizedAt: 0
      },
      {
        id: 'round_03',
        index: 3,
        userMessage: 'mainline round 3',
        situation: 'situation 3',
        createdAt: 140,
        finalizedAt: 150
      }
    ]
  );

  assert.deepEqual(
    merged.map((round) => ({
      index: round.index,
      userMessage: round.userMessage,
      finalizedAt: round.finalizedAt
    })),
    [
      { index: 1, userMessage: 'mainline round 1', finalizedAt: 110 },
      { index: 2, userMessage: 'mainline round 2', finalizedAt: 130 },
      { index: 3, userMessage: 'mainline round 3', finalizedAt: 150 }
    ]
  );
  assert.deepEqual(merged[1]?.missionIds, ['mission-2']);
});

test('round stabilization allows the frontier round to clear after cancellation', () => {
  const merged = stabilizeOrchestrationRoundSnapshots(
    [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'mainline round 1',
        situation: 'situation 1',
        createdAt: 100,
        finalizedAt: 110
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: 'pending round 2',
        situation: 'preview situation',
        createdAt: 120,
        finalizedAt: 0
      }
    ],
    [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'mainline round 1',
        situation: 'situation 1',
        createdAt: 100,
        finalizedAt: 110
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: '',
        situation: '',
        createdAt: 120,
        finalizedAt: 0
      }
    ]
  );

  assert.equal(merged[1]?.userMessage, '');
});

test('timeline keeps intermediate branch visible for branch of branch runs', () => {
  const layout = buildOrchestrationTimelineLayout({
    historyItems: [
      {
        orchestrationId: 'orch-root',
        runId: 'root',
        status: 'closed',
        latestRoundIndex: 2,
        enteredAt: 100,
        updatedAt: 120,
        exitedAt: 120,
        restoredAt: 0,
        parentOrchestrationId: '',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 0,
        branchDepth: 0
      },
      {
        orchestrationId: 'orch-branch',
        runId: 'branch',
        status: 'closed',
        latestRoundIndex: 4,
        enteredAt: 130,
        updatedAt: 150,
        exitedAt: 150,
        restoredAt: 0,
        parentOrchestrationId: 'orch-root',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 2,
        branchDepth: 1
      },
      {
        orchestrationId: 'orch-branch-child',
        runId: 'branch-child',
        status: 'active',
        latestRoundIndex: 5,
        enteredAt: 160,
        updatedAt: 170,
        exitedAt: 0,
        restoredAt: 0,
        parentOrchestrationId: 'orch-branch',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 4,
        branchDepth: 2
      }
    ],
    currentOrchestrationId: 'orch-branch-child',
    rounds: [
      {
        id: 'round_05',
        index: 5,
        userMessage: 'continue from nested branch'
      }
    ],
    activeRoundId: 'round_05',
    isActive: true,
    isBusy: false,
    currentRunFallback: {
      runId: 'branch-child',
      status: 'active',
      latestRoundIndex: 5
    }
  });

  const runItems = layout.items.filter((item) => item.type === 'run');
  const roundItems = layout.items.filter((item) => item.type === 'round');
  assert.deepEqual(
    runItems.map((item) => item.id),
    ['run:orch-root', 'run:orch-branch', 'run:orch-branch-child']
  );
  assert.equal(runItems[0]?.lane, 0);
  assert.equal(runItems[1]?.lane, 1);
  assert.equal(runItems[2]?.lane, 2);
  assert.equal(runItems[1]?.branchFromRoundIndex, 2);
  assert.equal(runItems[2]?.branchFromRoundIndex, 4);
  assert.deepEqual(
    roundItems
      .filter((item) => item.orchestrationId === 'orch-root')
      .map((item) => item.roundIndex),
    [1, 2]
  );
  assert.deepEqual(
    roundItems
      .filter((item) => item.orchestrationId === 'orch-branch')
      .map((item) => item.roundIndex),
    [3, 4]
  );
  assert.deepEqual(
    roundItems
      .filter((item) => item.orchestrationId === 'orch-branch-child')
      .map((item) => ({ roundIndex: item.roundIndex, preview: item.preview })),
    [{ roundIndex: 5, preview: false }, { roundIndex: 6, preview: true }]
  );
  assert.ok(
    layout.connectors.some((connector) =>
      connector.id === 'run-link-vertical:orch-branch:orch-branch-child:4'
    )
  );
});

test('timeline shows first preview round before the first round starts', () => {
  const layout = buildOrchestrationTimelineLayout({
    historyItems: [
      {
        orchestrationId: 'orch-root',
        runId: 'root',
        status: 'active',
        latestRoundIndex: 1,
        enteredAt: 100,
        updatedAt: 100,
        exitedAt: 0,
        restoredAt: 0,
        parentOrchestrationId: '',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 0,
        branchDepth: 0
      }
    ],
    currentOrchestrationId: 'orch-root',
    rounds: [],
    activeRoundId: '',
    isActive: true,
    isBusy: false,
    currentRunFallback: {
      runId: 'root',
      status: 'active',
      latestRoundIndex: 1
    }
  });

  const roundItems = layout.items.filter((item) => item.type === 'round');
  assert.deepEqual(
    roundItems.map((item) => ({ roundIndex: item.roundIndex, preview: item.preview })),
    [{ roundIndex: 1, preview: true }]
  );
});

test('timeline item ids stay unique when mainline and branch share round ids', () => {
  const layout = buildOrchestrationTimelineLayout({
    historyItems: [
      {
        orchestrationId: 'orch-root',
        runId: 'root',
        status: 'active',
        latestRoundIndex: 2,
        enteredAt: 100,
        updatedAt: 120,
        exitedAt: 0,
        restoredAt: 0,
        parentOrchestrationId: '',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 0,
        branchDepth: 0
      },
      {
        orchestrationId: 'orch-branch-from-1',
        runId: 'branch-from-1',
        status: 'closed',
        latestRoundIndex: 2,
        enteredAt: 130,
        updatedAt: 140,
        exitedAt: 140,
        restoredAt: 0,
        parentOrchestrationId: 'orch-root',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 1,
        branchDepth: 1
      }
    ],
    currentOrchestrationId: 'orch-root',
    rounds: [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'mainline round 1'
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: 'mainline round 2'
      }
    ],
    activeRoundId: 'round_02',
    isActive: true,
    isBusy: false,
    currentRunFallback: {
      runId: 'root',
      status: 'active',
      latestRoundIndex: 2
    }
  });

  const itemIds = layout.items.map((item) => item.id);
  assert.equal(new Set(itemIds).size, itemIds.length);

  const rootRoundOne = layout.items.find(
    (item) => item.type === 'round' && item.orchestrationId === 'orch-root' && item.roundIndex === 1
  );
  const branchRun = layout.items.find(
    (item) => item.type === 'run' && item.id === 'run:orch-branch-from-1'
  );

  assert.equal(rootRoundOne?.column, branchRun?.column);
  assert.ok(
    layout.connectors.some((connector) =>
      connector.id === 'run-link-vertical:orch-root:orch-branch-from-1:1'
    )
  );
});

test('completed round situation remains recoverable from rounds when planned map is empty', () => {
  const persisted = {} as Record<string, string>;
  const rounds = [
    {
      id: 'round_01',
      index: 1,
      userMessage: 'round 1 message',
      situation: 'round 1 situation'
    },
    {
      id: 'round_02',
      index: 2,
      userMessage: '',
      situation: 'round 2 preview situation'
    }
  ];

  const merged = { ...persisted };
  rounds.forEach((round) => {
    const key = String(round.index);
    const situation = String(round.situation || '').trim();
    if (!situation) return;
    merged[key] = situation;
  });

  assert.equal(merged['1'], 'round 1 situation');
  assert.equal(merged['2'], 'round 2 preview situation');
});

test('timeline keeps a completed middle round visible when a later round is already finished', () => {
  const layout = buildOrchestrationTimelineLayout({
    historyItems: [
      {
        orchestrationId: 'orch-root',
        runId: 'root',
        status: 'active',
        latestRoundIndex: 3,
        enteredAt: 100,
        updatedAt: 150,
        exitedAt: 0,
        restoredAt: 0,
        parentOrchestrationId: '',
        branchRootOrchestrationId: 'orch-root',
        branchFromRoundIndex: 0,
        branchDepth: 0
      }
    ],
    currentOrchestrationId: 'orch-root',
    rounds: [
      {
        id: 'round_01',
        index: 1,
        userMessage: 'round 1',
        finalizedAt: 110
      },
      {
        id: 'round_02',
        index: 2,
        userMessage: '',
        finalizedAt: 0
      },
      {
        id: 'round_03',
        index: 3,
        userMessage: 'round 3',
        finalizedAt: 150
      },
      {
        id: 'round_04',
        index: 4,
        userMessage: '',
        finalizedAt: 0
      }
    ],
    activeRoundId: 'round_03',
    isActive: true,
    isBusy: false,
    currentRunFallback: {
      runId: 'root',
      status: 'active',
      latestRoundIndex: 3
    }
  });

  const roundItems = layout.items.filter((item) => item.type === 'round');
  assert.deepEqual(roundItems.map((item) => item.roundIndex), [1, 2, 3, 4]);
  assert.equal(roundItems.find((item) => item.roundIndex === 2)?.pending, false);
  assert.equal(roundItems.find((item) => item.roundIndex === 4)?.pending, true);
});
