<template>
  <div class="chat-shell">
    <div class="app-shell">
      <header class="topbar">
        <div class="brand">
          <div class="brand-mark">AI</div>
          <div class="brand-meta">
            <div class="brand-title">智能体对话系统</div>
            <div class="brand-sub">
              AI 助手 · <span class="model-tag">通用问答</span>
              <span v-if="demoMode" class="demo-badge">演示模式</span>
            </div>
          </div>
        </div>
        <div class="topbar-actions">
          <button
            class="new-chat-btn"
            type="button"
            title="新建会话"
            aria-label="新建会话"
            @click="handleCreateSession"
          >
            新建会话
          </button>
          <button
            class="topbar-icon-btn"
            type="button"
            title="自建工具"
            aria-label="自建工具"
            @click="userToolsVisible = true"
          >
            <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
              <rect x="3" y="7" width="18" height="12" rx="2" />
              <path d="M9 7V5h6v2" />
              <path d="M3 13h18" />
            </svg>
          </button>
          <button
            class="topbar-icon-btn"
            type="button"
            title="共享工具"
            aria-label="共享工具"
            @click="sharedToolsVisible = true"
          >
            <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="7" cy="12" r="3" />
              <circle cx="17" cy="12" r="3" />
              <path d="M10 12h4" />
            </svg>
          </button>
          <button
            class="topbar-icon-btn"
            type="button"
            title="额外提示设置"
            aria-label="额外提示设置"
            @click="extraPromptVisible = true"
          >
            <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 3l2.2 4.5L19 9l-4.8 1.5L12 15l-2.2-4.5L5 9l4.8-1.5L12 3z" />
            </svg>
          </button>
          <ThemeToggle />
          <div class="topbar-user">
            <div class="user-meta">
              <div class="user-name">{{ currentUser?.username || '访客' }}</div>
              <div class="user-level">等级 {{ currentUser?.access_level || '-' }}</div>
            </div>
            <button class="logout-btn" @click="handleLogout">退出</button>
          </div>
        </div>
      </header>

      <div class="main-grid">
        <aside class="sidebar">
          <div class="glass-card info-panel">
            <WorkspacePanel />
          </div>

          <div class="glass-card history-panel">
            <div class="panel-header">
              <span class="history-title">历史记录</span>
            </div>
            <div
              ref="historyListRef"
              :class="['history-container', { virtual: historyVirtual }]"
              @scroll="handleHistoryScroll"
            >
              <div
                v-if="historyPaddingTop"
                class="history-spacer"
                :style="{ height: `${historyPaddingTop}px` }"
              ></div>
              <div
                v-for="session in historySessions"
                :key="session.id"
                :class="['history-item', session.id === chatStore.activeSessionId ? 'active' : '']"
                @click="handleSelectSession(session.id)"
              >
                <div class="history-info">
                  <div class="history-title-text">{{ formatTitle(session.title) }}</div>
                  <span class="history-time">{{ formatTime(session.updated_at) }}</span>
                </div>
                <button
                  class="history-delete-btn"
                  type="button"
                  title="删除会话"
                  aria-label="删除会话"
                  @click.stop="handleDeleteSession(session.id)"
                >
                  <svg class="history-delete-icon" viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M4 7h16" />
                    <path d="M9 7V5h6v2" />
                    <path d="M7 7l1 12h8l1-12" />
                    <path d="M10 11v5M14 11v5" />
                  </svg>
                </button>
              </div>
              <div
                v-if="historyPaddingBottom"
                class="history-spacer"
                :style="{ height: `${historyPaddingBottom}px` }"
              ></div>
            </div>
          </div>
        </aside>

        <section class="chat-panel">
          <div class="messages-container" @click="handleMessageClick">
            <div
              v-for="(message, index) in chatStore.messages"
              :key="index"
              :class="['message', message.role === 'user' ? 'from-user' : 'from-ai']"
            >
              <div class="avatar" :class="message.role === 'user' ? 'user-avatar' : 'ai-avatar'">
                {{ message.role === 'user' ? '你' : 'AI' }}
              </div>
              <div class="message-content">
                <div class="message-header">
                  <div class="message-header-left">
                    <div class="message-role">{{ message.role === 'user' ? '你' : '智能体' }}</div>
                    <MessageThinking
                      v-if="message.role === 'assistant'"
                      :content="message.reasoning"
                      :streaming="message.reasoningStreaming"
                    />
                  </div>
                  <div v-if="message.role === 'assistant'" class="message-actions">
                    <div class="message-time">{{ formatTime(message.created_at) }}</div>
                    <button
                      class="message-copy-btn"
                      type="button"
                      title="复制回复"
                      aria-label="复制回复"
                      @click="handleCopyMessage(message)"
                    >
                      <svg class="message-copy-icon" viewBox="0 0 24 24" aria-hidden="true">
                        <rect x="9" y="9" width="10" height="10" rx="2" />
                        <path d="M7 15H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h7a2 2 0 0 1 2 2v1" />
                      </svg>
                      <span>复制</span>
                    </button>
                  </div>
                  <div v-else class="message-meta">
                    <div class="message-time">{{ formatTime(message.created_at) }}</div>
                  </div>
                </div>
                <MessageWorkflow
                  v-if="message.role === 'assistant'"
                  :items="message.workflowItems || []"
                  :loading="message.workflowStreaming"
                  :visible="message.workflowStreaming || (message.workflowItems && message.workflowItems.length > 0)"
                />
                <div
                  v-if="shouldShowMessageText(message)"
                  class="message-text"
                  :class="{ greeting: message.isGreeting }"
                >
                  <template v-if="message.isGreeting">
                    <div class="greeting-text">{{ message.content }}</div>
                    <el-tooltip
                      ref="abilityTooltipRef"
                      placement="bottom-end"
                      :show-after="160"
                      :teleported="true"
                      :popper-options="abilityTooltipOptions"
                      popper-class="ability-tooltip-popper"
                      @show="handleAbilityTooltipShow"
                      @hide="handleAbilityTooltipHide"
                    >
                      <template #content>
                        <div class="ability-tooltip">
                          <div class="ability-header">
                            <span class="ability-title">智能体能力</span>
                            <span class="ability-sub">工具 · 技能</span>
                          </div>
                          <div v-if="toolSummaryLoading && !hasAbilitySummary" class="ability-muted">
                            能力加载中...
                          </div>
                          <div v-else-if="toolSummaryError" class="ability-error">
                            {{ toolSummaryError }}
                          </div>
                          <template v-else>
                            <div v-if="!hasAbilitySummary" class="ability-muted">暂无可用工具/技能</div>
                            <div v-else class="ability-scroll">
                              <div class="ability-section">
                                <div class="ability-section-title">
                                  <span>工具</span>
                                  <span class="ability-count">{{ abilitySummary.tools.length }}</span>
                                </div>
                                <div v-if="abilitySummary.tools.length" class="ability-item-list">
                                  <div
                                    v-for="tool in abilitySummary.tools"
                                    :key="`tool-${tool.name}`"
                                    class="ability-item tool"
                                  >
                                    <div class="ability-item-name">{{ tool.name }}</div>
                                    <div
                                      class="ability-item-desc"
                                      :class="{ 'is-empty': !tool.description }"
                                    >
                                      {{ tool.description || '暂无描述' }}
                                    </div>
                                  </div>
                                </div>
                                <div v-else class="ability-empty">暂无工具</div>
                              </div>
                              <div class="ability-section">
                                <div class="ability-section-title">
                                  <span>技能</span>
                                  <span class="ability-count">{{ abilitySummary.skills.length }}</span>
                                </div>
                                <div v-if="abilitySummary.skills.length" class="ability-item-list">
                                  <div
                                    v-for="skill in abilitySummary.skills"
                                    :key="`skill-${skill.name}`"
                                    class="ability-item skill"
                                  >
                                    <div class="ability-item-name">{{ skill.name }}</div>
                                    <div
                                      class="ability-item-desc"
                                      :class="{ 'is-empty': !skill.description }"
                                    >
                                      {{ skill.description || '暂无描述' }}
                                    </div>
                                  </div>
                                </div>
                                <div v-else class="ability-empty">暂无技能</div>
                              </div>
                            </div>
                          </template>
                        </div>
                      </template>
                      <button
                        class="prompt-preview-btn"
                        type="button"
                        title="提示词预览"
                        aria-label="提示词预览"
                        :disabled="promptPreviewLoading"
                        @click="openPromptPreview"
                      >
                        <svg class="prompt-preview-icon" viewBox="0 0 24 24" aria-hidden="true">
                          <path
                            d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"
                          />
                          <circle cx="12" cy="12" r="3.5" />
                        </svg>
                      </button>
                    </el-tooltip>
                  </template>
                  <template v-else-if="message.role === 'assistant'">
                    <div class="markdown-body" v-html="renderAssistantMarkdown(message.content)"></div>
                  </template>
                  <template v-else>
                    {{ message.content }}
                  </template>
                </div>
              </div>
            </div>
          </div>

          <ChatComposer
            :key="composerKey"
            :loading="chatStore.loading"
            :demo-mode="demoMode"
            @send="handleComposerSend"
            @stop="handleStop"
          />
        </section>
      </div>
    </div>

    <UserToolsModal v-model="userToolsVisible" />
    <UserSharedToolsModal v-model="sharedToolsVisible" />
    <UserExtraPromptModal v-model="extraPromptVisible" />

    <el-dialog
      v-model="promptPreviewVisible"
      class="system-prompt-dialog"
      width="760px"
      top="8vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="system-prompt-header">
          <div class="system-prompt-title">系统提示词预览</div>
          <button class="icon-btn" type="button" @click="closePromptPreview">×</button>
        </div>
      </template>
      <div class="system-prompt-body">
        <div v-if="promptPreviewLoading" class="muted">正在加载...</div>
        <pre v-else class="system-prompt-content" v-html="promptPreviewHtml"></pre>
      </div>
      <template #footer>
        <el-button class="system-prompt-footer-btn" @click="closePromptPreview">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchRealtimeSystemPrompt, fetchSessionSystemPrompt } from '@/api/chat';
