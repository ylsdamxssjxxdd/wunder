<template>
  <div class="chat-shell">
    <div class="app-shell">
      <header class="topbar">
        <div class="brand">
          <div class="brand-mark">AI</div>
          <div class="brand-meta">
            <div class="brand-title-row">
              <div class="brand-title">智能体交互系统</div>
              <div v-if="activeAgentLabel" class="agent-pill">
                <span class="agent-pill-label">当前智能体</span>
                <span class="agent-pill-name">{{ activeAgentLabel }}</span>
              </div>
            </div>
            <div class="brand-sub">
              <span v-if="demoMode" class="demo-badge">演示模式</span>
            </div>
          </div>
        </div>
        <div class="topbar-actions">
          <div v-if="isCompactLayout" class="topbar-compact-actions">
            <button
              class="topbar-panel-btn"
              type="button"
              title="临时文件区"
              aria-label="临时文件区"
              @click="openWorkspaceDialog"
            >
              <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7z" />
              </svg>
              <span class="topbar-panel-text">临时文件</span>
            </button>
            <button
              class="topbar-panel-btn"
              type="button"
              title="历史记录"
              aria-label="历史记录"
              @click="openHistoryDialog"
            >
              <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
                <circle cx="12" cy="12" r="9" />
                <path d="M12 7v6l4 2" />
              </svg>
              <span class="topbar-panel-text">历史记录</span>
            </button>
          </div>
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
            title="功能广场"
            aria-label="功能广场"
            @click="handleOpenPortal"
          >
            <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
              <rect x="3" y="3" width="7" height="7" rx="1.5" />
              <rect x="14" y="3" width="7" height="7" rx="1.5" />
              <rect x="3" y="14" width="7" height="7" rx="1.5" />
              <rect x="14" y="14" width="7" height="7" rx="1.5" />
            </svg>
          </button>
          <button
            class="topbar-icon-btn"
            type="button"
            title="调整工具"
            aria-label="调整工具"
            @click="openSessionTools"
          >
            <svg class="topbar-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M4 6h16" />
              <circle cx="9" cy="6" r="2" />
              <path d="M4 12h16" />
              <circle cx="15" cy="12" r="2" />
              <path d="M4 18h16" />
              <circle cx="7" cy="18" r="2" />
            </svg>
          </button>
          <ThemeToggle />
          <div class="topbar-user">
            <button
              class="user-meta user-meta-btn"
              type="button"
              aria-label="进入我的概况"
              @click="handleOpenProfile"
            >
              <div class="user-name">{{ currentUser?.username || '访客' }}</div>
              <div class="user-level">等级 {{ currentUser?.access_level || '-' }}</div>
            </button>
            <button class="logout-btn" type="button" @click="handleLogout">退出</button>
          </div>
        </div>
      </header>

      <div class="main-grid">
        <aside v-if="!isCompactLayout" class="sidebar">
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
          <div ref="messagesContainerRef" class="messages-container" @click="handleMessageClick">
            <div
              v-for="(message, index) in chatStore.messages"
              :key="index"
              :class="['message', message.role === 'user' ? 'from-user' : 'from-ai']"
            >
              <div
                class="avatar"
                :class="[
                  message.role === 'user' ? 'user-avatar' : 'ai-avatar',
                  { 'ai-avatar-working': message.role === 'assistant' && isAssistantStreaming(message) }
                ]"
                :aria-busy="message.role === 'assistant' && isAssistantStreaming(message) ? 'true' : 'false'"
              >
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
                  <div v-else class="message-actions">
                    <div class="message-time">{{ formatTime(message.created_at) }}</div>
                    <button
                      class="message-copy-btn"
                      type="button"
                      title="复制消息"
                      aria-label="复制消息"
                      @click="handleCopyMessage(message)"
                    >
                      <svg class="message-copy-icon" viewBox="0 0 24 24" aria-hidden="true">
                        <rect x="9" y="9" width="10" height="10" rx="2" />
                        <path d="M7 15H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h7a2 2 0 0 1 2 2v1" />
                      </svg>
                      <span>复制</span>
                    </button>
                  </div>
                </div>
                <MessageWorkflow
                  v-if="message.role === 'assistant'"
                  :items="message.workflowItems || []"
                  :loading="message.workflowStreaming"
                  :visible="
                    message.workflowStreaming ||
                    (message.workflowItems && message.workflowItems.length > 0) ||
                    hasPlan(message)
                  "
                  :plan="message.plan"
                  :plan-visible="message.planVisible"
                  @update:plan-visible="(value) => (message.planVisible = value)"
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
                    <div
                      v-if="isAssistantStreaming(message)"
                      class="markdown-body streaming"
                      v-text="message.content"
                    ></div>
                    <div v-else class="markdown-body" v-html="renderAssistantMarkdown(message)"></div>
                  </template>
                  <template v-else>
                    {{ message.content }}
                  </template>
                  <div v-if="shouldShowMessageStats(message)" class="message-stats">
                    <span
                      v-for="item in buildMessageStatsEntries(message)"
                      :key="item.label"
                      class="message-stat"
                    >
                      <span class="message-stat-label">{{ item.label }}：</span>
                      <span class="message-stat-value">{{ item.value }}</span>
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <InquiryPanel
            v-if="activeInquiryPanel"
            :panel="activeInquiryPanel.panel"
            @update:selected="handleInquirySelection"
          />
          <ChatComposer
            :key="composerKey"
            :loading="chatStore.loading"
            :demo-mode="demoMode"
            :inquiry-active="Boolean(activeInquiryPanel)"
            :inquiry-selection="inquirySelection"
            @send="handleComposerSend"
            @stop="handleStop"
          />
        </section>
      </div>
    </div>

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

    <el-dialog
      v-model="sessionToolsVisible"
      class="session-tools-dialog"
      width="780px"
      top="8vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="system-prompt-header">
          <div class="system-prompt-title">会话工具调整</div>
          <button class="icon-btn" type="button" @click="closeSessionTools">×</button>
        </div>
      </template>
      <div class="session-tools-body">
        <div class="session-tools-bar">
          <div class="session-tools-meta">
            可用 {{ availableToolCount }} 项，已选 {{ sessionToolSelectionCount }} 项
          </div>
        </div>
        <div v-if="sessionToolsDisabled" class="session-tools-muted">
          当前会话将禁用所有工具调用。
        </div>
        <div v-else class="session-tools-groups">
          <div v-if="!filteredToolGroups.length" class="session-tools-empty">
            暂无匹配工具
          </div>
          <div v-for="group in filteredToolGroups" :key="group.label" class="session-tools-group">
            <div class="session-tools-group-title">{{ group.label }}</div>
            <div class="session-tools-items">
              <label
                v-for="tool in group.items"
                :key="tool.name"
                class="session-tools-item"
              >
                <input
                  type="checkbox"
                  :checked="isSessionToolSelected(tool.name)"
                  @change="toggleSessionTool(tool.name, $event.target.checked)"
                />
                <div class="session-tools-item-info">
                  <div class="session-tools-item-name">{{ tool.name }}</div>
                  <div class="session-tools-item-desc">
                    {{ tool.description || '暂无描述' }}
                  </div>
                </div>
              </label>
            </div>
          </div>
        </div>
      </div>
      <template #footer>
        <el-button @click="resetSessionTools">恢复默认</el-button>
        <el-button @click="closeSessionTools">取消</el-button>
        <el-button type="primary" :loading="sessionToolsSaving" @click="saveSessionTools">
          应用
        </el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="imagePreviewVisible"
      class="image-preview-dialog"
      width="auto"
      top="6vh"
      :show-close="false"
      :close-on-click-modal="true"
      append-to-body
      @closed="closeImagePreview"
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">{{ imagePreviewTitle || '图片预览' }}</div>
          <button class="icon-btn" type="button" @click="closeImagePreview">×</button>
        </div>
      </template>
      <div class="image-preview-body">
        <img v-if="imagePreviewUrl" :src="imagePreviewUrl" class="image-preview-img" :alt="imagePreviewTitle" />
      </div>
    </el-dialog>

    <el-dialog
      v-model="workspaceDialogVisible"
      class="workspace-dialog compact-panel-dialog"
      width="90vw"
      top="6vh"
      :show-close="false"
      :append-to-body="false"
      :close-on-click-modal="true"
      destroy-on-close
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">临时文件区</div>
          <button class="icon-btn" type="button" @click="workspaceDialogVisible = false">×</button>
        </div>
      </template>
      <div class="panel-dialog-body">
        <WorkspacePanel />
      </div>
    </el-dialog>

    <el-dialog
      v-model="historyDialogVisible"
      class="workspace-dialog compact-panel-dialog"
      width="90vw"
      top="6vh"
      :show-close="false"
      :append-to-body="false"
      :close-on-click-modal="true"
      destroy-on-close
    >
      <template #header>
        <div class="image-preview-header">
          <div class="image-preview-title">历史记录</div>
          <button class="icon-btn" type="button" @click="historyDialogVisible = false">×</button>
        </div>
      </template>
      <div class="panel-dialog-body">
        <div class="history-panel">
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
      </div>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchRealtimeSystemPrompt, fetchSessionSystemPrompt } from '@/api/chat';
