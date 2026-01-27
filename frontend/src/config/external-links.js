export const externalLinkGroups = [
  {
    id: 'support',
    title: '支持与文档',
    description: '产品指南、知识库与社区入口',
    items: [
      {
        id: 'docs',
        title: '使用文档',
        description: '产品指南、最佳实践与常见问题。',
        type: 'external',
        url: 'https://example.com',
        icon: 'docs',
        tags: ['文档', '外链'],
        status: '待配置',
        enabled: false
      },
      {
        id: 'community',
        title: '交流社区',
        description: '加入用户社区，交流方案与经验。',
        type: 'external',
        url: 'https://example.com',
        icon: 'community',
        tags: ['社区', '外链'],
        status: '待配置',
        enabled: false
      }
    ]
  },
  {
    id: 'ops',
    title: '运维与状态',
    description: '服务状态与变更公告',
    items: [
      {
        id: 'status',
        title: '系统状态',
        description: '查看服务状态与可用性公告。',
        type: 'external',
        url: 'https://example.com',
        icon: 'status',
        tags: ['状态', '外链'],
        status: '待配置',
        enabled: false
      }
    ]
  }
];
