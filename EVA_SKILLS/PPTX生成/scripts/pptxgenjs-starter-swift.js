const pptxgen = require("pptxgenjs");

const pptx = new pptxgen();
pptx.layout = "LAYOUT_16x9";
pptx.author = "pptxgenjs-cn";
pptx.title = "Swift 模板";

const OUTPUT_FILE = "output.pptx";

const THEME = {
  heading: "111827",
  body: "6B7280",
  accent: "BFF4FF",
  background: "FFFFFF",
  line: "E5E7EB",
  white: "FFFFFF",
};

const FONTS = {
  heading: "Trebuchet MS",
  body: "Calibri",
};

const makeShadow = () => ({
  type: "outer",
  color: "000000",
  blur: 8,
  offset: 3,
  angle: 135,
  opacity: 0.12,
});

function addDiamond(slide, x, y, size, color) {
  slide.addShape(pptx.shapes.DIAMOND, {
    x,
    y,
    w: size,
    h: size,
    fill: { color },
    line: { color },
  });
}

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
    fill: { color: THEME.line },
    line: { color: THEME.line },
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

function addCard(slide, x, y, w, h, title, body, badge) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.white },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addShape(pptx.shapes.OVAL, {
    x: x + 0.2,
    y: y + 0.2,
    w: 0.5,
    h: 0.5,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(badge, {
    x: x + 0.2,
    y: y + 0.2,
    w: 0.5,
    h: 0.5,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.heading,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(title, {
    x: x + 0.85,
    y: y + 0.25,
    w: w - 1.1,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 16,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(body, {
    x: x + 0.85,
    y: y + 0.6,
    w: w - 1.1,
    h: h - 0.7,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    margin: 0,
  });
}

function addMetricCard(slide, x, y, w, h, value, line1, line2, desc) {
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
    w: 1.2,
    h: 0.4,
    fontFace: FONTS.heading,
    fontSize: 22,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(`${line1}\n${line2}`, {
    x: x + 1.6,
    y: y + 0.2,
    w: w - 2.0,
    h: 0.5,
    fontFace: FONTS.heading,
    fontSize: 12,
    color: THEME.heading,
    margin: 0,
  });
  slide.addText(desc, {
    x: x + 1.6,
    y: y + 0.75,
    w: w - 2.0,
    h: 0.5,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.body,
    margin: 0,
  });
}

function addTocItem(slide, x, y, num, title) {
  slide.addShape(pptx.shapes.OVAL, {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fill: { color: THEME.heading },
    line: { color: THEME.heading },
  });
  slide.addText(String(num), {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fontFace: FONTS.heading,
    fontSize: 12,
    color: THEME.white,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(title, {
    x: x + 0.65,
    y: y - 0.02,
    w: 6.5,
    h: 0.4,
    fontFace: FONTS.heading,
    fontSize: 16,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
}

function addTimelineItem(slide, x, y, label, title) {
  addDiamond(slide, x - 0.1, y - 0.1, 0.2, THEME.heading);
  slide.addText(label, {
    x: x - 0.5,
    y: y + 0.2,
    w: 1.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 10,
    color: THEME.body,
    align: "center",
    margin: 0,
  });
  slide.addText(title, {
    x: x - 0.9,
    y: y + 0.5,
    w: 1.8,
    h: 0.4,
    fontFace: FONTS.heading,
    fontSize: 12,
    color: THEME.heading,
    align: "center",
    margin: 0,
  });
}

// ====== 可改内容 ======
const DATA = {
  title: "Pitch Deck",
  paragraph: "简洁描述业务价值与目标受众，强调差异化与优势。",
  website: "www.example.com",
  introCard: { enabled: true, name: "John Doe", date: "2026-02-04" },
  rightImage: null,
  toc: ["背景", "方案", "市场", "路线图"],
  bullets: [
    { badge: "01", title: "清晰价值", body: "定位明确，价值可量化" },
    { badge: "02", title: "执行路径", body: "从策略到落地的路径" },
    { badge: "03", title: "增长节奏", body: "阶段性目标与里程碑" },
  ],
  timeline: [
    { label: "Q1", title: "试点" },
    { label: "Q2", title: "扩展" },
    { label: "Q3", title: "规模化" },
  ],
  metrics: [
    { value: "10K+", line1: "Total", line2: "Users", desc: "活跃用户规模" },
    { value: "150%", line1: "Revenue", line2: "Growth", desc: "同比增长" },
    { value: "95%", line1: "Customer", line2: "Satisfaction", desc: "满意度" },
  ],
};

// Slide 1: Intro (swift)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addImageOrPlaceholder(slide, {
    path: DATA.rightImage,
    x: 6.3,
    y: 0.2,
    w: 3.4,
    h: 4.8,
    label: "IMAGE",
  });

  addDiamond(slide, 5.8, 0.5, 0.18, THEME.heading);
  addDiamond(slide, 5.8, 0.85, 0.18, THEME.heading);
  addDiamond(slide, 5.8, 1.2, 0.18, THEME.heading);

  slide.addText(DATA.title, {
    x: 0.8,
    y: 0.8,
    w: 4.8,
    h: 0.8,
    fontFace: FONTS.heading,
    fontSize: 44,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(DATA.paragraph, {
    x: 0.8,
    y: 2.0,
    w: 4.8,
    h: 1.0,
    fontFace: FONTS.body,
    fontSize: 14,
    color: THEME.body,
    margin: 0,
  });

  if (DATA.introCard.enabled) {
    slide.addShape(pptx.shapes.RECTANGLE, {
      x: 0.8,
      y: 3.3,
      w: 3.6,
      h: 0.7,
      fill: { color: THEME.accent },
      line: { color: THEME.accent },
    });
    slide.addShape(pptx.shapes.RECTANGLE, {
      x: 1.0,
      y: 3.35,
      w: 0.1,
      h: 0.6,
      fill: { color: THEME.heading },
      line: { color: THEME.heading },
    });
    slide.addText(`${DATA.introCard.name}  ${DATA.introCard.date}`, {
      x: 1.3,
      y: 3.45,
      w: 3.0,
      h: 0.4,
      fontFace: FONTS.body,
      fontSize: 12,
      color: THEME.heading,
      margin: 0,
    });
  }

  slide.addText(DATA.website, {
    x: 0.8,
    y: 5.0,
    w: 3.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.body,
    margin: 0,
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 3.0,
    y: 5.1,
    w: 6.2,
    h: 0.05,
    fill: { color: THEME.line },
    line: { color: THEME.line },
  });
  addDiamond(slide, 9.2, 5.0, 0.25, THEME.heading);
}

// Slide 2: Table of Contents (swift)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  slide.addText("Contents", {
    x: 0.8,
    y: 0.8,
    w: 8.5,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });

  const startX = 0.9;
  const startY = 1.8;
  const gap = 0.9;

  DATA.toc.slice(0, 4).forEach((item, i) => {
    addTocItem(slide, startX, startY + i * gap, i + 1, item);
  });

  addDiamond(slide, 9.1, 0.9, 0.2, THEME.heading);
}

// Slide 3: Cards on accent band (swift)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  slide.addText("核心能力", {
    x: 0.8,
    y: 0.6,
    w: 8.5,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0,
    y: 2.8,
    w: 10,
    h: 2.8,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });

  const cardW = 2.7;
  const cardH = 1.6;
  const gap = 0.4;
  const startX = 0.8;
  const y = 2.2;

  DATA.bullets.slice(0, 3).forEach((b, i) => {
    addCard(slide, startX + i * (cardW + gap), y, cardW, cardH, b.title, b.body, b.badge);
  });
}

// Slide 4: Timeline (swift)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  slide.addText("Timeline", {
    x: 0.8,
    y: 0.8,
    w: 6.0,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 30,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 1.0,
    y: 2.8,
    w: 8.0,
    h: 0.05,
    fill: { color: THEME.heading },
    line: { color: THEME.heading },
  });

  const startX = 2.0;
  const gap = 3.0;
  const y = 2.7;

  DATA.timeline.slice(0, 3).forEach((t, i) => {
    addTimelineItem(slide, startX + i * gap, y, t.label, t.title);
  });
}

