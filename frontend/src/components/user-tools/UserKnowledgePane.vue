<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>知识库管理</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" @click="addBase">
          新增
        </button>
        <button class="user-tools-btn secondary compact" type="button" @click="refreshConfig">
          刷新
        </button>
      </div>
    </div>
    <div class="tips">知识库目录固定在 data/user_tools/&lt;user&gt;/knowledge，工具名称会以 user_id@知识库名 展示，新增或编辑在弹窗中完成。</div>

    <div class="management-layout knowledge-layout">
      <div class="management-list">
        <div class="list-header">
          <label>知识库列表</label>
        </div>
        <div class="list-body">
          <template v-if="bases.length">
            <button
              v-for="(base, index) in bases"
              :key="`${base.name || index}`"
              class="list-item"
              :class="{ active: index === selectedIndex }"
              type="button"
              @click="selectBase(index)"
            >
              <div>{{ base.name || '未命名知识库' }}</div>
              <small>{{ base.root || '未生成目录' }}</small>
            </button>
          </template>
          <div v-else class="empty-text">暂无知识库，请新增。</div>
        </div>
      </div>

      <div class="management-detail knowledge-detail">
        <div class="detail-header">
          <div>
            <div class="detail-title">{{ detailTitle }}</div>
            <div class="muted">{{ detailMeta }}</div>
            <div class="muted">{{ detailDesc }}</div>
          </div>
          <div class="detail-actions">
            <div class="actions">
              <button
                class="user-tools-btn secondary compact"
                type="button"
                :disabled="!activeBase"
                @click="editBase"
              >
                编辑
              </button>
              <button
                class="user-tools-btn danger compact"
                type="button"
                :disabled="!activeBase"
                @click="deleteBase"
              >
                删除知识库
              </button>
            </div>
          </div>
        </div>

        <div class="knowledge-section form-section">
          <div class="knowledge-content">
            <div class="user-tools-card knowledge-files-card">
              <div class="knowledge-file-layout">
                <div class="knowledge-file-pane">
                  <div class="knowledge-file-toolbar">
                    <button class="user-tools-btn secondary compact" type="button" @click="triggerUpload">
                      上传
                    </button>
                    <button class="user-tools-btn secondary compact" type="button" @click="createFile">
                      新建
                    </button>
                    <button class="user-tools-btn compact" type="button" @click="saveFile">
                      保存
                    </button>
                  </div>
                  <div class="knowledge-file-list">
                    <div v-if="!files.length" class="empty-text">暂无文档，请先刷新列表。</div>
                    <div
                      v-for="filePath in files"
                      :key="filePath"
                      class="knowledge-file-item"
                      :class="{ active: filePath === activeFile }"
                      @click="selectFile(filePath)"
                    >
                      <span class="knowledge-file-name">{{ filePath }}</span>
                      <button
                        class="knowledge-file-delete-btn"
                        type="button"
                        title="删除文档"
                        @click.stop="deleteFile(filePath)"
                      >
                        删除
                      </button>
                    </div>
                  </div>
                </div>
                <div class="knowledge-file-editor">
                  <div class="muted">{{ activeFile || '未选择文档' }}</div>
                  <div ref="knowledgeEditorRef" class="knowledge-editor-wrapper">
                    <div ref="knowledgeHighlightRef" class="knowledge-editor-highlight"></div>
                    <el-input
                      v-model="fileContent"
                      type="textarea"
                      :rows="14"
                      @input="scheduleKnowledgeEditorUpdate"
                    />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <input ref="fileUploadRef" type="file" :accept="uploadAccept" hidden @change="handleFileUpload" />

    <el-dialog
      v-model="knowledgeModalVisible"
      class="user-tools-dialog user-tools-subdialog"
      width="560px"
      top="10vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ knowledgeModalTitle }}</div>
          <button class="icon-btn" type="button" @click="closeKnowledgeModal">×</button>
        </div>
      </template>

      <div class="user-tools-form">
        <div class="form-row">
          <label>名称</label>
          <el-input v-model="knowledgeForm.name" placeholder="知识库名称" />
        </div>
        <div class="form-row">
          <label>描述</label>
          <el-input v-model="knowledgeForm.description" type="textarea" :rows="4" placeholder="知识库用途说明" />
        </div>
        <div class="form-row">
          <label class="checkbox-row">
            <input type="checkbox" v-model="knowledgeForm.enabled" />
            <span>启用</span>
          </label>
        </div>
        <div class="form-row">
          <label class="checkbox-row">
            <input type="checkbox" v-model="knowledgeForm.shared" />
            <span>共享</span>
          </label>
        </div>
        <div class="muted">目录固定在 data/user_tools/&lt;user&gt;/knowledge/&lt;名称&gt;，保存后自动生成。</div>
      </div>

      <template #footer>
        <button class="user-tools-btn secondary" type="button" @click="closeKnowledgeModal">取消</button>
        <button class="user-tools-btn" type="button" @click="applyKnowledgeModal">保存</button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteUserKnowledgeFile,
  fetchUserKnowledgeConfig,
  fetchUserKnowledgeFile,
  fetchUserKnowledgeFiles,
  saveUserKnowledgeConfig,
  saveUserKnowledgeFile,
  uploadUserKnowledgeFile
} from '@/api/userTools';

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  active: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['status']);

