const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');
const { build } = require('esbuild');
const { parse, compileScript } = require('@vue/compiler-sfc');
const { chromium } = require('@playwright/test');

const root = path.resolve(__dirname, '../../..');
const artifacts = path.join(root, 'temp_dir/trajectory-fold');
const fixture = (count = 3) => Array.from({ length: count }, (_, at) => ({
  user_turn_index: at + 1,
  items: [
    ['user_message', 'question'],
    ['assistant_message', 'working'],
    ['tool_call', 'read_file'],
    ['assistant_message', 'working again'],
    ['tool_call', 'write_file'],
    ['context', 'context note'],
    ['assistant_message', 'complete'],
  ].map(([kind, content], i) => ({
    item_id: `record-${at}-${i}`, item_index: i, kind, status: 'completed',
    created_time: 100 + at * 10 + i, updated_time: 100 + at * 10 + i + .5,
    payload: { content, tool: kind === 'tool_call' ? content : undefined, model_round: 1 },
  })),
}));

async function bundle() {
  const result = await build({
    stdin: {
      contents: `import {createApp} from 'vue';
import Hive from './frontend/src/views/ThreadTrajectoryView.vue';
import {createTrajectoryView} from './web/modules/trajectory-view.js';
window.mountTrajectory = (kind) => {
 if (kind === 'hive') createApp(Hive).mount('#host');
 else { const view = createTrajectoryView(document.querySelector('#host')); window.trajectoryView=view; view.setRawTurns(window.fixture); }
};`,
      resolveDir: root, loader: 'js',
    },
    write: false, bundle: true, format: 'iife', define: { 'process.env.NODE_ENV': '"test"', __VUE_OPTIONS_API__: 'true', __VUE_PROD_DEVTOOLS__: 'false' },
    plugins: [{
      name: 'trajectory-harness', setup(build) {
        build.onResolve({filter:/^[A-Za-z]:[\\/]/,namespace:'stub'}, args=>({path:args.path,namespace:'file'}));
        build.onResolve({filter:/^@\/utils\/chatPerf$/}, ()=>({path:path.join(root,'frontend/src/utils/chatPerf.ts')}));
        build.onResolve({ filter: /^(vue-router|element-plus|@\/i18n|@\/api\/chat|@\/utils\/)/ }, args => ({ path: args.path, namespace: 'stub' }));
        build.onLoad({ filter: /.*/, namespace: 'stub' }, args => ({ contents:
          args.path === 'vue-router' ? `export const useRoute = () => ({query:{session:'fixture'}}); export const useRouter = () => ({back(){}});` :
          args.path === '@/i18n' ? `import messages from '${path.join(root, 'frontend/src/i18n/messages/zh-CN.ts').replaceAll('\\', '/')}'; export const useI18n=()=>({t:(k,p={})=>String(messages[k]||k).replace(/\\{(\\w+)\\}/g,(_,n)=>p[n]??'')});` :
          args.path === '@/api/chat' ? `const page=()=>{ window.fixtureRequests=(window.fixtureRequests||0)+1; return Promise.resolve({data:{data:{page:window.fixturePage}}}); }; export const getThreadViewBootstrap=page, getThreadViewTrajectoryPage=page; export const searchThreadView=()=>Promise.resolve({data:{data:{hits:[],has_more:false}}}); export const getThreadViewBody=()=>Promise.resolve({data:{data:{chunks:[]}}});` :
          args.path === 'element-plus' ? `export const ElMessage = {success(){},error(){}};` :
          args.path.includes('markdown') ? `export const renderMarkdown=(s)=>String(s||'');` : `export const exportSingleSessionLog=async()=>{};`, loader: 'js' }));
        build.onResolve({ filter: /\.js\?v=/ }, args => ({ path: path.resolve(args.resolveDir, args.path.split('?')[0]) }));
        build.onLoad({ filter: /\.vue$/ }, args => {
          const { descriptor } = parse(fs.readFileSync(args.path, 'utf8'));
          return { contents: compileScript(descriptor, { id: args.path, inlineTemplate: true }).content, loader: 'ts', resolveDir: path.dirname(args.path) };
        });
      },
    }],
  });
  return result.outputFiles[0].text;
}

