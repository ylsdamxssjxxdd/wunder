<template>
  <el-dialog
    v-model="visibleProxy"
    :title="dialogTitle"
    width="640px"
    class="beeroom-agent-output-dialog"
    append-to-body
    draggable
    destroy-on-close
    :modal="false"
    :lock-scroll="false"
  >
    <div class="beeroom-agent-output-shell" data-testid="beeroom-agent-output-dialog">
      <div class="beeroom-agent-output-topbar">
        <div class="beeroom-agent-output-summary">
          {{ outputs.length ? t('beeroom.canvas.agentOutputCount', { count: outputs.length }) : t('beeroom.canvas.agentOutputEmpty') }}
        </div>
        <div class="beeroom-agent-output-badges">
          <span v-if="roleLabel" class="beeroom-agent-output-badge">{{ roleLabel }}</span>
          <span v-if="statusLabel" class="beeroom-agent-output-badge is-status">{{ statusLabel }}</span>
        </div>
      </div>

      <div v-if="outputs.length" class="beeroom-agent-output-list">
        <article v-for="message in outputs" :key="message.key" class="beeroom-agent-output-card">
          <header class="beeroom-agent-output-card-head">
            <span class="beeroom-agent-output-avatar">
              <img
                v-if="resolveMessageAvatarImage(message)"
                class="beeroom-agent-output-avatar-img"
                :src="resolveMessageAvatarImage(message)"
                alt=""
              />
              <span v-else class="beeroom-agent-output-avatar-text">
                {{ avatarLabel(message.senderName || agentName || '-') }}
              </span>
            </span>
            <div class="beeroom-agent-output-head-copy">
              <div class="beeroom-agent-output-head-title">{{ message.senderName || agentName || '-' }}</div>
              <div class="beeroom-agent-output-head-meta">
                <span>{{ message.timeLabel || '-' }}</span>
                <span v-if="message.mention">{{ t('beeroom.canvas.agentOutputMention', { target: message.mention }) }}</span>
                <span v-if="message.meta">{{ message.meta }}</span>
              </div>
            </div>
          </header>

          <BeeroomCanvasChatMarkdown
            :cache-key="`beeroom-agent-output:${message.key}`"
            :content="String(message.body || '')"
          />
        </article>
      </div>

      <div v-else class="beeroom-agent-output-empty">
        <i class="fa-regular fa-comment-dots" aria-hidden="true"></i>
        <span>{{ t('beeroom.canvas.agentOutputEmptyHint') }}</span>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import BeeroomCanvasChatMarkdown from '@/components/beeroom/BeeroomCanvasChatMarkdown.vue';
import { useI18n } from '@/i18n';

const props = defineProps<{
  visible: boolean;
  agentName: string;
  roleLabel?: string;
  statusLabel?: string;
  outputs: MissionChatMessage[];
  resolveMessageAvatarImage: (message: MissionChatMessage) => string;
  avatarLabel: (value: unknown) => string;
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
}>();

const { t } = useI18n();

const visibleProxy = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const dialogTitle = computed(() =>
  t('beeroom.canvas.agentOutputTitle', {
    agent: String(props.agentName || '-').trim() || '-'
  })
);
</script>

<style scoped>
.beeroom-agent-output-shell {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.beeroom-agent-output-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.beeroom-agent-output-summary {
  font-size: 12px;
  color: rgba(71, 85, 105, 0.8);
}

.beeroom-agent-output-badges {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.beeroom-agent-output-badge {
  display: inline-flex;
  align-items: center;
  min-height: 26px;
  padding: 0 10px;
  border-radius: 999px;
  background: rgba(245, 158, 11, 0.12);
  color: #92400e;
  font-size: 12px;
  font-weight: 600;
}

.beeroom-agent-output-badge.is-status {
  background: rgba(59, 130, 246, 0.12);
  color: #1d4ed8;
}

.beeroom-agent-output-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-height: min(66vh, 720px);
  overflow: auto;
  padding-right: 4px;
}

.beeroom-agent-output-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border-radius: 18px;
  border: 1px solid rgba(226, 232, 240, 0.92);
  background:
    linear-gradient(180deg, rgba(255, 252, 245, 0.96), rgba(255, 255, 255, 0.98));
  box-shadow: 0 10px 24px rgba(148, 163, 184, 0.12);
}

.beeroom-agent-output-card-head {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.beeroom-agent-output-avatar {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  flex: 0 0 36px;
  overflow: hidden;
  border-radius: 12px;
  background: linear-gradient(135deg, rgba(253, 224, 71, 0.92), rgba(249, 115, 22, 0.9));
  color: #5b2107;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.45);
}

.beeroom-agent-output-avatar-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.beeroom-agent-output-avatar-text {
  font-size: 14px;
  font-weight: 700;
}

.beeroom-agent-output-head-copy {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.beeroom-agent-output-head-title {
  font-size: 13px;
  font-weight: 700;
  color: #0f172a;
}

.beeroom-agent-output-head-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 10px;
  font-size: 12px;
  color: rgba(71, 85, 105, 0.84);
}

.beeroom-agent-output-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  min-height: 168px;
  border-radius: 18px;
  border: 1px dashed rgba(148, 163, 184, 0.42);
  background: linear-gradient(180deg, rgba(248, 250, 252, 0.92), rgba(241, 245, 249, 0.72));
  color: rgba(71, 85, 105, 0.86);
  font-size: 13px;
}

@media (max-width: 720px) {
  .beeroom-agent-output-topbar {
    flex-direction: column;
    align-items: flex-start;
  }

  .beeroom-agent-output-list {
    max-height: 58vh;
  }
}
</style>
