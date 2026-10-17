<template>
  <!-- 全站唯一挂载点（App.vue）：所有 mzConfirm / mzPrompt 都渲染到这里，
       弹层 teleport 到 body，不受任何局部 overflow 或 transform 影响。 -->
  <Teleport to="body">
    <div
      v-for="entry in queue"
      :key="entry.id"
      class="mz-dialog-overlay"
      @pointerdown.self="settleMzDialog(entry.id, { action: 'close', value: '' })"
      @keydown.esc.prevent="settleMzDialog(entry.id, { action: 'close', value: '' })"
    >
      <MzDialogCard :entry="entry" />
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import MzDialogCard from './MzDialogCard.vue';
import { mzDialogQueue, settleMzDialog } from '@/utils/mzDialog';

const queue = mzDialogQueue;
</script>
