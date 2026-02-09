<template>
  <div class="portal-shell channel-manager-shell">
    <UserTopbar :title="t('channels.title')" :subtitle="t('channels.subtitle')" :hide-chat="true" />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section channel-manager-section">
            <UserChannelSettingsPanel ref="panelRef" mode="page" @changed="handleChanged" />
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { onMounted, ref } from 'vue';

import UserChannelSettingsPanel from '@/components/channels/UserChannelSettingsPanel.vue';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const { t } = useI18n();
const panelRef = ref(null);

const handleChanged = () => {};

onMounted(() => {
  panelRef.value?.refreshAll?.();
});
</script>
