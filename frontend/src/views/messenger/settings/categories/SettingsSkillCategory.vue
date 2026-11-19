<template>
  <div
    class="messenger-settings-frame-category messenger-settings-tools"
    data-testid="settings-category-skill"
  >
    <section class="messenger-settings-card messenger-settings-card--pane">
      <div class="messenger-settings-tools-host">
        <UserSkillPane :visible="true" :active="true" />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

/** 设置分类「技能」：只承载技能面板，导入/导出与文件编辑都在面板内完成。 */
const UserSkillPane = defineRecoverableAsyncComponent(
  () => import('@/components/user-tools/UserSkillPane.vue')
);

// controller 由设置页统一注入，本分类当前不需要直接调用；保留声明保持形状一致。
defineProps<{ controller: MessengerControllerContext; categoryActive?: boolean }>();
</script>
