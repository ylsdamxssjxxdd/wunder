import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { ensureLlmConfigLoaded } from "./llm.js";
import { getAuthHeaders } from "./admin-auth.js?v=20260120-01";
import { formatBytes } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260215-01";

const MULTIMODAL_MODEL_TYPES = new Set(["asr", "tts", "image", "video"]);
const AUDIO_EXTENSIONS = new Set(["mp3", "wav", "flac", "aac", "ogg", "m4a", "opus", "pcm"]);
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "mov", "avi", "mkv", "webm"]);

const normalizeModelType = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "llm";
  }
  const normalized = raw.replace(/[\s-]+/g, "_");
  if (
    normalized === "asr" ||
    normalized === "stt" ||
    normalized === "speech_to_text" ||
    normalized === "speech2text" ||
    normalized === "audio_transcription" ||
    normalized === "transcription" ||
    normalized === "audio_to_text"
  ) {
    return "asr";
  }
  if (
    normalized === "tts" ||
    normalized === "speech" ||
    normalized === "text_to_speech" ||
    normalized === "text2speech" ||
    normalized === "audio_speech"
  ) {
    return "tts";
  }
  if (
    normalized === "image" ||
    normalized === "draw" ||
    normalized === "drawing" ||
    normalized === "text_to_image" ||
    normalized === "text2image" ||
    normalized === "image_generation"
  ) {
    return "image";
  }
  if (
    normalized === "video" ||
    normalized === "movie" ||
    normalized === "animation" ||
    normalized === "text_to_video" ||
    normalized === "text2video" ||
    normalized === "video_generation"
  ) {
    return "video";
  }
  return "llm";
};

