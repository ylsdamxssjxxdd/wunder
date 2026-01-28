const pptxgen = require('pptxgenjs');

const SLIDE_W = 10;
const SLIDE_H = 5.625;
const HEADER_H = 0.4;

const TITLE_COLOR = 'FFFFFF';
const BODY_COLOR = '1F1F1F';
const ACCENT_COLOR = '2F5597';
const BG_COLOR = 'F7F9FC';
const CARD_COLOR = 'FFFFFF';
const MUTED_COLOR = 'E6EEF7';

const FONT_CN = 'SimHei';
const FONT_EN = 'Times New Roman';
const TITLE_FONT_SIZE = 26;
const BODY_FONT_SIZE = 16;
const TEMPLATE_NAME = 'report';
const OUTPUT_FILE = 'docs/wunder-video-intro.pptx';

const SLIDES = [
  {
    title: 'wunder 智能体调度平台',
    bodyText: '视频介绍 | 约 10 分钟\n一个入口对接多模型、多工具、多流程',
    bodyOptions: { fontSize: 20, lineSpacingMultiple: 1.3 }
  },
  {
    title: '内容概览',
    bullets: [
      '设计理念与价值主张',
      '核心架构与运行流程',
      '用户侧：新建智能体与多智能体并行',
      '用户侧：工具/知识库/技能与流式过程',
      '管理侧：治理、监控与成本',
      '生态与集成：MCP/A2A/API'
    ]
  },
  {
    title: '设计理念：三句话',
    bullets: [
      '对开发者：一切都是接口',
      '对大模型：一切皆工具',
      '对企业：一切可治理',
      '能力可复用、成本可量化、风险可控制'
    ]
  },
  {
    title: '价值主张',
    bullets: [
      '统一入口对接多模型、多工具、多流程',
      '智能体能力中心：可管理、可复用、可评估',
      '过程透明、结果可追溯',
      '从试验走向规模化运营'
    ]
  },
  {
    title: '系统架构概览',
    bullets: [
      'Rust + Axum 提供 /wunder 统一入口',
      '调度引擎：模型选择、工具编排、流程控制',
      '工具体系：内置工具、MCP、Skills、知识库',
      '存储与监控：会话、产物、事件、工作区',
      'SSE 流式返回过程与最终结果'
    ]
  },
  {
    title: '运行流程：规划 → 执行 → 汇总',
    bullets: [
      '请求携带 user_id 与 agent_id',
      '构建提示词并加载工具/知识库/技能',
      '模型生成工具调用或答案',
      '工具执行结果写入存储',
      '流式返回中间过程与最终回复'
    ]
  },
  {
    title: '用户侧：新建智能体',
    bullets: [
      '功能广场首卡：新建智能体应用',
      '配置名称、描述与系统提示词',
      '挂载工具/知识库/技能',
      '可共享给同等级用户',
      '一键启动专属会话'
    ]
  },
  {
    title: '用户侧：多智能体并行',
    bullets: [
      '隔离维度：user_id + agent_id',
      '同一用户可并行多个智能体',
      '互不串话、独立工作区',
      '支持多任务协作与角色分工'
    ]
  },
  {
    title: '用户侧：工具与知识沉淀',
    bullets: [
      '工具管理统一配置 MCP/Skills/知识库',
      '技能固化流程，知识库增强回答',
      '计划面板/问询面板提升可控性',
      'A2UI 输出便于业务侧呈现',
      '工具调用与成本占用可视化'
    ]
  },
  {
    title: '管理侧：用户与权限治理',
    bullets: [
      '账号管理与权限分级',
      '工具/智能体访问策略',
      '每日额度配置与超额提示',
      '共享范围与白名单控制'
    ]
  },
  {
    title: '管理侧：监控与成本',
    bullets: [
      '会话状态与工具调用统计',
      '上下文占用与成本口径一致',
      '异常回放与线程详情',
      '性能采样与压测基线'
    ]
  },
  {
    title: '生态与集成',
    bullets: [
      'MCP 工具服务接入',
      'A2A 跨智能体协作接口',
      '统一对外接口：/wunder',
      '多模型适配与快速扩展'
    ]
  },
  {
    title: '总结与收尾',
    bullets: [
      '企业级智能体调度平台',
      '统一入口、流程编排、可治理',
      '让智能体能力真正落地业务现场'
    ]
  }
];

const RECT_SHAPE =
  pptxgen.ShapeType && pptxgen.ShapeType.rect ? pptxgen.ShapeType.rect : 'rect';

const CONTENT_TOP = HEADER_H + 0.35;
const CONTENT_BOTTOM = 0.45;
const CONTENT_H = SLIDE_H - CONTENT_TOP - CONTENT_BOTTOM;

