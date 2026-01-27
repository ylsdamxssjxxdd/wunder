const pptxgen = require('pptxgenjs');

const SLIDE_W = 10;
const SLIDE_H = 5.625;
const HEADER_H = 0.35; // EDIT_HEADER_HEIGHT

const TITLE_COLOR = 'FFFFFF'; // EDIT_TITLE_COLOR
const BODY_COLOR = '333333'; // EDIT_BODY_COLOR
const ACCENT_COLOR = '2F5597'; // EDIT_ACCENT_COLOR
const BG_COLOR = 'F7F9FC'; // EDIT_BG_COLOR
const CARD_COLOR = 'FFFFFF'; // EDIT_CARD_COLOR
const MUTED_COLOR = 'E6EEF7'; // EDIT_MUTED_COLOR

const FONT_CN = 'SimHei'; // EDIT_FONT_CN
const FONT_EN = 'Times New Roman'; // EDIT_FONT_EN
const TITLE_FONT_SIZE = 24; // EDIT_TITLE_SIZE
const BODY_FONT_SIZE = 18; // EDIT_BODY_SIZE
const TEMPLATE_NAME = 'report'; // EDIT_TEMPLATE_NAME: report/lecture/education/defense/simple
const OUTPUT_FILE = 'output.pptx'; // EDIT_OUTPUT_FILE

// CONTENT_START
const SLIDES = [
  {
    title: 'Slide Title 1',
    bullets: ['Bullet 1', 'Bullet 2', 'Bullet 3']
  },
  {
    title: 'Slide Title 2',
    bullets: ['Bullet 1', 'Bullet 2', 'Bullet 3'],
    chart: {
      type: 'bar',
      data: [
        {
          name: 'Series A',
          labels: ['Q1', 'Q2', 'Q3', 'Q4'],
          values: [12, 19, 8, 15]
        },
        {
          name: 'Series B',
          labels: ['Q1', 'Q2', 'Q3', 'Q4'],
          values: [10, 14, 11, 18]
        }
      ],
      options: {
        showLegend: true,
        legendPos: 'r',
        dataLabelPosition: 'outEnd'
      },
      caption: 'Sample chart with two series'
    }
  }
];
// CONTENT_END

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
  pptx.title = 'Generated PPTX';

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
    addBullets(slide, item.bullets || [], layout);
    addChart(slide, item.chart, pptx, layout, theme);
  });

  await pptx.writeFile({ fileName: OUTPUT_FILE });
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});
