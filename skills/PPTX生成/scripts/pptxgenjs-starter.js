const pptxgen = require("pptxgenjs");

const pptx = new pptxgen();
pptx.layout = "LAYOUT_16x9";
pptx.author = "pptxgenjs-cn";
pptx.title = "General 模板";

const OUTPUT_FILE = "output.pptx";

const THEME = {
  heading: "111827",
  body: "4B5563",
  accent: "9333EA",
  background: "FFFFFF",
  soft: "F5F8FE",
  line: "E5E7EB",
  muted: "9CA3AF",
  dark: "0F172A",
  white: "FFFFFF",
};

const FONTS = {
  heading: "Trebuchet MS",
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

function addTitle(slide, text, y = 0.6) {
  slide.addText(text, {
    x: 0.6,
    y,
    w: 8.8,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0.6,
    y: y + 0.65,
    w: 1.2,
    h: 0.08,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
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
    fill: { color: THEME.soft },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addText(label || "图片", {
    x,
    y: y + h / 2 - 0.2,
    w,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.muted,
    align: "center",
    valign: "middle",
    margin: 0,
  });
}

function addAgendaItem(slide, x, y, w, h, num, title, desc) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.white },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: x + 0.2,
    y: y + 0.2,
    w: 0.5,
    h: 0.5,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(String(num), {
    x: x + 0.2,
    y: y + 0.2,
    w: 0.5,
    h: 0.5,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.white,
    bold: true,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(title, {
    x: x + 0.85,
    y: y + 0.15,
    w: w - 1.1,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(desc, {
    x: x + 0.85,
    y: y + 0.5,
    w: w - 1.1,
    h: h - 0.7,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.body,
    margin: 0,
  });
}

function addFeatureItem(slide, x, y, title, body, badge) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(badge, {
    x,
    y,
    w: 0.45,
    h: 0.45,
    fontFace: FONTS.heading,
    fontSize: 12,
    color: THEME.white,
    bold: true,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(title, {
    x: x + 0.6,
    y: y - 0.02,
    w: 3.6,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 15,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(body, {
    x: x + 0.6,
    y: y + 0.3,
    w: 3.6,
    h: 0.5,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    margin: 0,
  });
}

function addMetricCard(slide, x, y, w, h, value, label) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.soft },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addText(value, {
    x: x + 0.3,
    y: y + 0.2,
    w: w - 0.6,
    h: 0.5,
    fontFace: FONTS.heading,
    fontSize: 26,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addText(label, {
    x: x + 0.3,
    y: y + 0.85,
    w: w - 0.6,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.body,
    margin: 0,
  });
}

// ====== 可改内容（优先修改 DATA） ======
const DATA = {
  title: "智能体（AI Agent）简介",
  subtitle: "从感知到行动的自治系统",
  tagline: "核心概念 · 能力构成 · 应用实践",
  presenter: "你的名字",
  date: "2026-02-04",
  introImage: null,
  agenda: [
    { num: 1, title: "背景", desc: "问题与机会" },
    { num: 2, title: "方案", desc: "核心能力与架构" },
    { num: 3, title: "场景", desc: "落地与价值" },
    { num: 4, title: "路线", desc: "里程碑与下一步" },
  ],
  featureImage: null,
  features: [
    { title: "可感知环境", body: "多模态输入解析，理解上下文", badge: "A" },
    { title: "推理与规划", body: "目标拆解与路径选择", badge: "B" },
    { title: "工具调用", body: "API/系统协作执行动作", badge: "C" },
  ],
  metrics: [
    { value: "30%", label: "效率提升" },
    { value: "2x", label: "吞吐提升" },
    { value: "95%", label: "满意度目标" },
  ],
  quote: "技术只是手段，价值才是目的。",
  quoteAuthor: "— 行业共识",
  closing: "谢谢",
};

// Slide 1: Intro
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addImageOrPlaceholder(slide, {
    path: DATA.introImage,
    x: 0.6,
    y: 1.2,
    w: 4.2,
    h: 3.0,
    label: "封面图",
  });

  slide.addText(DATA.title, {
    x: 5.1,
    y: 1.2,
    w: 4.4,
    h: 0.8,
    fontFace: FONTS.heading,
    fontSize: 34,
    color: THEME.heading,
    bold: true,
    margin: 0,
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 5.1,
    y: 2.05,
    w: 1.2,
    h: 0.08,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(DATA.subtitle, {
    x: 5.1,
    y: 2.3,
    w: 4.4,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 16,
    color: THEME.body,
    margin: 0,
  });
  slide.addText(DATA.tagline, {
    x: 5.1,
    y: 2.8,
    w: 4.4,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.muted,
    margin: 0,
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 5.1,
    y: 3.5,
    w: 4.0,
    h: 0.7,
    fill: { color: THEME.soft },
    line: { color: THEME.line, width: 1 },
  });
  slide.addText(`${DATA.presenter} · ${DATA.date}`, {
    x: 5.3,
    y: 3.7,
    w: 3.6,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.heading,
    margin: 0,
  });
}

// Slide 2: Agenda
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, "目录", 0.6);

  const cardW = 4.2;
  const cardH = 1.2;
  const gapX = 0.4;
  const gapY = 0.4;
  const startX = 0.6;
  const startY = 1.6;

  DATA.agenda.slice(0, 4).forEach((item, i) => {
    const col = i % 2;
    const row = Math.floor(i / 2);
    addAgendaItem(
      slide,
      startX + col * (cardW + gapX),
      startY + row * (cardH + gapY),
      cardW,
      cardH,
      item.num,
      item.title,
      item.desc
    );
  });

  slide.addText("建议按此结构组织全篇内容", {
    x: 0.6,
    y: 4.8,
    w: 8.8,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.muted,
    margin: 0,
  });
}

// Slide 3: Features
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, "关键特征", 0.6);

  addImageOrPlaceholder(slide, {
    path: DATA.featureImage,
    x: 0.6,
    y: 1.6,
    w: 4.0,
    h: 3.0,
    label: "场景图",
  });

  const startX = 5.2;
  const startY = 1.8;
  const gap = 1.0;

  DATA.features.slice(0, 3).forEach((f, i) => {
    addFeatureItem(slide, startX, startY + i * gap, f.title, f.body, f.badge);
  });
}

// Slide 4: Metrics
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.background };

  addTitle(slide, "关键指标", 0.6);

  const cardW = 2.8;
  const cardH = 1.5;
  const gap = 0.4;
  const startX = 0.6;
  const y = 2.0;

  DATA.metrics.slice(0, 3).forEach((m, i) => {
    addMetricCard(slide, startX + i * (cardW + gap), y, cardW, cardH, m.value, m.label);
  });

  slide.addText("指标可替换为业务数据或阶段目标", {
    x: 0.6,
    y: 4.2,
    w: 8.8,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.muted,
    margin: 0,
  });
}

// Slide 5: Quote / Closing
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.dark };

  slide.addText(DATA.quote, {
    x: 0.8,
    y: 1.6,
    w: 8.4,
    h: 1.2,
    fontFace: FONTS.heading,
    fontSize: 30,
    color: THEME.white,
    bold: true,
    margin: 0,
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0.8,
    y: 2.8,
    w: 1.2,
    h: 0.08,
    fill: { color: THEME.accent },
    line: { color: THEME.accent },
  });
  slide.addText(DATA.quoteAuthor, {
    x: 0.8,
    y: 3.1,
    w: 4.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.muted,
    margin: 0,
  });

  slide.addText(DATA.closing, {
    x: 0.8,
    y: 4.6,
    w: 3.0,
    h: 0.4,
    fontFace: FONTS.heading,
    fontSize: 20,
    color: THEME.white,
    margin: 0,
  });
}

pptx.writeFile({ fileName: OUTPUT_FILE });