import { defineStore } from 'pinia';

export type MessengerSection =
  | 'messages'
  | 'swarms'
  | 'plaza'
  | 'users'
  | 'groups'
  | 'agents'
  | 'tools'
  | 'files'
  | 'more';

export type MessengerConversationKind = 'agent' | 'direct' | 'group';

type ConversationIdentity = {
  kind: MessengerConversationKind;
  id: string;
  agentId?: string;
};

type RightPanelTab = 'sandbox' | 'timeline' | 'settings';

const normalizeSection = (value: unknown): MessengerSection => {
  const text = String(value || '').trim();
  const allowed: MessengerSection[] = [
    'messages',
    'swarms',
    'plaza',
    'users',
    'groups',
    'agents',
    'tools',
    'files',
    'more'
  ];
  if ((allowed as string[]).includes(text)) {
    return text as MessengerSection;
  }
  return 'messages';
};

const normalizeTab = (value: unknown): RightPanelTab => {
  const text = String(value || '').trim();
  if (text === 'timeline' || text === 'settings') {
    return text;
  }
  return 'sandbox';
};

const buildConversationKey = (identity?: ConversationIdentity | null): string => {
  if (!identity?.kind || !identity?.id) {
    return '';
  }
  return `${identity.kind}:${identity.id}`;
};

export const useSessionHubStore = defineStore('session-hub', {
  state: () => ({
    activeSection: 'messages' as MessengerSection,
    keyword: '',
    activeConversation: null as ConversationIdentity | null,
    rightTab: 'sandbox' as RightPanelTab
  }),
  getters: {
    activeConversationKey: (state) => buildConversationKey(state.activeConversation)
  },
  actions: {
    setSection(section: MessengerSection | string) {
      const normalized = normalizeSection(section);
      if (this.activeSection === normalized) {
        return;
      }
      this.activeSection = normalized;
    },
    setKeyword(keyword: string) {
      this.keyword = String(keyword || '').trimStart();
    },
    setActiveConversation(identity: ConversationIdentity | null) {
      if (!identity) {
        this.activeConversation = null;
        return;
      }
      const kind = String(identity.kind || '').trim();
      const id = String(identity.id || '').trim();
      if (!kind || !id) {
        return;
      }
      this.activeConversation = {
        kind: kind as MessengerConversationKind,
        id,
        agentId: identity.agentId ? String(identity.agentId).trim() : undefined
      };
    },
    clearActiveConversation() {
      this.activeConversation = null;
    },
    setRightTab(tab: RightPanelTab | string) {
      const normalized = normalizeTab(tab);
      if (this.rightTab === normalized) {
        return;
      }
      this.rightTab = normalized;
    }
  }
});

export const resolveSectionFromRoute = (
  routePath: string,
  querySection: unknown = ''
): MessengerSection => {
  const explicit = normalizeSection(querySection);
  if (explicit !== 'messages' || String(querySection || '').trim()) {
    return explicit;
  }

  const path = String(routePath || '').toLowerCase();
  if (path.includes('/tools') || path.includes('/channels') || path.includes('/cron')) {
    return 'tools';
  }
  if (path.includes('/beeroom')) {
    return 'swarms';
  }
  if (path.includes('/plaza')) {
    return 'plaza';
  }
  if (path.includes('/workspace')) {
    return 'files';
  }
  if (path.includes('/settings') || path.includes('/profile')) {
    return 'more';
  }
  if (path.includes('/user-world')) {
    return 'users';
  }
  if (path.includes('/home')) {
    return 'agents';
  }
  if (path.includes('/chat')) {
    return 'messages';
  }
  return 'messages';
};

export const buildMessengerConversationKey = buildConversationKey;