// doc2md 支持的扩展名列表（用于前端过滤）
const USER_KNOWLEDGE_UPLOAD_EXTENSIONS = [
  '.txt',
  '.md',
  '.markdown',
  '.html',
  '.htm',
  '.py',
  '.c',
  '.cpp',
  '.cc',
  '.h',
  '.hpp',
  '.json',
  '.js',
  '.ts',
  '.css',
  '.ini',
  '.cfg',
  '.log',
  '.doc',
  '.docx',
  '.odt',
  '.pptx',
  '.odp',
  '.xlsx',
  '.ods',
  '.wps',
  '.et',
  '.dps'
];
const uploadAccept = USER_KNOWLEDGE_UPLOAD_EXTENSIONS.join(',');

// 转义 HTML，避免用户输入被浏览器当作标签解析
const escapeHtml = (text) =>
  String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

// 将 Markdown 文本转换为高亮层 HTML，仅高亮一级标题（# 开头）
const buildHeadingHighlightHtml = (text) => {
  const raw = String(text ?? '');
  const lines = raw.replace(/\r/g, '').split('\n');
  return lines
    .map((line) => {
      const escaped = escapeHtml(line) || '&nbsp;';
      const isHeading = /^\s*#(?!#)\s*\S/.test(line);
      const classes = isHeading
        ? 'knowledge-editor-line knowledge-heading-line'
        : 'knowledge-editor-line';
      return `<span class="${classes}">${escaped}</span>`;
    })
    .join('');
};

const bases = ref([]);
const selectedIndex = ref(-1);
const files = ref([]);
const activeFile = ref('');
const fileContent = ref('');
const loaded = ref(false);
const loading = ref(false);
const fileUploadRef = ref(null);
// 知识库文档编辑器需要同步滚动与高亮层，因此保留 DOM 引用
const knowledgeEditorRef = ref(null);
const knowledgeHighlightRef = ref(null);
let knowledgeTextarea = null;
let knowledgeResizeBound = false;

const knowledgeModalVisible = ref(false);
const knowledgeEditingIndex = ref(-1);
const knowledgeForm = reactive({
  name: '',
  description: '',
  enabled: true,
  shared: false
});

// 获取编辑器内部的 textarea DOM，便于对齐高亮层
const getKnowledgeTextarea = () => {
  if (knowledgeTextarea && document.contains(knowledgeTextarea)) {
    return knowledgeTextarea;
  }
  const wrapper = knowledgeEditorRef.value;
  if (!wrapper) {
    return null;
  }
  const textarea = wrapper.querySelector('.el-textarea__inner');
  if (textarea) {
    knowledgeTextarea = textarea;
  }
  return textarea;
};

// 同步高亮层滚动位置，确保与文本滚动一致
const syncKnowledgeEditorScroll = () => {
  const textarea = getKnowledgeTextarea();
  const highlight = knowledgeHighlightRef.value;
  if (!textarea || !highlight) {
    return;
  }
  highlight.scrollTop = textarea.scrollTop;
  highlight.scrollLeft = textarea.scrollLeft;
};