import { downloadWunderWorkspaceFile } from '@/api/workspace';
import { fetchUserToolsSummary } from '@/api/userTools';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import InquiryPanel from '@/components/chat/InquiryPanel.vue';
import MessageThinking from '@/components/chat/MessageThinking.vue';
import MessageWorkflow from '@/components/chat/MessageWorkflow.vue';
import ThemeToggle from '@/components/common/ThemeToggle.vue';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { copyText } from '@/utils/clipboard';
import { renderMarkdown } from '@/utils/markdown';
import { parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
import { onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import { isDemoMode } from '@/utils/demo';
import { collectAbilityDetails, collectAbilityNames } from '@/utils/toolSummary';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();
const agentStore = useAgentStore();
const currentUser = computed(() => authStore.user);
// 演示模式用于快速体验
const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => (demoMode.value ? '/demo' : '/app'));
const draftKey = ref(0);
const composerKey = computed(() =>
  chatStore.activeSessionId ? `session-${chatStore.activeSessionId}` : `draft-${draftKey.value}`
);
const inquirySelection = ref([]);
const historyListRef = ref(null);
const historyScrollTop = ref(0);
const messagesContainerRef = ref(null);
// 系统提示词预览状态
const promptPreviewVisible = ref(false);
const promptPreviewLoading = ref(false);
const promptPreviewContent = ref('');
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const workspaceDialogVisible = ref(false);
const historyDialogVisible = ref(false);
const isCompactLayout = ref(false);
const promptToolSummary = ref(null);
const toolSummaryLoading = ref(false);
const toolSummaryError = ref('');
const sessionToolsVisible = ref(false);
const sessionToolsSaving = ref(false);
const sessionToolSearch = ref('');
const sessionToolSelection = ref(new Set());
const sessionToolsDisabled = ref(false);
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
const TOOL_OVERRIDE_NONE = '__no_tools__';

const normalizeToolItemName = (item) => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  return item.name || item.tool_name || item.toolName || item.id || '';
};

