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
  z-index: 1;
  display: flex;
  width: var(--beeroom-chat-width, 344px);
  min-width: 0;
  flex-direction: column;
  gap: 12px;
  padding: 14px 14px 14px 18px;
  border-left: 1px solid rgba(148, 163, 184, 0.2);
  background:
    linear-gradient(180deg, rgba(13, 14, 20, 0.95), rgba(9, 10, 15, 0.97)),
    linear-gradient(180deg, rgba(239, 68, 68, 0.03), rgba(148, 163, 184, 0.02));
  color: #e5e7eb;
  box-shadow:
    inset 1px 0 0 rgba(255, 255, 255, 0.03),
    inset 0 1px 0 rgba(255, 255, 255, 0.02);
  overflow: hidden;
  transition:
    width 240ms cubic-bezier(0.22, 1, 0.36, 1),
    padding 240ms cubic-bezier(0.22, 1, 0.36, 1),
    background 180ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 180ms cubic-bezier(0.22, 1, 0.36, 1);
}

.beeroom-canvas-chat::before {
  content: '';
  position: absolute;
  inset: 0 0 auto 0;
  height: 56px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.04), transparent);
  pointer-events: none;
}

.beeroom-canvas-chat.collapsed {
  width: 0;
  padding: 0;
  border-left: 0;
  box-shadow: none;
  background: transparent;
  gap: 0;
  overflow: visible;
}

.beeroom-canvas-chat-handle {
  position: absolute;
  left: -12px;
  top: 50%;
  transform: translateY(-50%);
  width: 22px;
  height: 78px;
  border: 1px solid rgba(148, 163, 184, 0.36);
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.95), rgba(15, 23, 42, 0.94));
  color: #cbd5e1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  z-index: 2;
  opacity: 0.82;
  box-shadow: 0 12px 28px rgba(2, 6, 23, 0.3);
}

.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
  left: -14px;
}

.beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat-handle:focus-visible {
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.98), rgba(15, 23, 42, 0.98));
  border-color: rgba(148, 163, 184, 0.56);
  outline: none;
}

.beeroom-canvas-chat-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 4px;
  border-bottom: 1px solid rgba(148, 163, 184, 0.14);
}

