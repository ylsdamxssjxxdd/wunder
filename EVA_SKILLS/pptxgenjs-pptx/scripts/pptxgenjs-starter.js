const path = require('path');
const pptxgen = require('pptxgenjs');

const COLORS = {
  bg: 'FFF6EC',
  teal: '4FA3A5',
  coral: 'F59D3D',
  text: '2E2E2E',
  subtext: '6B6B6B',
  white: 'FFFFFF'
};

const FONT = 'Arial';
const SLIDE_W = 10;
const SLIDE_H = 5.625;
const HEADER_H = 0.7;

let SHAPES;

function addBackground(slide) {
  slide.addShape(SHAPES.rect, {
    x: 0,
    y: 0,
    w: SLIDE_W,
    h: SLIDE_H,
    fill: { color: COLORS.bg },
    line: { color: COLORS.bg }
  });
}

function addHeader(slide, title) {
  slide.addShape(SHAPES.rect, {
    x: 0,
    y: 0,
    w: SLIDE_W,
    h: HEADER_H,
    fill: { color: COLORS.teal },
    line: { color: COLORS.teal }
  });
  slide.addText(title, {
    x: 0.6,
    y: 0.15,
    w: SLIDE_W - 1.2,
    h: 0.4,
    fontFace: FONT,
    fontSize: 22,
    bold: true,
    color: COLORS.white,
    valign: 'middle'
  });
}

function addCard(slide, x, y, w, h, borderColor) {
  slide.addShape(SHAPES.rect, {
    x,
    y,
    w,
    h,
    fill: { color: COLORS.white },
    line: { color: borderColor, width: 1 }
  });
}

async function build() {
  const pptx = new pptxgen();
  SHAPES = pptx.ShapeType;
  pptx.layout = 'LAYOUT_16x9';
  pptx.author = 'Codex';
  pptx.title = '小学期末总结示例';

  const slide1 = pptx.addSlide();
  addBackground(slide1);
  slide1.addText('小学期末总结', {
    x: 0.8,
    y: 1.2,
    w: 8.4,
    h: 0.7,
    fontFace: FONT,
    fontSize: 34,
    bold: true,
    color: COLORS.text
  });
  slide1.addText('本学期成长记录', {
    x: 0.8,
    y: 1.95,
    w: 7.8,
    h: 0.4,
    fontFace: FONT,
    fontSize: 16,
    color: COLORS.teal
  });
  addCard(slide1, 0.8, 2.7, 4.2, 1.1, COLORS.coral);
  slide1.addText('班级：____\n学生：____\n日期：____', {
    x: 0.95,
    y: 2.82,
    w: 3.8,
    h: 0.9,
    fontFace: FONT,
    fontSize: 13,
    color: COLORS.subtext,
    lineSpacingMultiple: 1.2
  });

  const slide2 = pptx.addSlide();
  addBackground(slide2);
  addHeader(slide2, '目录 / 本次汇报内容');
  addCard(slide2, 0.6, 1.1, 4.2, 1.6, COLORS.coral);
  slide2.addText('学习表现', {
    x: 0.8,
    y: 1.35,
    w: 3.8,
    h: 0.4,
    fontFace: FONT,
    fontSize: 18,
    bold: true,
    color: COLORS.text
  });
  slide2.addText('学科进步与学习习惯', {
    x: 0.8,
    y: 1.85,
    w: 3.8,
    h: 0.4,
    fontFace: FONT,
    fontSize: 13,
    color: COLORS.subtext
  });
  addCard(slide2, 0.6, 2.9, 4.2, 1.6, COLORS.coral);
  slide2.addText('自我反思', {
    x: 0.8,
    y: 3.15,
    w: 3.8,
    h: 0.4,
    fontFace: FONT,
    fontSize: 18,
    bold: true,
    color: COLORS.text
  });
  slide2.addText('成长亮点与改进方向', {
    x: 0.8,
    y: 3.65,
    w: 3.8,
    h: 0.4,
    fontFace: FONT,
    fontSize: 13,
    color: COLORS.subtext
  });
  addCard(slide2, 5.0, 1.1, 4.4, 3.4, COLORS.teal);
  slide2.addText('提示', {
    x: 5.2,
    y: 1.35,
    w: 4.0,
    h: 0.4,
    fontFace: FONT,
    fontSize: 18,
    bold: true,
    color: COLORS.text
  });
  slide2.addText(
    '继续新增幻灯片时，保持边距一致，避免文字裁切。',
    {
      x: 5.2,
      y: 1.85,
      w: 4.0,
      h: 1.4,
      fontFace: FONT,
      fontSize: 12,
      color: COLORS.subtext,
      lineSpacingMultiple: 1.2
    }
  );

  const outPath = path.join(__dirname, 'output.pptx');
  await pptx.writeFile({ fileName: outPath });
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});