const normalizeToolItem = (item) => {
  if (!item) return null;
  if (typeof item === 'string') {
    const name = item.trim();
    return name ? { name, description: '' } : null;
  }
  const name = String(item.name || '').trim();
  if (!name) return null;
  return {
    name,
    description: String(item.description || '').trim()
  };
};

const buildAllowedToolSet = (summary) => {
  const names = collectAbilityNames(summary || {});
  return new Set([...(names.tools || []), ...(names.skills || [])]);
};

const filterSummaryByNames = (summary, allowedSet) => {
  if (!summary) return null;
  const filterList = (list) =>
    Array.isArray(list)
      ? list.filter((item) => {
          const name = String(normalizeToolItemName(item)).trim();
          return name && allowedSet.has(name);
        })
      : [];
  return {
    ...summary,
    builtin_tools: filterList(summary.builtin_tools),
    mcp_tools: filterList(summary.mcp_tools),
    a2a_tools: filterList(summary.a2a_tools),
    skills: filterList(summary.skills),
    knowledge_tools: filterList(summary.knowledge_tools),
    user_tools: filterList(summary.user_tools),
    shared_tools: filterList(summary.shared_tools)
  };
};

function applyToolOverridesToSummary(summary, overrides = [], agentDefaults = []) {
  if (!summary) return null;
  const allowedSet = buildAllowedToolSet(summary);
  const defaultSet = new Set();
  if (Array.isArray(agentDefaults) && agentDefaults.length > 0) {
    agentDefaults.forEach((name) => {
      if (allowedSet.has(name)) {
        defaultSet.add(name);
      }
    });
  } else {
    allowedSet.forEach((name) => defaultSet.add(name));
  }
  let effectiveSet = new Set();
  if (Array.isArray(overrides) && overrides.includes(TOOL_OVERRIDE_NONE)) {
    effectiveSet = new Set();
  } else if (Array.isArray(overrides) && overrides.length > 0) {
    overrides.forEach((name) => {
      if (allowedSet.has(name)) {
        effectiveSet.add(name);
      }
    });
  } else {
    effectiveSet = defaultSet;
  }
  return filterSummaryByNames(summary, effectiveSet);
}

const activeSession = computed(
  () => chatStore.sessions.find((item) => item.id === chatStore.activeSessionId) || null
);
const routeAgentId = computed(() => String(route.query.agent_id || '').trim());
const routeEntry = computed(() => String(route.query.entry || '').trim().toLowerCase());
const activeAgentId = computed(
  () => activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || ''
);
const activeAgent = computed(() =>
  activeAgentId.value ? agentStore.agentMap[activeAgentId.value] || null : null
);
const activeAgentLabel = computed(
  () => activeAgent.value?.name || activeAgentId.value || ''
);
const greetingOverride = computed(() => {
  const desc = String(activeAgent.value?.description || '').trim();
  return desc;
});
const effectiveToolSummary = computed(() => {
  const overrides = activeSession.value?.tool_overrides;
  const draftOverrides = Array.isArray(chatStore.draftToolOverrides)
    ? chatStore.draftToolOverrides
    : [];
  return applyToolOverridesToSummary(
    promptToolSummary.value,
    overrides && overrides.length > 0 ? overrides : draftOverrides,
    activeAgent.value?.tool_names || []
  );
});
const promptPreviewHtml = computed(() => {
  const content = promptPreviewContent.value || '暂无系统提示词';
  return renderSystemPromptHighlight(content, effectiveToolSummary.value || {});
});
// 能力悬浮提示使用的工具/技能明细
const abilitySummary = computed(() => collectAbilityDetails(effectiveToolSummary.value || {}));
const hasAbilitySummary = computed(
  () => abilitySummary.value.tools.length > 0 || abilitySummary.value.skills.length > 0
);
const availableToolCount = computed(() => buildAllowedToolSet(promptToolSummary.value).size);
const sessionToolSelectionCount = computed(() =>
  sessionToolsDisabled.value ? 0 : sessionToolSelection.value.size
);
const sessionToolGroups = computed(() => {
  const summary = promptToolSummary.value || {};
  const buildGroup = (label, list) => ({
    label,
    items: (Array.isArray(list) ? list : []).map(normalizeToolItem).filter(Boolean)
  });
  return [
    buildGroup('内置工具', summary.builtin_tools),
    buildGroup('MCP 工具', summary.mcp_tools),
    buildGroup('A2A 工具', summary.a2a_tools),
    buildGroup('知识库工具', summary.knowledge_tools),
    buildGroup('我的工具', summary.user_tools),
    buildGroup('共享工具', summary.shared_tools),
    buildGroup('技能', summary.skills)
  ].filter((group) => group.items.length > 0);
});
const filteredToolGroups = computed(() => {
  const keyword = sessionToolSearch.value.trim().toLowerCase();
  if (!keyword) {
    return sessionToolGroups.value;
  }
  return sessionToolGroups.value
    .map((group) => ({
      ...group,
      items: group.items.filter((item) => {
        const name = String(item.name || '').toLowerCase();
        const desc = String(item.description || '').toLowerCase();
        return name.includes(keyword) || desc.includes(keyword);
      })
    }))
    .filter((group) => group.items.length > 0);
});

