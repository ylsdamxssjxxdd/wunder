// Real isolated server: validates model form serialization and persistence of all speed presets.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({headless:true});
try {
  const page = await browser.newPage({viewport:{width:1440,height:1100}});
  const errors=[]; page.on("pageerror",error=>errors.push(error.message));
  await page.goto("http://127.0.0.1:18000");
  await page.locator("#adminLoginUsername").fill(process.env.WUNDER_TEST_ADMIN_USER || "admin");
  await page.locator("#adminLoginPassword").fill(process.env.WUNDER_TEST_ADMIN_PASSWORD || "admin");
  await page.locator("#adminLoginBtn").click();
  await page.locator("#adminLoginModal").waitFor({state:"hidden"});
  if (!(await page.locator("#navLlm").isVisible())) await page.locator('[data-group="agent"] .nav-group-btn').click();
  await page.locator("#navLlm").click();
  const selectModel = async name => {
    await page.locator("#llmConfigList .list-item").filter({has:page.locator(".llm-list-item-name",{hasText:new RegExp(`^${name}$`)})}).click();
  };
  await selectModel("virtual");
  assert.equal(await page.locator("#llmSimulationSpeed").inputValue(),"fast");
  assert.equal(await page.locator("#llmSimulationSpeed option").count(),3);
  for (const speed of ["medium","slow","fast"]) {
    await page.locator("#llmSimulationSpeed").selectOption(speed);
    const saved=page.waitForResponse(response=>response.url().endsWith("/wunder/admin/llm") && response.request().method()==="POST");
    await page.locator("#saveLlmBtn").click();
    const response=await saved;
    assert.equal(response.status(),200);
    assert.equal((await response.json()).llm.models.virtual.simulation_speed,speed);
    await selectModel("api");
    assert.equal(await page.locator("#llmSimulationRow").isVisible(),false);
    await selectModel("virtual");
    assert.equal(await page.locator("#llmSimulationSpeed").inputValue(),speed);
  }
  await page.locator("#llmSimulationSpeed").scrollIntoViewIfNeeded();
  await page.screenshot({path:"temp_dir/throughput-docker/model-speed.png",fullPage:true});
  assert.deepEqual(errors,[]);
  console.log("PASS: simulation speed form defaults, save payload, model switching and API round trip");
} finally {await browser.close();}
