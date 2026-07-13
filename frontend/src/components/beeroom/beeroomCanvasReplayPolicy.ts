export const buildBeeroomCanvasReplayScopeKey = (options: {
  active: boolean;
  groupId: unknown;
  missionId: unknown;
  teamRunId: unknown;
}) => [
  options.active ? 'active' : 'inactive',
  String(options.groupId || '').trim(),
  String(options.missionId || '').trim(),
  String(options.teamRunId || '').trim()
].join('|');