// 根据 textarea 样式与内容刷新高亮层
const updateKnowledgeEditorHighlight = () => {
  const textarea = getKnowledgeTextarea();
  const highlight = knowledgeHighlightRef.value;
  if (!textarea || !highlight) {
    return;
  }
  const styles = window.getComputedStyle(textarea);
  highlight.style.font = styles.font;
  highlight.style.letterSpacing = styles.letterSpacing;
  highlight.style.wordSpacing = styles.wordSpacing;
  highlight.style.textAlign = styles.textAlign;
  highlight.style.textTransform = styles.textTransform;
  highlight.style.textIndent = styles.textIndent;
  highlight.style.textRendering = styles.textRendering;
  highlight.style.whiteSpace = styles.whiteSpace;
  highlight.style.wordBreak = styles.wordBreak;
  highlight.style.overflowWrap = styles.overflowWrap;
  highlight.style.tabSize = styles.tabSize;
  highlight.style.direction = styles.direction;
  highlight.style.setProperty('--knowledge-editor-padding-top', styles.paddingTop);
  highlight.style.setProperty('--knowledge-editor-padding-right', styles.paddingRight);
  highlight.style.setProperty('--knowledge-editor-padding-bottom', styles.paddingBottom);
  highlight.style.setProperty('--knowledge-editor-padding-left', styles.paddingLeft);
  const borderX = parseFloat(styles.borderLeftWidth) + parseFloat(styles.borderRightWidth);
  const borderY = parseFloat(styles.borderTopWidth) + parseFloat(styles.borderBottomWidth);
  const scrollbarWidth = Math.max(
    0,
    textarea.offsetWidth - textarea.clientWidth - borderX
  );
  const scrollbarHeight = Math.max(
    0,
    textarea.offsetHeight - textarea.clientHeight - borderY
  );
  // 同步滚动条占位，避免自动换行宽度不一致导致高亮错位
  highlight.style.setProperty('--knowledge-scrollbar-width', `${scrollbarWidth}px`);
  highlight.style.setProperty('--knowledge-scrollbar-height', `${scrollbarHeight}px`);
  // 更新一级标题高亮层内容，便于快速识别知识条目
  highlight.innerHTML = buildHeadingHighlightHtml(fileContent.value);
  syncKnowledgeEditorScroll();
};

// 绑定滚动/尺寸监听，避免重复绑定造成开销
const bindKnowledgeEditorEvents = () => {
  const textarea = getKnowledgeTextarea();
  if (!textarea) {
    return;
  }
  if (knowledgeTextarea) {
    knowledgeTextarea.removeEventListener('scroll', syncKnowledgeEditorScroll);
  }
  knowledgeTextarea = textarea;
  knowledgeTextarea.addEventListener('scroll', syncKnowledgeEditorScroll);
  if (!knowledgeResizeBound) {
    window.addEventListener('resize', updateKnowledgeEditorHighlight);
    knowledgeResizeBound = true;
  }
};

// 清理监听，避免弹窗关闭后仍然占用资源
const cleanupKnowledgeEditorEvents = () => {
  if (knowledgeTextarea) {
    knowledgeTextarea.removeEventListener('scroll', syncKnowledgeEditorScroll);
    knowledgeTextarea = null;
  }
  if (knowledgeResizeBound) {
    window.removeEventListener('resize', updateKnowledgeEditorHighlight);
    knowledgeResizeBound = false;
  }
};

// 在 DOM 更新后刷新高亮层，确保排版与滚动同步
const scheduleKnowledgeEditorUpdate = () => {
  nextTick(() => {
    bindKnowledgeEditorEvents();
    updateKnowledgeEditorHighlight();
  });
};

const activeBase = computed(() => bases.value[selectedIndex.value] || null);
const detailTitle = computed(() => activeBase.value?.name || '未选择知识库');
const detailMeta = computed(() => {
  if (!activeBase.value) {
    return '';
  }
  const root = activeBase.value.root || '未生成目录';
  const parts = [root, activeBase.value.enabled !== false ? '已启用' : '未启用'];
  if (activeBase.value.shared) {
    parts.push('已共享');
  }
  return parts.join(' · ');
});
const detailDesc = computed(() => activeBase.value?.description || '');
const knowledgeModalTitle = computed(() =>
  knowledgeEditingIndex.value >= 0 ? '编辑知识库' : '新增知识库'
);

