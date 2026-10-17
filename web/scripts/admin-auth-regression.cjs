// Browser regression for real authentication transitions, using synthetic API data.
// Run: node web/scripts/admin-auth-regression.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { chromium } = require("playwright");

const webRoot = path.resolve(__dirname, "..");
const source = fs.readFileSync(path.join(webRoot, "modules/admin-auth.js"), "utf8");
const sequence = JSON.parse(source.match(/const TREE_UNLOCK_SEQUENCE = (\[[^\]]+\])/)[1]);
const index = fs.readFileSync(path.join(webRoot, "index.html"), "utf8");
// Keep the actual lock markup, stylesheet and auth module; isolate unrelated panels.
const lockMarkup = index.slice(index.indexOf('<svg width="0" height="0"'), index.indexOf('    <script src="./third/echarts/'));
const fixture = `<!doctype html><meta charset="utf-8">
<link rel="stylesheet" href="/styles/airlock.css">
<style>body{margin:0;background:#e5eef5}main{min-height:100vh}</style>
<main><button id="adminLogoutBtn">Sign out</button></main>${lockMarkup}
<script type="module">
import { initAdminAuth, logoutAdmin } from "/modules/admin-auth.js";
const modal = document.getElementById("adminLoginModal");
const field = modal.querySelector(".airlock-hex-field");
const stage = modal.querySelector(".airlock-tree-stage");
window.fixture = { booted: false, ready: false, starts: [], opens: [], lastFade: 0, exitFrames: 0, treeLeaks: 0, opaqueFrames: 0 };
window.signOut = logoutAdmin;
let fading = false;
let open = false;
new MutationObserver(() => {
  const isFading = field.classList.contains("is-fading");
  const isOpen = modal.classList.contains("airlock--open");
  if (isFading && !fading) {
    window.fixture.lastFade = 0;
    window.fixture.starts.push({ tree: getComputedStyle(stage).display, backing: getComputedStyle(field).backgroundColor });
  }
  if (isOpen && !open) window.fixture.opens.push({ time: performance.now(), lastFade: window.fixture.lastFade });
  fading = isFading;
  open = isOpen;
}).observe(modal, { attributes: true, subtree: true, attributeFilter: ["class"] });
field.addEventListener("animationend", (event) => {
  if (event.animationName === "airlock-hex-fade-out") window.fixture.lastFade = performance.now();
}, true);
const observeFrame = () => {
  if (field.classList.contains("is-fading") && !modal.classList.contains("airlock--open")) {
    window.fixture.exitFrames++;
    if (getComputedStyle(stage).display !== "none") window.fixture.treeLeaks++;
    if (getComputedStyle(field).backgroundColor !== "rgba(0, 0, 0, 0)") window.fixture.opaqueFrames++;
  }
  requestAnimationFrame(observeFrame);
};
requestAnimationFrame(observeFrame);
initAdminAuth().then(() => { window.fixture.booted = true; });
window.fixture.ready = true;
</script>`;