const TEMPLATES = {
  report: {
    bg: BG_COLOR,
    accent: ACCENT_COLOR,
    card: CARD_COLOR,
    muted: MUTED_COLOR
  },
  lecture: {
    bg: BG_COLOR,
    accent: ACCENT_COLOR,
    card: CARD_COLOR,
    muted: MUTED_COLOR
  },
  education: {
    bg: BG_COLOR,
    accent: ACCENT_COLOR,
    card: CARD_COLOR,
    muted: MUTED_COLOR
  },
  defense: {
    bg: 'FFFFFF',
    accent: ACCENT_COLOR,
    card: CARD_COLOR,
    muted: MUTED_COLOR
  },
  simple: {
    bg: 'FFFFFF',
    accent: ACCENT_COLOR,
    card: CARD_COLOR,
    muted: MUTED_COLOR
  }
};

const LAYOUTS = {
  report: {
    titleX: 0.7,
    titleY: 0,
    titleW: 8.6,
    titleH: HEADER_H,
    bodyX: 0.7,
    bodyY: CONTENT_TOP,
    bodyW: 8.6,
    bodyH: CONTENT_H
  },
  lecture: {
    titleX: 1.4,
    titleY: 0,
    titleW: 8.1,
    titleH: HEADER_H,
    bodyX: 1.4,
    bodyY: CONTENT_TOP,
    bodyW: 8.1,
    bodyH: CONTENT_H
  },
  education: {
    titleX: 0.7,
    titleY: 0,
    titleW: 8.6,
    titleH: HEADER_H,
    bodyX: 0.7,
    bodyY: CONTENT_TOP,
    bodyW: 8.6,
    bodyH: CONTENT_H
  },
  defense: {
    titleX: 0.7,
    titleY: 0,
    titleW: 8.6,
    titleH: HEADER_H,
    bodyX: 0.7,
    bodyY: CONTENT_TOP,
    bodyW: 8.6,
    bodyH: CONTENT_H
  },
  simple: {
    titleX: 0.7,
    titleY: 0,
    titleW: 8.6,
    titleH: HEADER_H,
    bodyX: 0.7,
    bodyY: CONTENT_TOP,
    bodyW: 8.6,
    bodyH: CONTENT_H
  }
};

function resolveTemplate(name) {
  if (Object.prototype.hasOwnProperty.call(TEMPLATES, name)) {
    return name;
  }
  return 'report';
}

function addRect(slide, x, y, w, h, fillColor, lineColor) {
  slide.addShape(RECT_SHAPE, {
    x,
    y,
    w,
    h,
    fill: { color: fillColor },
    line: { color: lineColor || fillColor }
  });
}

function isCjkChar(char) {
  return /[\u2E80-\u2FFF\u3000-\u303F\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF00-\uFFEF]/.test(
    char
  );
}

function containsCjk(text) {
  if (!text) {
    return false;
  }
  return /[\u2E80-\u2FFF\u3000-\u303F\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF00-\uFFEF]/.test(
    text
  );
}

function resolveRunFont(char, fallbackFont) {
  if (isCjkChar(char)) {
    return FONT_CN;
  }
  if (/\s/.test(char)) {
    return fallbackFont || FONT_EN;
  }
  return FONT_EN;
}

function buildTextRuns(text) {
  if (!text) {
    return [];
  }
  const runs = [];
  let currentFont = null;
  let buffer = '';
  for (const char of text) {
    const font = resolveRunFont(char, currentFont);
    if (currentFont && font !== currentFont) {
      runs.push({ text: buffer, options: { fontFace: currentFont } });
      buffer = '';
    }
    currentFont = font;
    buffer += char;
  }
  if (buffer) {
    runs.push({ text: buffer, options: { fontFace: currentFont || FONT_EN } });
  }
  return runs;
}

function addBackground(slide, templateName, theme) {
  addRect(slide, 0, 0, SLIDE_W, SLIDE_H, theme.bg);
  addRect(slide, 0, 0, SLIDE_W, HEADER_H, theme.accent);
  if (templateName === 'lecture') {
    addRect(slide, 0, 0, 1.2, SLIDE_H, theme.accent);
    return;
  }
  if (templateName === 'simple') {
    addRect(slide, 0, 0, 0.15, SLIDE_H, theme.accent);
    return;
  }
}

function addTitle(slide, text, layout) {
  const runs = buildTextRuns(text || '');
  slide.addText(runs.length ? runs : text || '', {
    x: layout.titleX,
    y: layout.titleY,
    w: layout.titleW,
    h: layout.titleH,
    fontSize: TITLE_FONT_SIZE,
    bold: true,
    color: TITLE_COLOR,
    valign: 'middle'
  });
}

