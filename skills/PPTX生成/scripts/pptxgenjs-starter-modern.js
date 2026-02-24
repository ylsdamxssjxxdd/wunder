const pptxgen = require("pptxgenjs");

const pptx = new pptxgen();
pptx.layout = "LAYOUT_16x9";
pptx.author = "pptxgenjs-cn";
pptx.title = "Modern 模板";

const OUTPUT_FILE = "output.pptx";

const THEME = {
  blue: "1E4CD9",
  blueDark: "1E3A8A",
  light: "F5F8FF",
  line: "E5EAFE",
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

function addHeader(slide, company, date, color) {
  slide.addText(company, {
    x: 0.6,
    y: 0.3,
    w: 4.5,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color,
    bold: true,
    margin: 0,
  });
  slide.addText(date, {
    x: 6.0,
    y: 0.3,
    w: 3.4,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 12,
    color,
    bold: true,
    align: "right",
    margin: 0,
  });
}

function addUnderline(slide, x, y, w) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h: 0.08,
    fill: { color: THEME.blue },
    line: { color: THEME.blue },
  });
}

function addContact(slide, x, y, label, value) {
  slide.addText(`${label}: ${value}`, {
    x,
    y,
    w: 3.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.blue,
    margin: 0,
  });
}

function addCard(slide, x, y, w, h, title, body) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.white, transparency: 10 },
    line: { color: THEME.white, width: 1 },
    shadow: makeShadow(),
  });
  slide.addText(title, {
    x: x + 0.3,
    y: y + 0.15,
    w: w - 0.6,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });
  slide.addText(body, {
    x: x + 0.3,
    y: y + 0.5,
    w: w - 0.6,
    h: h - 0.6,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.blueDark,
    margin: 0,
  });
}

function addStatCard(slide, x, y, w, h, label, value) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y,
    w,
    h,
    fill: { color: THEME.light },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: x + 0.2,
    y: y + 0.15,
    w: 1.0,
    h: 0.3,
    fill: { color: THEME.blue },
    line: { color: THEME.blue },
  });
  slide.addText(label, {
    x: x + 0.2,
    y: y + 0.15,
    w: 1.0,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 10,
    color: THEME.white,
    align: "center",
    valign: "middle",
    margin: 0,
  });
  slide.addText(value, {
    x: x + 0.2,
    y: y + 0.55,
    w: w - 0.4,
    h: 0.3,
    fontFace: FONTS.heading,
    fontSize: 14,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });
}

function addMarketBar(slide, x, baseY, w, h, label, value) {
  slide.addShape(pptx.shapes.RECTANGLE, {
    x,
    y: baseY - h,
    w,
    h,
    fill: { color: THEME.blue },
    line: { color: THEME.blue },
  });
  slide.addText(value, {
    x,
    y: baseY - h - 0.3,
    w,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.blue,
    align: "center",
    margin: 0,
  });
  slide.addText(label, {
    x,
    y: baseY + 0.05,
    w,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.blueDark,
    align: "center",
    margin: 0,
  });
}

// ====== 可改内容 ======
const DATA = {
  company: "presenton",
  date: "2026-02-04",
  title: "Pitch Deck",
  subtitle: "业务蓝图与增长路径",
  contacts: {
    tel: "+86-000-0000",
    addr: "Shanghai, China",
    web: "www.example.com",
  },
  problemTitle: "Problem",
  problemBody: "阐述当前业务痛点与市场机会，强调问题的紧迫性与规模。",
  problemCards: [
    { title: "低效率", body: "流程割裂导致协作成本上升。" },
    { title: "高成本", body: "传统方式投入高、回报慢。" },
    { title: "增长受限", body: "缺乏可复制的增长路径。" },
  ],
  solutionTitle: "Solution",
  solutionBody: "我们提供一体化平台，覆盖从需求到交付的全链路。",
  solutionPoints: [
    "统一数据与流程",
    "自动化执行",
    "持续优化与度量",
  ],
  marketTitle: "Market Size",
  marketBars: [
    { label: "TAM", value: "$12B", height: 2.2 },
    { label: "SAM", value: "$3.6B", height: 1.5 },
    { label: "SOM", value: "$0.8B", height: 1.0 },
  ],
  tractionTitle: "Company Traction",
  tractionBody: "展示关键增长指标与阶段性成果，突出业务动能。",
  stats: [
    { label: "ARR", value: "+48%" },
    { label: "Users", value: "120K" },
    { label: "NRR", value: "118%" },
  ],
  closingTitle: "Thank You",
  closingSubtitle: "期待与你共建增长",
};

// Slide 1: Intro (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.white };

  addHeader(slide, DATA.company, DATA.date, THEME.blue);

  slide.addText(DATA.title, {
    x: 0.6,
    y: 2.0,
    w: 8.5,
    h: 0.8,
    fontFace: FONTS.heading,
    fontSize: 54,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });
  addUnderline(slide, 0.6, 2.9, 2.4);

  slide.addText(DATA.subtitle, {
    x: 0.6,
    y: 3.2,
    w: 8.0,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 14,
    color: THEME.blueDark,
    margin: 0,
  });

  addContact(slide, 0.6, 4.9, "Tel", DATA.contacts.tel);
  addContact(slide, 3.4, 4.9, "Addr", DATA.contacts.addr);
  addContact(slide, 6.2, 4.9, "Web", DATA.contacts.web);
}