import { fetchUserToolsSummary } from '@/api/userTools';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageWorkflow from '@/components/chat/MessageWorkflow.vue';
import ThemeToggle from '@/components/chat/ThemeToggle.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import UserExtraPromptModal from '@/components/user-tools/UserExtraPromptModal.vue';
import UserSharedToolsModal from '@/components/user-tools/UserSharedToolsModal.vue';
import UserToolsModal from '@/components/user-tools/UserToolsModal.vue';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { copyText } from '@/utils/clipboard';
import { renderMarkdown } from '@/utils/markdown';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import { isDemoMode } from '@/utils/demo';
import { collectAbilityDetails } from '@/utils/toolSummary';
import { loadSharedToolSelection } from '@/utils/toolSelection';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();
const currentUser = computed(() => authStore.user);
// 演示模式用于快速体验
const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const draftKey = ref(0);
const composerKey = computed(() =>
  chatStore.activeSessionId ? `session-${chatStore.activeSessionId}` : `draft-${draftKey.value}`
);
const userToolsVisible = ref(false);
const sharedToolsVisible = ref(false);
const extraPromptVisible = ref(false);
const historyListRef = ref(null);
const historyScrollTop = ref(0);
// 系统提示词预览状态
const promptPreviewVisible = ref(false);
const promptPreviewLoading = ref(false);
const promptPreviewContent = ref('');
const promptToolSummary = ref(null);
const toolSummaryLoading = ref(false);
const toolSummaryError = ref('');
const abilityTooltipRef = ref(null);
const abilityTooltipVisible = ref(false);
const abilityTooltipOptions = {
  strategy: 'fixed',
  modifiers: [
    { name: 'offset', options: { offset: [0, 10] } },
    { name: 'shift', options: { padding: 8 } },
    { name: 'flip', options: { padding: 8, fallbackPlacements: ['top', 'bottom', 'right', 'left'] } },
    {
      name: 'preventOverflow',
      options: { padding: 8, altAxis: true, boundary: 'viewport' }
    }
  ]
};
const promptPreviewHtml = computed(() => {
  const content = promptPreviewContent.value || '暂无系统提示词';
  return renderSystemPromptHighlight(content, promptToolSummary.value || {});
});
// 能力悬浮提示使用的工具/技能明细
const abilitySummary = computed(() => collectAbilityDetails(promptToolSummary.value || {}));
const hasAbilitySummary = computed(
  () => abilitySummary.value.tools.length > 0 || abilitySummary.value.skills.length > 0
);