function addBodyText(slide, text, layout, options) {
  if (!text) {
    return;
  }
  const runs = buildTextRuns(text);
  slide.addText(runs.length ? runs : text, {
    x: layout.bodyX,
    y: layout.bodyY,
    w: layout.bodyW,
    h: layout.bodyH,
    fontSize: options?.fontSize || BODY_FONT_SIZE,
    color: BODY_COLOR,
    valign: options?.valign || 'top',
    align: options?.align || 'left',
    lineSpacingMultiple: options?.lineSpacingMultiple || 1.2
  });
}

function addBullets(slide, bullets, layout) {
  if (!Array.isArray(bullets) || bullets.length === 0) {
    return;
  }
  const lines = bullets.filter(Boolean).join('\n');
  if (!lines) {
    return;
  }
  const runs = buildTextRuns(lines);
  slide.addText(runs.length ? runs : lines, {
    x: layout.bodyX,
    y: layout.bodyY,
    w: layout.bodyW,
    h: layout.bodyH,
    fontSize: BODY_FONT_SIZE,
    color: BODY_COLOR,
    bullet: { type: 'bullet' },
    lineSpacingMultiple: 1.2
  });
}

function resolveChartType(pptx, type) {
  if (pptx.ChartType && type && pptx.ChartType[type]) {
    return pptx.ChartType[type];
  }
  return pptx.ChartType ? pptx.ChartType.bar : 'bar';
}

function addChart(slide, chart, pptx, layout, theme) {
  if (!chart || !Array.isArray(chart.data) || chart.data.length === 0) {
    return;
  }
  const chartHasCjk =
    containsCjk(chart.caption) ||
    chart.data.some((series) => {
      if (containsCjk(series.name)) {
        return true;
      }
      if (Array.isArray(series.labels)) {
        return series.labels.some((label) =>
          Array.isArray(label)
            ? label.some((value) => containsCjk(value))
            : containsCjk(label)
        );
      }
      return false;
    });
  const chartFont = chartHasCjk ? FONT_CN : FONT_EN;
  const showLegendDefault = chart.data.length > 1;
  const baseOptions = {
    x: layout.bodyX,
    y: layout.bodyY,
    w: layout.bodyW,
    h: layout.bodyH,
    chartColors: chart.colors || [theme.accent, '6FBF73', 'A7D8AB'],
    showLegend: showLegendDefault,
    legendPos: 'r',
    catAxisLabelColor: BODY_COLOR,
    valAxisLabelColor: BODY_COLOR,
    catAxisLabelFontFace: chartFont,
    valAxisLabelFontFace: chartFont,
    legendFontFace: chartFont,
    dataLabelColor: BODY_COLOR,
    dataLabelFontFace: chartFont,
    dataLabelFontSize: 11
  };
  const options = Object.assign(baseOptions, chart.options || {});
  slide.addChart(resolveChartType(pptx, chart.type), chart.data, options);
  if (chart.caption) {
    const captionRuns = buildTextRuns(chart.caption);
    slide.addText(captionRuns.length ? captionRuns : chart.caption, {
      x: options.x,
      y: options.y + options.h + 0.08,
      w: options.w,
      h: 0.3,
      fontSize: 12,
      color: BODY_COLOR
    });
  }
}

async function build() {
  const pptx = new pptxgen();
  pptx.layout = 'LAYOUT_16x9';
  pptx.author = 'Wunder';
  pptx.title = 'wunder 视频介绍';

  const templateKey = resolveTemplate(TEMPLATE_NAME);
  const theme = TEMPLATES[templateKey];
  const layout = LAYOUTS[templateKey];

  SLIDES.forEach((item) => {
    const slide = pptx.addSlide();
    const hasBullets =
      Array.isArray(item.bullets) && item.bullets.filter(Boolean).length > 0;
    const hasChart = !!(
      item.chart &&
      Array.isArray(item.chart.data) &&
      item.chart.data.length > 0
    );
    addBackground(slide, templateKey, theme);
    addTitle(slide, item.title || '', layout);
    if (hasBullets && hasChart) {
      const gap = 0.15;
      const bulletHeight = Math.max(0.9, layout.bodyH * 0.4);
      const bulletLayout = Object.assign({}, layout, { bodyH: bulletHeight });
      const chartLayout = {
        bodyX: layout.bodyX,
        bodyY: layout.bodyY + bulletHeight + gap,
        bodyW: layout.bodyW,
        bodyH: layout.bodyH - bulletHeight - gap
      };
      addBullets(slide, item.bullets || [], bulletLayout);
      addChart(slide, item.chart, pptx, chartLayout, theme);
      return;
    }
    if (item.bodyText) {
      addBodyText(slide, item.bodyText, layout, item.bodyOptions || {});
      return;
    }
    addBullets(slide, item.bullets || [], layout);
    addChart(slide, item.chart, pptx, layout, theme);
  });

  await pptx.writeFile({ fileName: OUTPUT_FILE });
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});
