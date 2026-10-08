type DefaultAgentOverviewSourceOptions = {
  profile: Record<string, unknown> | null | undefined;
  defaultAgentKey: string;
  defaultName: string;
  defaultDescription: string;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

export const buildDefaultAgentOverviewSource = ({
  profile,
  defaultAgentKey,
  defaultName,
  defaultDescription
}: DefaultAgentOverviewSourceOptions): Record<string, unknown> => {
  const source = asRecord(profile);
  return {
    ...source,
    id: defaultAgentKey,
    name: String(source.name || defaultName),
    description: String(source.description || defaultDescription),
    icon: source.icon,
    sandbox_container_id: source.sandbox_container_id ?? 1
  };
};
