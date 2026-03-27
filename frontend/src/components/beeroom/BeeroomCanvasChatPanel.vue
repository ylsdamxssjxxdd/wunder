<template>
  <aside class="beeroom-canvas-chat" :class="{ collapsed }">
    <button
      class="beeroom-canvas-chat-handle"
      type="button"
      :title="collapsed ? t('common.expand') : t('common.collapse')"
      :aria-label="collapsed ? t('common.expand') : t('common.collapse')"
      @click="emit('update:collapsed', !collapsed)"
    >
      <i class="fa-solid" :class="collapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
    </button>

    <template v-if="!collapsed">
      <div class="beeroom-canvas-chat-head">
        <div>
          <div class="beeroom-canvas-chat-title">{{ t('beeroom.canvas.chatTitle') }}</div>
          <div v-if="dispatchRuntimeStatus !== 'idle' || dispatchSessionId" class="beeroom-canvas-chat-runtime">
            <span v-if="dispatchRuntimeStatus !== 'idle'" class="beeroom-canvas-runtime-chip" :class="`is-${dispatchRuntimeTone}`">
              {{ dispatchRuntimeLabel }}
            </span>
            <span v-if="dispatchSessionId" class="beeroom-canvas-runtime-session">
              #{{ shortIdentity(dispatchSessionId, 6, 4) }}
            </span>
          </div>
        </div>
        <div class="beeroom-canvas-chat-head-actions">
          <button class="beeroom-canvas-icon-btn" type="button" :title="t('common.clear')" @click="emit('clear')">
            <i class="fa-solid fa-broom" aria-hidden="true"></i>
          </button>
          <button
            class="beeroom-canvas-icon-btn"
            type="button"
            :title="t('common.stop')"
            :disabled="!dispatchCanStop"
            @click="emit('stop')"
          >
            <i class="fa-solid fa-stop" aria-hidden="true"></i>
          </button>
          <button
            class="beeroom-canvas-icon-btn"
            type="button"
            :title="t('chat.message.resume')"
            :disabled="!dispatchCanResume"
            @click="emit('resume')"
          >
            <i class="fa-solid fa-play" aria-hidden="true"></i>
          </button>
        </div>
      </div>

      <section ref="chatStreamRef" class="beeroom-canvas-chat-stream">
        <article
          v-for="message in messages"
          :key="message.key"
          class="beeroom-canvas-chat-message"
          :class="[`is-${message.tone}`]"
        >
          <button
            v-if="message.senderAgentId"
            class="beeroom-canvas-chat-avatar"
            type="button"
            @click="emit('open-agent', message.senderAgentId)"
          >
            <img
              v-if="resolveAgentAvatarImageByAgentId(message.senderAgentId)"
              class="beeroom-canvas-chat-avatar-img"
              :src="resolveAgentAvatarImageByAgentId(message.senderAgentId)"
              alt=""
            />
            <span v-else>{{ avatarLabel(message.senderName) }}</span>
          </button>
          <div v-else class="beeroom-canvas-chat-avatar" :class="message.tone === 'user' ? 'is-user' : 'is-system'">
            <i class="fa-solid" :class="message.tone === 'user' ? 'fa-user' : 'fa-wave-square'" aria-hidden="true"></i>
          </div>
          <div class="beeroom-canvas-chat-main">
            <div class="beeroom-canvas-chat-meta-row">
              <button
                v-if="message.senderAgentId"
                class="beeroom-canvas-chat-sender"
                type="button"
                @click="emit('open-agent', message.senderAgentId)"
              >
                {{ message.senderName }}
              </button>
              <span v-else class="beeroom-canvas-chat-sender" :class="message.tone === 'user' ? 'is-user' : 'is-system'">
                {{ message.senderName }}
              </span>
              <span class="beeroom-canvas-chat-time">{{ message.timeLabel }}</span>
            </div>
            <div class="beeroom-canvas-chat-bubble">
              <span v-if="message.mention" class="beeroom-canvas-chat-mention">@{{ message.mention }}</span>
              <span>{{ message.body }}</span>
            </div>
            <div v-if="message.meta" class="beeroom-canvas-chat-extra">{{ message.meta }}</div>
          </div>
        </article>
      </section>

      <section v-if="approvals.length" class="beeroom-canvas-chat-approvals">
        <div class="beeroom-canvas-chat-approvals-head">
          <span>{{ t('chat.approval.title') }}</span>
          <span class="beeroom-canvas-chat-approvals-count">{{ approvals.length }}</span>
        </div>
        <article
          v-for="approval in approvals"
          :key="approval.approval_id"
          class="beeroom-canvas-chat-approval-item"
        >
          <div class="beeroom-canvas-chat-approval-summary">
            {{ approval.summary || approval.tool || approval.approval_id }}
          </div>
          <div class="beeroom-canvas-chat-approval-meta">
            {{ t('chat.approval.tool') }}: {{ approval.tool || '-' }}
          </div>
          <div class="beeroom-canvas-chat-approval-actions">
            <button
              class="beeroom-canvas-chat-approval-btn"
              type="button"
              :disabled="dispatchApprovalBusy"
              @click="emit('approval', { decision: 'approve_once', approvalId: approval.approval_id })"
            >
              {{ t('chat.approval.once') }}
            </button>
            <button
              class="beeroom-canvas-chat-approval-btn"
              type="button"
              :disabled="dispatchApprovalBusy"
              @click="emit('approval', { decision: 'approve_session', approvalId: approval.approval_id })"
            >
              {{ t('chat.approval.session') }}
            </button>
            <button
              class="beeroom-canvas-chat-approval-btn is-danger"
              type="button"
              :disabled="dispatchApprovalBusy"
              @click="emit('approval', { decision: 'deny', approvalId: approval.approval_id })"
            >
              {{ t('chat.approval.deny') }}
            </button>
          </div>
        </article>
      </section>

      <section class="beeroom-canvas-chat-composer">
        <textarea
          class="beeroom-canvas-chat-textarea"
          :value="composerText"
          :placeholder="t('beeroom.canvas.chatInputPlaceholder')"
          :disabled="composerSending"
          rows="3"
          @input="emit('update:composerText', ($event.target as HTMLTextAreaElement).value)"
          @keydown.enter.exact.prevent="emit('send')"
        ></textarea>
        <div class="beeroom-canvas-chat-compose-foot">
          <el-select
            id="beeroom-chat-target"
            :model-value="composerTargetAgentId"
            class="beeroom-canvas-chat-select"
            popper-class="beeroom-canvas-chat-select-popper"
            :placeholder="t('beeroom.canvas.chatTarget')"
            :disabled="composerSending"
            @update:model-value="emit('update:composerTargetAgentId', String($event || ''))"
          >
            <el-option
              v-for="option in composerTargetOptions"
              :key="option.agentId"
              :label="option.label"
              :value="option.agentId"
            />
          </el-select>
          <button
            class="beeroom-canvas-chat-send"
            type="button"
            :disabled="composerSending || !composerCanSend"
            @click="emit('send')"
          >
            {{ composerSending ? t('common.loading') : t('chat.input.send') }}
          </button>
          <button
            class="beeroom-canvas-chat-demo"
            :class="{ 'is-running': demoCanCancel }"
            type="button"
            :disabled="demoActionDisabled"
            @click="emit('demo')"
          >
            {{ demoActionLabel }}
          </button>
        </div>
        <div v-if="composerError" class="beeroom-canvas-chat-compose-status is-error">{{ composerError }}</div>
        <div v-else-if="demoError" class="beeroom-canvas-chat-compose-status is-error">{{ demoError }}</div>
      </section>
    </template>
  </aside>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

