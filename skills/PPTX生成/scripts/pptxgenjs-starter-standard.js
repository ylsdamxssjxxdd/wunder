const pptxgen = require("pptxgenjs");

const pptx = new pptxgen();
pptx.layout = "LAYOUT_16x9";
pptx.author = "pptxgenjs-cn";
pptx.title = "Standard 模板";

const OUTPUT_FILE = "output.pptx";

const THEME = {
  heading: "111827",
  body: "6B7280",
  accent: "1B8C2D",
  background: "FFFFFF",
  panel: "E5E7EB",
  line: "D1D5DB",
  white: "FFFFFF",
};

const FONTS = {
  heading: "Georgia",
  body: "Calibri",
};

const makeShadow = () => ({
  type: "outer",
  color: "000000",
  blur: 6,
  offset: 2,
  angle: 135,
  opacity: 0.12,
});

function addImageOrPlaceholder(slide, { path, x, y, w, h, label }) {
  if (path) {
    slide.addImage({ path, x, y, w, h });
    return;
  }
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.panel },
    line: { color: THEME.line, width: 1 },
  });
  slide.addText(label || "IMAGE", {
    x,
    y: y + h / 2 - 0.2,
    w,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    align: "center",
    valign: "middle",
    margin: 0,
  });
}

function addTitle(slide, text, x, y, w) {
  slide.addText(text, {
    x,
    y,
    w,
    h: 1.0,
    fontFace: FONTS.heading,
    fontSize: 40,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
}

function addMetricCard(slide, x, y, w, h, value, label) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
    shadow: makeShadow(),
  });
  slide.addText(value, {
    x: x + 0.3,
    y: y + 0.2,
    w: w - 0.6,
    h: 0.5,
    fontFace: FONTS.heading,
    fontSize: 28,
    color: THEME.white,
    bold: true,
    margin: 0,
  });
  slide.addText(label, {
    x: x + 0.3,
    y: y + 0.8,
    w: w - 0.6,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.white,
    margin: 0,
  });
}

function addOutlineItem(slide, x, y, num, title, desc) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(String(num), {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.white,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(title, {
    x: x + 0.6,
    y: y - 0.02,
    w: 6.0,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 16,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(desc, {
    x: x + 0.6,
    y: y + 0.3,
    w: 6.0,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    margin: 0,
  });
}

// ====== 可改内容 ======
const DATA = {
  title: "Introduction\nPitch Deck",
  paragraph: "概述当前业务、愿景与市场背景。建议保持 2-3 句，重点突出价值。",
  introCard: {
    enabled: true,
    initials: "PDT",
    name: "Pitch Deck Team",
    date: "2026-02-04",
  },
  leftImage: null,
  outline: [
    { num: 1, title: "背景", desc: "问题与机会" },
    { num: 2, title: "方案", desc: "核心能力" },
    { num: 3, title: "市场", desc: "规模与定位" },
    { num: 4, title: "里程", desc: "路线图" },
  ],
  sectionTitle: "关键要点",
  bullets: [
    "目标清晰：聚焦核心问题",
    "方案可行：路径与资源明确",
    "增长可衡量：指标与节奏清晰",
  ],
  rightImage: null,
  metrics: [
    { value: "87%", label: "客户满意度" },
    { value: "2.5M", label: "月活用户" },
    { value: "99%", label: "系统稳定性" },
    { value: "142+", label: "合作伙伴" },
  ],
  closingTitle: "Thank You",
  closingSubtitle: "期待与你共建增长",
  contact: {
    email: "hello@example.com",
    phone: "+86-000-0000",
    website: "www.example.com",
  },
};

// Slide 1: Intro (standard)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addImageOrPlaceholder(slide, {
    path: DATA.leftImage,
    x: 0,
    y: 0,
    w: 4.2,
    h: 5.625,
    label: "LEFT IMAGE",
  });

  addTitle(slide, DATA.title, 4.6, 1.0, 5.0);

  slide.addText(DATA.paragraph, {
    x: 4.6,
    y: 2.6,
    w: 4.8,
    h: 1.2,
    fontFace: FONTS.body,
    fontSize: 14,
    color: THEME.body,
    margin: 0,
  });

  if (DATA.introCard.enabled) {
    slide.addShape(pptx.shapes.RECTANGLE, {
      x: 4.6,
      y: 4.0,
      w: 4.3,
      h: 0.9,
      fill: { color: THEME.white },
      line: { color: THEME.line, width: 1 },
      shadow: makeShadow(),
    });
    slide.addShape(pptx.shapes.OVAL, {
      x: 4.8,
      y: 4.15,
      w: 0.6,
      h: 0.6,
      fill: { color: THEME.accent },
      line: { color: THEME.accent },
    });
    slide.addText(DATA.introCard.initials, {
      x: 4.8,
      y: 4.15,
      w: 0.6,
      h: 0.6,
      fontFace: FONTS.heading,
      fontSize: 14,
      color: THEME.white,
      bold: true,
      align: "center",
      valign: "middle",
      margin: 0,
    });
    slide.addText(DATA.introCard.name, {
      x: 5.6,
      y: 4.2,
      w: 3.0,
      h: 0.3,
      fontFace: FONTS.heading,
      fontSize: 14,
      color: THEME.heading,
      bold: true,
      margin: 0,
    });
    slide.addText(DATA.introCard.date, {
      x: 5.6,
      y: 4.5,
      w: 3.0,
      h: 0.3,
      fontFace: FONTS.body,
      fontSize: 12,
      color: THEME.accent,
      margin: 0,
    });
  }
}