const emitStatus = (message) => {
  emit('status', message || '');
};

const normalizeKnowledgeConfig = (raw) => {
  const config = raw || {};
  return {
    bases: Array.isArray(config.bases)
      ? config.bases
          .filter((base) => String(base?.name || '').trim())
          .map((base) => ({
            name: base.name || '',
            description: base.description || '',
            root: base.root || '',
            enabled: base.enabled !== false,
            shared: Boolean(base.shared)
          }))
      : []
  };
};

const buildConfigPayload = () => ({
  bases: bases.value
    .map((base) => ({
      name: base.name.trim(),
      description: base.description || '',
      enabled: base.enabled !== false,
      shared: base.shared === true
    }))
    .filter((base) => base.name)
});

const validateConfigPayload = (payload) => {
  const invalid = payload.bases.filter((base) => !base.name);
  if (invalid.length) {
    return '存在未填写名称的知识库，请补全后再保存。';
  }
  const nameSet = new Set();
  for (const base of payload.bases) {
    if (nameSet.has(base.name)) {
      return `知识库名称重复：${base.name}`;
    }
    nameSet.add(base.name);
  }
  return '';
};

// 保存当前知识库状态，便于保存失败时回滚
const captureKnowledgeSnapshot = () => ({
  bases: bases.value.map((base) => ({ ...base })),
  selectedIndex: selectedIndex.value,
  files: [...files.value],
  activeFile: activeFile.value,
  fileContent: fileContent.value
});

const restoreKnowledgeSnapshot = (snapshot) => {
  bases.value = snapshot.bases;
  selectedIndex.value = snapshot.selectedIndex;
  files.value = snapshot.files;
  activeFile.value = snapshot.activeFile;
  fileContent.value = snapshot.fileContent;
};

const loadConfig = async () => {
  if (loading.value) return;
  loading.value = true;
  try {
    const { data } = await fetchUserKnowledgeConfig();
    const payload = data?.data || {};
    const normalized = normalizeKnowledgeConfig(payload.knowledge || {});
    bases.value = normalized.bases;
    selectedIndex.value = bases.value.length ? 0 : -1;
    files.value = [];
    activeFile.value = '';
    fileContent.value = '';
    loaded.value = true;
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '知识库配置加载失败');
  } finally {
    loading.value = false;
  }
};

// 统一保存知识库配置，并以服务端结果回写
const saveConfig = async (preferredName = '') => {
  emitStatus('正在保存...');
  const payload = buildConfigPayload();
  const validationError = validateConfigPayload(payload);
  if (validationError) {
    emitStatus(validationError);
    throw new Error(validationError);
  }
  try {
    const currentName = bases.value[selectedIndex.value]?.name || '';
    const { data } = await saveUserKnowledgeConfig({ knowledge: payload });
    const result = data?.data || {};
    const normalized = normalizeKnowledgeConfig(result.knowledge || {});
    bases.value = normalized.bases;
    if (!bases.value.length) {
      selectedIndex.value = -1;
    } else {
      const targetName = preferredName || currentName;
      if (targetName) {
        const nextIndex = bases.value.findIndex((base) => base.name === targetName);
        selectedIndex.value = nextIndex >= 0 ? nextIndex : 0;
      } else {
        selectedIndex.value = 0;
      }
    }
    files.value = [];
    activeFile.value = '';
    fileContent.value = '';
    emitStatus('已保存。');
    return normalized;
  } catch (error) {
    emitStatus(`保存失败：${error.message || '请求失败'}`);
    throw error;
  }
};

const selectBase = async (index) => {
  selectedIndex.value = index;
  files.value = [];
  activeFile.value = '';
  fileContent.value = '';
  await loadFiles();
};

const resetKnowledgeForm = () => {
  knowledgeForm.name = '';
  knowledgeForm.description = '';
  knowledgeForm.enabled = true;
  knowledgeForm.shared = false;
};

// 打开知识库配置弹窗
const openKnowledgeModal = (base = null, index = -1) => {
  knowledgeEditingIndex.value = Number.isInteger(index) ? index : -1;
  knowledgeForm.name = base?.name || '';
  knowledgeForm.description = base?.description || '';
  knowledgeForm.enabled = base?.enabled !== false;
  knowledgeForm.shared = base?.shared === true;
  knowledgeModalVisible.value = true;
};

