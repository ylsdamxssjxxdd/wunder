// AI生成
/**
 * 设置页 11 分类注册表（方案 §九.2 + 账号分类拆分 + 互通「我的设备」§5.3；
 * 帮助入口已并入常规分类，点击在独立窗口打开文档站；运行记录页已移除，
 * 日运行趋势并入账号分类）。
 *
 * 只描述「导航结构 + 搜索关键词」，不承载业务逻辑：每个分类的内容组件
 * 在 `settingsCategoryComponents.ts` 里按需异步加载。
 *
 * 导航只有标题一行（分类摘要已移除），搜索也只匹配标题与关键词，避免出现
 * 「搜『模型』命中多个分类」这类误命中；关键词同时覆盖中英文口语叫法。
 */

export type SettingsCategoryId =
  | 'general'
  | 'account'
  | 'tools'
  | 'agent'
  | 'companion'
  | 'cron'
  | 'memory'
  | 'channels'
  | 'devices'
  | 'prompts'
  | 'archived';

export type SettingsCategoryMeta = {
  id: SettingsCategoryId;
  /**
   * Navigation icon. FontAwesome solid class (`fa-solid fa-gear`) or
   * `svg:<name>` — a bundled SVG under `src/assets/` rendered through a
   * currentColor mask (see messenger-settings-page.css), e.g. `svg:brain`.
   */
  icon: string;
  /** i18n key for the navigation title (also the content pane title). */
  titleKey: string;
  /** Lower-case search aliases; matched against the search keyword. */
  keywords: string[];
};

/** Navigation order is the contract from 方案 §九.2 + 互通「我的设备」(§5.3). */
export const SETTINGS_CATEGORY_IDS: SettingsCategoryId[] = [
  'general',
  'account',
  'tools',
  'agent',
  'companion',
  'cron',
  'memory',
  'channels',
  'devices',
  'prompts',
  'archived'
];

const META: Record<SettingsCategoryId, Omit<SettingsCategoryMeta, 'id'>> = {
  general: {
    icon: 'fa-solid fa-gear',
    titleKey: 'messenger.settingsPage.cat.general.title',
    keywords: [
      '常规',
      '语言',
      '主题',
      '外观',
      '配色',
      '字号',
      '发送键',
      '退出',
      '登录',
      '诊断',
      '测试',
      '调试',
      '性能',
      'debug',
      'test',
      'performance',
      '帮助',
      '手册',
      '帮助文档',
      'general',
      'theme',
      'language',
      'logout',
      'font',
      'help',
      'docs'
    ]
  },
  account: {
    icon: 'fa-solid fa-id-card',
    titleKey: 'messenger.settingsPage.cat.account.title',
    keywords: [
      '账号',
      '昵称',
      '头像',
      '密码',
      '资料',
      '个人信息',
      '运行记录',
      '日运行趋势',
      '统计',
      '消耗',
      '眺望心舰',
      '3d',
      'wunder',
      'account',
      'profile',
      'avatar',
      'password',
      'usage',
      'trend'
    ]
  },
  tools: {
    icon: 'fa-solid fa-screwdriver-wrench',
    titleKey: 'messenger.settingsPage.cat.tools.title',
    keywords: ['工具', '技能', '知识库', '脚本', 'tool', 'skill', 'knowledge', 'mcp']
  },
  agent: {
    icon: 'fa-solid fa-robot',
    titleKey: 'messenger.settingsPage.cat.agent.title',
    keywords: ['智能体', '提示词', '欢迎语', '审批', '行为', 'agent', 'prompt', 'approval']
  },
  companion: {
    icon: 'fa-solid fa-paw',
    titleKey: 'messenger.settingsPage.cat.companion.title',
    keywords: ['桌宠', '形象', '动画', '缩放', '气泡', 'companion', 'avatar', 'sprite', 'pet']
  },
  cron: {
    icon: 'fa-solid fa-clock',
    titleKey: 'messenger.settingsPage.cat.cron.title',
    keywords: ['定时', '计划', '任务', 'cron', 'schedule']
  },
  memory: {
    icon: 'svg:brain',
    titleKey: 'messenger.settingsPage.cat.memory.title',
    keywords: ['记忆', '碎片', 'memory']
  },
  channels: {
    icon: 'fa-solid fa-plug',
    titleKey: 'messenger.settingsPage.cat.channels.title',
    keywords: ['渠道', '扫码', '绑定', '重连', 'channel', 'qr', 'bind']
  },
  devices: {
    icon: 'fa-solid fa-satellite-dish',
    titleKey: 'messenger.settingsPage.cat.devices.title',
    keywords: [
      '设备',
      '我的设备',
      '互通',
      '互联',
      '影子',
      '远程工作区',
      'device',
      'interlink',
      'sync',
      'remote'
    ]
  },
  prompts: {
    icon: 'fa-solid fa-file-lines',
    titleKey: 'messenger.settingsPage.cat.prompts.title',
    keywords: ['提示词包', '分段', 'prompt pack', 'prompts']
  },
  archived: {
    icon: 'fa-solid fa-box-archive',
    titleKey: 'messenger.settingsPage.cat.archived.title',
    keywords: ['归档', '线程', '恢复', 'archive', 'thread']
  }
};

export const SETTINGS_CATEGORIES: SettingsCategoryMeta[] = SETTINGS_CATEGORY_IDS.map((id) => ({
  id,
  ...META[id]
}));

export const findSettingsCategory = (id: unknown): SettingsCategoryMeta | null => {
  const key = String(id || '').trim() as SettingsCategoryId;
  return SETTINGS_CATEGORIES.find((item) => item.id === key) || null;
};

/** Filter the navigation by a free-text keyword (title + keywords, case-insensitive). */
export const filterSettingsCategories = (
  keyword: string,
  translate: (key: string) => string
): SettingsCategoryMeta[] => {
  const needle = String(keyword || '').trim().toLowerCase();
  if (!needle) return SETTINGS_CATEGORIES;
  return SETTINGS_CATEGORIES.filter((item) => {
    const title = String(translate(item.titleKey) || '').toLowerCase();
    if (title.includes(needle)) return true;
    return item.keywords.some((alias) => alias.toLowerCase().includes(needle));
  });
};
