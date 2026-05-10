export const shouldShowAgentSettingsPanelForSection = (section: unknown): boolean =>
  String(section || '').trim() === 'agents';
