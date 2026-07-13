type BeeroomGroupMemberScope = {
  members?: Array<{ agent_id?: unknown }> | null;
} | null | undefined;

// The workbench only receives group detail, so even an empty member list is
// authoritative and stale runtime state must not create canvas nodes.
export const isBeeroomGroupMember = (group: BeeroomGroupMemberScope, agentId: unknown): boolean => {
  const normalizedAgentId = String(agentId || '').trim();
  if (!normalizedAgentId) return false;
  if (!group || !Array.isArray(group.members)) return false;
  const members = Array.isArray(group?.members) ? group.members : [];
  return members.some((member) => String(member?.agent_id || '').trim() === normalizedAgentId);
};