const user = { user_id: "fixture_user", roles: ["admin"] };
let loginCount = 0;
let validationCount = 0;
let failNextLogin = false;
let browser;
const server = http.createServer((request, response) => {
  const url = new URL(request.url, "http://localhost");
  if (url.pathname === "/wunder/auth/login") {
    request.resume();
    loginCount++;
    response.setHeader("Content-Type", "application/json");
    if (failNextLogin) {
      failNextLogin = false;
      response.writeHead(401);
      return response.end(JSON.stringify({ error: "fixture rejected" }));
    }
    return response.end(JSON.stringify({ data: { access_token: "fixture-token", user } }));
  }
  if (url.pathname === "/wunder/auth/me") {
    validationCount++;
    response.setHeader("Content-Type", "application/json");
    return response.end(JSON.stringify({ data: user }));
  }
  if (url.pathname === "/") {
    response.setHeader("Content-Type", "text/html; charset=utf-8");
    return response.end(fixture);
  }
  const file = path.resolve(webRoot, "." + decodeURIComponent(url.pathname));
  if (!file.startsWith(webRoot + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
    response.writeHead(404);
    return response.end();
  }
  response.setHeader("Content-Type", file.endsWith(".css") ? "text/css" : "text/javascript");
  response.end(fs.readFileSync(file));
});

const unlock = async (page) => {
  for (const node of sequence) await page.locator(`.airlock-node[data-node="${node}"] .airlock-node-ring`).click();
};

const assertExit = async (page, expected) => {
  await page.waitForFunction((count) => window.fixture.opens.length === count, expected);
  const result = await page.evaluate(() => window.fixture);
  assert.equal(result.starts.length, expected);
  for (const start of result.starts) {
    assert.equal(start.tree, "none", "tree must be hidden when hex fading begins");
    assert.equal(start.backing, "rgba(0, 0, 0, 0)", "hex holes must reveal the console immediately");
  }
  assert.equal(result.treeLeaks, 0, "tree must never reappear during hex fading");
  assert.equal(result.opaqueFrames, 0, "no opaque backing may remain during hex fading");
  for (const event of result.opens) {
    assert.ok(event.lastFade > 0, "normal motion must complete on animation events, not the fallback");
    assert.ok(event.time - event.lastFade < 50, "overlay must close in the final hex animation frame");
  }
  assert.equal(result.booted, true, "auth boot must resolve while the wall exits");
};

const assertStaticLock = async (page) => {
  const state = await page.evaluate(() => {
    const stage = document.querySelector(".airlock-tree-stage");
    return {
      display: getComputedStyle(stage).display,
      highlighted: document.querySelectorAll(".airlock-node.lit, .airlock-node.wrong").length,
      stored: localStorage.getItem("wunder_admin_auth"),
      animations: stage.getAnimations({ subtree: true }).length,
      hidden: document.getElementById("adminLoginModal").getAttribute("aria-hidden"),
    };
  });
  assert.equal(state.display, "grid");
  assert.equal(state.highlighted, 0);
  assert.equal(state.stored, null);
  assert.equal(state.animations, 0, "the restored tree must have no growth, fade or node transitions");
  assert.equal(state.hidden, "false");
};

(async () => {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const errors = [];
  try {
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, reducedMotion: "no-preference" });
    page.on("pageerror", (error) => errors.push(error.message));
    let navigations = 0;
    page.on("framenavigated", (frame) => { if (frame === page.mainFrame()) navigations++; });
    await page.goto(origin);
    await page.waitForFunction(() => window.fixture?.ready);

    failNextLogin = true;
    await unlock(page);
    await page.waitForFunction(() => document.querySelector(".airlock-node.wrong"));
    assert.equal(await page.locator(".airlock-tree-stage").isVisible(), true, "rejected auth keeps the lock visible");
    await page.waitForFunction(() => !document.querySelector(".airlock-node.wrong, .airlock-node.lit"));
    await unlock(page);
    await assertExit(page, 1);
    assert.ok((await page.evaluate(() => window.fixture.exitFrames)) > 0, "sample real animated frames");

    // Drive the actual bound logout button and inspect before refill finishes.
    await page.locator("#adminLogoutBtn").click();
    await assertStaticLock(page);
    await page.waitForFunction(() => !document.querySelector(".airlock-hex-field.is-refilling"));
    // Give the former reload/fallback delay time to fire before logging in again.
    await page.waitForTimeout(1750);
    await assertStaticLock(page);
    assert.equal(navigations, 1, "logout must not reload the document");
    await unlock(page);
    await assertExit(page, 2);

    // A viewport changed while hidden must have no fully offscreen pool members.
    await page.setViewportSize({ width: 390, height: 320 });
    await page.evaluate(() => window.signOut());
    const offscreen = await page.locator(".airlock-hex-cell").evaluateAll((cells) => cells.filter((cell) => {
      const rect = cell.getBoundingClientRect();
      return rect.right <= 0 || rect.left >= innerWidth || rect.bottom <= 0 || rect.top >= innerHeight;
    }).length);
    assert.equal(offscreen, 0, "offscreen cells must not delay overlay completion");
    await assertStaticLock(page);
    await unlock(page);
    await assertExit(page, 3);

    // Restoring a valid token must use the same immediate tree removal.
    await page.reload();
    await assertExit(page, 1);
    assert.equal(validationCount, 1);
    assert.equal(navigations, 2);

    const reduced = await browser.newPage({ viewport: { width: 900, height: 600 }, reducedMotion: "reduce" });
    reduced.on("pageerror", (error) => errors.push(error.message));
    await reduced.goto(origin);
    await reduced.waitForFunction(() => window.fixture?.ready);
    await unlock(reduced);
    await reduced.waitForFunction(() => window.fixture.opens.length === 1);
    await reduced.evaluate(() => window.signOut());
    await assertStaticLock(reduced);
    await unlock(reduced);
    await reduced.waitForFunction(() => window.fixture.opens.length === 2);
    assert.equal(await reduced.locator(".airlock-tree-stage").isVisible(), false);
    assert.equal(loginCount, 6);
    assert.deepEqual(errors, []);
    console.log("PASS: rejected auth, login/logout/login, immediate tree removal, transparent wall, event completion, no reload, resize, token restore, reduced motion");
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
