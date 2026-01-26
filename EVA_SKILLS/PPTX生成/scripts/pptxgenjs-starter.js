const pptxgen = require('pptxgenjs');

const SLIDE_W = 10;
const SLIDE_H = 5.625;

const TITLE_COLOR = '111111'; // EDIT_TITLE_COLOR
const BODY_COLOR = '333333'; // EDIT_BODY_COLOR
const ACCENT_COLOR = '2F5597'; // EDIT_ACCENT_COLOR
const BG_COLOR = 'F7F9FC'; // EDIT_BG_COLOR
const CARD_COLOR = 'FFFFFF'; // EDIT_CARD_COLOR
const MUTED_COLOR = 'E6EEF7'; // EDIT_MUTED_COLOR

const TITLE_FONT = 'SimHei'; // EDIT_TITLE_FONT
const BODY_FONT = 'Arial'; // EDIT_BODY_FONT
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
    bullets: ['Bullet 1', 'Bullet 2', 'Bullet 3']
  }
];
// CONTENT_END

const RECT_SHAPE =
  pptxgen.ShapeType && pptxgen.ShapeType.rect ? pptxgen.ShapeType.rect : 'rect';

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
    titleX: 1.0,
    titleY: 1.25,
    titleW: 7.6,
    titleH: 0.6,
    bodyX: 1.0,
    bodyY: 2.0,
    bodyW: 7.6,
    bodyH: 2.6
  },
  lecture: {
    titleX: 1.6,
    titleY: 0.85,
    titleW: 7.4,
    titleH: 0.6,
    bodyX: 1.6,
    bodyY: 1.7,
    bodyW: 7.2,
    bodyH: 3.1
  },
  education: {
    titleX: 0.9,
    titleY: 1.25,
    titleW: 8.2,
    titleH: 0.6,
    bodyX: 0.9,
    bodyY: 2.0,
    bodyW: 8.2,
    bodyH: 2.7
  },
  defense: {
    titleX: 0.9,
    titleY: 0.75,
    titleW: 8.2,
    titleH: 0.6,
    bodyX: 0.9,
    bodyY: 1.6,
    bodyW: 8.2,
    bodyH: 3.0
  },
  simple: {
    titleX: 0.9,
    titleY: 0.9,
    titleW: 8.2,
    titleH: 0.6,
    bodyX: 0.9,
    bodyY: 1.8,
    bodyW: 8.2,
    bodyH: 3.0
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

function addBackground(slide, templateName, theme) {
  addRect(slide, 0, 0, SLIDE_W, SLIDE_H, theme.bg);
  if (templateName === 'report') {
    addRect(slide, 0, 0, SLIDE_W, 0.75, theme.accent);
    addRect(slide, 0.6, 1.05, 8.8, 4.0, theme.card, theme.muted);
    addRect(slide, 0.6, 1.05, 0.12, 4.0, theme.accent);
    return;
  }
  if (templateName === 'lecture') {
    addRect(slide, 0, 0, 1.2, SLIDE_H, theme.accent);
    addRect(slide, 1.2, 0, 8.8, 0.65, theme.muted);
    addRect(slide, 1.6, 1.1, 7.8, 3.9, theme.card, theme.muted);
    return;
  }
  if (templateName === 'education') {
    addRect(slide, 0, 0, 2.4, 0.9, theme.accent);
    addRect(slide, 2.4, 0, 7.6, 0.9, theme.muted);
    addRect(slide, 0.6, 1.2, 8.8, 3.9, theme.card, theme.muted);
    addRect(slide, 0, 5.15, SLIDE_W, 0.35, theme.muted);
    return;
  }
  if (templateName === 'defense') {
    addRect(slide, 0, 0.15, SLIDE_W, 0.06, theme.accent);
    addRect(slide, 9.35, 0, 0.65, 0.65, theme.accent);
    addRect(slide, 0.7, 1.1, 8.6, 3.6, theme.card, theme.muted);
    addRect(slide, 0, 5.1, SLIDE_W, 0.4, theme.muted);
    return;
  }
  addRect(slide, 0, 0, 0.15, SLIDE_H, theme.accent);
  addRect(slide, 0.7, 0.9, 8.6, 3.9, theme.card, theme.muted);
  addRect(slide, 0.7, 0.85, 8.6, 0.05, theme.muted);
}

function addTitle(slide, text, layout) {
  slide.addText(text, {
    x: layout.titleX,
    y: layout.titleY,
    w: layout.titleW,
    h: layout.titleH,
    fontFace: TITLE_FONT,
    fontSize: 30,
    bold: true,
    color: TITLE_COLOR
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
  slide.addText(lines, {
    x: layout.bodyX,
    y: layout.bodyY,
    w: layout.bodyW,
    h: layout.bodyH,
    fontFace: BODY_FONT,
    fontSize: 18,
    color: BODY_COLOR,
    bullet: { type: 'bullet' },
    lineSpacingMultiple: 1.2
  });
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
    addBackground(slide, templateKey, theme);
    addTitle(slide, item.title || '', layout);
    addBullets(slide, item.bullets || [], layout);
  });

  await pptx.writeFile({ fileName: OUTPUT_FILE });
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});
