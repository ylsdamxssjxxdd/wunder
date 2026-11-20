<template>
  <div
    class="messenger-settings-frame-category messenger-settings-tools"
    data-testid="settings-category-mcp"
  >
    <section class="messenger-settings-card messenger-settings-card--pane">
      <div class="messenger-settings-tools-host">
        <UserMcpPane :visible="true" />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

/** 设置分类「MCP」：只承载 MCP 面板，不再与技能/知识库共用选项卡。 */
const UserMcpPane = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserMcpPane.vue')
);

// controller 由设置页统一注入，本分类当前不需要直接调用；保留声明保持形状一致。
defineProps<{ controller: MessengerControllerContext; categoryActive?: boolean }>();
</script>
