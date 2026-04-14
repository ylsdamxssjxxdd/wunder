<template>
  <main class="beeroom-e2e" data-testid="beeroom-e2e-harness">
    <header class="beeroom-e2e-toolbar">
      <button type="button" data-testid="scenario-idle" @click="loadScenario('idle')">Idle</button>
      <button type="button" data-testid="scenario-long-thread" @click="loadScenario('long-thread')">
        Long Thread
      </button>
      <button type="button" data-testid="scenario-worker-shadow" @click="loadScenario('worker-shadow')">
        Worker Shadow
      </button>
      <button type="button" data-testid="scenario-real-subagent-running" @click="loadScenario('real-subagent-running')">
        Real Subagent Running
      </button>
      <button type="button" data-testid="scenario-real-subagent-dormant" @click="loadScenario('real-subagent-dormant')">
        Real Subagent Dormant
      </button>
      <button type="button" data-testid="append-tail-message" @click="appendTailAssistantMessage()">
        Append Tail
      </button>
      <button type="button" data-testid="collapse-chat" @click="chatCollapsed = true">Collapse Chat</button>
      <button type="button" data-testid="expand-chat" @click="chatCollapsed = false">Expand Chat</button>
    </header>

    <section class="beeroom-e2e-body">
      <div class="beeroom-e2e-canvas" data-testid="beeroom-e2e-canvas">
        <BeeroomSwarmCanvasPane
          :group="group"
          :mission="mission"
          :agents="agents"
          :dispatch-preview="dispatchPreview"
          :subagents-by-task="subagentsByTask"
          :mother-workflow-items="[]"
          :workflow-items-by-task="{}"
          :workflow-preview-by-task="{}"
          :resolve-agent-avatar-image-by-agent-id="resolveAgentAvatarImageByAgentId"
        />
      </div>

      <BeeroomCanvasChatPanel
        :collapsed="chatCollapsed"
        :messages="displayMessages"
        :approvals="[]"
        :dispatch-can-stop="composerSending"
        :dispatch-approval-busy="false"
        :composer-text="composerText"
        :composer-target-agent-id="composerTargetAgentId"
        :composer-target-options="composerTargetOptions"
        :composer-sending="composerSending"
        :composer-can-send="Boolean(composerText.trim())"
        :composer-error="''"
        :artifacts-enabled="true"
        :resolve-message-avatar-image="resolveMessageAvatarImage"
        :avatar-label="avatarLabel"
        @update:collapsed="chatCollapsed = $event"
        @update:composer-text="composerText = $event"
        @update:composer-target-agent-id="composerTargetAgentId = $event"
        @open-artifacts="handleOpenArtifacts"
        @send="handleComposerSend"
      />
    </section>

    <pre class="beeroom-e2e-state" data-testid="beeroom-e2e-state">{{ stateSnapshot }}</pre>
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import BeeroomCanvasChatPanel from '@/components/beeroom/BeeroomCanvasChatPanel.vue';
import type {
  ComposerTargetOption,
  MissionChatMessage
} from '@/components/beeroom/beeroomCanvasChatModel';
import {
  BEEROOM_SUBAGENT_REPLY_SORT_ORDER,
  BEEROOM_SUBAGENT_REQUEST_SORT_ORDER,
  compareMissionChatMessages
} from '@/components/beeroom/beeroomCanvasChatModel';
import type { BeeroomMissionSubagentItem } from '@/components/beeroom/beeroomMissionSubagentState';
import BeeroomSwarmCanvasPane from '@/components/beeroom/canvas/BeeroomSwarmCanvasPane.vue';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';
import { chatDebugLog } from '@/utils/chatDebug';

type HarnessScenario =
  | 'idle'
  | 'long-thread'
  | 'worker-shadow'
  | 'real-subagent-running'
  | 'real-subagent-dormant';

type HarnessApi = {
  loadScenario: (name: HarnessScenario) => void;
  appendTailAssistantMessage: () => void;
  setChatCollapsed: (value: boolean) => void;
  snapshot: () => string;
};

declare global {
  interface Window {
    __beeroomE2EHarness?: HarnessApi;
  }
}

const MOTHER_AGENT_ID = 'mother-agent';
const WORKER_AGENT_ID = 'worker-agent-1';
const SUBAGENT_AGENT_ID = 'subagent-agent-1';
const MOTHER_NAME = '默认智能体';
const WORKER_NAME = '工蜂一号';
const SUBAGENT_NAME = '子智能体';

