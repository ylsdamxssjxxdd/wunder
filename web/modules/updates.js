// AI生成
import { elements } from "./elements.js?v=20261101-01";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20261016-01";
import { formatBytes, formatTimestamp } from "./utils.js";

const CLIENT_OPTIONS = ["desktop", "cli", "supplement"];
const OS_OPTIONS = ["windows", "linux"];
const ARCH_OPTIONS = ["x86", "x64", "arm64"];

// 服务端清单是唯一事实：每次改动都整单发布，前端不持有待发布的本地草稿。
const catalog = { releases: [], files: [] };

const updatesElements = () => ({
  panel: elements.updatesPanel,
  dirText: elements.updatesDirText,
  filesBody: elements.updatesFilesBody,
  releasesBody: elements.updatesReleasesBody,
  fileInput: elements.updatesFileInput,
  uploadBtn: elements.updatesUploadBtn,
  versionInput: elements.updatesVersionInput,
  assetRows: elements.updatesAssetRows,
});

const requestFailed = async (response) => {
  const status = response.status;
  let detail = "";
  try {
    const body = await response.json();
    detail = body?.error?.message || body?.message || "";
  } catch {
    detail = "";
  }
  return new Error(t("updates.toast.requestFailed", { status }) + (detail ? ` ${detail}` : ""));
};

