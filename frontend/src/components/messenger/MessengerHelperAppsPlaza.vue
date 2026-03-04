<template>
  <div class="messenger-helper-plaza" :class="{ compact }">
    <div class="messenger-helper-plaza-grid">
      <button class="messenger-helper-plaza-card" type="button" @click="openLocalFileSearch">
        <span class="messenger-helper-plaza-card-icon">
          <i class="fa-solid fa-folder-tree" aria-hidden="true"></i>
        </span>
        <span class="messenger-helper-plaza-card-main">
          <span class="messenger-helper-plaza-card-title">
            {{ t('userWorld.helperApps.localFileSearch.cardTitle') }}
          </span>
          <span class="messenger-helper-plaza-card-desc">
            {{ t('userWorld.helperApps.localFileSearch.cardDesc') }}
          </span>
        </span>
      </button>
      <button class="messenger-helper-plaza-card" type="button" @click="openGlobeApp">
        <span class="messenger-helper-plaza-card-icon">
          <i class="fa-solid fa-earth-asia" aria-hidden="true"></i>
        </span>
        <span class="messenger-helper-plaza-card-main">
          <span class="messenger-helper-plaza-card-title">
            {{ t('userWorld.helperApps.globe.cardTitle') }}
          </span>
          <span class="messenger-helper-plaza-card-desc">
            {{ t('userWorld.helperApps.globe.cardDesc') }}
          </span>
        </span>
      </button>
    </div>
  </div>

  <MessengerLocalFileSearchDialog
    v-model:visible="localFileSearchVisible"
  />
  <GlobeAppDialog v-model:visible="globeVisible" />
</template>

<script setup lang="ts">
import { ref } from 'vue';

import { useI18n } from '@/i18n';

import MessengerLocalFileSearchDialog from './MessengerLocalFileSearchDialog.vue';
import GlobeAppDialog from '@/components/globe/GlobeAppDialog.vue';

defineProps<{
  compact?: boolean;
}>();

const { t } = useI18n();

const localFileSearchVisible = ref(false);
const globeVisible = ref(false);

const openLocalFileSearch = () => {
  localFileSearchVisible.value = true;
};

const openGlobeApp = () => {
  globeVisible.value = true;
};
</script>

<style scoped>
.messenger-helper-plaza {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.messenger-helper-plaza-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 14px;
}

.messenger-helper-plaza.compact .messenger-helper-plaza-grid {
  grid-template-columns: 1fr;
}

.messenger-helper-plaza-card {
  width: 100%;
  min-height: 148px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.24);
  border-radius: 14px;
  background: linear-gradient(
    165deg,
    rgba(var(--ui-accent-rgb), 0.14),
    rgba(var(--ui-accent-rgb), 0.08)
  );
  color: var(--hula-text, #0f172a);
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: flex-start;
  gap: 12px;
  padding: 14px;
  text-align: left;
  cursor: pointer;
  box-shadow: 0 8px 22px rgba(15, 23, 42, 0.08);
  transition:
    transform 0.16s ease,
    box-shadow 0.16s ease,
    border-color 0.16s ease,
    background 0.16s ease;
}

.messenger-helper-plaza-card:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.42);
  background: linear-gradient(
    165deg,
    rgba(var(--ui-accent-rgb), 0.2),
    rgba(var(--ui-accent-rgb), 0.12)
  );
  box-shadow: 0 14px 30px rgba(15, 23, 42, 0.12);
  transform: translateY(-2px);
}

.messenger-helper-plaza-card-icon {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  background: rgba(var(--ui-accent-rgb), 0.26);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  font-size: 16px;
}

.messenger-helper-plaza-card-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.messenger-helper-plaza-card-title {
  font-size: 15px;
  font-weight: 600;
}

.messenger-helper-plaza-card-desc {
  color: var(--hula-muted, #64748b);
  font-size: 13px;
  line-height: 1.45;
}

.messenger-helper-plaza.compact .messenger-helper-plaza-card {
  min-height: 126px;
}
</style>