// 关闭知识库配置弹窗并清理状态
const closeKnowledgeModal = () => {
  knowledgeModalVisible.value = false;
  knowledgeEditingIndex.value = -1;
  resetKnowledgeForm();
};

const validateKnowledgeBase = (payload, index) => {
  if (!payload.name) {
    return '请填写知识库名称。';
  }
  for (let i = 0; i < bases.value.length; i += 1) {
    if (i === index) {
      continue;
    }
    if (bases.value[i].name.trim() === payload.name) {
      return `知识库名称重复：${payload.name}`;
    }
  }
  return '';
};

const getKnowledgeFormPayload = () => ({
  name: knowledgeForm.name.trim(),
  description: knowledgeForm.description.trim(),
  enabled: knowledgeForm.enabled !== false,
  shared: knowledgeForm.shared === true
});

// 保存知识库配置（新增/编辑）
const applyKnowledgeModal = async () => {
  const payload = getKnowledgeFormPayload();
  const error = validateKnowledgeBase(payload, knowledgeEditingIndex.value);
  if (error) {
    ElMessage.warning(error);
    return;
  }
  const snapshot = captureKnowledgeSnapshot();
  const editing = knowledgeEditingIndex.value;
  if (editing >= 0) {
    const current = bases.value[editing] || {};
    const nextRoot = current.name === payload.name ? current.root || '' : '';
    bases.value[editing] = { ...current, ...payload, root: nextRoot };
    selectedIndex.value = editing;
  } else {
    bases.value.push({ ...payload, root: '' });
    selectedIndex.value = bases.value.length - 1;
  }
  files.value = [];
  activeFile.value = '';
  fileContent.value = '';
  try {
    await saveConfig(payload.name);
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
    ElMessage.success(editing >= 0 ? '知识库已更新。' : '知识库已新增。');
    closeKnowledgeModal();
  } catch (error) {
    restoreKnowledgeSnapshot(snapshot);
    ElMessage.error(error.response?.data?.detail || `保存失败：${error.message || '请求失败'}`);
  }
};

const addBase = () => {
  openKnowledgeModal();
};

const editBase = () => {
  const base = activeBase.value;
  if (!base) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  openKnowledgeModal(base, selectedIndex.value);
};

const deleteBase = async () => {
  const base = activeBase.value;
  if (!base) return;
  try {
    await ElMessageBox.confirm(`确认删除知识库 ${base.name || '未命名知识库'} 吗？`, '提示', {
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  const snapshot = captureKnowledgeSnapshot();
  bases.value.splice(selectedIndex.value, 1);
  if (!bases.value.length) {
    selectedIndex.value = -1;
  } else {
    selectedIndex.value = Math.max(0, selectedIndex.value - 1);
  }
  files.value = [];
  activeFile.value = '';
  fileContent.value = '';
  try {
    const preferredName = bases.value[selectedIndex.value]?.name || '';
    await saveConfig(preferredName);
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
    ElMessage.success('知识库已删除。');
  } catch (error) {
    restoreKnowledgeSnapshot(snapshot);
    ElMessage.error(error.response?.data?.detail || `删除失败：${error.message || '请求失败'}`);
  }
};

const loadFiles = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    files.value = [];
    activeFile.value = '';
    fileContent.value = '';
    return;
  }
  try {
    const { data } = await fetchUserKnowledgeFiles(base.name);
    const payload = data?.data || {};
    files.value = Array.isArray(payload.files) ? payload.files : [];
    if (!files.value.includes(activeFile.value)) {
      activeFile.value = '';
      fileContent.value = '';
    }
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '文档列表加载失败');
  }
};

const selectFile = async (filePath) => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  try {
    const { data } = await fetchUserKnowledgeFile(base.name, filePath);
    const payload = data?.data || {};
    activeFile.value = payload.path || filePath;
    fileContent.value = payload.content || '';
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '读取失败');
  }
};

const saveFile = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  if (!activeFile.value) {
    ElMessage.warning('请先选择要保存的文档。');
    return;
  }
  try {
    await saveUserKnowledgeFile({
      base: base.name,
      path: activeFile.value,
      content: fileContent.value
    });
    await loadFiles();
    ElMessage.success('文档已保存并刷新索引。');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '文档保存失败');
  }
};

