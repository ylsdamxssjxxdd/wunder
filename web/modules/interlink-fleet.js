import { getWunderBase } from "./api.js";
import { getCurrentLanguage } from "./i18n.js?v=20261016-01";

const label = (zh, en) => getCurrentLanguage().startsWith("zh") ? zh : en;
const CAPABILITIES = ["shadow:minimal", "shadow:full", "query.basic", "thread.drive", "workspace.read.binary", "workspace.write"];
const el = (tag, text, className) => {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
};

async function request(path, options = {}) {
  const response = await fetch(`${getWunderBase()}${path}`, options);
  const body = await response.json();
  if (!response.ok) throw new Error(body?.error?.message || body?.message || body?.detail || `HTTP ${response.status}`);
  return body.data;
}

export function initInterlinkFleet() {
  const trigger = document.getElementById("interlinkFleetOpen");
  if (!trigger) return;
  trigger.textContent = label("设备互通", "Devices");
  const dialog = el("dialog", undefined, "interlink-fleet");
  dialog.setAttribute("aria-label", label("设备互通治理", "Device governance"));
  const head = el("div", undefined, "interlink-fleet-toolbar");
  head.append(el("strong", label("设备互通治理", "Device governance")));
  const filter = el("input");
  filter.placeholder = label("按用户 ID 筛选", "Filter by user ID");
  filter.setAttribute("aria-label", filter.placeholder);
  const refresh = el("button", label("刷新", "Refresh"), "secondary");
  const close = el("button", label("关闭", "Close"), "secondary");
  head.append(filter, refresh, close);
  const note = el("p", label("登录状态与互通通道分别显示。修改策略会断开设备通道，使新策略在重连后生效。", "Login and tunnel status are separate. Policy changes reconnect the device to apply the new grants."), "muted");
  const status = el("p"); status.setAttribute("role", "status");
  const table = el("table", undefined, "monitor-table");
  const header = el("tr");
  [label("用户 / 设备", "User / device"), label("登录 / 通道", "Login / tunnel"), label("共享与权限", "Sharing / permissions"), label("操作", "Actions")].forEach(text => header.append(el("th", text)));
  const body = el("tbody"); table.append(el("thead"), body); table.firstChild.append(header);
  const wrap = el("div", undefined, "interlink-fleet-table"); wrap.append(table);
  const footer = el("div", undefined, "interlink-fleet-toolbar");
  const prev = el("button", label("上一页", "Previous"), "secondary");
  const next = el("button", label("下一页", "Next"), "secondary");
  const page = el("span"); footer.append(prev, page, next);
  dialog.append(head, note, status, wrap, footer); document.body.append(dialog);
  let offset = 0, busy = false, generation = 0;
  const limit = 25;
  const action = (title, handler) => { const button = el("button", title, "secondary"); button.type = "button"; button.onclick = handler; return button; };
  async function mutate(device, title, path, method, payload) {
    if (busy || !window.confirm(`${title} · ${device.name || device.device_id}?`)) return;
    busy = true; status.textContent = label("正在提交…", "Applying…");
    try {
      await request(path, {method, ...(payload ? {headers: {"Content-Type": "application/json"}, body: JSON.stringify(payload)} : {})});
      status.textContent = label("操作成功", "Applied");
    } catch (error) { busy = false; await load(); status.textContent = error.message; return; }
    finally { busy = false; }
    await load();
  }
  async function load() {
    if (busy || !dialog.open) return;
    const ticket = ++generation;
    busy = true; refresh.disabled = true; prev.disabled = true; next.disabled = true;
    status.textContent = label("加载中…", "Loading…");
    try {
      const query = new URLSearchParams({offset: String(offset), limit: String(limit)});
      if (filter.value.trim()) query.set("user_id", filter.value.trim());
      const data = await request(`/admin/interlink/fleet?${query}`);
      if (ticket !== generation || !dialog.open) return;
      body.replaceChildren();
      for (const device of data.devices || []) {
        const row = el("tr");
        const identity = el("td"); identity.append(el("div", `${device.user_id} / ${device.name}`), el("small", `${device.client} · ${device.device_id}`, "muted"));
        const state = el("td", `${device.revoked ? label("已撤销", "Revoked") : device.session_active ? label("已登录", "Signed in") : label("已退出 / 已过期", "Signed out / expired")} / ${device.connected ? label("已连接", "Connected") : label("未连接", "Disconnected")}`);
        state.append(el("small", `${label("活动会话", "Active sessions")}: ${device.active_sessions || 0} · ${device.status}`, "muted"));
        const permissions = el("td");
        const caps = new Set(device.authorized_capabilities || device.capabilities || []);
        const inputs = CAPABILITIES.map(cap => {
          const field = el("label"); const input = el("input"); input.type = "checkbox"; input.checked = caps.has(cap); input.disabled = device.revoked;
          field.append(input, document.createTextNode(cap)); permissions.append(field); return [cap, input];
        });
        const controls = el("td");
        const path = `/admin/interlink/devices/${encodeURIComponent(device.device_id)}`;
        if (!device.revoked) {
          controls.append(action(label("保存权限", "Save grants"), () => mutate(device, label("保存权限并重连", "Save grants and reconnect"), `${path}/policy`, "PATCH", {capabilities: [...caps].filter(cap => !CAPABILITIES.includes(cap)).concat(inputs.filter(([, input]) => input.checked).map(([cap]) => cap))})));
          controls.append(action(device.interlink_enabled ? label("停用互通", "Disable tunnel") : label("启用互通", "Enable tunnel"), () => mutate(device, label("更改互通状态", "Change tunnel state"), `${path}/policy`, "PATCH", {interlink_enabled: !device.interlink_enabled})));
          controls.append(action(label("轮换设备密钥", "Rotate device secret"), () => mutate(device, label("轮换设备密钥", "Rotate device secret"), `${path}/rotate_secret`, "POST")));
          controls.append(action(label("撤销设备登录", "Revoke device login"), () => mutate(device, label("撤销后需重新注册设备", "Revoke device registration"), `/admin/cloud/devices/${encodeURIComponent(device.device_id)}`, "DELETE")));
        }
        row.append(identity, state, permissions, controls); body.append(row);
      }
      prev.disabled = offset === 0; next.disabled = offset + limit >= data.total;
      page.textContent = `${data.total ? offset + 1 : 0}–${Math.min(offset + limit, data.total)} / ${data.total}`;
      status.textContent = (data.devices || []).length ? "" : label("暂无设备", "No devices");
    } catch (error) { if (ticket === generation) status.textContent = error.message; }
    finally { if (ticket === generation) { busy = false; refresh.disabled = false; } }
  }
  trigger.onclick = () => { offset = 0; dialog.showModal(); void load(); };
  close.onclick = () => dialog.close();
  dialog.addEventListener("close", () => { generation++; busy = false; });
  refresh.onclick = () => { offset = 0; void load(); };
  filter.onkeydown = event => { if (event.key === "Enter") { offset = 0; void load(); } };
  prev.onclick = () => { if (!busy) { offset = Math.max(0, offset - limit); void load(); } };
  next.onclick = () => { if (!busy) { offset += limit; void load(); } };
}