const group = {
  group_id: 'e2e-group',
  mother_agent_id: MOTHER_AGENT_ID,
  mother_agent_name: MOTHER_NAME,
  members: []
} as any;

const mission = {
  mission_id: 'e2e-mission',
  team_run_id: 'e2e-team-run',
  mother_agent_id: MOTHER_AGENT_ID,
  entry_agent_id: MOTHER_AGENT_ID,
  completion_status: 'running',
  tasks: [
    {
      task_id: 'task-worker-1',
      agent_id: WORKER_AGENT_ID,
      status: 'running',
      updated_time: 1700000002
    }
  ]
} as any;

const agents = [
  {
    agent_id: MOTHER_AGENT_ID,
    name: MOTHER_NAME,
    idle: false,
    active_session_total: 1
  },
  {
    agent_id: WORKER_AGENT_ID,
    name: WORKER_NAME,
    idle: true,
    active_session_total: 0
  }
] as any[];

const composerTargetOptions: ComposerTargetOption[] = [
  { agentId: MOTHER_AGENT_ID, label: `${MOTHER_NAME} (Mother)`, role: 'mother' },
  { agentId: WORKER_AGENT_ID, label: `${WORKER_NAME} (Worker)`, role: 'worker' }
];

const chatCollapsed = ref(false);
const composerText = ref('');
const composerSending = ref(false);
const composerTargetAgentId = ref(MOTHER_AGENT_ID);
const messages = ref<MissionChatMessage[]>([]);
const dispatchPreview = ref<BeeroomSwarmDispatchPreview | null>(null);
const subagentsByTask = ref<Record<string, BeeroomMissionSubagentItem[]>>({});
const scenarioName = ref<HarnessScenario>('idle');

const logHarness = (event: string, payload?: unknown) => {
  chatDebugLog('beeroom.e2e-harness', event, payload);
};

const createAvatarDataUri = (label: string, background: string) => {
  const initials = String(label || '').trim().slice(0, 1) || '?';
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="16" fill="${background}"/><text x="32" y="39" font-size="28" text-anchor="middle" fill="#fff" font-family="Segoe UI, Arial, sans-serif">${initials}</text></svg>`;
  return `data:image/svg+xml;charset=UTF-8,${encodeURIComponent(svg)}`;
};

const avatarMap: Record<string, string> = {
  [MOTHER_AGENT_ID]: createAvatarDataUri('母', '#f59e0b'),
  [WORKER_AGENT_ID]: createAvatarDataUri('工', '#3b82f6'),
  [SUBAGENT_AGENT_ID]: createAvatarDataUri('子', '#22c55e'),
  user: createAvatarDataUri('用', '#64748b')
};

const formatTimeLabel = (time: number) => {
  const date = new Date(time * 1000);
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}:${String(
    date.getSeconds()
  ).padStart(2, '0')}`;
};

const createMessage = (
  partial: Partial<MissionChatMessage> & Pick<MissionChatMessage, 'key' | 'senderName' | 'body' | 'time' | 'tone'>
): MissionChatMessage => ({
  key: partial.key,
  senderName: partial.senderName,
  senderAgentId: partial.senderAgentId || '',
  avatarImageUrl: partial.avatarImageUrl || '',
  mention: partial.mention || '',
  body: partial.body,
  meta: partial.meta || '',
  time: partial.time,
  timeLabel: partial.timeLabel || formatTimeLabel(partial.time),
  tone: partial.tone,
  sortOrder: partial.sortOrder
});

const createSubagentItem = (
  partial: Partial<BeeroomMissionSubagentItem>
): BeeroomMissionSubagentItem =>
  ({
    key: String(partial.key || partial.sessionId || partial.runId || 'subagent-key'),
    sessionId: String(partial.sessionId || 'sess_subagent_real'),
    runId: String(partial.runId || 'run_subagent_real'),
    runKind: String(partial.runKind || 'subagent'),
    requestedBy: String(partial.requestedBy || 'subagent_control'),
    spawnedBy: String(partial.spawnedBy || ''),
    agentId: String(partial.agentId || SUBAGENT_AGENT_ID),
    title: String(partial.title || SUBAGENT_NAME),
    label: String(partial.label || SUBAGENT_NAME),
    status: String(partial.status || 'running'),
    summary: String(partial.summary || ''),
    userMessage: String(partial.userMessage || ''),
    assistantMessage: String(partial.assistantMessage || ''),
    errorMessage: String(partial.errorMessage || ''),
    updatedTime: Number(partial.updatedTime || 1700000004),
    terminal: partial.terminal === true,
    failed: partial.failed === true,
    depth: partial.depth ?? 1,
    role: String(partial.role || 'worker'),
    controlScope: String(partial.controlScope || 'dispatch'),
    spawnMode: String(partial.spawnMode || 'auto'),
    strategy: String(partial.strategy || ''),
    dispatchLabel: String(partial.dispatchLabel || 'subagent_control'),
    controllerSessionId: String(partial.controllerSessionId || 'sess_mother_main'),
    parentSessionId: String(partial.parentSessionId || 'sess_mother_main'),
    parentTurnRef: String(partial.parentTurnRef || 'turn-1'),
    parentUserRound: Number(partial.parentUserRound || 1),
    parentModelRound: Number(partial.parentModelRound || 2)
  }) as BeeroomMissionSubagentItem;