import type {
  ComposerTargetOption,
  DispatchApprovalItem,
  DispatchRuntimeStatus,
  MissionChatMessage
} from './beeroomCanvasChatModel';

const props = defineProps<{
  collapsed: boolean;
  messages: MissionChatMessage[];
  approvals: DispatchApprovalItem[];
  dispatchRuntimeStatus: DispatchRuntimeStatus;
  dispatchRuntimeTone: string;
  dispatchRuntimeLabel: string;
  dispatchSessionId: string;
  dispatchCanStop: boolean;
  dispatchCanResume: boolean;
  dispatchApprovalBusy: boolean;
  composerText: string;
  composerTargetAgentId: string;
  composerTargetOptions: ComposerTargetOption[];
  composerSending: boolean;
  composerCanSend: boolean;
  composerError: string;
  demoError: string;
  demoActionDisabled: boolean;
  demoActionLabel: string;
  demoCanCancel: boolean;
  resolveAgentAvatarImageByAgentId: (agentId: unknown) => string;
  avatarLabel: (value: unknown) => string;
}>();

const emit = defineEmits<{
  (event: 'update:collapsed', value: boolean): void;
  (event: 'update:composerText', value: string): void;
  (event: 'update:composerTargetAgentId', value: string): void;
  (event: 'clear'): void;
  (event: 'stop'): void;
  (event: 'resume'): void;
  (event: 'send'): void;
  (event: 'demo'): void;
  (event: 'open-agent', agentId: string): void;
  (event: 'approval', value: { decision: 'approve_once' | 'approve_session' | 'deny'; approvalId: string }): void;
}>();

const { t } = useI18n();
const chatStreamRef = ref<HTMLElement | null>(null);

const shortIdentity = (value: unknown, head = 8, tail = 6) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= head + tail + 3) return text;
  return `${text.slice(0, head)}...${text.slice(-tail)}`;
};