// Slide 2: Problem (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.blue };

  addHeader(slide, DATA.company, DATA.date, THEME.white);

  slide.addText(DATA.problemTitle, {
    x: 0.8,
    y: 1.2,
    w: 4.2,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 36,
    color: THEME.white,
    bold: true,
    margin: 0,
  });

  slide.addText(DATA.problemBody, {
    x: 0.8,
    y: 2.0,
    w: 4.2,
    h: 2.2,
    fontFace: FONTS.body,
    fontSize: 14,
    color: THEME.white,
    margin: 0,
  });

  const cardX = 5.3;
  const cardW = 4.0;
  const cardH = 0.9;
  const startY = 1.4;
  const gap = 0.3;

  DATA.problemCards.slice(0, 3).forEach((card, i) => {
    addCard(slide, cardX, startY + i * (cardH + gap), cardW, cardH, card.title, card.body);
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0,
    y: 5.45,
    w: 10,
    h: 0.12,
    fill: { color: THEME.white },
    line: { color: THEME.white },
  });
}

// Slide 3: Solution (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.white };

  addHeader(slide, DATA.company, DATA.date, THEME.blue);

  slide.addText(DATA.solutionTitle, {
    x: 0.8,
    y: 1.0,
    w: 4.2,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });

  slide.addText(DATA.solutionBody, {
    x: 0.8,
    y: 1.8,
    w: 4.2,
    h: 1.2,
    fontFace: FONTS.body,
    fontSize: 13,
    color: THEME.blueDark,
    margin: 0,
  });

  const startX = 5.3;
  const startY = 1.6;
  const itemW = 4.0;
  const itemH = 0.8;
  const gap = 0.3;

  DATA.solutionPoints.slice(0, 3).forEach((text, i) => {
    addCard(slide, startX, startY + i * (itemH + gap), itemW, itemH, `Point ${i + 1}`, text);
  });
}

// Slide 4: Market Size (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.white };

  addHeader(slide, DATA.company, DATA.date, THEME.blue);

  slide.addText(DATA.marketTitle, {
    x: 0.8,
    y: 0.8,
    w: 8.0,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });

  const baseY = 4.5;
  const barW = 1.2;
  const gap = 0.8;
  const startX = 1.6;

  DATA.marketBars.slice(0, 3).forEach((bar, i) => {
    addMarketBar(slide, startX + i * (barW + gap), baseY, barW, bar.height, bar.label, bar.value);
  });

  slide.addText("规模数据可替换为 TAM/SAM/SOM 或市场分层", {
    x: 0.8,
    y: 4.9,
    w: 8.5,
    h: 0.3,
    fontFace: FONTS.body,
    fontSize: 11,
    color: THEME.blueDark,
    margin: 0,
  });
}

// Slide 5: Traction (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.white };

  addHeader(slide, DATA.company, DATA.date, THEME.blue);

  slide.addText(DATA.tractionTitle, {
    x: 0.8,
    y: 0.8,
    w: 4.8,
    h: 0.6,
    fontFace: FONTS.heading,
    fontSize: 32,
    color: THEME.blue,
    bold: true,
    margin: 0,
  });

  // Chart placeholder
  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0.8,
    y: 1.6,
    w: 4.4,
    h: 2.8,
    fill: { color: THEME.light },
    line: { color: THEME.line, width: 1 },
    shadow: makeShadow(),
  });
  slide.addText("Chart", {
    x: 0.8,
    y: 2.8,
    w: 4.4,
    h: 0.4,
    fontFace: FONTS.body,
    fontSize: 12,
    color: THEME.blueDark,
    align: "center",
    valign: "middle",
    margin: 0,
  });

  slide.addText(DATA.tractionBody, {
    x: 5.4,
    y: 1.6,
    w: 3.8,
    h: 1.0,
    fontFace: FONTS.body,
    fontSize: 13,
    color: THEME.blueDark,
    margin: 0,
  });

  const statX = 5.4;
  const statW = 3.8;
  const statH = 0.7;
  const statY = 2.8;
  const statGap = 0.2;

  DATA.stats.slice(0, 3).forEach((stat, i) => {
    addStatCard(slide, statX, statY + i * (statH + statGap), statW, statH, stat.label, stat.value);
  });

  slide.addShape(pptx.shapes.RECTANGLE, {
    x: 0,
    y: 5.45,
    w: 10,
    h: 0.12,
    fill: { color: THEME.blue },
    line: { color: THEME.blue },
  });
}

// Slide 6: Closing (modern)
{
  const slide = pptx.addSlide();
  slide.background = { color: THEME.blue };

  slide.addText(DATA.closingTitle, {
    x: 0.8,
    y: 2.0,
    w: 8.0,
    h: 0.8,
    fontFace: FONTS.heading,
    fontSize: 46,
    color: THEME.white,
    bold: true,
    margin: 0,
  });
  slide.addText(DATA.closingSubtitle, {
    x: 0.8,
    y: 2.9,
    w: 8.0,
    h: 0.5,
    fontFace: FONTS.body,
    fontSize: 16,
    color: THEME.white,
    margin: 0,
  });

  addContact(slide, 0.8, 4.6, "Tel", DATA.contacts.tel);
  addContact(slide, 3.6, 4.6, "Addr", DATA.contacts.addr);
  addContact(slide, 6.4, 4.6, "Web", DATA.contacts.web);
}

pptx.writeFile({ fileName: OUTPUT_FILE });