const parseOptionalInt = (element) => {
  const parsed = Number.parseInt(String(element?.value || "").trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const parseOptionalFloat = (element) => {
  const parsed = Number.parseFloat(String(element?.value || "").trim());
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const readOptionalText = (element) => {
  const value = String(element?.value || "").trim();
  return value ? value : undefined;
};

const normalizePath = (value) => String(value || "").trim().replace(/\\/g, "/");

const getSelectedUserId = () => {
  const value = String(elements.multimodalDebugUserId?.value || "").trim();
  return value || "admin";
};

const getSelectedContainerId = () => {
  const parsed = Number.parseInt(String(elements.multimodalDebugContainerId?.value || "").trim(), 10);
  if (!Number.isFinite(parsed)) {
    return 0;
  }
  return Math.min(10, Math.max(0, parsed));
};

const getSelectedBasePath = () => normalizePath(elements.multimodalDebugPath?.value || "");

const getWorkspaceDownloadUrl = (userId, containerId, publicPath) => {
  const wunderBase = getWunderBase();
  const relativePath = normalizeWorkspaceRelativePath(publicPath);
  const params = new URLSearchParams({
    user_id: userId,
    path: relativePath,
    container_id: String(containerId),
  });
  return `${wunderBase}/workspace/download?${params.toString()}`;
};

const normalizeWorkspaceRelativePath = (publicPath) => {
  const raw = String(publicPath || "").trim().replace(/\\/g, "/");
  const marker = "/workspaces/";
  const markerIndex = raw.indexOf(marker);
  if (markerIndex >= 0) {
    const suffix = raw.slice(markerIndex + marker.length);
    const slashIndex = suffix.indexOf("/");
    return slashIndex >= 0 ? suffix.slice(slashIndex + 1) : "";
  }
  return raw.replace(/^\/+/, "");
};

const clearPreviewObjectUrl = () => {
  if (state.multimodalDebug.previewObjectUrl) {
    URL.revokeObjectURL(state.multimodalDebug.previewObjectUrl);
    state.multimodalDebug.previewObjectUrl = null;
  }
};

const setLogHtml = (html) => {
  if (!elements.multimodalDebugLog) {
    return;
  }
  elements.multimodalDebugLog.innerHTML = html;
  elements.multimodalDebugLog.scrollTop = elements.multimodalDebugLog.scrollHeight;
};

const appendLogLine = (title, detail = "") => {
  if (!elements.multimodalDebugLog) {
    return;
  }
  const item = document.createElement("div");
  item.className = "multimodal-debug-log-item";

  const time = document.createElement("div");
  time.className = "multimodal-debug-log-time";
  time.textContent = new Date().toLocaleTimeString();

  const main = document.createElement("div");
  main.className = "multimodal-debug-log-main";

  const heading = document.createElement("div");
  heading.className = "multimodal-debug-log-title";
  heading.textContent = title;
  main.appendChild(heading);

  if (detail) {
    const body = document.createElement("pre");
    body.className = "multimodal-debug-log-detail";
    body.textContent = detail;
    main.appendChild(body);
  }

  item.appendChild(time);
  item.appendChild(main);
  elements.multimodalDebugLog.appendChild(item);
  elements.multimodalDebugLog.scrollTop = elements.multimodalDebugLog.scrollHeight;
};

const setBusy = (busy) => {
  state.multimodalDebug.busy = busy;
  [
    elements.multimodalDebugTranscriptionRun,
    elements.multimodalDebugSpeechRun,
    elements.multimodalDebugImageRun,
    elements.multimodalDebugVideoRun,
    elements.multimodalDebugClearBtn,
  ].forEach((button) => {
    if (button) {
      button.disabled = busy;
    }
  });
};

const setStatusText = (message) => {
  if (elements.multimodalDebugStatusText) {
    elements.multimodalDebugStatusText.textContent = message || "-";
  }
};

const setResultJson = (value) => {
  state.multimodalDebug.lastResult = value || null;
  if (elements.multimodalDebugResultJson) {
    elements.multimodalDebugResultJson.textContent = value ? JSON.stringify(value, null, 2) : "";
  }
};

const updateResultSummary = (payload) => {
  const data = payload?.data || {};
  if (elements.multimodalDebugLastType) {
    elements.multimodalDebugLastType.textContent = data.kind || "-";
  }
  if (elements.multimodalDebugLastModel) {
    elements.multimodalDebugLastModel.textContent = data.model_name || "-";
  }
  if (elements.multimodalDebugLastPath) {
    elements.multimodalDebugLastPath.textContent =
      data.workspace_relative_path || data.source_workspace_relative_path || "-";
  }
  if (elements.multimodalDebugLastSize) {
    elements.multimodalDebugLastSize.textContent = Number.isFinite(data.size_bytes)
      ? formatBytes(data.size_bytes)
      : "-";
  }
};

const clearPreview = (message) => {
  clearPreviewObjectUrl();
  if (!elements.multimodalDebugPreview) {
    return;
  }
  elements.multimodalDebugPreview.innerHTML = "";
  const placeholder = document.createElement("div");
  placeholder.className = "multimodal-debug-preview-empty";
  placeholder.textContent = message || t("multimodalDebug.preview.empty");
  elements.multimodalDebugPreview.appendChild(placeholder);
  if (elements.multimodalDebugPreviewMeta) {
    elements.multimodalDebugPreviewMeta.textContent = "";
  }
};

const getKindByPath = (path) => {
  const ext = String(path || "").split(".").pop().toLowerCase();
  if (AUDIO_EXTENSIONS.has(ext)) {
    return "audio";
  }
  if (IMAGE_EXTENSIONS.has(ext)) {
    return "image";
  }
  if (VIDEO_EXTENSIONS.has(ext)) {
    return "video";
  }
  return "";
};

const renderPreview = async (payload) => {
  const data = payload?.data || {};
  if (data.kind === "transcription") {
    clearPreviewObjectUrl();
    elements.multimodalDebugPreview.innerHTML = "";
    const pre = document.createElement("pre");
    pre.className = "multimodal-debug-log-detail";
    pre.textContent = String(data.text || "").trim() || t("multimodalDebug.preview.empty");
    elements.multimodalDebugPreview.appendChild(pre);
    if (elements.multimodalDebugPreviewMeta) {
      const meta = [];
      if (data.source_public_path) {
        meta.push(String(data.source_public_path));
      }
      if (data.content_type) {
        meta.push(String(data.content_type));
      }
      elements.multimodalDebugPreviewMeta.textContent = meta.join(" 路 ");
    }
    return;
  }
  const publicPath = String(data.public_path || "").trim();
  if (!publicPath) {
    clearPreview(t("multimodalDebug.preview.empty"));
    return;
  }
  const userId = String(data.user_id || getSelectedUserId()).trim() || "admin";
  const containerId = Number.isFinite(data.container_id) ? data.container_id : getSelectedContainerId();
  const kind = data.kind || getKindByPath(publicPath);
  const downloadUrl = getWorkspaceDownloadUrl(userId, containerId, publicPath);

  clearPreviewObjectUrl();
  elements.multimodalDebugPreview.innerHTML = "";
  if (elements.multimodalDebugPreviewMeta) {
    const meta = [];
    if (data.public_path) {
      meta.push(String(data.public_path));
    }
    if (Number.isFinite(data.size_bytes)) {
      meta.push(formatBytes(data.size_bytes));
    }
    if (data.content_type) {
      meta.push(String(data.content_type));
    }
    elements.multimodalDebugPreviewMeta.textContent = meta.join(" · ");
  }

  appendLogLine("preview.fetch", downloadUrl);
  const response = await fetch(downloadUrl, {
    headers: getAuthHeaders(),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const blob = await response.blob();
  state.multimodalDebug.previewObjectUrl = URL.createObjectURL(blob);

  if (kind === "audio") {
    const audio = document.createElement("audio");
    audio.className = "multimodal-debug-preview-audio";
    audio.controls = true;
    audio.preload = "metadata";
    audio.src = state.multimodalDebug.previewObjectUrl;
    elements.multimodalDebugPreview.appendChild(audio);
    return;
  }

  if (kind === "image") {
    const image = document.createElement("img");
    image.className = "multimodal-debug-preview-image";
    image.alt = "multimodal result";
    image.src = state.multimodalDebug.previewObjectUrl;
    elements.multimodalDebugPreview.appendChild(image);
    return;
  }

  if (kind === "video") {
    const video = document.createElement("video");
    video.className = "multimodal-debug-preview-video";
    video.controls = true;
    video.preload = "metadata";
    video.src = state.multimodalDebug.previewObjectUrl;
    elements.multimodalDebugPreview.appendChild(video);
    return;
  }

  clearPreview(t("multimodalDebug.preview.unsupported"));
};

const syncImageParamsFromModel = () => {
  const modelName = String(elements.multimodalDebugImageModel?.value || "").trim();
  const config = modelName ? state.llm.configs?.[modelName] : null;
  // Sync steps: use model config value, or default to 30 if not set
  const steps = config?.image_num_inference_steps ?? 30;
  if (elements.multimodalDebugImageSteps) {
    elements.multimodalDebugImageSteps.value = steps;
  }
  // Sync guidance scale
  const guidance = config?.image_guidance_scale ?? "";
  if (elements.multimodalDebugImageGuidance) {
    elements.multimodalDebugImageGuidance.value = guidance;
  }
  // Sync negative prompt
  const negativePrompt = config?.image_negative_prompt ?? "";
  if (elements.multimodalDebugImageNegativePrompt) {
    elements.multimodalDebugImageNegativePrompt.value = negativePrompt;
  }
  // Sync size
  const size = config?.image_size ?? "";
  if (elements.multimodalDebugImageSize) {
    elements.multimodalDebugImageSize.value = size;
  }
  // Sync output format
  const format = config?.image_output_format ?? "";
  if (elements.multimodalDebugImageFormat) {
    elements.multimodalDebugImageFormat.value = format;
  }
};

const renderModelOptions = (select, type, defaultName) => {
  if (!select) {
    return;
  }
  const previous = String(select.value || "").trim();
  select.textContent = "";

  const optionDefault = document.createElement("option");
  optionDefault.value = "";
  optionDefault.textContent = defaultName ? `${t("llm.default")} (${defaultName})` : t("llm.default");
  select.appendChild(optionDefault);

  state.llm.order
    .filter((name) => MULTIMODAL_MODEL_TYPES.has(type) && normalizeModelType(state.llm.configs?.[name]?.model_type) === type)
    .forEach((name) => {
      const option = document.createElement("option");
      option.value = name;
      option.textContent = name;
      select.appendChild(option);
    });

  if (previous && select.querySelector(`option[value="${previous}"]`)) {
    select.value = previous;
  } else {
    select.value = "";
  }
};

const renderAllModelOptions = () => {
  renderModelOptions(
    elements.multimodalDebugTranscriptionModel,
    "asr",
    state.llm.defaultAsrName || ""
  );
  renderModelOptions(
    elements.multimodalDebugSpeechModel,
    "tts",
    state.llm.defaultTtsName || ""
  );
  renderModelOptions(
    elements.multimodalDebugImageModel,
    "image",
    state.llm.defaultImageName || ""
  );
  renderModelOptions(
    elements.multimodalDebugVideoModel,
    "video",
    state.llm.defaultVideoName || ""
  );
};

const buildBasePayload = () => {
  const payload = {
    user_id: getSelectedUserId(),
    container_id: getSelectedContainerId(),
  };
  const path = getSelectedBasePath();
  if (path) {
    payload.path = path;
  }
  return payload;
};

const buildSpeechPayload = () => ({
  ...buildBasePayload(),
  text: String(elements.multimodalDebugSpeechText?.value || "").trim(),
  model_name: readOptionalText(elements.multimodalDebugSpeechModel),
  voice: readOptionalText(elements.multimodalDebugSpeechVoice),
  instructions: readOptionalText(elements.multimodalDebugSpeechInstructions),
  response_format: readOptionalText(elements.multimodalDebugSpeechFormat),
  speed: parseOptionalFloat(elements.multimodalDebugSpeechSpeed),
});

const buildTranscriptionPayload = () => ({
  ...buildBasePayload(),
  source_public_path: readOptionalText(elements.multimodalDebugTranscriptionSourcePath),
  model_name: readOptionalText(elements.multimodalDebugTranscriptionModel),
  language: readOptionalText(elements.multimodalDebugTranscriptionLanguage),
  prompt: readOptionalText(elements.multimodalDebugTranscriptionPrompt),
  response_format: readOptionalText(elements.multimodalDebugTranscriptionResponseFormat),
  temperature: parseOptionalFloat(elements.multimodalDebugTranscriptionTemperature),
});

const buildImagePayload = () => ({
  ...buildBasePayload(),
  prompt: String(elements.multimodalDebugImagePrompt?.value || "").trim(),
  model_name: readOptionalText(elements.multimodalDebugImageModel),
  size: readOptionalText(elements.multimodalDebugImageSize),
  output_format: readOptionalText(elements.multimodalDebugImageFormat),
  negative_prompt: readOptionalText(elements.multimodalDebugImageNegativePrompt),
  num_inference_steps: parseOptionalInt(elements.multimodalDebugImageSteps),
  guidance_scale: parseOptionalFloat(elements.multimodalDebugImageGuidance),
  seed: parseOptionalInt(elements.multimodalDebugImageSeed),
});

const buildVideoPayload = () => ({
  ...buildBasePayload(),
  prompt: String(elements.multimodalDebugVideoPrompt?.value || "").trim(),
  model_name: readOptionalText(elements.multimodalDebugVideoModel),
  size: readOptionalText(elements.multimodalDebugVideoSize),
  seconds: parseOptionalFloat(elements.multimodalDebugVideoSeconds),
  fps: parseOptionalInt(elements.multimodalDebugVideoFps),
  num_frames: parseOptionalInt(elements.multimodalDebugVideoFrames),
  negative_prompt: readOptionalText(elements.multimodalDebugVideoNegativePrompt),
  num_inference_steps: parseOptionalInt(elements.multimodalDebugVideoSteps),
  guidance_scale: parseOptionalFloat(elements.multimodalDebugVideoGuidance),
  guidance_scale_2: parseOptionalFloat(elements.multimodalDebugVideoGuidance2),
  boundary_ratio: parseOptionalFloat(elements.multimodalDebugVideoBoundaryRatio),
  flow_shift: parseOptionalFloat(elements.multimodalDebugVideoFlowShift),
  seed: parseOptionalInt(elements.multimodalDebugVideoSeed),
  enable_frame_interpolation: elements.multimodalDebugVideoInterpolation?.checked === true,
});

const sanitizePayload = (payload) => {
  const result = {};
  Object.entries(payload).forEach(([key, value]) => {
    if (value === undefined || value === null || value === "") {
      return;
    }
    result[key] = value;
  });
  return result;
};

const runRequest = async (kind, endpoint, payload) => {
  const wunderBase = getWunderBase();
  const body = sanitizePayload(payload);
  appendLogLine(`request.${kind}`, JSON.stringify(body, null, 2));
  setBusy(true);
  setStatusText(t("multimodalDebug.status.running", { kind }));
  try {
    const response = await fetch(`${wunderBase}${endpoint}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(getAuthHeaders() || {}),
      },
      body: JSON.stringify(body),
    });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) {
      const message =
        result?.error?.message || result?.detail?.message || t("common.requestFailed", { status: response.status });
      appendLogLine(`response.${kind}.error`, JSON.stringify(result, null, 2));
      throw new Error(message);
    }
    appendLogLine(`response.${kind}.ok`, JSON.stringify(result, null, 2));
    setResultJson(result);
    updateResultSummary(result);
    setStatusText(t("multimodalDebug.status.completed", { kind }));
    await renderPreview(result);
    notify(t("multimodalDebug.status.completed", { kind }), "success");
  } finally {
    setBusy(false);
  }
};

const runMultipartRequest = async (kind, endpoint, payload, file) => {
  const wunderBase = getWunderBase();
  const form = new FormData();
  Object.entries(sanitizePayload(payload)).forEach(([key, value]) => {
    form.append(key, String(value));
  });
  if (file) {
    form.append("file", file, file.name || "audio");
  }
  appendLogLine(
    `request.${kind}`,
    JSON.stringify(
      {
        ...sanitizePayload(payload),
        file: file
          ? {
              name: file.name,
              type: file.type,
              size: file.size,
            }
          : null,
      },
      null,
      2
    )
  );
  setBusy(true);
  setStatusText(t("multimodalDebug.status.running", { kind }));
  try {
    const response = await fetch(`${wunderBase}${endpoint}`, {
      method: "POST",
      headers: {
        ...(getAuthHeaders() || {}),
      },
      body: form,
    });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) {
      const message =
        result?.error?.message || result?.detail?.message || t("common.requestFailed", { status: response.status });
      appendLogLine(`response.${kind}.error`, JSON.stringify(result, null, 2));
      throw new Error(message);
    }
    appendLogLine(`response.${kind}.ok`, JSON.stringify(result, null, 2));
    setResultJson(result);
    updateResultSummary(result);
    setStatusText(t("multimodalDebug.status.completed", { kind }));
    await renderPreview(result);
    notify(t("multimodalDebug.status.completed", { kind }), "success");
  } finally {
    setBusy(false);
  }
};

const handleSpeechRun = async () => {
  const payload = buildSpeechPayload();
  if (!payload.text) {
    throw new Error(t("multimodalDebug.error.textRequired"));
  }
  await runRequest("speech", "/admin/multimodal/speech", payload);
};

const handleTranscriptionRun = async () => {
  const payload = buildTranscriptionPayload();
  const file = elements.multimodalDebugTranscriptionFile?.files?.[0] || null;
  if (!payload.source_public_path && !file) {
    throw new Error(t("multimodalDebug.error.audioSourceRequired"));
  }
  await runMultipartRequest(
    "transcription",
    "/admin/multimodal/transcription",
    payload,
    file
  );
};

const handleImageRun = async () => {
  const payload = buildImagePayload();
  if (!payload.prompt) {
    throw new Error(t("multimodalDebug.error.promptRequired"));
  }
  await runRequest("image", "/admin/multimodal/image", payload);
};

const handleVideoRun = async () => {
  const payload = buildVideoPayload();
  if (!payload.prompt) {
    throw new Error(t("multimodalDebug.error.promptRequired"));
  }
  await runRequest("video", "/admin/multimodal/video", payload);
};

const bindButton = (button, handler) => {
  if (!button || button.dataset.bound === "1") {
    return;
  }
  button.dataset.bound = "1";
  button.addEventListener("click", async () => {
    try {
      await handler();
    } catch (error) {
      const message = error?.message || t("common.unknownError");
      appendLogLine("error", message);
      setStatusText(message);
      notify(message, "error");
    }
  });
};

const handleClear = () => {
  setLogHtml("");
  setResultJson(null);
  clearPreview();
  updateResultSummary(null);
  setStatusText("-");
};

const handleCopyResult = async () => {
  const text = elements.multimodalDebugResultJson?.textContent || "";
  if (!text.trim()) {
    return;
  }
  await navigator.clipboard.writeText(text);
  notify(t("multimodalDebug.message.copied"), "success");
};

export const initMultimodalDebugPanel = () => {
  bindButton(elements.multimodalDebugTranscriptionRun, handleTranscriptionRun);
  bindButton(elements.multimodalDebugSpeechRun, handleSpeechRun);
  bindButton(elements.multimodalDebugImageRun, handleImageRun);
  bindButton(elements.multimodalDebugVideoRun, handleVideoRun);
  bindButton(elements.multimodalDebugClearBtn, async () => handleClear());
  bindButton(elements.multimodalDebugCopyResultBtn, handleCopyResult);
  // Sync image params when model selection changes
  if (elements.multimodalDebugImageModel) {
    elements.multimodalDebugImageModel.addEventListener("change", syncImageParamsFromModel);
  }
  window.addEventListener("wunder:llm-updated", () => {
    renderAllModelOptions();
  });
  clearPreview();
  updateResultSummary(null);
};

export const loadMultimodalDebugPanel = async () => {
  await ensureLlmConfigLoaded();
  renderAllModelOptions();
  // Sync image params from default/selected model after loading
  syncImageParamsFromModel();
  state.multimodalDebug.loaded = true;
};
