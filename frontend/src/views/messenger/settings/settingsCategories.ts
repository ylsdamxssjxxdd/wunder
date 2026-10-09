// AI生成
/**
 * 设置页 13 分类注册表（方案 §九.2 + 账号分类拆分）。
 *
 * 只描述「导航结构 + 搜索关键词」，不承载业务逻辑：每个分类的内容组件
 * 在 `settingsCategoryComponents.ts` 里按需异步加载。
 *
 * 搜索只匹配标题与关键词（不匹配副标题），避免出现「搜『模型』命中多个分类」
 * 这类误命中；关键词同时覆盖中英文口语叫法。
 */

export type SettingsCategoryId =
  | 'general'
  | 'account'
  | 'models'
  | 'tools'
  | 'agent'
  | 'companion'
  | 'cron'
  | 'memory'
  | 'channels'
  | 'runtime'
  | 'prompts'
  | 'archived'
  | 'help';

export type SettingsCategoryMeta = {
  id: SettingsCategoryId;
  /** FontAwesome solid icon class, e.g. `fa-solid fa-gear`. */
  icon: string;
  /** i18n key for the navigation title (also the content pane title). */
  titleKey: string;
  /** i18n key for the navigation subtitle (also the content pane subtitle). */
  descKey: string;
  /** Lower-case search aliases; matched against the search keyword. */
  keywords: string[];
};

/** Navigation order is the contract from 方案 §九.2 (1 → 13，账号紧随常规之后). */
export const SETTINGS_CATEGORY_IDS: SettingsCategoryId[] = [
  'general',
  'account',
  'models',
  'tools',
  'agent',
  'companion',
  'cron',
  'memory',
  'channels',
  'runtime',
  'prompts',
  'archived',
  'help'
];

const META: Record<SettingsCategoryId, Omit<SettingsCategoryMeta, 'id'>> = {
  general: {
    icon: 'fa-solid fa-gear',
    titleKey: 'messenger.settingsPage.cat.general.title',
    descKey: 'messenger.settingsPage.cat.general.desc',
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
      'general',
      'theme',
      'language',
      'logout',
      'font'
    ]
  },
  account: {
    icon: 'fa-solid fa-id-card',
    titleKey: 'messenger.settingsPage.cat.account.title',
    descKey: 'messenger.settingsPage.cat.account.desc',
    keywords: [
      '账号',
      '昵称',
      '头像',
      '密码',
      '资料',
      '个人信息',
      'account',
      'profile',
      'avatar',
      'password'
    ]
  },
  models: {
    icon: 'fa-solid fa-microchip',
    titleKey: 'messenger.settingsPage.cat.models.title',
    descKey: 'messenger.settingsPage.cat.models.desc',
    keywords: ['模型', '默认模型', '思考等级', '推理', '上下文', 'model', 'reasoning', 'context']
  },
  tools: {
    icon: 'fa-solid fa-screwdriver-wrench',
    titleKey: 'messenger.settingsPage.cat.tools.title',
    descKey: 'messenger.settingsPage.cat.tools.desc',
    keywords: ['工具', '技能', '知识库', '脚本', 'tool', 'skill', 'knowledge', 'mcp']
  },
  agent: {
    icon: 'fa-solid fa-robot',
    titleKey: 'messenger.settingsPage.cat.agent.title',
    descKey: 'messenger.settingsPage.cat.agent.desc',
    keywords: ['智能体', '提示词', '欢迎语', '审批', '行为', 'agent', 'prompt', 'approval']
  },
  companion: {
    icon: 'fa-solid fa-paw',
    titleKey: 'messenger.settingsPage.cat.companion.title',
    descKey: 'messenger.settingsPage.cat.companion.desc',
    keywords: ['桌宠', '形象', '动画', '缩放', '气泡', 'companion', 'avatar', 'sprite', 'pet']
  },
  cron: {
    icon: 'fa-solid fa-clock',
    titleKey: 'messenger.settingsPage.cat.cron.title',
    descKey: 'messenger.settingsPage.cat.cron.desc',
    keywords: ['定时', '计划', '任务', 'cron', 'schedule']
  },
  memory: {
    icon: 'fa-solid fa-brain',
    titleKey: 'messenger.settingsPage.cat.memory.title',
    descKey: 'messenger.settingsPage.cat.memory.desc',
    keywords: ['记忆', '碎片', 'memory']
  },
  channels: {
    icon: 'fa-solid fa-plug',
    titleKey: 'messenger.settingsPage.cat.channels.title',
    descKey: 'messenger.settingsPage.cat.channels.desc',
    keywords: ['渠道', '扫码', '绑定', '重连', 'channel', 'qr', 'bind']
  },
  runtime: {
    icon: 'fa-solid fa-chart-line',
    titleKey: 'messenger.settingsPage.cat.runtime.title',
    descKey: 'messenger.settingsPage.cat.runtime.desc',
    keywords: ['运行记录', '统计', '消耗', '时长', '热力', 'runtime', 'usage', 'stats']
  },
  prompts: {
    icon: 'fa-solid fa-file-lines',
    titleKey: 'messenger.settingsPage.cat.prompts.title',
    descKey: 'messenger.settingsPage.cat.prompts.desc',
    keywords: ['提示词包', '分段', 'prompt pack', 'prompts']
  },
  archived: {
    icon: 'fa-solid fa-box-archive',
    titleKey: 'messenger.settingsPage.cat.archived.title',
    descKey: 'messenger.settingsPage.cat.archived.desc',
    keywords: ['归档', '线程', '恢复', 'archive', 'thread']
  },
  help: {
    icon: 'fa-solid fa-circle-question',
    titleKey: 'messenger.settingsPage.cat.help.title',
    descKey: 'messenger.settingsPage.cat.help.desc',
    keywords: ['帮助', '手册', '快捷键', '关于', '更新', '反馈', 'help', 'docs', 'shortcut', 'about']
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
