// Run after throughput-docker-acceptance.mjs to inspect saved measurements without another model run.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";

export async function verifyBrowser(results) {
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage({viewport:{width:1440,height:1100}});
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto("http://127.0.0.1:18000");
    await page.locator("#adminLoginUsername").fill(process.env.WUNDER_TEST_ADMIN_USER || "admin");
    await page.locator("#adminLoginPassword").fill(process.env.WUNDER_TEST_ADMIN_PASSWORD || "admin");
    await page.locator("#adminLoginBtn").click();
    await page.locator("#adminLoginModal").waitFor({state:"hidden"});
    const debugGroup = page.locator(".nav-group").filter({has:page.locator("#navThroughput")});
    if (!(await page.locator("#navThroughput").isVisible())) await debugGroup.locator(".nav-group-btn").click();
    await page.locator("#navThroughput").click();
    await page.waitForFunction(()=>document.querySelectorAll("#tpHistory tr").length>=9);
    await page.waitForFunction(()=>document.getElementById("tpConnection").textContent.includes("实时"));
    assert.ok(await page.locator('#tpModel option[value="virtual"]').count());
    await page.locator(`#tpHistory [data-view="${results[1].id}"]`).click();
    assert.match(await page.locator("#tpDetail").innerText(),/2000/);
    const detailsBox = await page.locator("#tpDetail").boundingBox();
    assert.ok(detailsBox.y >= 0 && detailsBox.y < 400, "Selected history details must be in view");
    const series = await page.evaluate(()=>window.echarts.getInstanceByDom(document.getElementById("tpChart")).getOption().series);
    assert.ok(series.some(item=>item.data.length>=2));
    await page.locator("#tpDetail").evaluate(element=>element.closest("section").scrollIntoView({block:"start"}));
    await page.screenshot({path:"temp_dir/throughput-docker/page.png",fullPage:true});
    await page.locator("#tpHistory").scrollIntoViewIfNeeded();
    await page.locator(".tp-table-wrap").evaluate(element=>{element.scrollLeft=element.scrollWidth;});
    await page.screenshot({path:"temp_dir/throughput-docker/history.png",fullPage:true});
    // Navigation must close the feed and reconnect on returning without duplicate controllers.
    await page.locator("#navMonitor").click();
    await page.locator("#navThroughput").click();
    await page.waitForFunction(()=>document.getElementById("tpConnection").textContent.includes("实时"));
    assert.deepEqual(errors,[]);
  } finally { await browser.close(); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const {results} = JSON.parse(await readFile("temp_dir/throughput-docker/results.json","utf8"));
  await verifyBrowser(results);
  console.log("PASS: real administrator login, navigation, WebSocket, history details and curves");
}