const scrollChatToBottom = async () => {
  await nextTick();
  const element = chatStreamRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

watch(
  () => [props.messages.length, props.messages[props.messages.length - 1]?.key || '', props.collapsed] as const,
  async ([, , collapsed]) => {
    if (collapsed) return;
    await scrollChatToBottom();
  },
  { immediate: true }
);
</script>

<style scoped>
.beeroom-canvas-chat {
  position: relative;
  width: min(360px, 34vw);
  min-width: 320px;
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px 18px 18px 14px;
  border-left: 1px solid rgba(148, 163, 184, 0.14);
  background: linear-gradient(180deg, rgba(248, 250, 252, 0.96), rgba(241, 245, 249, 0.98));
}

.beeroom-canvas-chat::before {
  content: '';
  position: absolute;
  left: 0;
  top: 22px;
  bottom: 22px;
  width: 1px;
  background: linear-gradient(180deg, transparent, rgba(148, 163, 184, 0.24), transparent);
}

.beeroom-canvas-chat.collapsed {
  width: 28px;
  min-width: 28px;
  padding: 18px 10px 18px 0;
  border-left: none;
  background: transparent;
}

.beeroom-canvas-chat.collapsed::before {
  display: none;
}

.beeroom-canvas-chat-handle {
  position: absolute;
  left: -14px;
  top: 18px;
  width: 28px;
  height: 56px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.94);
  color: rgba(71, 85, 105, 0.92);
  cursor: pointer;
  box-shadow: 0 10px 24px rgba(148, 163, 184, 0.16);
}

.beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat-handle:focus-visible {
  border-color: rgba(96, 165, 250, 0.36);
  color: rgba(30, 64, 175, 0.92);
  outline: none;
}

.beeroom-canvas-chat-head,
.beeroom-canvas-chat-head-actions,
.beeroom-canvas-chat-runtime,
.beeroom-canvas-chat-meta-row,
.beeroom-canvas-chat-compose-foot,
.beeroom-canvas-chat-approvals-head,
.beeroom-canvas-chat-approval-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-chat-title {
  font-size: 14px;
  font-weight: 700;
  color: rgba(15, 23, 42, 0.96);
}

.beeroom-canvas-chat-runtime,
.beeroom-canvas-chat-time,
.beeroom-canvas-chat-extra,
.beeroom-canvas-chat-approval-meta,
.beeroom-canvas-chat-compose-status {
  font-size: 12px;
  color: rgba(100, 116, 139, 0.92);
}

.beeroom-canvas-runtime-chip {
  display: inline-flex;
  align-items: center;
  padding: 5px 10px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(148, 163, 184, 0.1);
  font-size: 11px;
  font-weight: 700;
}

.beeroom-canvas-runtime-chip.is-running {
  color: rgba(21, 128, 61, 0.92);
  border-color: rgba(34, 197, 94, 0.22);
  background: rgba(34, 197, 94, 0.1);
}

.beeroom-canvas-runtime-chip.is-success {
  color: rgba(30, 64, 175, 0.92);
  border-color: rgba(96, 165, 250, 0.22);
  background: rgba(59, 130, 246, 0.1);
}

.beeroom-canvas-runtime-chip.is-danger {
  color: rgba(185, 28, 28, 0.92);
  border-color: rgba(239, 68, 68, 0.22);
  background: rgba(239, 68, 68, 0.1);
}

.beeroom-canvas-runtime-chip.is-warn {
  color: rgba(146, 64, 14, 0.92);
  border-color: rgba(245, 158, 11, 0.22);
  background: rgba(245, 158, 11, 0.1);
}

.beeroom-canvas-runtime-session {
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, monospace;
}

.beeroom-canvas-icon-btn,
.beeroom-canvas-chat-approval-btn {
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(255, 255, 255, 0.92);
  color: rgba(51, 65, 85, 0.92);
  cursor: pointer;
}

.beeroom-canvas-icon-btn {
  width: 36px;
  height: 36px;
  border-radius: 12px;
}

.beeroom-canvas-icon-btn:hover:not(:disabled),
.beeroom-canvas-icon-btn:focus-visible:not(:disabled),
.beeroom-canvas-chat-approval-btn:hover:not(:disabled),
.beeroom-canvas-chat-approval-btn:focus-visible:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.38);
  color: rgba(30, 64, 175, 0.92);
  outline: none;
}

.beeroom-canvas-chat-stream {
  flex: 1 1 auto;
  min-height: 220px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-right: 4px;
}

.beeroom-canvas-chat-message {
  display: grid;
  grid-template-columns: 38px minmax(0, 1fr);
  gap: 10px;
  align-items: start;
}