// Slide 5: Metrics (swift)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 5.0,
    y: 0.6,
    w: 0.02,
    h: 4.7,
    fill: { color: THEME.line },
    line: { color: THEME.line },
  });

  slide.addText("Our Impact in Numbers", {
    x: 0.8,
    y: 0.8,
    w: 4.0,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 26,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addShape(pptx.shapes.OVAL, {
    x: 0.8,
    y: 1.7,
    w: 0.25,
    h: 0.25,
    fill: { color: THEME.heading },
    line: { color: THEME.heading },
  });
  slide.addText("Proven Results\nThrough Data", {
    x: 1.2,
    y: 1.6,
    w: 3.5,
    h: 0.7,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.heading,
    margin: 0,
  });
  slide.addText("数据驱动的成果展示，突出关键指标。", {
    x: 0.8,
    y: 2.6,
    w: 3.8,
    h: 0.8,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    margin: 0,
  });

  const cardX = 5.4;
  const cardW = 4.0;
  const cardH = 1.1;
  const gap = 0.25;
  const startY = 1.0;

  DATA.metrics.slice(0, 3).forEach((m, i) => {
    addMetricCard(slide, cardX, startY + i * (cardH + gap), cardW, cardH, m.value, m.line1, m.line2, m.desc);
  });

  slide.addText(DATA.website, {
    x: 0.8,
    y: 5.0,
    w: 3.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.body,
    margin: 0,
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 3.0,
    y: 5.1,
    w: 6.2,
    h: 0.05,
    fill: { color: THEME.heading },
    line: { color: THEME.heading },
  });
  addDiamond(slide, 9.2, 5.0, 0.25, THEME.heading);
}

pptx.writeFile({ fileName: OUTPUT_FILE });