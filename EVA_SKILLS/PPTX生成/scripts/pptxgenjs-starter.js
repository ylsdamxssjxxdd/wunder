const pptxgen = require('pptxgenjs');

const SLIDE_W = 10;
const SLIDE_H = 5.625;
const MARGIN_X = 0.6;

const TITLE_COLOR = '111111'; // EDIT_TITLE_COLOR
const BODY_COLOR = '333333'; // EDIT_BODY_COLOR
const ACCENT_COLOR = '2F5597'; // EDIT_ACCENT_COLOR

const TITLE_FONT = 'SimHei'; // EDIT_TITLE_FONT
const BODY_FONT = 'Arial'; // EDIT_BODY_FONT
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

function addTitle(slide, text) {
  slide.addText(text, {
    x: MARGIN_X,
    y: 0.6,
    w: SLIDE_W - MARGIN_X * 2,
    h: 0.7,
    fontFace: TITLE_FONT,
    fontSize: 30,
    bold: true,
    color: TITLE_COLOR
  });
}

function addBullets(slide, bullets) {
  if (!Array.isArray(bullets) || bullets.length === 0) {
    return;
  }
  const lines = bullets.filter(Boolean).join('\n');
  if (!lines) {
    return;
  }
  slide.addText(lines, {
    x: MARGIN_X,
    y: 1.6,
    w: SLIDE_W - MARGIN_X * 2,
    h: SLIDE_H - 2.2,
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

  SLIDES.forEach((item) => {
    const slide = pptx.addSlide();
    addTitle(slide, item.title || '');
    addBullets(slide, item.bullets || []);
  });

  await pptx.writeFile({ fileName: OUTPUT_FILE });
}

build().catch((err) => {
  console.error(err);
  process.exit(1);
});