.beeroom-canvas-chat-avatar {
  width: 38px;
  height: 38px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(255, 255, 255, 0.92);
  color: rgba(51, 65, 85, 0.92);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  cursor: pointer;
}

.beeroom-canvas-chat-avatar.is-system {
  background: rgba(226, 232, 240, 0.84);
}

.beeroom-canvas-chat-avatar.is-user {
  background: rgba(191, 219, 254, 0.74);
}

.beeroom-canvas-chat-avatar-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.beeroom-canvas-chat-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.beeroom-canvas-chat-sender {
  border: none;
  background: transparent;
  padding: 0;
  font-size: 13px;
  font-weight: 700;
  color: rgba(15, 23, 42, 0.96);
  cursor: pointer;
}

.beeroom-canvas-chat-sender.is-user {
  color: rgba(30, 64, 175, 0.92);
}

.beeroom-canvas-chat-sender.is-system {
  color: rgba(71, 85, 105, 0.92);
}

.beeroom-canvas-chat-bubble {
  padding: 12px 14px;
  border-radius: 18px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  background: rgba(255, 255, 255, 0.9);
  color: rgba(30, 41, 59, 0.96);
  line-height: 1.6;
}

.beeroom-canvas-chat-message.is-mother .beeroom-canvas-chat-bubble {
  background: rgba(254, 243, 199, 0.72);
}

.beeroom-canvas-chat-message.is-worker .beeroom-canvas-chat-bubble {
  background: rgba(219, 234, 254, 0.72);
}

.beeroom-canvas-chat-message.is-system .beeroom-canvas-chat-bubble {
  background: rgba(226, 232, 240, 0.74);
}

.beeroom-canvas-chat-message.is-user .beeroom-canvas-chat-bubble {
  background: rgba(220, 252, 231, 0.74);
}

.beeroom-canvas-chat-mention {
  color: rgba(30, 64, 175, 0.9);
  font-weight: 700;
  margin-right: 6px;
}

.beeroom-canvas-chat-approvals,
.beeroom-canvas-chat-approval-item,
.beeroom-canvas-chat-composer {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.beeroom-canvas-chat-approval-item {
  padding: 12px;
  border-radius: 18px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  background: rgba(255, 255, 255, 0.86);
}

.beeroom-canvas-chat-approvals-count {
  min-width: 24px;
  height: 24px;
  border-radius: 999px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(59, 130, 246, 0.12);
  color: rgba(30, 64, 175, 0.92);
  font-size: 12px;
  font-weight: 700;
}

.beeroom-canvas-chat-approval-btn {
  padding: 7px 10px;
  border-radius: 12px;
}

.beeroom-canvas-chat-approval-btn.is-danger {
  color: rgba(185, 28, 28, 0.92);
}

.beeroom-canvas-chat-textarea {
  width: 100%;
  min-height: 108px;
  resize: vertical;
  padding: 12px 14px;
  border-radius: 18px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(255, 255, 255, 0.92);
  color: rgba(30, 41, 59, 0.96);
  line-height: 1.6;
}

.beeroom-canvas-chat-textarea:focus-visible {
  border-color: rgba(96, 165, 250, 0.42);
  outline: none;
}

.beeroom-canvas-chat-send,
.beeroom-canvas-chat-demo {
  min-width: 96px;
  height: 40px;
  padding: 0 16px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  cursor: pointer;
}

.beeroom-canvas-chat-send {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.96), rgba(37, 99, 235, 0.94));
  color: rgba(255, 255, 255, 0.98);
}

.beeroom-canvas-chat-demo {
  background: rgba(255, 255, 255, 0.92);
  color: rgba(51, 65, 85, 0.92);
}

.beeroom-canvas-chat-demo.is-running {
  background: rgba(254, 243, 199, 0.9);
  color: rgba(146, 64, 14, 0.92);
}

.beeroom-canvas-chat-send:disabled,
.beeroom-canvas-chat-demo:disabled,
.beeroom-canvas-chat-approval-btn:disabled,
.beeroom-canvas-icon-btn:disabled {
  cursor: not-allowed;
  opacity: 0.58;
}

.beeroom-canvas-chat-compose-status.is-error {
  color: rgba(185, 28, 28, 0.92);
}

.beeroom-canvas-chat-select {
  flex: 1 1 auto;
}

.beeroom-canvas-chat-select :deep(.el-select__wrapper) {
  min-height: 40px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(255, 255, 255, 0.92);
  box-shadow: none;
}

.beeroom-canvas-chat-select :deep(.is-focused .el-select__wrapper),
.beeroom-canvas-chat-select :deep(.el-select__wrapper.is-focused) {
  border-color: rgba(96, 165, 250, 0.42);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper) {
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.16);
}

@media (max-width: 1100px) {
  .beeroom-canvas-chat {
    width: min(320px, 42vw);
    min-width: 280px;
  }
}
</style>