const HISTORY_ROW_HEIGHT = 52;
const HISTORY_OVERSCAN = 6;
const historySourceSessions = computed(() => {
  const agentId = activeAgentId.value;
  return chatStore.sessions.filter((session) => {
    const sessionAgentId = String(session.agent_id || '').trim();
    return agentId ? sessionAgentId === agentId : !sessionAgentId;
  });
});
const historyTotal = computed(() => historySourceSessions.value.length);
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
    ? historySourceSessions.value.slice(historyStartIndex.value, historyEndIndex.value)
    : historySourceSessions.value
);
const historyPaddingTop = computed(() =>
  historyVirtual.value ? historyStartIndex.value * HISTORY_ROW_HEIGHT : 0
);
const historyPaddingBottom = computed(() =>
  historyVirtual.value ? Math.max(0, (historyTotal.value - historyEndIndex.value) * HISTORY_ROW_HEIGHT) : 0
);
let pendingAutoScroll = false;
let pendingAutoScrollCount = 0;

const init = async () => {
  if (demoMode.value || !authStore.user) {
    await authStore.loadProfile();
  }
  await chatStore.loadSessions();
  if (routeEntry.value === 'default') {
    chatStore.openDraftSession();
    router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
    return;
  }
  if (routeAgentId.value) {
    chatStore.openDraftSession({ agent_id: routeAgentId.value });
    router.replace({ path: route.path, query: { ...route.query, agent_id: undefined } });
    return;
  }
  chatStore.openDraftSession();
};

const handleCreateSession = () => {
  const agentId = activeAgentId.value;
  chatStore.openDraftSession({ agent_id: agentId });
  draftKey.value += 1;
};

const handleOpenProfile = () => {
  router.push(`${basePath.value}/profile`);
};

const handleOpenPortal = () => {
  router.push(`${basePath.value}/home`);
};

const handleSelectSession = async (sessionId) => {
  await chatStore.loadSessionDetail(sessionId);
  if (isCompactLayout.value) {
    historyDialogVisible.value = false;
  }
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

const hasPlan = (message) =>
  Array.isArray(message?.plan?.steps) && message.plan.steps.length > 0;

const markdownCache = new WeakMap();

const isAssistantStreaming = (message) => {
  if (!message || message.role !== 'assistant') return false;
  return Boolean(message.workflowStreaming || message.reasoningStreaming || message.stream_incomplete);
};

// AI 回复使用 Markdown 渲染，主要用于表格等富文本展示
const renderAssistantMarkdown = (message) => {
  const content = String(message?.content || '');
  if (!content) return '';
  const cached = markdownCache.get(message);
  if (cached && cached.source === content) {
    return cached.html;
  }
  const html = renderMarkdown(content);
  markdownCache.set(message, { source: content, html });
  return html;
};

const workspaceResourceCache = new Map();
let workspaceResourceHydrationFrame = null;
let stopWorkspaceRefreshListener = null;

const isAdminUser = (user) =>
  Array.isArray(user?.roles) && user.roles.some((role) => role === 'admin' || role === 'super_admin');

const resolveWorkspaceResource = (publicPath) => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const user = authStore.user;
  if (!user) return null;
  const currentId = String(user.id || '').trim();
  if (!currentId || parsed.userId === currentId) {
    return { ...parsed, requestUserId: null, allowed: true };
  }
  if (isAdminUser(user)) {
    return { ...parsed, requestUserId: parsed.userId, allowed: true };
  }
  return { ...parsed, requestUserId: null, allowed: false };
};

const getFilenameFromHeaders = (headers, fallback) => {
  const disposition = headers?.['content-disposition'] || headers?.['Content-Disposition'];
  if (!disposition) return fallback;
  const utf8Match = /filename\\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match) {
    return decodeURIComponent(utf8Match[1]);
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match ? match[1] : fallback;
};

const getFileExtension = (filename) => {
  const base = String(filename || '').split('?')[0].split('#')[0];
  const parts = base.split('.');
  if (parts.length < 2) return '';
  return parts.pop()?.toLowerCase() || '';
};