const resolveAgentAvatarImageByAgentId = (agentId: unknown) => avatarMap[String(agentId || '').trim()] || '';
const resolveMessageAvatarImage = (message: MissionChatMessage) =>
  String(message.avatarImageUrl || '').trim() ||
  resolveAgentAvatarImageByAgentId(message.senderAgentId || (message.tone === 'user' ? 'user' : ''));
const avatarLabel = (value: unknown) => String(value || '').trim().slice(0, 1).toUpperCase() || '?';

const displayMessages = computed(() => [...messages.value].sort(compareMissionChatMessages));

const handleOpenArtifacts = () => {
  logHarness('open-artifacts');
};

const resetHarness = () => {
  chatCollapsed.value = false;
  composerText.value = '';
  composerSending.value = false;
  composerTargetAgentId.value = MOTHER_AGENT_ID;
  messages.value = [];
  dispatchPreview.value = null;
  subagentsByTask.value = {};
};

const setMessages = (nextMessages: MissionChatMessage[]) => {
  messages.value = [...nextMessages].sort(compareMissionChatMessages);
};

const appendTailAssistantMessage = () => {
  const lastTime = Math.max(...displayMessages.value.map((message) => Number(message.time || 0)), 1700000010);
  messages.value = [...displayMessages.value, createMessage({
    key: `assistant-tail:${lastTime + 1}`,
    senderName: MOTHER_NAME,
    senderAgentId: MOTHER_AGENT_ID,
    avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
    mention: '用户',
    body: `尾部追加回复 ${lastTime + 1}`,
    time: lastTime + 1,
    tone: 'mother'
  })].sort(compareMissionChatMessages);
  logHarness('append-tail-assistant-message', {
    messageCount: messages.value.length,
    lastMessageKey: messages.value[messages.value.length - 1]?.key || ''
  });
};

const buildLongThreadMessages = () => {
  const next: MissionChatMessage[] = [];
  for (let index = 0; index < 26; index += 1) {
    const userTime = 1700000100 + index * 2;
    const assistantTime = userTime + 1;
    next.push(
      createMessage({
        key: `user:${index}`,
        senderName: '用户',
        avatarImageUrl: avatarMap.user,
        mention: MOTHER_NAME,
        body: `用户消息 ${index + 1}\n\n这是一段较长的内容，用于制造右侧栏滚动高度，并验证用户向上滚动后不会被强制拉回到底部。`.repeat(4),
        time: userTime,
        tone: 'user'
      }),
      createMessage({
        key: `assistant:${index}`,
        senderName: MOTHER_NAME,
        senderAgentId: MOTHER_AGENT_ID,
        avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
        mention: '用户',
        body: `母蜂回复 ${index + 1}\n\n- 保持主线程\n- 同步右栏\n- 不强制回到底部\n- 等待最终回复追平`.repeat(2),
        time: assistantTime,
        tone: 'mother'
      })
    );
  }
  return next;
};