.beeroom-canvas-chat-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.beeroom-canvas-chat-title {
  color: #f3f4f6;
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.beeroom-canvas-chat-runtime,
.beeroom-canvas-chat-time,
.beeroom-canvas-chat-extra,
.beeroom-canvas-chat-approval-meta,
.beeroom-canvas-chat-compose-status {
  font-size: 11px;
  color: rgba(156, 163, 175, 0.92);
}

.beeroom-canvas-chat-runtime {
  margin-top: 6px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.beeroom-canvas-runtime-chip {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(30, 41, 59, 0.48);
  color: rgba(226, 232, 240, 0.92);
  font-size: 11px;
  line-height: 1;
}

.beeroom-canvas-runtime-chip.is-running {
  border-color: rgba(239, 68, 68, 0.36);
  background: rgba(127, 29, 29, 0.3);
  color: rgba(254, 202, 202, 0.96);
}

.beeroom-canvas-runtime-chip.is-success {
  border-color: rgba(34, 197, 94, 0.36);
  background: rgba(21, 128, 61, 0.28);
  color: rgba(187, 247, 208, 0.96);
}

.beeroom-canvas-runtime-chip.is-danger {
  border-color: rgba(239, 68, 68, 0.4);
  background: rgba(127, 29, 29, 0.3);
  color: rgba(254, 202, 202, 0.96);
}

.beeroom-canvas-runtime-chip.is-warn {
  border-color: rgba(245, 158, 11, 0.42);
  background: rgba(146, 64, 14, 0.28);
  color: rgba(254, 240, 138, 0.98);
}

.beeroom-canvas-runtime-session {
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, monospace;
}

.beeroom-canvas-icon-btn,
.beeroom-canvas-chat-approval-btn {
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: rgba(19, 21, 29, 0.84);
  color: #d1d5db;
  cursor: pointer;
}

.beeroom-canvas-icon-btn {
  width: 28px;
  height: 28px;
  border-radius: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.beeroom-canvas-icon-btn:hover:not(:disabled),
.beeroom-canvas-icon-btn:focus-visible:not(:disabled),
.beeroom-canvas-chat-approval-btn:hover:not(:disabled),
.beeroom-canvas-chat-approval-btn:focus-visible:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.42);
  background: rgba(30, 41, 59, 0.96);
  color: #e2e8f0;
  outline: none;
}

.beeroom-canvas-chat-stream {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
  padding-right: 2px;
}

.beeroom-canvas-chat-message {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.beeroom-canvas-chat-avatar {
  width: 34px;
  height: 34px;
  padding: 0;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 12px;
  background: rgba(23, 25, 34, 0.9);
  color: #e5e7eb;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
  overflow: hidden;
  box-sizing: border-box;
  line-height: 0;
  cursor: pointer;
}

.beeroom-canvas-chat-avatar.is-system {
  cursor: default;
  background: rgba(23, 25, 34, 0.76);
  color: #9ca3af;
}

.beeroom-canvas-chat-avatar.is-user {
  cursor: default;
  background: rgba(127, 29, 29, 0.52);
  color: #fee2e2;
}

.beeroom-canvas-chat-avatar-img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  border-radius: inherit;
}

.beeroom-canvas-chat-main {
  display: grid;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-meta-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.beeroom-canvas-chat-sender {
  padding: 0;
  border: none;
  background: transparent;
  color: #f3f4f6;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
  border-radius: 8px;
}

.beeroom-canvas-chat-sender.is-system {
  color: #9ca3af;
  cursor: default;
}

.beeroom-canvas-chat-sender.is-user {
  color: #fee2e2;
}

.beeroom-canvas-chat-bubble {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.12);
  background: linear-gradient(180deg, rgba(24, 26, 34, 0.86), rgba(16, 18, 24, 0.82));
  color: #e5e7eb;
  font-size: 12.5px;
  line-height: 1.65;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-message.is-mother .beeroom-canvas-chat-bubble {
  border-color: rgba(239, 68, 68, 0.24);
  background: rgba(69, 10, 10, 0.24);
}

.beeroom-canvas-chat-message.is-worker .beeroom-canvas-chat-bubble {
  border-color: rgba(148, 163, 184, 0.2);
  background: rgba(31, 41, 55, 0.32);
}

.beeroom-canvas-chat-message.is-system .beeroom-canvas-chat-bubble {
  border-style: dashed;
  background: rgba(17, 24, 39, 0.56);
}

.beeroom-canvas-chat-message.is-user .beeroom-canvas-chat-bubble {
  border-color: rgba(239, 68, 68, 0.32);
  background: rgba(127, 29, 29, 0.3);
}

.beeroom-canvas-chat-mention {
  color: #fca5a5;
  font-weight: 700;
}

.beeroom-canvas-chat-approvals,
.beeroom-canvas-chat-composer {
  display: grid;
  gap: 8px;
}

.beeroom-canvas-chat-approvals {
  max-height: 178px;
  overflow: auto;
  padding: 8px 0 4px;
  border-top: 1px solid rgba(148, 163, 184, 0.14);
  border-bottom: 1px solid rgba(148, 163, 184, 0.14);
}

.beeroom-canvas-chat-approvals-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: rgba(226, 232, 240, 0.94);
  font-size: 12px;
  font-weight: 600;
}

.beeroom-canvas-chat-approvals-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 20px;
  padding: 0 6px;
  border-radius: 999px;
  border: 1px solid rgba(245, 158, 11, 0.32);
  background: rgba(120, 53, 15, 0.28);
  color: rgba(254, 240, 138, 0.96);
  font-size: 11px;
}

.beeroom-canvas-chat-approval-item {
  display: grid;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: rgba(15, 23, 42, 0.5);
}

.beeroom-canvas-chat-approval-summary {
  color: rgba(243, 244, 246, 0.94);
  font-size: 12px;
  line-height: 1.5;
}

.beeroom-canvas-chat-approval-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.beeroom-canvas-chat-approval-btn {
  min-height: 26px;
  padding: 0 8px;
  border-radius: 8px;
  background: rgba(30, 41, 59, 0.65);
  color: rgba(226, 232, 240, 0.96);
  font-size: 11px;
}