const loadUpdates = async (options = {}) => {
  const refs = updatesElements();
  if (!refs.panel) {
    return;
  }
  if (!options.silent && refs.filesBody) {
    refs.filesBody.textContent = t("common.loading");
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/updates`);
  if (!response.ok) {
    throw await requestFailed(response);
  }
  const payload = await response.json();
  catalog.releases = Array.isArray(payload?.releases) ? payload.releases : [];
  catalog.files = Array.isArray(payload?.files) ? payload.files : [];
  if (refs.dirText) {
    refs.dirText.textContent = payload?.releases_dir || "-";
  }
  renderFiles();
  renderReleases();
};

const renderFiles = () => {
  const { filesBody } = updatesElements();
  if (!filesBody) {
    return;
  }
  filesBody.textContent = "";
  if (!catalog.files.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 4;
    cell.className = "updates-empty";
    cell.textContent = t("updates.empty.files");
    row.appendChild(cell);
    filesBody.appendChild(row);
    return;
  }
  for (const file of catalog.files) {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    nameCell.textContent = file.file;
    const sizeCell = document.createElement("td");
    sizeCell.textContent = formatBytes(file.size);
    const stateCell = document.createElement("td");
    stateCell.textContent = file.referenced
      ? t("updates.state.referenced")
      : t("updates.state.pending");
    const actionCell = document.createElement("td");
    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "secondary btn-with-icon btn-compact";
    deleteBtn.title = t("updates.action.deleteFile");
    deleteBtn.textContent = t("updates.action.deleteFile");
    deleteBtn.disabled = Boolean(file.referenced);
    deleteBtn.addEventListener("click", () => deletePackageFile(file.file));
    actionCell.appendChild(deleteBtn);
    row.append(nameCell, sizeCell, stateCell, actionCell);
    filesBody.appendChild(row);
  }
};

const renderReleases = () => {
  const { releasesBody } = updatesElements();
  if (!releasesBody) {
    return;
  }
  releasesBody.textContent = "";
  if (!catalog.releases.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 4;
    cell.className = "updates-empty";
    cell.textContent = t("updates.empty.releases");
    row.appendChild(cell);
    releasesBody.appendChild(row);
    return;
  }
  for (const entry of [...catalog.releases].sort((left, right) =>
    String(right.version).localeCompare(String(left.version), undefined, { numeric: true })
  )) {
    const row = document.createElement("tr");
    const versionCell = document.createElement("td");
    versionCell.textContent = entry.version;
    const timeCell = document.createElement("td");
    timeCell.textContent = formatTimestamp((entry.published_at || 0) * 1000);
    const assetsCell = document.createElement("td");
    assetsCell.className = "updates-assets-cell";
    for (const asset of entry.assets || []) {
      const line = document.createElement("div");
      line.className = "updates-asset-line";
      line.textContent = `${asset.client}/${asset.os}/${asset.arch} → ${asset.file}`;
      line.title = asset.notes || "";
      assetsCell.appendChild(line);
    }
    const actionCell = document.createElement("td");
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "secondary btn-with-icon btn-compact";
    removeBtn.title = t("updates.action.removeRelease");
    removeBtn.textContent = t("updates.action.removeRelease");
    removeBtn.addEventListener("click", () => removeRelease(entry.version));
    actionCell.appendChild(removeBtn);
    row.append(versionCell, timeCell, assetsCell, actionCell);
    releasesBody.appendChild(row);
  }
};

const uploadPackage = async () => {
  const { fileInput, uploadBtn } = updatesElements();
  const file = fileInput?.files?.[0];
  if (!file) {
    notify(t("updates.toast.fileRequired"), "warn");
    return;
  }
  if (uploadBtn) {
    uploadBtn.disabled = true;
  }
  try {
    const form = new FormData();
    form.append("name", file.name);
    form.append("package", file, file.name);
    const response = await fetch(`${getWunderBase()}/admin/updates/packages`, {
      method: "POST",
      body: form,
    });
    if (!response.ok) {
      throw await requestFailed(response);
    }
    notify(t("updates.toast.uploadSuccess", { file: file.name }), "success");
    if (fileInput) {
      fileInput.value = "";
    }
    await loadUpdates({ silent: true });
  } catch (error) {
    notify(t("updates.toast.uploadFailed", { message: error.message }), "error");
  } finally {
    if (uploadBtn) {
      uploadBtn.disabled = false;
    }
  }
};

const deletePackageFile = async (name) => {
  try {
    const response = await fetch(
      `${getWunderBase()}/admin/updates/packages?file=${encodeURIComponent(name)}`,
      { method: "DELETE" }
    );
    if (!response.ok) {
      throw await requestFailed(response);
    }
    notify(t("updates.toast.deleteSuccess", { file: name }), "success");
    await loadUpdates({ silent: true });
  } catch (error) {
    notify(t("updates.toast.deleteFailed", { message: error.message }), "error");
  }
};

const addAssetRow = () => {
  const { assetRows } = updatesElements();
  if (!assetRows) {
    return;
  }
  const row = document.createElement("div");
  row.className = "updates-asset-row";

  const createSelect = (options) => {
    const select = document.createElement("select");
    for (const option of options) {
      const item = document.createElement("option");
      item.value = option;
      item.textContent = option;
      select.appendChild(item);
    }
    return select;
  };

  const clientSelect = createSelect(CLIENT_OPTIONS);
  clientSelect.title = t("updates.label.client");
  const osSelect = createSelect(OS_OPTIONS);
  osSelect.title = t("updates.label.os");
  const archSelect = createSelect(ARCH_OPTIONS);
  archSelect.title = t("updates.label.arch");

  const fileSelect = createSelect(catalog.files.map((file) => file.file));
  fileSelect.title = t("updates.label.file");

  const notesInput = document.createElement("input");
  notesInput.type = "text";
  notesInput.placeholder = t("updates.placeholder.notes");
  notesInput.title = t("updates.placeholder.notes");

  const mandatoryLabel = document.createElement("label");
  mandatoryLabel.className = "updates-mandatory";
  const mandatoryInput = document.createElement("input");
  mandatoryInput.type = "checkbox";
  mandatoryLabel.append(mandatoryInput, document.createTextNode(t("updates.option.mandatory")));

  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "secondary btn-compact";
  removeBtn.textContent = "×";
  removeBtn.title = t("updates.action.removeAsset");
  removeBtn.addEventListener("click", () => row.remove());

  row.append(clientSelect, osSelect, archSelect, fileSelect, notesInput, mandatoryLabel, removeBtn);
  assetRows.appendChild(row);
};

const collectAssetRows = () => {
  const { assetRows } = updatesElements();
  const rows = [];
  assetRows?.querySelectorAll(".updates-asset-row").forEach((row) => {
    const [clientSelect, osSelect, archSelect, fileSelect, notesInput] = row.querySelectorAll(
      "select, input[type=text]"
    );
    const mandatory = row.querySelector("input[type=checkbox]");
    rows.push({
      client: clientSelect?.value || "",
      os: osSelect?.value || "",
      arch: archSelect?.value || "",
      file: fileSelect?.value || "",
      notes: notesInput?.value?.trim() || "",
      mandatory: Boolean(mandatory?.checked),
    });
  });
  return rows;
};

const publishManifest = async () => {
  const response = await fetch(`${getWunderBase()}/admin/updates/manifest`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ releases: catalog.releases }),
  });
  if (!response.ok) {
    throw await requestFailed(response);
  }
  const payload = await response.json();
  catalog.releases = Array.isArray(payload?.releases) ? payload.releases : [];
};

const appendRelease = async () => {
  const { versionInput } = updatesElements();
  const version = versionInput?.value?.trim() || "";
  if (!version) {
    notify(t("updates.toast.versionRequired"), "warn");
    return;
  }
  const assets = collectAssetRows();
  if (!assets.length) {
    notify(t("updates.toast.assetsRequired"), "warn");
    return;
  }
  const entry = { version, assets };
  catalog.releases = [
    ...catalog.releases.filter((item) => item.version !== version),
    entry,
  ];
  try {
    await publishManifest();
    if (versionInput) {
      versionInput.value = "";
    }
    const { assetRows } = updatesElements();
    if (assetRows) {
      assetRows.textContent = "";
    }
    notify(t("updates.toast.publishSuccess", { version }), "success");
    await loadUpdates({ silent: true });
  } catch (error) {
    notify(t("updates.toast.publishFailed", { message: error.message }), "error");
    await loadUpdates({ silent: true });
  }
};

const removeRelease = async (version) => {
  catalog.releases = catalog.releases.filter((item) => item.version !== version);
  try {
    await publishManifest();
    notify(t("updates.toast.publishSuccess", { version }), "success");
    await loadUpdates({ silent: true });
  } catch (error) {
    notify(t("updates.toast.publishFailed", { message: error.message }), "error");
    await loadUpdates({ silent: true });
  }
};

const publishCatalog = async () => {
  try {
    await publishManifest();
    notify(t("updates.toast.publishSuccess", { count: catalog.releases.length }), "success");
    await loadUpdates({ silent: true });
  } catch (error) {
    notify(t("updates.toast.publishFailed", { message: error.message }), "error");
  }
};

export const initUpdatesPanel = () => {
  elements.updatesRefreshBtn?.addEventListener("click", () => {
    loadUpdates().catch((error) =>
      notify(t("updates.toast.loadFailed", { message: error.message }), "error")
    );
  });
  elements.updatesUploadBtn?.addEventListener("click", () => uploadPackage());
  elements.updatesAddAssetBtn?.addEventListener("click", () => addAssetRow());
  elements.updatesAppendReleaseBtn?.addEventListener("click", () => appendRelease());
  elements.updatesPublishBtn?.addEventListener("click", () => publishCatalog());
};

export const loadUpdatesPanel = async () => {
  try {
    await loadUpdates();
  } catch (error) {
    notify(t("updates.toast.loadFailed", { message: error.message }), "error");
  }
};