const loadScenario = (name: HarnessScenario) => {
  resetHarness();
  scenarioName.value = name;
  switch (name) {
    case 'idle':
      setMessages([
        createMessage({
          key: 'idle:user',
          senderName: '用户',
          avatarImageUrl: avatarMap.user,
          mention: MOTHER_NAME,
          body: '你好，母蜂。',
          time: 1700000000,
          tone: 'user'
        }),
        createMessage({
          key: 'idle:mother',
          senderName: MOTHER_NAME,
          senderAgentId: MOTHER_AGENT_ID,
          avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
          mention: '用户',
          body: '主线程正常保持。',
          time: 1700000001,
          tone: 'mother'
        })
      ]);
      dispatchPreview.value = {
        sessionId: 'sess_mother_main',
        targetAgentId: MOTHER_AGENT_ID,
        targetName: MOTHER_NAME,
        status: 'running',
        summary: 'mother-active',
        dispatchLabel: 'main-thread',
        updatedTime: 1700000001,
        subagents: []
      };
      break;
    case 'long-thread':
      setMessages(buildLongThreadMessages());
      dispatchPreview.value = {
        sessionId: 'sess_mother_main',
        targetAgentId: MOTHER_AGENT_ID,
        targetName: MOTHER_NAME,
        status: 'running',
        summary: 'long-thread',
        dispatchLabel: 'main-thread',
        updatedTime: 1700000138,
        subagents: []
      };
      break;
    case 'worker-shadow':
      setMessages([
        createMessage({
          key: 'shadow:user',
          senderName: '用户',
          avatarImageUrl: avatarMap.user,
          mention: MOTHER_NAME,
          body: '请调用蜂群工具唤起工蜂，但不要投影成子智能体。',
          time: 1700000200,
          tone: 'user'
        }),
        createMessage({
          key: 'shadow:mother',
          senderName: MOTHER_NAME,
          senderAgentId: MOTHER_AGENT_ID,
          avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
          mention: '用户',
          body: '收到，准备唤起工蜂。',
          time: 1700000201,
          tone: 'mother'
        })
      ]);
      dispatchPreview.value = {
        sessionId: 'sess_worker_main',
        targetAgentId: WORKER_AGENT_ID,
        targetName: WORKER_NAME,
        status: 'running',
        summary: 'worker-shadow',
        dispatchLabel: 'agent_swarm',
        updatedTime: 1700000202,
        subagents: [
          createSubagentItem({
            key: 'worker-shadow',
            sessionId: 'sess_worker_shadow',
            runId: 'run_worker_shadow',
            runKind: 'swarm',
            requestedBy: 'agent_swarm',
            agentId: WORKER_AGENT_ID,
            label: WORKER_NAME,
            title: WORKER_NAME,
            dispatchLabel: 'agent_swarm'
          })
        ]
      };
      break;
    case 'real-subagent-running':
      setMessages([
        createMessage({
          key: 'real:user',
          senderName: '用户',
          avatarImageUrl: avatarMap.user,
          mention: MOTHER_NAME,
          body: '请用子智能体工具派生一个子智能体。',
          time: 1700000300,
          tone: 'user'
        }),
        createMessage({
          key: 'real:mother',
          senderName: MOTHER_NAME,
          senderAgentId: MOTHER_AGENT_ID,
          avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
          mention: '用户',
          body: '收到，开始创建子智能体。',
          time: 1700000301,
          tone: 'mother'
        }),
        createMessage({
          key: 'real:subagent-request',
          senderName: WORKER_NAME,
          senderAgentId: WORKER_AGENT_ID,
          avatarImageUrl: avatarMap[WORKER_AGENT_ID],
          mention: SUBAGENT_NAME,
          body: '请检查蜂群右栏的最终回复同步。',
          time: 1700000302,
          tone: 'worker',
          sortOrder: BEEROOM_SUBAGENT_REQUEST_SORT_ORDER
        }),
        createMessage({
          key: 'real:subagent-reply',
          senderName: SUBAGENT_NAME,
          senderAgentId: SUBAGENT_AGENT_ID,
          avatarImageUrl: avatarMap[SUBAGENT_AGENT_ID],
          mention: WORKER_NAME,
          body: '我正在处理这个问题。',
          time: 1700000303,
          tone: 'worker',
          sortOrder: BEEROOM_SUBAGENT_REPLY_SORT_ORDER
        })
      ]);
      dispatchPreview.value = {
        sessionId: 'sess_worker_main',
        targetAgentId: WORKER_AGENT_ID,
        targetName: WORKER_NAME,
        status: 'running',
        summary: 'subagent-running',
        dispatchLabel: 'subagent_control',
        updatedTime: 1700000304,
        subagents: [
          createSubagentItem({
            key: 'real-subagent',
            sessionId: 'sess_subagent_real',
            runId: 'run_subagent_real',
            status: 'running',
            summary: 'running',
            userMessage: '请检查蜂群右栏的最终回复同步。',
            assistantMessage: '我正在处理这个问题。',
            dispatchLabel: 'subagent_control'
          })
        ]
      };
      break;
    case 'real-subagent-dormant':
      loadScenario('real-subagent-running');
      scenarioName.value = name;
      dispatchPreview.value = {
        ...(dispatchPreview.value as BeeroomSwarmDispatchPreview),
        status: 'completed',
        summary: 'subagent-completed',
        updatedTime: 1700000310,
        subagents: [
          createSubagentItem({
            key: 'real-subagent',
            sessionId: 'sess_subagent_real',
            runId: 'run_subagent_real',
            status: 'completed',
            summary: 'completed',
            userMessage: '请检查蜂群右栏的最终回复同步。',
            assistantMessage: '检查完成，终态已同步。',
            updatedTime: 1700000310,
            terminal: true,
            dispatchLabel: 'subagent_control'
          })
        ]
      };
      messages.value = [
        ...messages.value,
        createMessage({
          key: 'real:mother-final',
          senderName: MOTHER_NAME,
          senderAgentId: MOTHER_AGENT_ID,
          avatarImageUrl: avatarMap[MOTHER_AGENT_ID],
          mention: '用户',
          body: '子智能体检查完成，最终回复已进入右栏。',
          time: 1700000311,
          tone: 'mother'
        })
      ].sort(compareMissionChatMessages);
      break;
  }
  logHarness('load-scenario', {
    scenario: name,
    messageCount: messages.value.length,
    dispatchSessionId: dispatchPreview.value?.sessionId || '',
    dispatchStatus: dispatchPreview.value?.status || '',
    subagentCount: dispatchPreview.value?.subagents?.length || 0
  });
};