// 支持列表项悬停删除指定文档
const deleteFile = async (targetPath = '') => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  const path = targetPath || activeFile.value;
  if (!path) {
    ElMessage.warning('请先选择要删除的文档。');
    return;
  }
  try {
    await ElMessageBox.confirm(`确认删除 ${path} 吗？`, '提示', {
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  try {
    await deleteUserKnowledgeFile(base.name, path);
    if (path === activeFile.value) {
      activeFile.value = '';
      fileContent.value = '';
    }
    await loadFiles();
    ElMessage.success('文档已删除并刷新索引。');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '文档删除失败');
  }
};

const createFile = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  let filename = '';
  try {
    const { value } = await ElMessageBox.prompt('请输入新文档文件名（.md）', '新建文档', {
      confirmButtonText: '创建',
      cancelButtonText: '取消',
      inputValue: 'example.md'
    });
    filename = value || '';
  } catch (error) {
    return;
  }
  const trimmed = filename.trim();
  if (!trimmed) {
    ElMessage.warning('文件名不能为空。');
    return;
  }
  if (!trimmed.toLowerCase().endsWith('.md')) {
    ElMessage.warning('仅支持 .md 文档。');
    return;
  }
  activeFile.value = trimmed;
  fileContent.value = '';
  await saveFile();
  await selectFile(trimmed);
};

const normalizeUploadExtension = (filename) => {
  const parts = String(filename || '').trim().split('.');
  if (parts.length <= 1) {
    return '';
  }
  return `.${parts.pop().toLowerCase()}`;
};

const triggerUpload = () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  if (!fileUploadRef.value) return;
  fileUploadRef.value.value = '';
  fileUploadRef.value.click();
};

const handleFileUpload = async () => {
  const file = fileUploadRef.value?.files?.[0];
  if (!file) return;
  const extension = normalizeUploadExtension(file.name);
  if (!extension) {
    ElMessage.warning('文件缺少扩展名。');
    return;
  }
  if (!USER_KNOWLEDGE_UPLOAD_EXTENSIONS.includes(extension)) {
    ElMessage.warning(`不支持的文件类型：${extension}`);
    return;
  }
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning('请先选择知识库。');
    return;
  }
  try {
    const { data } = await uploadUserKnowledgeFile(base.name, file);
    const payload = data?.data || {};
    await loadFiles();
    if (payload.path) {
      await selectFile(payload.path);
    }
    ElMessage.success(`上传完成：${payload.path || file.name}`);
    const warnings = Array.isArray(payload.warnings) ? payload.warnings : [];
    if (warnings.length) {
      ElMessage.warning(`转换警告：${warnings.join(' | ')}`);
    }
  } catch (error) {
    const status = error.response?.status;
    if (status === 404) {
      ElMessage.error('上传接口不存在，请更新后端服务并重启。');
      return;
    }
    ElMessage.error(error.response?.data?.detail || `文档上传失败：${error.message || '请求失败'}`);
  }
};

const refreshConfig = async () => {
  try {
    await loadConfig();
    ElMessage.success('知识库配置已刷新。');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '知识库刷新失败');
  }
};

// 首次挂载时初始化高亮层，确保默认空文档也能对齐
onMounted(() => {
  scheduleKnowledgeEditorUpdate();
});

// 组件销毁时清理事件监听，避免内存泄漏
onBeforeUnmount(() => {
  cleanupKnowledgeEditorEvents();
});

// 文档内容变化时刷新高亮层
watch(fileContent, () => {
  scheduleKnowledgeEditorUpdate();
});

// 弹窗首次挂载即为可见时也触发加载，避免首次进入列表为空
watch(
  () => props.active,
  (value) => {
    if (value) {
      scheduleKnowledgeEditorUpdate();
    } else {
      cleanupKnowledgeEditorEvents();
    }
  }
);

watch(
  () => props.visible,
  (value) => {
    if (value && !loaded.value) {
      loadConfig();
    }
    if (value) {
      scheduleKnowledgeEditorUpdate();
    }
    if (!value) {
      cleanupKnowledgeEditorEvents();
      closeKnowledgeModal();
    }
  },
  { immediate: true }
);
</script>