const normalizeWorkspaceImageBlob = (blob, filename, contentType) => {
  if (!(blob instanceof Blob)) return blob;
  const extension = getFileExtension(filename);
  if (extension !== 'svg') return blob;
  const expectedType = 'image/svg+xml';
  if (blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.includes('image/svg')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob.slice(0, blob.size, expectedType);
};

const saveBlobUrl = (url, filename) => {
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

const fetchWorkspaceResource = async (resource) => {
  const cached = workspaceResourceCache.get(resource.publicPath);
  if (cached?.objectUrl) return cached;
  if (cached?.promise) return cached.promise;
  const params = { path: resource.relativePath };
  if (resource.requestUserId) {
    params.user_id = resource.requestUserId;
  }
  const promise = downloadWunderWorkspaceFile(params)
    .then((response) => {
      const filename = getFilenameFromHeaders(response.headers, resource.filename || 'download');
      const contentType = response.headers?.['content-type'] || response.headers?.['Content-Type'];
      const blob = normalizeWorkspaceImageBlob(response.data, filename, contentType);
      const objectUrl = URL.createObjectURL(blob);
      const entry = { objectUrl, filename };
      workspaceResourceCache.set(resource.publicPath, entry);
      return entry;
    })
    .catch((error) => {
      workspaceResourceCache.delete(resource.publicPath);
      throw error;
    });
  workspaceResourceCache.set(resource.publicPath, { promise });
  return promise;
};

const isWorkspaceResourceMissing = (error) => {
  const status = error?.response?.status;
  if (status === 404 || status === 410) return true;
  const raw =
    error?.response?.data?.detail ??
    error?.response?.data?.message ??
    error?.message ??
    '';
  const message = typeof raw === 'string' ? raw : String(raw || '');
  return /not found|no such|不存在|找不到|已删除|已移除|removed/i.test(message);
};

const hydrateWorkspaceResourceCard = async (card) => {
  if (!card || card.dataset.workspaceState) return;
  const kind = card.dataset.workspaceKind || 'image';
  if (kind !== 'image') {
    card.dataset.workspaceState = 'ready';
    return;
  }
  const publicPath = card.dataset.workspacePath || '';
  const status = card.querySelector('.ai-resource-status');
  const preview = card.querySelector('.ai-resource-preview');
  if (!publicPath || !preview) return;
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) {
    if (status) status.textContent = '资源不可用';
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
    return;
  }
  if (!resource.allowed) {
    if (status) status.textContent = '当前用户无权限访问该资源';
    card.dataset.workspaceState = 'forbidden';
    card.classList.add('is-error');
    return;
  }
  card.dataset.workspaceState = 'loading';
  try {
    const entry = await fetchWorkspaceResource(resource);
    preview.src = entry.objectUrl;
    card.dataset.workspaceState = 'ready';
    card.classList.add('is-ready');
    if (status) status.textContent = '';
  } catch (error) {
    if (status) {
      status.textContent = isWorkspaceResourceMissing(error) ? '该文件已被移除' : '图片加载失败';
    }
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
  }
};

const hydrateWorkspaceResources = () => {
  const container = messagesContainerRef.value;
  if (!container || !authStore.user) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    hydrateWorkspaceResourceCard(card);
  });
};

const resetWorkspaceResourceCards = () => {
  const container = messagesContainerRef.value;
  if (!container) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    const kind = card.dataset?.workspaceKind || 'image';
    if (kind !== 'image') return;
    const state = card.dataset?.workspaceState || '';
    if (state === 'ready') return;
    card.dataset.workspaceState = '';
    card.classList.remove('is-error');
    card.classList.remove('is-ready');
    const status = card.querySelector('.ai-resource-status');
    if (status) {
      status.textContent = '图片加载中...';
    }
  });
};

const handleWorkspaceRefresh = () => {
  resetWorkspaceResourceCards();
  scheduleWorkspaceResourceHydration();
};

const scheduleWorkspaceResourceHydration = () => {
  if (workspaceResourceHydrationFrame) return;
  workspaceResourceHydrationFrame = requestAnimationFrame(() => {
    workspaceResourceHydrationFrame = null;
    hydrateWorkspaceResources();
  });
};

const clearWorkspaceResourceCache = () => {
  if (workspaceResourceHydrationFrame) {
    cancelAnimationFrame(workspaceResourceHydrationFrame);
    workspaceResourceHydrationFrame = null;
  }
  workspaceResourceCache.forEach((entry) => {
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
  });
  workspaceResourceCache.clear();
};

const downloadWorkspaceResource = async (publicPath) => {
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) return;
  if (!resource.allowed) {
    ElMessage.warning('当前用户无权限访问该资源');
    return;
  }
  try {
    const entry = await fetchWorkspaceResource(resource);
    saveBlobUrl(entry.objectUrl, entry.filename || resource.filename || 'download');
  } catch (error) {
    ElMessage.error(isWorkspaceResourceMissing(error) ? '该文件已被移除' : '资源下载失败');
  }
};

const handleComposerSend = async ({ content, attachments }) => {
  const payloadAttachments = Array.isArray(attachments) ? attachments : [];
  const trimmedContent = String(content || '').trim();
  const active = activeInquiryPanel.value;
  const selectedRoutes = resolveInquirySelectionRoutes(active?.panel, inquirySelection.value);
  const hasSelection = selectedRoutes.length > 0;
  if (!trimmedContent && payloadAttachments.length === 0 && !hasSelection) return;

  let finalContent = trimmedContent;
  if (active) {
    if (hasSelection) {
      chatStore.resolveInquiryPanel(active.message, {
        status: 'answered',
        selected: selectedRoutes.map((route) => route.label)
      });
      const selectionText = buildInquiryReply(active.panel, selectedRoutes);
      finalContent = trimmedContent ? `${selectionText}\n\n用户补充：${trimmedContent}` : selectionText;
    } else {
      chatStore.resolveInquiryPanel(active.message, { status: 'dismissed' });
    }
  }

  pendingAutoScroll = true;
  pendingAutoScrollCount = chatStore.messages.length;
  try {
    await chatStore.sendMessage(finalContent, { attachments: payloadAttachments });
  } catch (error) {
    pendingAutoScroll = false;
    pendingAutoScrollCount = 0;
    throw error;
  } finally {
    inquirySelection.value = [];
  }
};