const handleComposerSend = () => {
  appendTailAssistantMessage();
};

const stateSnapshot = computed(() =>
  JSON.stringify(
    {
      scenario: scenarioName.value,
      collapsed: chatCollapsed.value,
      composerTargetAgentId: composerTargetAgentId.value,
      messageKeys: displayMessages.value.map((message) => ({
        key: message.key,
        tone: message.tone,
        sender: message.senderName
      })),
      dispatchPreview: dispatchPreview.value
        ? {
            sessionId: dispatchPreview.value.sessionId,
            status: dispatchPreview.value.status,
            targetAgentId: dispatchPreview.value.targetAgentId,
            subagents: dispatchPreview.value.subagents.map((item) => ({
              sessionId: item.sessionId,
              status: item.status
            }))
          }
        : null
    },
    null,
    2
  )
);

watch(
  () => stateSnapshot.value,
  (snapshot, previousSnapshot) => {
    if (snapshot === previousSnapshot) return;
    logHarness('state-changed', JSON.parse(snapshot));
  },
  { immediate: true }
);

onMounted(() => {
  loadScenario('idle');
  window.__beeroomE2EHarness = {
    loadScenario,
    appendTailAssistantMessage,
    setChatCollapsed: (value: boolean) => {
      chatCollapsed.value = value;
    },
    snapshot: () => stateSnapshot.value
  };
});

onBeforeUnmount(() => {
  if (window.__beeroomE2EHarness) {
    delete window.__beeroomE2EHarness;
  }
});
</script>

<style scoped>
.beeroom-e2e {
  min-height: 100vh;
  height: 100vh;
  padding: 20px;
  box-sizing: border-box;
  overflow: hidden;
  background:
    radial-gradient(circle at top left, rgba(245, 158, 11, 0.12), transparent 34%),
    radial-gradient(circle at right center, rgba(59, 130, 246, 0.14), transparent 28%),
    linear-gradient(180deg, #081018, #060c12);
  color: #e5e7eb;
}

.beeroom-e2e-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 16px;
}

.beeroom-e2e-toolbar button {
  border: 1px solid rgba(148, 163, 184, 0.24);
  border-radius: 999px;
  background: rgba(15, 23, 42, 0.86);
  color: #e5e7eb;
  padding: 10px 14px;
  cursor: pointer;
}

.beeroom-e2e-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 344px;
  height: calc(100vh - 148px);
  min-height: 760px;
  max-height: calc(100vh - 148px);
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 24px;
  overflow: hidden;
  background: rgba(5, 8, 12, 0.82);
}

.beeroom-e2e-canvas {
  min-width: 0;
  min-height: 0;
}

.beeroom-e2e-state {
  margin-top: 14px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 16px;
  background: rgba(2, 6, 23, 0.72);
  color: #cbd5e1;
  padding: 12px;
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
}

@media (max-width: 1100px) {
  .beeroom-e2e-body {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
