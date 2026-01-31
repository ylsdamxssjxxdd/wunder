export const externalLinkGroups = [
  {
    id: 'support',
    titleKey: 'portal.external.group.support.title',
    descriptionKey: 'portal.external.group.support.desc',
    items: [
      {
        id: 'docs',
        titleKey: 'portal.external.item.docs.title',
        descriptionKey: 'portal.external.item.docs.desc',
        type: 'external',
        url: 'https://example.com',
        icon: 'docs',
        tagKeys: ['portal.external.tag.docs', 'portal.external.tag.external'],
        statusKey: 'portal.card.pending',
        enabled: false
      },
      {
        id: 'community',
        titleKey: 'portal.external.item.community.title',
        descriptionKey: 'portal.external.item.community.desc',
        type: 'external',
        url: 'https://example.com',
        icon: 'community',
        tagKeys: ['portal.external.tag.community', 'portal.external.tag.external'],
        statusKey: 'portal.card.pending',
        enabled: false
      }
    ]
  },
  {
    id: 'ops',
    titleKey: 'portal.external.group.ops.title',
    descriptionKey: 'portal.external.group.ops.desc',
    items: [
      {
        id: 'status',
        titleKey: 'portal.external.item.status.title',
        descriptionKey: 'portal.external.item.status.desc',
        type: 'external',
        url: 'https://example.com',
        icon: 'status',
        tagKeys: ['portal.external.tag.status', 'portal.external.tag.external'],
        statusKey: 'portal.card.pending',
        enabled: false
      }
    ]
  }
];
