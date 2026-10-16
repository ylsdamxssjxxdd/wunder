// Isolated browser contract for the real governance module, with synthetic API data.
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

(async () => {
  const root = path.resolve(__dirname, '../..');
  const mutations = [];
  let failPolicy = false;
  const devices = Array.from({length:26}, (_,i) => ({device_id:`fixture_${i}`, user_id:'fixture_user', name:`Device ${i}`, client:'desktop',
    connected:i===0, status:i===0?'online':'offline', session_active:i===0, active_sessions:i===0?1:0,
    interlink_enabled:true, revoked:false, capabilities:['query.basic'], authorized_capabilities:['query.basic','workspace.write','tool.exec:fixture']}));
  const server = http.createServer((req,res) => {
    const url = new URL(req.url, 'http://localhost');
    if (url.pathname.startsWith('/wunder/')) {
      res.setHeader('Content-Type','application/json');
      if (req.method === 'GET') {
        const offset = Number(url.searchParams.get('offset')||0);
        return res.end(JSON.stringify({data:{devices:devices.slice(offset,offset+25),total:26}}));
      }
      let body=''; req.on('data',chunk=>body+=chunk); req.on('end',()=>{
        mutations.push({path:url.pathname,body:body?JSON.parse(body):null});
        if (failPolicy && url.pathname.endsWith('/policy')) {res.statusCode=403;res.end(JSON.stringify({error:{message:'fixture policy refused'}}));}
        else res.end(JSON.stringify({data:{ok:true}}));
      }); return;
    }
    if (url.pathname === '/') {
      res.setHeader('Content-Type','text/html');
      return res.end('<meta charset="utf-8"><link rel="stylesheet" href="/web/styles/interlink-fleet.css"><button id="interlinkFleetOpen"></button><script type="module">import {initInterlinkFleet} from "/web/modules/interlink-fleet.js"; initInterlinkFleet();</script>');
    }
    const file = path.resolve(root, '.'+decodeURIComponent(url.pathname));
    if (!file.startsWith(root+path.sep) || !fs.existsSync(file)) {res.statusCode=404;return res.end();}
    res.setHeader('Content-Type', file.endsWith('.css')?'text/css':'text/javascript');
    res.end(fs.readFileSync(file));
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  let browser;
  try {
    browser = await chromium.launch({headless:true});
    const page = await browser.newPage({viewport:{width:1400,height:1000}});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    page.on('dialog',dialog=>dialog.accept());
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.locator('#interlinkFleetOpen').click();
    await page.waitForFunction(()=>document.querySelectorAll('tbody tr').length===25);
    assert.match(await page.locator('tbody tr').first().innerText(), /已登录|Signed in/);
    const write = page.locator('tbody tr').first().locator('label').filter({hasText:'workspace.write'}).locator('input');
    assert.equal(await write.isChecked(),true,'authorized grant remains checked despite absent negotiated capability');
    await page.getByRole('button',{name:/保存权限$|Save grants$/}).first().click();
    await page.waitForFunction(()=>!document.querySelector('.interlink-fleet-toolbar button').disabled);
    assert.ok(mutations[0].body.capabilities.includes('tool.exec:fixture'),'unrepresented grants preserved');
    failPolicy = true;
    await page.getByRole('button',{name:/保存权限$|Save grants$/}).first().click();
    await page.getByRole('status').filter({hasText:'fixture policy refused'}).waitFor();
    await page.getByRole('button',{name:/下一页|Next/}).click();
    await page.waitForFunction(()=>document.querySelectorAll('tbody tr').length===1);
    assert.match(await page.locator('tbody').innerText(),/fixture_25/);
    const output = path.join(root,'target/interlink-review/fleet.png');
    fs.mkdirSync(path.dirname(output),{recursive:true});
    await page.screenshot({path:output});
    assert.deepEqual(errors,[]);
    console.log('PASS: fleet pagination, login evidence, grant preservation, error feedback; '+output);
  } finally {if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
