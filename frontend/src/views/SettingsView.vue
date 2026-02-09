<template>
  <div class="settings-view">
    <el-card>
      <h3>{{ t('settings.title') }}</h3>
      <p>{{ t('settings.placeholder') }}</p>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

const { t } = useI18n();
</script>