const handleStop = async () => {
  await chatStore.stopStream();
};

const buildInquiryReply = (panel, routes) => {
  const header = '【问询面板选择】';
  const question = panel?.question ? `问题：${panel.question}` : '';
  const lines = routes.map((route) => {
    const detail = route.description ? `：${route.description}` : '';
    return `- ${route.label}${detail}`;
  });
  return [header, question, ...lines].filter(Boolean).join('\n');
};

const resolveInquirySelectionRoutes = (panel, selected) => {
  if (!panel || !Array.isArray(selected) || selected.length === 0) {
    return [];
  }
  return selected
    .map((index) => panel.routes?.[index])
    .filter((route) => route && route.label);
};

const activeInquiryPanel = computed(() => {
  for (let i = chatStore.messages.length - 1; i >= 0; i -= 1) {
    const message = chatStore.messages[i];
    if (message?.role !== 'assistant') continue;
    const panel = message?.questionPanel;
    if (panel?.status === 'pending') {
      return { message, panel };
    }
  }
  return null;
});

const handleInquirySelection = (selected) => {
  inquirySelection.value = Array.isArray(selected) ? selected : [];
};

const openImagePreview = (src, title = '') => {
  if (!src) return;
  imagePreviewUrl.value = src;
  const trimmedTitle = String(title || '').trim();
  imagePreviewTitle.value = trimmedTitle || '图片预览';
  imagePreviewVisible.value = true;
};

const closeImagePreview = () => {
  imagePreviewVisible.value = false;
  imagePreviewUrl.value = '';
  imagePreviewTitle.value = '';
};

const handleCopyMessage = async (message) => {
  const content = String(message?.content || '').trim();
  if (!content) {
    ElMessage.warning('暂无可复制内容');
    return;
  }
  const ok = await copyText(content);
  if (ok) {
    ElMessage.success('内容已复制');
  } else {
    ElMessage.error('复制失败');
  }
};

const handleMessageClick = async (event) => {
  const target = event?.target;
  if (!(target instanceof Element)) return;
  const resourceButton = target.closest('[data-workspace-action]');
  if (resourceButton) {
    const action = resourceButton.getAttribute('data-workspace-action');
    const container = resourceButton.closest('[data-workspace-path]');
    const publicPath = container?.dataset?.workspacePath || '';
    if (action === 'download' && publicPath) {
      event.preventDefault();
      await downloadWorkspaceResource(publicPath);
    }
    return;
  }
  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]');
  if (resourceLink) {
    const publicPath = resourceLink.dataset?.workspacePath || '';
    if (publicPath) {
      event.preventDefault();
      await downloadWorkspaceResource(publicPath);
    }
    return;
  }
  const previewImage = target.closest('img.ai-resource-preview');
  if (previewImage) {
    const card = previewImage.closest('.ai-resource-card');
    if (card?.dataset?.workspaceState !== 'ready') return;
    const src = previewImage.getAttribute('src') || '';
    if (!src) return;
    const title = card?.querySelector('.ai-resource-name')?.textContent || '';
    openImagePreview(src, title);
    return;
  }
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

const flushChatSnapshot = () => {
  chatStore.scheduleSnapshot(true);
};