for (const kind of ['hive', 'bridge']) for (const count of [3, 70]) {
  test(`${kind} ${count}-turn trajectory turn/call controls, summaries and keyboard expand`, async () => {
    fs.mkdirSync(artifacts, { recursive: true });
    const code = await bundle();
    const browser = await chromium.launch({ headless: true });
    try {
      const page = await browser.newPage({ viewport: {width:1280, height:800} });
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await page.goto('about:blank');
      await page.setContent('<style>html,body,#host{margin:0;width:100%;height:100%;}*{box-sizing:border-box}</style><div id="host"></div>');
      await page.addStyleTag({ content: fs.readFileSync(path.join(root, kind === 'hive' ? 'frontend/src/styles/pages/thread-trajectory.css' : 'web/styles/trajectory.css'), 'utf8') });
      await page.evaluate(turns => {
        window.fixture = turns;
        window.fixturePage = {epoch:1, projection_state:'ready', has_more:false, has_newer:false, tiles:[], trajectory:turns.flatMap((turn, ti) => turn.items.map((item, i) => ({
          item_id:item.item_id, item_index:i, kind:item.kind, status:item.status, user_round:turn.user_turn_index,
          root_turn_id:`root-${ti}`, model_round:1, summary:item.payload.content, tool_name:item.payload.tool,
          created_seq:ti*10+i, started_at:item.created_time, completed_at:item.updated_time,
        })))};
      }, fixture(count));
      await page.addScriptTag({ content: code });
      await page.evaluate(k => window.mountTrajectory(k), kind);
      const turnButton = page.locator('.tt-toolbar .tt-toggle').filter({ hasText:'轮次' });
      const callButton = page.locator('.tt-toolbar .tt-toggle').filter({ hasText:'调用' });
      const summaries = page.locator('.tt-row-summary');
      const resetScroll = () => page.locator('.tt-ledger').evaluate(el => {el.scrollTop=0;el.dispatchEvent(new Event('scroll'));});
      const waitCount = async (locator, count) => {
        await page.waitForFunction(({selector, count}) => document.querySelectorAll(selector).length === count, {selector:locator, count});
      };
      await page.locator('.tt-turn-control').first().waitFor();
      if (count > 3) {
        await page.locator('.tt-ledger').evaluate(el => { el.scrollTop = 5000; el.dispatchEvent(new Event('scroll')); });
        await page.waitForTimeout(50);
        const anchor = await page.locator('.tt-ledger').evaluate(el => {
          const top = el.getBoundingClientRect().top;
          return [...el.querySelectorAll('.tt-turn-control')].find(node => node.getBoundingClientRect().top >= top)?.textContent.match(/\d+/)?.[0];
        });
        await turnButton.click();
        await page.waitForTimeout(50);
        assert.ok(await page.locator('.tt-row').count() > 0, 'fold at a deep scroll position must not blank the virtual list');
        assert.ok(await page.locator('.tt-row').count() < 100, 'rendered rows remain bounded');
        await page.locator(`.tt-turn-control`).filter({hasText:`#${anchor}`}).first().waitFor();
        await page.screenshot({path:path.join(artifacts, `${kind}-large-fold.png`)});
        await summaries.first().click();
        await page.waitForTimeout(50);
        assert.ok(await page.locator('.tt-row-message:not(.tt-row-summary)').count() > 0);
        assert.deepEqual(errors, []);
        return;
      }
      await resetScroll();
      await turnButton.click();
      await waitCount('.tt-row-summary', 3);
      assert.equal(await page.locator('.tt-row-user:not(.tt-row-summary)').count(), 3);
      assert.equal(await turnButton.getAttribute('aria-pressed'), 'true');
      if (kind === 'bridge') {
        await page.evaluate(() => window.trajectoryView.setRawTurns(window.fixture));
        await page.waitForTimeout(50);
        await waitCount('.tt-row-summary', 3);
        assert.equal(await turnButton.getAttribute('aria-pressed'), 'true');
      }
      const width = await summaries.first().locator('.tt-collapsed-summary').evaluate(el => el.getBoundingClientRect().width);
      assert.ok(width > 200, `summary should span content column, got ${width}`);
      await page.screenshot({path:path.join(artifacts, `${kind}-turns.png`)});
      await summaries.first().click();
      await waitCount('.tt-row-summary', 2);
      assert.equal(await turnButton.getAttribute('aria-pressed'), 'false');
      // A partial expansion makes the next toolbar click fold every group again.
      await turnButton.click();
      await waitCount('.tt-row-summary', 3);
      await summaries.first().focus();
      await summaries.first().press('Enter');
      await waitCount('.tt-row-summary', 2);
      await turnButton.click();
      await turnButton.click();
      await waitCount('.tt-row-summary', 0);
      await resetScroll();
      await page.locator('.tt-turn-control').first().click();
      await waitCount('.tt-row-summary', 1);
      await summaries.first().press('Space');
      await waitCount('.tt-row-summary', 0);
      await callButton.click();
      await resetScroll();
      await waitCount('.tt-row-summary', 6);
      assert.equal(await callButton.getAttribute('aria-pressed'), 'true');
      assert.equal(await page.locator('.tt-row-context').count(), 3);
      await summaries.first().click();
      await waitCount('.tt-row-summary', 5);
      assert.equal(await callButton.getAttribute('aria-pressed'), 'false');
      await callButton.click();
      await callButton.click();
      await waitCount('.tt-row-summary', 0);
      await resetScroll();
      await page.locator('.tt-row-message .tt-content-cell').first().dblclick({delay:120});
      await waitCount('.tt-row-summary', 1);
      await summaries.first().click();
      await waitCount('.tt-row-summary', 0);
      await resetScroll();
      await page.locator('.tt-row-user .tt-content-cell').first().dblclick({delay:120});
      await waitCount('.tt-row-summary', 1);
      await summaries.first().click();
      await waitCount('.tt-row-summary', 0);
      // Search must reveal matching content even when its parent turn was folded.
      await turnButton.click();
      await page.locator('.tt-search-input').fill('context note');
      await waitCount('.tt-row-summary', 0);
      await waitCount('.tt-row-context', 3);
      assert.deepEqual(errors, []);
      if (kind === 'hive') assert.equal(await page.evaluate(() => window.fixtureRequests), 1, 'folds must not reload the turn');
    } finally { await browser.close(); }
  });
}
