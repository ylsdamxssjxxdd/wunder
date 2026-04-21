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
  assert.deepEqual(
    runItems.map((item) => item.id),
    ['run:orch-root', 'run:orch-branch', 'run:orch-branch-child']
  );
  assert.equal(runItems[0]?.lane, 0);
  assert.equal(runItems[1]?.lane, 1);
  assert.equal(runItems[2]?.lane, 2);
  assert.equal(runItems[1]?.branchFromRoundIndex, 2);
  assert.equal(runItems[2]?.branchFromRoundIndex, 4);
  assert.ok(
    layout.connectors.some((connector) =>
      connector.id === 'run-link-vertical:orch-branch:orch-branch-child:4'
    )
  );
});