const HISTORY_ROW_HEIGHT = 52;
const HISTORY_OVERSCAN = 6;
const historyTotal = computed(() => chatStore.sessions.length);
const historyVirtual = computed(() => historyTotal.value > 40);
const historyViewportHeight = computed(() => historyListRef.value?.clientHeight || 0);
const historyVisibleCount = computed(() =>
  Math.max(1, Math.ceil(historyViewportHeight.value / HISTORY_ROW_HEIGHT))
);
const historyStartIndex = computed(() => {
  if (!historyVirtual.value) return 0;
  const raw = Math.floor(historyScrollTop.value / HISTORY_ROW_HEIGHT) - HISTORY_OVERSCAN;
  const maxStart = Math.max(0, historyTotal.value - historyVisibleCount.value);
  return Math.max(0, Math.min(raw, maxStart));
});
const historyEndIndex = computed(() => {
  if (!historyVirtual.value) return historyTotal.value;
  return Math.min(
    historyTotal.value,
    historyStartIndex.value + historyVisibleCount.value + HISTORY_OVERSCAN * 2
  );
});
const historySessions = computed(() =>
  historyVirtual.value
    ? chatStore.sessions.slice(historyStartIndex.value, historyEndIndex.value)
    : chatStore.sessions
);
const historyPaddingTop = computed(() =>
  historyVirtual.value ? historyStartIndex.value * HISTORY_ROW_HEIGHT : 0
);
const historyPaddingBottom = computed(() =>
  historyVirtual.value ? Math.max(0, (historyTotal.value - historyEndIndex.value) * HISTORY_ROW_HEIGHT) : 0
);