// Slide 2: Outline
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, "目录", 0.8, 0.6, 8.5);

  const startX = 0.8;
  const startY = 1.8;
  const gap = 0.9;

  DATA.outline.slice(0, 4).forEach((item, i) => {
    addOutlineItem(slide, startX, startY + i * gap, item.num, item.title, item.desc);
  });
}

// Slide 3: Bullet + Image (standard)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, DATA.sectionTitle, 0.8, 0.6, 8.5);

  slide.addText(
    DATA.bullets.map((b, i) => ({
      text: b,
      options: { bullet: true, breakLine: i < DATA.bullets.length - 1 },
    })),
    {
      x: 0.9,
      y: 1.8,
      w: 4.2,
      h: 2.8,
      fontFace: FONTS.body,
      fontSize: 16,
      color: THEME.body,
      paraSpaceAfter: 6,
    }
  );

  addImageOrPlaceholder(slide, {
    path: DATA.rightImage,
    x: 5.4,
    y: 1.6,
    w: 3.8,
    h: 3.0,
    label: "RIGHT IMAGE",
  });
}

// Slide 4: Metrics (standard)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, "核心指标", 0.8, 0.6, 8.5);

  const cardW = 4.2;
  const cardH = 1.4;
  const gapX = 0.4;
  const gapY = 0.4;
  const startX = 0.6;
  const startY = 1.8;

  DATA.metrics.slice(0, 4).forEach((m, i) => {
    const col = i % 2;
    const row = Math.floor(i / 2);
    addMetricCard(
      slide,
      startX + col * (cardW + gapX),
      startY + row * (cardH + gapY),
      cardW,
      cardH,
      m.value,
      m.label
    );
  });
}

// Slide 5: Closing (standard)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  slide.addText(DATA.closingTitle, {
    x: 0.8,
    y: 1.2,
    w: 8.5,
    h: 0.8,
    fontFace: FONTS.heading,
    fontSize: 40,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(DATA.closingSubtitle, {
    x: 0.8,
    y: 2.0,
    w: 8.5,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 14,
    color: THEME.body,
    margin: 0,
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0.8,
    y: 3.0,
    w: 8.4,
    h: 0.8,
    fill: { color: THEME.panel },
    line: { color: THEME.line, width: 1 },
  });
  slide.addText(`Email: ${DATA.contact.email}`, {
    x: 1.0,
    y: 3.1,
    w: 3.5,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.heading,
    margin: 0,
  });
  slide.addText(`Phone: ${DATA.contact.phone}`, {
    x: 1.0,
    y: 3.4,
    w: 3.5,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.heading,
    margin: 0,
  });
  slide.addText(`Web: ${DATA.contact.website}`, {
    x: 1.0,
    y: 3.7,
    w: 3.5,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.heading,
    margin: 0,
  });
}

pptx.writeFile({ fileName: OUTPUT_FILE });