const handleBeforeUnload = () => {
  chatStore.markPageUnloading();
  flushChatSnapshot();
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
    const overrides =
      activeSession.value?.tool_overrides && activeSession.value.tool_overrides.length > 0
        ? activeSession.value.tool_overrides
        : Array.isArray(chatStore.draftToolOverrides) && chatStore.draftToolOverrides.length > 0
          ? chatStore.draftToolOverrides
          : undefined;
    const agentId =
      activeSession.value?.agent_id || chatStore.draftAgentId || routeAgentId.value || undefined;
    const requestPayload = {
      ...(agentId ? { agent_id: agentId } : {}),
      ...(overrides ? { tool_overrides: overrides } : {})
    };
    const promptRequest = chatStore.activeSessionId
      ? fetchSessionSystemPrompt(chatStore.activeSessionId, requestPayload)
      : fetchRealtimeSystemPrompt(requestPayload);
    const promptResult = await promptRequest;
    await toolSummaryPromise;
    const responsePayload = promptResult?.data?.data || {};
    promptPreviewContent.value = responsePayload.prompt || '';
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

const buildDefaultToolSet = () => {
  const allowedSet = buildAllowedToolSet(promptToolSummary.value);
  const defaultSet = new Set();
  const agentTools = activeAgent.value?.tool_names || [];
  if (agentTools.length > 0) {
    agentTools.forEach((name) => {
      if (allowedSet.has(name)) {
        defaultSet.add(name);
      }
    });
  } else {
    allowedSet.forEach((name) => defaultSet.add(name));
  }
  return { allowedSet, defaultSet };
};

const openSessionTools = async () => {
  await loadToolSummary();
  if (activeAgentId.value && !activeAgent.value) {
    await agentStore.getAgent(activeAgentId.value);
  }
  const { allowedSet, defaultSet } = buildDefaultToolSet();
  const overrides =
    activeSession.value?.tool_overrides ||
    (Array.isArray(chatStore.draftToolOverrides) ? chatStore.draftToolOverrides : []);
  if (overrides.includes(TOOL_OVERRIDE_NONE)) {
    sessionToolsDisabled.value = true;
    sessionToolSelection.value = new Set(defaultSet);
  } else if (overrides.length > 0) {
    sessionToolsDisabled.value = false;
    sessionToolSelection.value = new Set(
      overrides.filter((name) => allowedSet.has(name))
    );
  } else {
    sessionToolsDisabled.value = false;
    sessionToolSelection.value = new Set(defaultSet);
  }
  sessionToolSearch.value = '';
  sessionToolsVisible.value = true;
};

const closeSessionTools = () => {
  sessionToolsVisible.value = false;
};

const isSessionToolSelected = (name) => sessionToolSelection.value.has(name);

const toggleSessionTool = (name, checked) => {
  if (sessionToolsDisabled.value) return;
  const next = new Set(sessionToolSelection.value);
  if (checked) {
    next.add(name);
  } else {
    next.delete(name);
  }
  sessionToolSelection.value = next;
};

const resetSessionTools = () => {
  const { defaultSet } = buildDefaultToolSet();
  sessionToolsDisabled.value = false;
  sessionToolSelection.value = new Set(defaultSet);
  if (!chatStore.activeSessionId) {
    chatStore.setDraftToolOverrides([]);
  }
};

const saveSessionTools = async () => {
  const { allowedSet, defaultSet } = buildDefaultToolSet();
  let overrides = [];
  if (sessionToolsDisabled.value) {
    overrides = [TOOL_OVERRIDE_NONE];
  } else {
    const selection = Array.from(sessionToolSelection.value).filter((name) =>
      allowedSet.has(name)
    );
    const selectionSet = new Set(selection);
    const isDefault =
      selectionSet.size === defaultSet.size &&
      Array.from(defaultSet).every((name) => selectionSet.has(name));
    overrides = isDefault ? [] : selection.sort();
  }
  if (!chatStore.activeSessionId) {
    chatStore.setDraftToolOverrides(overrides);
    closeSessionTools();
    ElMessage.success('已保存到新会话，发送消息后生效');
    return;
  }
  sessionToolsSaving.value = true;
  try {
    await chatStore.updateSessionTools(chatStore.activeSessionId, overrides);
    closeSessionTools();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工具调整失败');
  } finally {
    sessionToolsSaving.value = false;
  }
};

const parseTimeValue = (value) => {
  if (value === null || value === undefined) return null;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? null : value;
  }
  if (typeof value === 'number') {
    const millis = value < 1e12 ? value * 1000 : value;
    const date = new Date(millis);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  const text = String(value).trim();
  if (!text) return null;
  if (/^\d+$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return null;
    const millis = numeric < 1e12 ? numeric * 1000 : numeric;
    const date = new Date(millis);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  const parsed = new Date(text);
  if (!Number.isNaN(parsed.getTime())) {
    return parsed;
  }
  const match = text.match(
    /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?/
  );
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const hour = Number(match[4] || 0);
  const minute = Number(match[5] || 0);
  const second = Number(match[6] || 0);
  const date = new Date(year, month, day, hour, minute, second);
  return Number.isNaN(date.getTime()) ? null : date;
};

const formatTime = (value) => {
  const parsed = parseTimeValue(value);
  if (!parsed) {
    if (value === null || value === undefined) return '';
    return String(value).trim();
  }
  const pad = (part) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}:${pad(parsed.getSeconds())}`;
};

const formatTitle = (title) => {
  const text = String(title || '').trim();
  if (!text) return '未命名会话';
  return text.length > 20 ? `${text.slice(0, 20)}...` : text;
};

const updateCompactLayout = () => {
  if (typeof window === 'undefined') return;
  if (window.matchMedia) {
    isCompactLayout.value = window.matchMedia('(max-width: 960px)').matches;
    return;
  }
  isCompactLayout.value = window.innerWidth <= 960;
};

const openWorkspaceDialog = () => {
  workspaceDialogVisible.value = true;
};

const openHistoryDialog = async () => {
  historyDialogVisible.value = true;
  historyScrollTop.value = 0;
  await nextTick();
  const container = historyListRef.value;
  if (container) {
    container.scrollTop = 0;
  }
};

const formatDuration = (seconds) => {
  if (seconds === null || seconds === undefined || Number.isNaN(seconds)) return '-';
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return '-';
  if (value < 1) {
    return `${Math.max(1, Math.round(value * 1000))} ms`;
  }
  return `${value.toFixed(2)} s`;
};

const formatCount = (value) => {
  if (value === null || value === undefined) return '-';
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) return '-';
  return String(parsed);
};

const formatSpeed = (value) => {
  if (value === null || value === undefined) return '-';
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '-';
  return `${parsed.toFixed(2)} token/s`;
};

const normalizeDurationSeconds = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const resolveDurationSeconds = (stats) => {
  const interaction = normalizeDurationSeconds(
    stats?.interaction_duration_s ??
      stats?.interactionDurationS ??
      stats?.interactionDuration ??
      stats?.duration_s ??
      stats?.elapsed_s
  );
  if (interaction !== null) return interaction;
  const prefill = normalizeDurationSeconds(stats?.prefill_duration_s);
  const decode = normalizeDurationSeconds(stats?.decode_duration_s);
  if (prefill === null && decode === null) return null;
  return (prefill ?? 0) + (decode ?? 0);
};

const resolveTokenSpeed = (stats, durationSeconds) => {
  const outputTokens = Number(stats?.usage?.output);
  const decode = normalizeDurationSeconds(stats?.decode_duration_s);
  if (Number.isFinite(outputTokens) && outputTokens > 0 && decode !== null && decode > 0) {
    return outputTokens / decode;
  }
  const totalTokens = Number(stats?.usage?.total);
  if (Number.isFinite(totalTokens) && totalTokens > 0 && durationSeconds && durationSeconds > 0) {
    return totalTokens / durationSeconds;
  }
  return null;
};

const buildMessageStatsEntries = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return [];
  if (isAssistantStreaming(message)) return [];
  const stats = message.stats || null;
  if (!stats) return [];
  const durationSeconds = resolveDurationSeconds(stats);
  const speed = resolveTokenSpeed(stats, durationSeconds);
  const hasUsage = Number.isFinite(Number(stats?.usage?.total)) && Number(stats.usage.total) > 0;
  const hasDuration = Number.isFinite(Number(durationSeconds)) && Number(durationSeconds) > 0;
  const hasToolCalls = Number.isFinite(Number(stats?.toolCalls)) && Number(stats.toolCalls) > 0;
  const hasQuota = Number.isFinite(Number(stats?.quotaConsumed)) && Number(stats.quotaConsumed) > 0;
  if (!hasUsage && !hasDuration && !hasToolCalls && !hasQuota) {
    return [];
  }
  const entries = [
    { label: '耗时', value: formatDuration(durationSeconds) },
    { label: '速度', value: formatSpeed(speed) },
    { label: 'token占用', value: formatCount(stats?.usage?.total) },
    { label: '工具调用', value: formatCount(stats?.toolCalls) },
    { label: '额度消耗', value: formatCount(stats?.quotaConsumed) }
  ];
  return entries;
};

const shouldShowMessageStats = (message) => buildMessageStatsEntries(message).length > 0;

const scrollMessagesToBottom = async () => {
  await nextTick();
  const container = messagesContainerRef.value;
  if (!container) return;
  container.scrollTop = container.scrollHeight;
};

onMounted(async () => {
  await init();
  loadToolSummary();
  scheduleWorkspaceResourceHydration();
  stopWorkspaceRefreshListener = onWorkspaceRefresh(handleWorkspaceRefresh);
  updateCompactLayout();
  window.addEventListener('resize', updateCompactLayout);
  window.addEventListener('beforeunload', handleBeforeUnload);
  document.addEventListener('visibilitychange', flushChatSnapshot);
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', updateCompactLayout);
  window.removeEventListener('beforeunload', handleBeforeUnload);
  document.removeEventListener('visibilitychange', flushChatSnapshot);
  if (stopWorkspaceRefreshListener) {
    stopWorkspaceRefreshListener();
    stopWorkspaceRefreshListener = null;
  }
  clearWorkspaceResourceCache();
});

watch(
  () => chatStore.activeSessionId,
  (value, oldValue) => {
    if (!value && oldValue) {
      draftKey.value += 1;
    }
    if (value !== oldValue) {
      clearWorkspaceResourceCache();
      scheduleWorkspaceResourceHydration();
    }
  }
);

watch(
  () => chatStore.messages.length,
  (value) => {
    if (!pendingAutoScroll) return;
    if (value <= pendingAutoScrollCount) return;
    pendingAutoScroll = false;
    pendingAutoScrollCount = value;
    scrollMessagesToBottom();
  }
);

watch(
  () => chatStore.messages.length,
  () => {
    scheduleWorkspaceResourceHydration();
  }
);

watch(
  () => activeInquiryPanel.value,
  (value) => {
    if (!value) {
      inquirySelection.value = [];
    }
  }
);

watch(
  () => chatStore.messages[chatStore.messages.length - 1]?.content,
  () => {
    scheduleWorkspaceResourceHydration();
  }
);

watch(
  () => authStore.user?.id,
  () => {
    scheduleWorkspaceResourceHydration();
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
  () => activeAgentId.value,
  (value) => {
    if (!value) return;
    agentStore.getAgent(value).catch(() => null);
  },
  { immediate: true }
);

watch(
  () => greetingOverride.value,
  (value, oldValue) => {
    if (value === oldValue) return;
    chatStore.setGreetingOverride(value);
  },
  { immediate: true }
);

watch(
  () => routeEntry.value,
  async (value, oldValue) => {
    if (!value || value === oldValue || value !== 'default') return;
    chatStore.openDraftSession();
    router.replace({ path: route.path, query: { ...route.query, entry: undefined } });
  }
);

watch(
  () => routeAgentId.value,
  async (value, oldValue) => {
    if (!value || value === oldValue) return;
    if (routeEntry.value === 'default') return;
    const wasDraft = !chatStore.activeSessionId;
    chatStore.openDraftSession({ agent_id: value });
    if (wasDraft) {
      draftKey.value += 1;
    }
    router.replace({ path: route.path, query: { ...route.query, agent_id: undefined } });
  }
);

watch(
  () => isCompactLayout.value,
  (value) => {
    if (!value) {
      workspaceDialogVisible.value = false;
      historyDialogVisible.value = false;
    }
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
