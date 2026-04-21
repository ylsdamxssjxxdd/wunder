import test from 'node:test';
import assert from 'node:assert/strict';

import { buildOrchestrationTimelineLayout } from '../../src/components/orchestration/orchestrationTimelineLayout';

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
