// AI生成
/**
 * 设置页分类内容组件的按需加载表（方案 §九 性能要求）。
 *
 * 14 个分类都是独立异步 chunk：切到哪个分类才加载并挂载它，覆盖层与其它分类
 * 不会被重建；`MessengerSettingsHost` 用 `<KeepAlive :max="3">` 只保留最近 3 个
 * 已访问分类的实例，避免 14 个面板同时初始化。
 */

import type { Component } from 'vue';

import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

import type { SettingsCategoryId } from './settingsCategories';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const SettingsGeneralCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsGeneralCategory.vue')
);
export const SettingsAccountCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsAccountCategory.vue')
);
export const SettingsModelsCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsModelsCategory.vue')
);
export const SettingsToolsCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsToolsCategory.vue')
);
export const SettingsAgentCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsAgentCategory.vue')
);
export const SettingsCompanionCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsCompanionCategory.vue')
);
export const SettingsCronCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsCronCategory.vue')
);
export const SettingsMemoryCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsMemoryCategory.vue')
);
export const SettingsChannelsCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsChannelsCategory.vue')
);
export const SettingsDevicesCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsDevicesCategory.vue')
);
export const SettingsRuntimeCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsRuntimeCategory.vue')
);
export const SettingsPromptsCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsPromptsCategory.vue')
);
export const SettingsArchivedCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsArchivedCategory.vue')
);
export const SettingsHelpCategory = lazy(
  () => import('@/views/messenger/settings/categories/SettingsHelpCategory.vue')
);

export const SETTINGS_CATEGORY_COMPONENTS: Record<SettingsCategoryId, Component> = {
  general: SettingsGeneralCategory,
  account: SettingsAccountCategory,
  models: SettingsModelsCategory,
  tools: SettingsToolsCategory,
  agent: SettingsAgentCategory,
  companion: SettingsCompanionCategory,
  cron: SettingsCronCategory,
  memory: SettingsMemoryCategory,
  channels: SettingsChannelsCategory,
  devices: SettingsDevicesCategory,
  runtime: SettingsRuntimeCategory,
  prompts: SettingsPromptsCategory,
  archived: SettingsArchivedCategory,
  help: SettingsHelpCategory
};