.beeroom-canvas-chat-approval-btn.is-danger {
  border-color: rgba(239, 68, 68, 0.34);
  background: rgba(127, 29, 29, 0.44);
  color: rgba(254, 202, 202, 0.98);
}

.beeroom-canvas-chat-composer {
  padding-top: 12px;
  border-top: 1px solid rgba(148, 163, 184, 0.16);
  background: linear-gradient(180deg, rgba(9, 10, 15, 0), rgba(9, 10, 15, 0.52));
}

.beeroom-canvas-chat-compose-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.beeroom-canvas-chat-textarea {
  width: 100%;
  min-height: 84px;
  resize: none;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.92), rgba(15, 17, 23, 0.88));
  color: #f3f4f6;
  line-height: 1.6;
  outline: none;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-textarea:focus-visible {
  box-shadow:
    0 0 0 2px rgba(96, 165, 250, 0.46),
    inset 0 1px 0 rgba(255, 255, 255, 0.05);
}

.beeroom-canvas-chat-compose-status.is-error {
  color: #f87171;
}

.beeroom-canvas-chat-select {
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-select :deep(.el-select__wrapper) {
  min-height: 38px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.92), rgba(15, 17, 23, 0.88));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-select :deep(.el-select__selected-item),
.beeroom-canvas-chat-select :deep(.el-select__placeholder),
.beeroom-canvas-chat-select :deep(.el-select__input) {
  color: #f3f4f6;
}

.beeroom-canvas-chat-select :deep(.el-select__caret) {
  color: rgba(209, 213, 219, 0.78);
}

.beeroom-canvas-chat-select :deep(.is-focused .el-select__wrapper),
.beeroom-canvas-chat-select :deep(.el-select__wrapper.is-focused) {
  box-shadow:
    0 0 0 2px rgba(96, 165, 250, 0.46),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper) {
  border: 1px solid rgba(148, 163, 184, 0.28);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.98), rgba(14, 16, 22, 0.98));
  box-shadow: 0 18px 40px rgba(0, 0, 0, 0.42);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper .el-popper__arrow::before) {
  border-color: rgba(148, 163, 184, 0.28);
  background: rgba(14, 16, 22, 0.98);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item) {
  color: #e5e7eb;
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-hovering),
:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item:hover) {
  background: rgba(31, 41, 55, 0.78);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-selected) {
  color: #fca5a5;
  background: rgba(127, 29, 29, 0.6);
}

.beeroom-canvas-chat-send,
.beeroom-canvas-chat-demo {
  min-height: 34px;
  padding: 0 12px;
  border-radius: 12px;
  cursor: pointer;
}

.beeroom-canvas-chat-send {
  min-width: 74px;
  border: 1px solid rgba(239, 68, 68, 0.34);
  background: linear-gradient(135deg, rgba(220, 38, 38, 0.92), rgba(185, 28, 28, 0.92));
  color: #fee2e2;
  box-shadow: 0 10px 24px rgba(127, 29, 29, 0.24);
}

.beeroom-canvas-chat-demo {
  min-width: 86px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(30, 41, 59, 0.72);
  color: rgba(226, 232, 240, 0.92);
}

.beeroom-canvas-chat-demo.is-running {
  border-color: rgba(245, 158, 11, 0.34);
  background: rgba(120, 53, 15, 0.44);
  color: rgba(254, 240, 138, 0.98);
}

.beeroom-canvas-chat-send:disabled,
.beeroom-canvas-chat-demo:disabled,
.beeroom-canvas-chat-approval-btn:disabled,
.beeroom-canvas-icon-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

@media (max-width: 900px) {
  .beeroom-canvas-chat {
    width: 100%;
    padding: 18px 14px 14px;
    border-left: 0;
    border-top: 1px solid rgba(148, 163, 184, 0.2);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.03),
      inset 0 1px 0 rgba(255, 255, 255, 0.02);
  }

  .beeroom-canvas-chat.collapsed {
    width: 100%;
  }

  .beeroom-canvas-chat-handle {
    left: 50%;
    top: -12px;
    width: 76px;
    height: 22px;
    transform: translateX(-50%);
  }

  .beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
    left: 50%;
    top: -14px;
  }

  .beeroom-canvas-chat-head,
  .beeroom-canvas-chat-compose-foot {
    flex-wrap: wrap;
  }
}
</style>