const init = async () => {
  if (demoMode.value || !authStore.user) {
    await authStore.loadProfile();
  }
  await chatStore.loadSessions();
  if (chatStore.sessions.length > 0) {
    await chatStore.loadSessionDetail(chatStore.sessions[0].id);
  } else {
    chatStore.openDraftSession();
  }
};

const handleCreateSession = () => {
  chatStore.openDraftSession();
  draftKey.value += 1;
};

const handleSelectSession = async (sessionId) => {
  await chatStore.loadSessionDetail(sessionId);
};

const handleDeleteSession = async (sessionId) => {
  try {
    await ElMessageBox.confirm('确认删除该会话记录吗？', '提示', {
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  await chatStore.deleteSession(sessionId);
};

const shouldShowMessageText = (message) => {
  if (!message) return false;
  if (message.role !== 'assistant') return true;
  return Boolean(String(message.content || '').trim());
};

// AI 回复使用 Markdown 渲染，主要用于表格等富文本展示
const renderAssistantMarkdown = (content) => renderMarkdown(content || '');

const handleComposerSend = async ({ content, attachments }) => {
  const payloadAttachments = Array.isArray(attachments) ? attachments : [];
  if (!content && payloadAttachments.length === 0) return;
  await chatStore.sendMessage(content, { attachments: payloadAttachments });
};

const handleStop = async () => {
  await chatStore.stopStream();
};

const handleCopyMessage = async (message) => {
  const content = String(message?.content || '').trim();
  if (!content) {
    ElMessage.warning('暂无可复制内容');
    return;
  }
  const ok = await copyText(content);
  if (ok) {
    ElMessage.success('回复已复制');
  } else {
    ElMessage.error('复制失败');
  }
};

const handleMessageClick = async (event) => {
  const target = event?.target;
  if (!(target instanceof Element)) return;
  const copyButton = target.closest('.ai-code-copy');
  if (!copyButton) return;
  event.preventDefault();
  const codeBlock = copyButton.closest('.ai-code-block');
  const codeElement = codeBlock?.querySelector('code');
  const codeText = codeElement?.textContent || '';
  if (!codeText.trim()) {
    ElMessage.warning('暂无可复制内容');
    return;
  }
  const ok = await copyText(codeText);
  if (ok) {
    ElMessage.success('代码已复制');
  } else {
    ElMessage.error('复制失败');
  }
};

const handleLogout = () => {
  if (demoMode.value) {
    router.push('/login');
    return;
  }
  authStore.logout();
  router.push('/login');
};

const handleHistoryScroll = (event) => {
  historyScrollTop.value = event.target.scrollTop || 0;
};

// 触发 Popper 重新计算，避免首帧内容变化导致溢出
const updateAbilityTooltip = async () => {
  await nextTick();
  const tooltip = abilityTooltipRef.value;
  if (tooltip?.updatePopper) {
    tooltip.updatePopper();
  } else if (tooltip?.popperRef?.update) {
    tooltip.popperRef.update();
  }
  requestAnimationFrame(() => {
    if (tooltip?.updatePopper) {
      tooltip.updatePopper();
    } else if (tooltip?.popperRef?.update) {
      tooltip.popperRef.update();
    }
  });
};

// 读取工具与技能汇总信息，供提示词预览与悬浮提示使用
const loadToolSummary = async () => {
  if (toolSummaryLoading.value || promptToolSummary.value) {
    return promptToolSummary.value;
  }
  toolSummaryLoading.value = true;
  toolSummaryError.value = '';
  try {
    const toolsResult = await fetchUserToolsSummary();
    const payload = toolsResult?.data?.data || null;
    if (payload) {
      promptToolSummary.value = payload;
    }
    return payload;
  } catch (error) {
    toolSummaryError.value = error.response?.data?.detail || '能力信息加载失败';
    return null;
  } finally {
    toolSummaryLoading.value = false;
    if (abilityTooltipVisible.value) {
      await updateAbilityTooltip();
    }
  }
};

const handleAbilityTooltipShow = () => {
  abilityTooltipVisible.value = true;
  loadToolSummary();
  updateAbilityTooltip();
};

const handleAbilityTooltipHide = () => {
  abilityTooltipVisible.value = false;
};

// 打开系统提示词预览，优先读取会话快照
const openPromptPreview = async () => {
  promptPreviewVisible.value = true;
  promptPreviewLoading.value = true;
  promptPreviewContent.value = '';
  const toolSummaryPromise = loadToolSummary();
  try {
    const selectedShared = Array.from(loadSharedToolSelection(authStore.user?.id).values());
    const promptRequest = chatStore.activeSessionId
      ? fetchSessionSystemPrompt(chatStore.activeSessionId, {
          selected_shared_tools: selectedShared
        })
      : fetchRealtimeSystemPrompt({ selected_shared_tools: selectedShared });
    const promptResult = await promptRequest;
    await toolSummaryPromise;
    const payload = promptResult?.data?.data || {};
    promptPreviewContent.value = payload.prompt || '';
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '系统提示词加载失败');
    promptPreviewContent.value = '';
  } finally {
    promptPreviewLoading.value = false;
  }
};

const closePromptPreview = () => {
  promptPreviewVisible.value = false;
};

const formatTime = (value) => {
  if (!value) return '';
  if (typeof value === 'string') {
    return value.replace('T', ' ').replace('Z', '');
  }
  if (value instanceof Date) {
    return value.toISOString().replace('T', ' ').replace('Z', '');
  }
  return String(value);
};

const formatTitle = (title) => {
  const text = String(title || '').trim();
  if (!text) return '未命名会话';
  return text.length > 20 ? `${text.slice(0, 20)}...` : text;
};

onMounted(async () => {
  await init();
  loadToolSummary();
});

watch(
  () => chatStore.activeSessionId,
  (value, oldValue) => {
    if (!value && oldValue) {
      draftKey.value += 1;
    }
  }
);

watch(
  () => demoMode.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    await init();
    loadToolSummary();
  }
);

watch(
  () => abilitySummary.value,
  () => {
    if (abilityTooltipVisible.value) {
      updateAbilityTooltip();
    }
  },
  { deep: true }
);
</script>
