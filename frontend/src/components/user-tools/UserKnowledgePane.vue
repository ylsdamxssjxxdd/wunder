<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>{{ t('userTools.knowledge.title') }}</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary btn-with-icon" type="button" @click="addBase">
          <i class="fa-solid fa-plus" aria-hidden="true"></i>
          <span>{{ t('knowledge.action.add') }}</span>
        </button>
        <button class="user-tools-btn secondary btn-with-icon" type="button" @click="refreshConfig">
          <i class="fa-solid fa-arrows-rotate" aria-hidden="true"></i>
          <span>{{ t('knowledge.action.refresh') }}</span>
        </button>
        <div v-if="status" class="user-tools-status list-status">{{ status }}</div>
      </div>
    </div>
    <div class="tips">
      {{ t('userTools.knowledge.tip') }}
    </div>

    <div class="management-layout knowledge-layout">
      <div class="management-list">
        <div class="list-header">
          <label>{{ t('knowledge.list.title') }}</label>
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
              <div>{{ base.name || t('knowledge.name.unnamed') }}</div>
              <small>
                {{ base.root || t('userTools.knowledge.root.uncreated') }} ·
                {{
                  normalizeBaseType(base.base_type) === 'vector'
                    ? t('knowledge.type.vector')
                    : t('knowledge.type.literal')
                }}
              </small>
            </button>
          </template>
          <div v-else class="empty-text">{{ t('userTools.knowledge.list.empty') }}</div>
        </div>
      </div>

      <div class="management-detail knowledge-detail">
        <div class="detail-header">
          <div>
            <div class="detail-title">{{ detailTitle }}</div>
            <div class="muted">{{ detailMeta }}</div>
            <div class="muted">{{ detailDesc }}</div>
          </div>
          <div class="detail-actions knowledge-detail-actions">
            <button
              v-if="!isVectorBase"
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase || uploadLoading"
              :title="t('common.upload')"
              :aria-label="t('common.upload')"
              :class="{ 'is-loading': uploadLoading }"
              @click="triggerUpload"
            >
              <i class="fa-solid" :class="uploadIcon" aria-hidden="true"></i>
            </button>
            <button
              v-if="!isVectorBase"
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('common.create')"
              :aria-label="t('common.create')"
              @click="createFile"
            >
              <i class="fa-solid fa-plus" aria-hidden="true"></i>
            </button>
            <button
              v-if="!isVectorBase"
              class="user-tools-btn btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('common.save')"
              :aria-label="t('common.save')"
              @click="saveFile"
            >
              <i class="fa-solid fa-floppy-disk" aria-hidden="true"></i>
            </button>
            <button
              v-if="isVectorBase"
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase || uploadLoading"
              :title="t('common.upload')"
              :aria-label="t('common.upload')"
              :class="{ 'is-loading': uploadLoading }"
              @click="triggerUpload"
            >
              <i class="fa-solid" :class="uploadIcon" aria-hidden="true"></i>
            </button>
            <button
              v-if="isVectorBase"
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('knowledge.action.reindex')"
              :aria-label="t('knowledge.action.reindex')"
              @click="reindexDocs()"
            >
              <i class="fa-solid fa-rotate" aria-hidden="true"></i>
            </button>
            <button
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('knowledge.action.test')"
              :aria-label="t('knowledge.action.test')"
              @click="openTestModal"
            >
              <i class="fa-solid fa-vial" aria-hidden="true"></i>
            </button>
            <button
              class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('common.edit')"
              :aria-label="t('common.edit')"
              @click="editBase"
            >
              <i class="fa-solid fa-pen" aria-hidden="true"></i>
            </button>
            <button
              class="user-tools-btn danger btn-with-icon btn-compact icon-only"
              type="button"
              :disabled="!activeBase"
              :title="t('knowledge.action.delete')"
              :aria-label="t('knowledge.action.delete')"
              @click="deleteBase"
            >
              <i class="fa-solid fa-trash" aria-hidden="true"></i>
            </button>
          </div>
        </div>

        <div class="knowledge-section form-section">
          <div class="knowledge-content">
            <div class="user-tools-card knowledge-files-card">
              <div v-if="!isVectorBase" class="knowledge-file-layout">
                <div class="knowledge-file-pane">
                  <div class="knowledge-file-list">
                    <div v-if="!files.length" class="empty-text">{{ t('knowledge.file.empty') }}</div>
                    <div
                      v-for="filePath in files"
                      :key="filePath"
                      class="knowledge-file-item"
                      :class="{ active: filePath === activeFile }"
                      @click="selectFile(filePath)"
                    >
                      <span class="knowledge-file-name">{{ filePath }}</span>
                    </div>
                  </div>
                </div>
                <div class="knowledge-file-editor">
                  <div class="muted">{{ activeFile || t('knowledge.file.none') }}</div>
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
              <div v-else class="knowledge-vector-layout">
                <div class="knowledge-vector-pane">
                  <div class="knowledge-doc-list">
                    <div v-if="!vectorDocs.length" class="empty-text">
                      {{ t('knowledge.doc.list.empty') }}
                    </div>
                    <div
                      v-for="doc in vectorDocs"
                      :key="doc.doc_id"
                      class="knowledge-doc-item"
                      :class="{ active: doc.doc_id === activeDocId }"
                      @click="selectDoc(doc.doc_id)"
                    >
                      <div class="knowledge-doc-title">{{ doc.name || doc.doc_id }}</div>
                      <div class="knowledge-doc-meta">{{ buildDocMetaText(doc) }}</div>
                    </div>
                  </div>
                </div>
                <div class="knowledge-vector-detail">
                  <div class="knowledge-doc-header">
                    <div>
                      <div class="detail-title">{{ activeDocTitle }}</div>
                      <div class="muted">{{ activeDocMeta }}</div>
                    </div>
                    <div class="detail-actions knowledge-doc-actions">
                      <button
                        class="user-tools-btn secondary btn-with-icon btn-compact icon-only"
                        type="button"
                        :disabled="!canSelectChunks"
                        :title="selectAllLabel"
                        :aria-label="selectAllLabel"
                        @click="toggleSelectAllChunks"
                      >
                        <i class="fa-solid" :class="selectAllIcon" aria-hidden="true"></i>
                      </button>
                      <button
                        class="user-tools-btn btn-with-icon btn-compact icon-only"
                        type="button"
                        :disabled="!canBatchEmbed"
                        :title="embedActionLabel"
                        :aria-label="embedActionLabel"
                        :class="{ 'is-loading': embeddingActive }"
                        @click="embedSelectedChunks"
                      >
                        <i class="fa-solid" :class="embedActionIcon" aria-hidden="true"></i>
                      </button>
                      <button
                        class="user-tools-btn danger btn-with-icon btn-compact icon-only"
                        type="button"
                        :disabled="!canBatchDelete"
                        :title="t('knowledge.doc.action.deleteChunks')"
                        :aria-label="t('knowledge.doc.action.deleteChunks')"
                        @click="deleteSelectedChunks"
                      >
                        <i class="fa-solid fa-trash" aria-hidden="true"></i>
                      </button>
                    </div>
                  </div>
                  <div class="knowledge-vector-content">
                    <div class="knowledge-doc-chunks-pane">
                      <div class="knowledge-doc-section-title">
                        {{ t('knowledge.doc.section.chunks') }}
                      </div>
                      <div class="knowledge-doc-chunk-list">
                        <div v-if="!docChunks.length" class="empty-text">
                          {{ t('knowledge.chunk.empty') }}
                        </div>
                        <div
                          v-for="chunk in docChunks"
                          :key="chunk.index"
                          class="knowledge-doc-chunk-item"
                          :class="{
                            selected: isChunkSelected(chunk.index),
                            embedding: isChunkEmbedding(chunk.index)
                          }"
                          @click="toggleChunkSelection(chunk.index)"
                        >
                          <div class="knowledge-doc-chunk-title-row">
                            <div class="knowledge-doc-chunk-title">
                              <span class="knowledge-doc-chunk-select"></span>
                              <span>#{{ chunk.index }} {{ chunk.start }}-{{ chunk.end }}</span>
                            </div>
                            <span
                              class="knowledge-doc-chunk-status"
                              :class="`status-${resolveChunkStatus(chunk)}`"
                            >
                              {{ formatChunkStatus(chunk) }}
                            </span>
                          </div>
                          <div class="knowledge-doc-chunk-preview">
                            {{ chunk.preview || chunk.content }}
                          </div>
                        </div>
                      </div>
                    </div>
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
          <label>{{ t('knowledge.modal.field.name') }}</label>
          <el-input v-model="knowledgeForm.name" :placeholder="t('knowledge.modal.placeholder.name')" />
        </div>
        <div class="form-row">
          <label>{{ t('knowledge.modal.field.type') }}</label>
          <el-select
            v-model="knowledgeForm.base_type"
            :placeholder="t('knowledge.modal.placeholder.type')"
          >
            <el-option :label="t('knowledge.type.literal')" value="literal" />
            <el-option :label="t('knowledge.type.vector')" value="vector" />
          </el-select>
        </div>
        <div v-if="knowledgeForm.base_type === 'vector'" class="form-row">
          <label>{{ t('knowledge.modal.field.embeddingModel') }}</label>
          <el-select
            v-model="knowledgeForm.embedding_model"
            filterable
            allow-create
            default-first-option
            :placeholder="t('knowledge.modal.placeholder.embeddingModel')"
          >
            <el-option v-for="model in embeddingModels" :key="model" :label="model" :value="model" />
          </el-select>
          <div v-if="!embeddingModels.length" class="muted">
            {{ t('knowledge.embedding.empty') }}
          </div>
        </div>
        <div v-if="knowledgeForm.base_type === 'vector'" class="grid">
          <div class="form-row">
            <label>{{ t('knowledge.modal.field.chunkSize') }}</label>
            <el-input
              v-model="knowledgeForm.chunk_size"
              type="number"
              :placeholder="t('knowledge.modal.placeholder.chunkSize')"
            />
          </div>
          <div class="form-row">
            <label>{{ t('knowledge.modal.field.chunkOverlap') }}</label>
            <el-input
              v-model="knowledgeForm.chunk_overlap"
              type="number"
              :placeholder="t('knowledge.modal.placeholder.chunkOverlap')"
            />
          </div>
        </div>
        <div v-if="knowledgeForm.base_type === 'vector'" class="grid">
          <div class="form-row">
            <label>{{ t('knowledge.modal.field.topK') }}</label>
            <el-input
              v-model="knowledgeForm.top_k"
              type="number"
              :placeholder="t('knowledge.modal.placeholder.topK')"
            />
          </div>
          <div class="form-row">
            <label>{{ t('knowledge.modal.field.scoreThreshold') }}</label>
            <el-input
              v-model="knowledgeForm.score_threshold"
              type="number"
              :placeholder="t('knowledge.modal.placeholder.scoreThreshold')"
            />
          </div>
        </div>
        <div class="form-row">
          <label>{{ t('knowledge.modal.field.description') }}</label>
          <el-input
            v-model="knowledgeForm.description"
            type="textarea"
            :rows="4"
            :placeholder="t('knowledge.modal.placeholder.description')"
          />
        </div>
        <div class="form-row">
          <label class="checkbox-row">
            <input type="checkbox" v-model="knowledgeForm.enabled" />
            <span>{{ t('common.enable') }}</span>
          </label>
        </div>
        <div class="form-row">
          <label class="checkbox-row">
            <input type="checkbox" v-model="knowledgeForm.shared" />
            <span>{{ t('common.share') }}</span>
          </label>
        </div>
        <div v-if="knowledgeForm.base_type === 'vector'" class="muted">
          {{ t('knowledge.modal.tip.vectorRoot') }}
        </div>
        <div v-else class="muted">
          {{ t('userTools.knowledge.modal.tip') }}
        </div>
      </div>

      <template #footer>
        <button class="user-tools-btn secondary" type="button" @click="closeKnowledgeModal">
          {{ t('common.cancel') }}
        </button>
        <button class="user-tools-btn" type="button" @click="applyKnowledgeModal">
          {{ t('common.save') }}
        </button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="knowledgeTestVisible"
      class="user-tools-dialog user-tools-subdialog knowledge-test-dialog"
      width="920px"
      top="8vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ t('knowledge.test.title') }}</div>
          <button class="icon-btn" type="button" @click="closeTestModal">×</button>
        </div>
      </template>
      <div class="knowledge-test-layout">
        <div class="knowledge-test-input">
          <label>{{ t('knowledge.test.question') }}</label>
          <el-input
            v-model="knowledgeTestQuery"
            type="textarea"
            :rows="6"
            :placeholder="t('knowledge.test.question.placeholder')"
          />
          <div class="knowledge-test-actions">
            <button
              class="user-tools-btn btn-with-icon"
              type="button"
              :disabled="knowledgeTestLoading"
              :class="{ 'is-loading': knowledgeTestLoading }"
              @click="runKnowledgeTest"
            >
              <i class="fa-solid" :class="knowledgeTestRunIcon" aria-hidden="true"></i>
              <span>{{ knowledgeTestRunLabel }}</span>
            </button>
            <div class="muted">{{ knowledgeTestStatus }}</div>
          </div>
        </div>
        <div class="knowledge-test-results">
          <div class="knowledge-doc-section-title">{{ t('knowledge.test.result.title') }}</div>
          <div class="knowledge-test-result-list">
            <div v-if="knowledgeTestResultMessage" class="empty-text">
              {{ knowledgeTestResultMessage }}
            </div>
            <template v-else>
              <div
                v-for="(hit, index) in knowledgeTestResults"
                :key="`${hit.doc_id || hit.document}-${hit.chunk_index}-${index}`"
                class="knowledge-test-result-item"
              >
                <div class="knowledge-test-result-header">
                  {{
                    `${index + 1}. ${hit.document || hit.doc_id || t('knowledge.doc.unnamed')} #${
                      hit.chunk_index ?? '-'
                    } · ${formatTestScore(Number(hit.score))}`
                  }}
                </div>
                <div class="knowledge-test-result-content">{{ hit.content || '' }}</div>
              </div>
              <div v-if="knowledgeTestText" class="knowledge-test-result-item">
                <div class="knowledge-test-result-header">
                  {{ t('knowledge.test.output.title') }}
                </div>
                <div class="knowledge-test-result-content">{{ knowledgeTestText }}</div>
              </div>
            </template>
          </div>
        </div>
      </div>
      <template #footer>
        <button class="user-tools-btn secondary" type="button" @click="closeTestModal">
          {{ t('common.close') }}
        </button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  fetchUserKnowledgeConfig,
  fetchUserKnowledgeDocs,
  fetchUserKnowledgeChunks,
  fetchUserKnowledgeFile,
  fetchUserKnowledgeFiles,
  embedUserKnowledgeChunk,
  deleteUserKnowledgeChunk,
  reindexUserKnowledge,
  saveUserKnowledgeConfig,
  saveUserKnowledgeFile,
  testUserKnowledge,
  uploadUserKnowledgeFile
} from '@/api/userTools';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  active: {
    type: Boolean,
    default: false
  },
  status: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['status']);
const { t } = useI18n();

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
  '.pdf',
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

const normalizeBaseType = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) {
    return 'literal';
  }
  if (raw === 'vector' || raw === 'embedding') {
    return 'vector';
  }
  return 'literal';
};

const parseOptionalInt = (value) => {
  if (value === null || value === undefined || value === '') {
    return null;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : null;
};

const parseOptionalFloat = (value) => {
  if (value === null || value === undefined || value === '') {
    return null;
  }
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const bases = ref([]);
const selectedIndex = ref(-1);
const files = ref([]);
const activeFile = ref('');
const fileContent = ref('');
const vectorDocs = ref([]);
const activeDocId = ref('');
const docMeta = ref(null);
const docChunks = ref([]);
const selectedChunkIndices = ref(new Set());
const embeddingChunkIndices = ref(new Set());
const embeddingModels = ref([]);
const uploadLoading = ref(false);
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
const knowledgeTestVisible = ref(false);
const knowledgeTestQuery = ref('');
const knowledgeTestStatus = ref('');
const knowledgeTestResults = ref([]);
const knowledgeTestText = ref('');
const knowledgeTestLoading = ref(false);
const knowledgeForm = reactive({
  name: '',
  description: '',
  enabled: true,
  shared: false,
  base_type: 'literal',
  embedding_model: '',
  chunk_size: '',
  chunk_overlap: '',
  top_k: '',
  score_threshold: ''
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

const DOC_STATUS_KEYS = {
  ready: 'knowledge.doc.status.ready',
  indexing: 'knowledge.doc.status.indexing',
  pending: 'knowledge.doc.status.pending',
  failed: 'knowledge.doc.status.failed'
};

const CHUNK_STATUS_KEYS = {
  embedded: 'knowledge.chunk.status.embedded',
  pending: 'knowledge.chunk.status.pending',
  deleted: 'knowledge.chunk.status.deleted',
  failed: 'knowledge.chunk.status.failed',
  embedding: 'knowledge.chunk.status.embedding'
};

const formatDocStatus = (status) => {
  const normalized = String(status || '').trim().toLowerCase();
  if (!normalized) return '';
  const key = DOC_STATUS_KEYS[normalized];
  return key ? t(key) : normalized;
};

const formatDocUpdatedAt = (timestamp) => {
  if (!Number.isFinite(timestamp)) {
    return '';
  }
  const date = new Date(timestamp * 1000);
  if (Number.isNaN(date.getTime())) {
    return '';
  }
  return date.toLocaleString();
};

const buildDocMetaText = (meta) => {
  if (!meta) return '';
  const parts = [];
  if (meta.embedding_model) {
    parts.push(t('knowledge.doc.meta.embedding', { name: meta.embedding_model }));
  }
  if (Number.isFinite(meta.chunk_count)) {
    parts.push(t('knowledge.doc.meta.chunks', { count: meta.chunk_count }));
  }
  const updated = formatDocUpdatedAt(meta.updated_at);
  if (updated) {
    parts.push(t('knowledge.doc.meta.updated', { time: updated }));
  }
  const status = formatDocStatus(meta.status);
  if (status) {
    parts.push(status);
  }
  return parts.join(' · ');
};

const resolveChunkStatus = (chunk) => {
  if (embeddingChunkIndices.value.has(chunk?.index)) {
    return 'embedding';
  }
  const raw = String(chunk?.status || '').trim().toLowerCase();
  return raw || 'pending';
};

const formatChunkStatus = (chunk) => {
  const status = resolveChunkStatus(chunk);
  if (!status) {
    return '-';
  }
  const key = CHUNK_STATUS_KEYS[status];
  return key ? t(key) : status;
};

const isChunkSelected = (index) => selectedChunkIndices.value.has(index);
const isChunkEmbedding = (index) => embeddingChunkIndices.value.has(index);
const getSelectedChunkIndices = () => Array.from(selectedChunkIndices.value);

const activeBase = computed(() => bases.value[selectedIndex.value] || null);
const isVectorBase = computed(
  () => normalizeBaseType(activeBase.value?.base_type) === 'vector'
);
const detailTitle = computed(() => activeBase.value?.name || t('knowledge.detail.empty'));
const detailMeta = computed(() => {
  if (!activeBase.value) {
    return '';
  }
  const root = activeBase.value.root || t('userTools.knowledge.root.uncreated');
  const parts = [
    root,
    activeBase.value.enabled !== false
      ? t('knowledge.status.enabled')
      : t('knowledge.status.disabled')
  ];
  parts.push(isVectorBase.value ? t('knowledge.type.vector') : t('knowledge.type.literal'));
  if (isVectorBase.value && activeBase.value.embedding_model) {
    parts.push(
      t('knowledge.doc.meta.embedding', { name: activeBase.value.embedding_model })
    );
  }
  if (activeBase.value.shared) {
    parts.push(t('common.shared'));
  }
  return parts.join(' · ');
});
const detailDesc = computed(() => activeBase.value?.description || '');
const knowledgeModalTitle = computed(() =>
  knowledgeEditingIndex.value >= 0 ? t('knowledge.modal.editTitle') : t('knowledge.modal.addTitle')
);
const activeDocTitle = computed(
  () => docMeta.value?.name || activeDocId.value || t('knowledge.doc.none')
);
const activeDocMeta = computed(() => buildDocMetaText(docMeta.value));
const selectedChunkCount = computed(() => selectedChunkIndices.value.size);
const embeddingActive = computed(() => embeddingChunkIndices.value.size > 0);
const canSelectChunks = computed(
  () => Boolean(activeDocId.value) && docChunks.value.length > 0
);
const allChunksSelected = computed(
  () => canSelectChunks.value && selectedChunkIndices.value.size === docChunks.value.length
);
const selectAllLabel = computed(() =>
  allChunksSelected.value
    ? t('knowledge.chunk.action.clearSelection')
    : t('knowledge.chunk.action.selectAll')
);
const selectAllIcon = computed(() => (allChunksSelected.value ? 'fa-square-minus' : 'fa-square-check'));
const embedActionLabel = computed(() =>
  embeddingActive.value ? t('knowledge.chunk.action.embedding') : t('knowledge.doc.action.embed')
);
const embedActionIcon = computed(() => (embeddingActive.value ? 'fa-spinner' : 'fa-cube'));
const uploadIcon = computed(() => (uploadLoading.value ? 'fa-spinner' : 'fa-upload'));
const canBatchEmbed = computed(
  () => canSelectChunks.value && selectedChunkCount.value > 0 && !embeddingActive.value
);
const canBatchDelete = computed(
  () => canSelectChunks.value && selectedChunkCount.value > 0 && !embeddingActive.value
);
const knowledgeTestRunLabel = computed(() =>
  knowledgeTestLoading.value ? t('knowledge.test.running') : t('knowledge.test.action.run')
);
const knowledgeTestRunIcon = computed(() =>
  knowledgeTestLoading.value ? 'fa-spinner' : 'fa-play'
);
const knowledgeTestResultMessage = computed(() => {
  if (knowledgeTestLoading.value) {
    return t('common.loading');
  }
  if (knowledgeTestResults.value.length || knowledgeTestText.value) {
    return '';
  }
  return t('knowledge.test.empty');
});
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
            shared: Boolean(base.shared),
            base_type: normalizeBaseType(base.base_type),
            embedding_model: base.embedding_model || '',
            chunk_size: parseOptionalInt(base.chunk_size),
            chunk_overlap: parseOptionalInt(base.chunk_overlap),
            top_k: parseOptionalInt(base.top_k),
            score_threshold: parseOptionalFloat(base.score_threshold)
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
      shared: base.shared === true,
      base_type: normalizeBaseType(base.base_type),
      embedding_model: base.embedding_model || '',
      chunk_size: base.chunk_size ?? null,
      chunk_overlap: base.chunk_overlap ?? null,
      top_k: base.top_k ?? null,
      score_threshold: base.score_threshold ?? null
    }))
    .filter((base) => base.name)
});

const validateConfigPayload = (payload) => {
  const invalid = payload.bases.filter((base) => !base.name);
  if (invalid.length) {
    return t('knowledge.payload.invalid');
  }
  for (const base of payload.bases) {
    if (normalizeBaseType(base.base_type) === 'vector' && !base.embedding_model) {
      return t('knowledge.embedding.required');
    }
  }
  const nameSet = new Set();
  for (const base of payload.bases) {
    if (nameSet.has(base.name)) {
      return t('knowledge.name.duplicate', { name: base.name });
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
  fileContent: fileContent.value,
  vectorDocs: [...vectorDocs.value],
  activeDocId: activeDocId.value,
  docMeta: docMeta.value ? { ...docMeta.value } : null,
  docChunks: [...docChunks.value],
  selectedChunkIndices: getSelectedChunkIndices(),
  embeddingChunkIndices: Array.from(embeddingChunkIndices.value),
  embeddingModels: [...embeddingModels.value]
});

const restoreKnowledgeSnapshot = (snapshot) => {
  bases.value = snapshot.bases;
  selectedIndex.value = snapshot.selectedIndex;
  files.value = snapshot.files;
  activeFile.value = snapshot.activeFile;
  fileContent.value = snapshot.fileContent;
  vectorDocs.value = snapshot.vectorDocs;
  activeDocId.value = snapshot.activeDocId;
  docMeta.value = snapshot.docMeta;
  docChunks.value = snapshot.docChunks;
  selectedChunkIndices.value = new Set(snapshot.selectedChunkIndices || []);
  embeddingChunkIndices.value = new Set(snapshot.embeddingChunkIndices || []);
  embeddingModels.value = snapshot.embeddingModels;
};

const loadConfig = async () => {
  if (loading.value) return;
  loading.value = true;
  try {
    const { data } = await fetchUserKnowledgeConfig();
    const payload = data?.data || {};
    const normalized = normalizeKnowledgeConfig(payload.knowledge || {});
    bases.value = normalized.bases;
    embeddingModels.value = Array.isArray(payload.embedding_models)
      ? payload.embedding_models
      : [];
    selectedIndex.value = bases.value.length ? 0 : -1;
    files.value = [];
    activeFile.value = '';
    fileContent.value = '';
    vectorDocs.value = [];
    activeDocId.value = '';
    docMeta.value = null;
    docChunks.value = [];
    selectedChunkIndices.value = new Set();
    embeddingChunkIndices.value = new Set();
    loaded.value = true;
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
  } catch (error) {
    showApiError(error, t('knowledge.refreshFailed', { message: error.message || '' }));
  } finally {
    loading.value = false;
  }
};

// 统一保存知识库配置，并以服务端结果回写
const saveConfig = async (preferredName = '') => {
  emitStatus(t('common.saving'));
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
    vectorDocs.value = [];
    activeDocId.value = '';
    docMeta.value = null;
    docChunks.value = [];
    selectedChunkIndices.value = new Set();
    embeddingChunkIndices.value = new Set();
    emitStatus(t('common.saved'));
    return normalized;
  } catch (error) {
    emitStatus(t('knowledge.saveFailed', { message: error.message || t('common.requestFailed') }));
    throw error;
  }
};

const selectBase = async (index) => {
  selectedIndex.value = index;
  files.value = [];
  activeFile.value = '';
  fileContent.value = '';
  vectorDocs.value = [];
  activeDocId.value = '';
  docMeta.value = null;
  docChunks.value = [];
  selectedChunkIndices.value = new Set();
  embeddingChunkIndices.value = new Set();
  await loadFiles();
};

const resetKnowledgeForm = () => {
  knowledgeForm.name = '';
  knowledgeForm.description = '';
  knowledgeForm.enabled = true;
  knowledgeForm.shared = false;
  knowledgeForm.base_type = 'literal';
  knowledgeForm.embedding_model = '';
  knowledgeForm.chunk_size = '';
  knowledgeForm.chunk_overlap = '';
  knowledgeForm.top_k = '';
  knowledgeForm.score_threshold = '';
};

// 打开知识库配置弹窗
const openKnowledgeModal = (base = null, index = -1) => {
  knowledgeEditingIndex.value = Number.isInteger(index) ? index : -1;
  knowledgeForm.name = base?.name || '';
  knowledgeForm.description = base?.description || '';
  knowledgeForm.enabled = base?.enabled !== false;
  knowledgeForm.shared = base?.shared === true;
  knowledgeForm.base_type = normalizeBaseType(base?.base_type);
  knowledgeForm.embedding_model = base?.embedding_model || '';
  knowledgeForm.chunk_size =
    base?.chunk_size !== null && base?.chunk_size !== undefined ? base.chunk_size : '';
  knowledgeForm.chunk_overlap =
    base?.chunk_overlap !== null && base?.chunk_overlap !== undefined ? base.chunk_overlap : '';
  knowledgeForm.top_k =
    base?.top_k !== null && base?.top_k !== undefined ? base.top_k : '';
  knowledgeForm.score_threshold =
    base?.score_threshold !== null && base?.score_threshold !== undefined
      ? base.score_threshold
      : '';
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
    return t('knowledge.name.required');
  }
  if (normalizeBaseType(payload.base_type) === 'vector' && !payload.embedding_model) {
    return t('knowledge.embedding.required');
  }
  for (let i = 0; i < bases.value.length; i += 1) {
    if (i === index) {
      continue;
    }
    if (bases.value[i].name.trim() === payload.name) {
      return t('knowledge.name.duplicate', { name: payload.name });
    }
  }
  return '';
};

const getKnowledgeFormPayload = () => {
  const baseType = normalizeBaseType(knowledgeForm.base_type);
  const isVector = baseType === 'vector';
  return {
    name: knowledgeForm.name.trim(),
    description: knowledgeForm.description.trim(),
    enabled: knowledgeForm.enabled !== false,
    shared: knowledgeForm.shared === true,
    base_type: baseType,
    embedding_model: isVector ? knowledgeForm.embedding_model.trim() : '',
    chunk_size: isVector ? parseOptionalInt(knowledgeForm.chunk_size) : null,
    chunk_overlap: isVector ? parseOptionalInt(knowledgeForm.chunk_overlap) : null,
    top_k: isVector ? parseOptionalInt(knowledgeForm.top_k) : null,
    score_threshold: isVector ? parseOptionalFloat(knowledgeForm.score_threshold) : null
  };
};

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
  vectorDocs.value = [];
  activeDocId.value = '';
  docMeta.value = null;
  docChunks.value = [];
  selectedChunkIndices.value = new Set();
  embeddingChunkIndices.value = new Set();
  try {
    await saveConfig(payload.name);
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
    ElMessage.success(
      editing >= 0 ? t('knowledge.base.updated') : t('knowledge.base.added')
    );
    closeKnowledgeModal();
  } catch (error) {
    restoreKnowledgeSnapshot(snapshot);
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.saveFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const addBase = () => {
  openKnowledgeModal();
};

const editBase = () => {
  const base = activeBase.value;
  if (!base) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  openKnowledgeModal(base, selectedIndex.value);
};

const deleteBase = async () => {
  const base = activeBase.value;
  if (!base) return;
  try {
    const name = base.name || t('knowledge.name.unnamed');
    await ElMessageBox.confirm(t('knowledge.base.deleteConfirm', { name }), t('common.notice'), {
      confirmButtonText: t('common.delete'),
      cancelButtonText: t('common.cancel'),
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
  vectorDocs.value = [];
  activeDocId.value = '';
  docMeta.value = null;
  docChunks.value = [];
  selectedChunkIndices.value = new Set();
  embeddingChunkIndices.value = new Set();
  try {
    const preferredName = bases.value[selectedIndex.value]?.name || '';
    await saveConfig(preferredName);
    if (selectedIndex.value >= 0) {
      await loadFiles();
    }
    ElMessage.success(t('knowledge.base.deleted'));
  } catch (error) {
    restoreKnowledgeSnapshot(snapshot);
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.deleteFailed', { message: error.message || t('common.requestFailed') })
    );
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
  if (normalizeBaseType(base.base_type) === 'vector') {
    files.value = [];
    activeFile.value = '';
    fileContent.value = '';
    await loadVectorDocs();
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
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.file.listFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const resetDocState = () => {
  vectorDocs.value = [];
  activeDocId.value = '';
  docMeta.value = null;
  docChunks.value = [];
  selectedChunkIndices.value = new Set();
  embeddingChunkIndices.value = new Set();
};

const loadVectorDocs = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    resetDocState();
    return;
  }
  try {
    const { data } = await fetchUserKnowledgeDocs(base.name);
    const payload = data?.data || {};
    vectorDocs.value = Array.isArray(payload.docs) ? payload.docs : [];
    if (activeDocId.value) {
      const matched = vectorDocs.value.find((doc) => doc.doc_id === activeDocId.value) || null;
      if (!matched) {
        activeDocId.value = '';
        docMeta.value = null;
        docChunks.value = [];
        selectedChunkIndices.value = new Set();
        embeddingChunkIndices.value = new Set();
      } else {
        docMeta.value = matched;
      }
    }
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.doc.loadFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const selectDoc = async (docId, options = {}) => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (!docId) {
    ElMessage.warning(t('knowledge.doc.selectRequired'));
    return;
  }
  const keepSelection = options.keepSelection === true;
  const previousSelection = keepSelection ? new Set(selectedChunkIndices.value) : new Set();
  try {
    const chunkRes = await fetchUserKnowledgeChunks(base.name, docId);
    const chunkPayload = chunkRes?.data?.data || {};
    activeDocId.value = docId;
    docMeta.value =
      vectorDocs.value.find((doc) => doc.doc_id === docId) || { doc_id: docId, name: docId };
    docChunks.value = Array.isArray(chunkPayload.chunks) ? chunkPayload.chunks : [];
    selectedChunkIndices.value = previousSelection;
    embeddingChunkIndices.value = new Set();
    syncChunkSelection();
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.doc.loadFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const reindexDocs = async (docId) => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  try {
    const payload = { base: base.name };
    if (docId) {
      payload.doc_id = docId;
    }
    const { data } = await reindexUserKnowledge(payload);
    const result = data?.data || {};
    if (result.ok === false) {
      ElMessage.error(t('knowledge.doc.reindexFailed', { message: t('common.notice') }));
    } else {
      ElMessage.success(t('knowledge.doc.reindexSuccess'));
    }
    await loadVectorDocs();
    if (docId) {
      await selectDoc(docId);
    }
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.doc.reindexFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const syncChunkSelection = () => {
  if (!docChunks.value.length) {
    selectedChunkIndices.value = new Set();
    return;
  }
  const available = new Set(docChunks.value.map((chunk) => chunk.index));
  const next = new Set();
  selectedChunkIndices.value.forEach((index) => {
    if (available.has(index)) {
      next.add(index);
    }
  });
  selectedChunkIndices.value = next;
};

const toggleChunkSelection = (index) => {
  const next = new Set(selectedChunkIndices.value);
  if (next.has(index)) {
    next.delete(index);
  } else {
    next.add(index);
  }
  selectedChunkIndices.value = next;
};

const toggleSelectAllChunks = () => {
  if (!docChunks.value.length) {
    return;
  }
  if (selectedChunkIndices.value.size === docChunks.value.length) {
    selectedChunkIndices.value = new Set();
  } else {
    selectedChunkIndices.value = new Set(docChunks.value.map((chunk) => chunk.index));
  }
};

const refreshActiveDoc = async () => {
  if (!activeDocId.value) {
    return;
  }
  await selectDoc(activeDocId.value, { keepSelection: true });
  await loadVectorDocs();
};

const embedSelectedChunks = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (!activeDocId.value) {
    ElMessage.warning(t('knowledge.doc.selectRequired'));
    return;
  }
  const selected = getSelectedChunkIndices();
  if (!selected.length) {
    ElMessage.warning(t('knowledge.chunk.selectRequired'));
    return;
  }
  const pending = selected.filter((index) => {
    const chunk = docChunks.value.find((item) => item.index === index);
    return chunk && resolveChunkStatus(chunk) !== 'embedded';
  });
  if (!pending.length) {
    ElMessage.info(t('knowledge.chunk.embedSkipped'));
    return;
  }
  embeddingChunkIndices.value = new Set(pending);
  let succeeded = 0;
  let failed = 0;
  for (const index of pending) {
    try {
      await embedUserKnowledgeChunk({
        base: base.name,
        doc_id: activeDocId.value,
        chunk_index: index
      });
      succeeded += 1;
      const localChunk = docChunks.value.find((item) => item.index === index);
      if (localChunk) {
        localChunk.status = 'embedded';
      }
    } catch (error) {
      failed += 1;
    } finally {
      const next = new Set(embeddingChunkIndices.value);
      next.delete(index);
      embeddingChunkIndices.value = next;
      await nextTick();
    }
  }
  await refreshActiveDoc();
  if (succeeded) {
    ElMessage.success(t('knowledge.chunk.embedBatchSuccess', { count: succeeded }));
  }
  if (failed) {
    ElMessage.error(t('knowledge.chunk.embedBatchFailed', { count: failed }));
  }
};

const deleteSelectedChunks = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (!activeDocId.value) {
    ElMessage.warning(t('knowledge.doc.selectRequired'));
    return;
  }
  const selected = getSelectedChunkIndices();
  if (!selected.length) {
    ElMessage.warning(t('knowledge.chunk.selectRequired'));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t('knowledge.chunk.deleteBatchConfirm', { count: selected.length }),
      t('common.notice'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
      type: 'warning'
      }
    );
  } catch (error) {
    return;
  }
  let succeeded = 0;
  let failed = 0;
  for (const index of selected) {
    try {
      await deleteUserKnowledgeChunk({
        base: base.name,
        doc_id: activeDocId.value,
        chunk_index: index
      });
      succeeded += 1;
    } catch (error) {
      failed += 1;
    }
  }
  selectedChunkIndices.value = new Set();
  await refreshActiveDoc();
  if (succeeded) {
    ElMessage.success(t('knowledge.chunk.deleteBatchSuccess', { count: succeeded }));
  }
  if (failed) {
    ElMessage.error(t('knowledge.chunk.deleteBatchFailed', { count: failed }));
  }
};

const openTestModal = () => {
  if (!activeBase.value?.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  knowledgeTestQuery.value = '';
  knowledgeTestStatus.value = '';
  knowledgeTestResults.value = [];
  knowledgeTestText.value = '';
  knowledgeTestVisible.value = true;
};

const closeTestModal = () => {
  knowledgeTestVisible.value = false;
  knowledgeTestLoading.value = false;
};

const formatTestScore = (score) => {
  if (!Number.isFinite(score)) {
    return '-';
  }
  return score.toFixed(3);
};

const runKnowledgeTest = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  const query = knowledgeTestQuery.value.trim();
  if (!query) {
    ElMessage.warning(t('knowledge.test.queryRequired'));
    return;
  }
  knowledgeTestLoading.value = true;
  knowledgeTestStatus.value = t('knowledge.test.running');
  knowledgeTestResults.value = [];
  knowledgeTestText.value = '';
  try {
    const { data } = await testUserKnowledge({ base: base.name, query });
    const payload = data?.data || {};
    const hits = Array.isArray(payload.hits) ? payload.hits : [];
    knowledgeTestResults.value = hits;
    knowledgeTestText.value = payload.text || '';
    knowledgeTestStatus.value = t('knowledge.test.done');
  } catch (error) {
    const message =
      error.response?.data?.detail?.message || error.message || t('common.requestFailed');
    knowledgeTestStatus.value = t('knowledge.test.failed', { message });
  } finally {
    knowledgeTestLoading.value = false;
  }
};

const selectFile = async (filePath) => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (normalizeBaseType(base.base_type) === 'vector') {
    ElMessage.warning(t('knowledge.vector.readonly'));
    return;
  }
  try {
    const { data } = await fetchUserKnowledgeFile(base.name, filePath);
    const payload = data?.data || {};
    activeFile.value = payload.path || filePath;
    fileContent.value = payload.content || '';
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.file.readFailed', {
          status: error.response?.status || t('common.unknown')
        })
    );
  }
};

const saveFile = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (normalizeBaseType(base.base_type) === 'vector') {
    ElMessage.warning(t('knowledge.vector.readonly'));
    return;
  }
  if (!activeFile.value) {
    ElMessage.warning(t('knowledge.file.saveRequired'));
    return;
  }
  try {
    await saveUserKnowledgeFile({
      base: base.name,
      path: activeFile.value,
      content: fileContent.value
    });
    await loadFiles();
    ElMessage.success(t('knowledge.file.saved'));
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('knowledge.file.saveFailed', { message: error.message || t('common.requestFailed') })
    );
  }
};

const createFile = async () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  if (normalizeBaseType(base.base_type) === 'vector') {
    ElMessage.warning(t('knowledge.vector.readonly'));
    return;
  }
  let filename = '';
  try {
    const { value } = await ElMessageBox.prompt(
      t('knowledge.file.newPrompt'),
      t('knowledge.file.newTitle'),
      {
        confirmButtonText: t('common.create'),
        cancelButtonText: t('common.cancel'),
      inputValue: 'example.md'
      }
    );
    filename = value || '';
  } catch (error) {
    return;
  }
  const trimmed = filename.trim();
  if (!trimmed) {
    ElMessage.warning(t('userTools.knowledge.file.nameRequired'));
    return;
  }
  if (!trimmed.toLowerCase().endsWith('.md')) {
    ElMessage.warning(t('userTools.knowledge.file.mdOnly'));
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

const resolveUploadErrorMessage = (error) => {
  const payload = error.response?.data;
  const detail = payload?.detail;
  let message = '';
  if (typeof detail === 'string') {
    message = detail.trim();
  } else if (detail && typeof detail.message === 'string') {
    message = detail.message.trim();
  } else if (typeof payload?.message === 'string') {
    message = payload.message.trim();
  } else if (typeof error.message === 'string') {
    message = error.message.trim();
  }
  const status = error.response?.status;
  if (status) {
    const fallback = t('common.requestFailed');
    return message ? `${message} (${status})` : `${fallback} (${status})`;
  }
  return message || t('common.requestFailed');
};

const triggerUpload = () => {
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
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
    ElMessage.warning(t('knowledge.file.extensionMissing'));
    return;
  }
  if (!USER_KNOWLEDGE_UPLOAD_EXTENSIONS.includes(extension)) {
    ElMessage.warning(t('knowledge.file.unsupported', { extension }));
    return;
  }
  const base = activeBase.value;
  if (!base || !base.name) {
    ElMessage.warning(t('knowledge.base.selectRequired'));
    return;
  }
  uploadLoading.value = true;
  try {
    const { data } = await uploadUserKnowledgeFile(base.name, file);
    const payload = data?.data || {};
    if (normalizeBaseType(base.base_type) === 'vector') {
      await loadVectorDocs();
      if (payload.doc_id) {
        await selectDoc(payload.doc_id);
      }
      ElMessage.success(t('knowledge.doc.uploaded', { name: payload.doc_name || file.name }));
    } else {
      await loadFiles();
      if (payload.path) {
        await selectFile(payload.path);
      }
      ElMessage.success(t('knowledge.file.uploaded', { name: payload.path || file.name }));
    }
    const warnings = Array.isArray(payload.warnings) ? payload.warnings : [];
    if (warnings.length) {
      ElMessage.warning(t('knowledge.file.warnings', { message: warnings.join(' | ') }));
    }
  } catch (error) {
    const status = error.response?.status;
    if (status === 404) {
      ElMessage.error(t('knowledge.file.uploadEndpointMissing'));
      return;
    }
    ElMessage.error(resolveUploadErrorMessage(error));
  } finally {
    uploadLoading.value = false;
  }
};

const refreshConfig = async () => {
  try {
    await loadConfig();
    ElMessage.success(t('userTools.knowledge.refresh.success'));
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail ||
        t('userTools.knowledge.refresh.failed', {
          message: error.message || t('common.requestFailed')
        })
    );
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
  if (!isVectorBase.value) {
    scheduleKnowledgeEditorUpdate();
  }
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
      closeTestModal();
    }
  },
  { immediate: true }
);

watch(
  isVectorBase,
  (value) => {
    if (value) {
      cleanupKnowledgeEditorEvents();
    } else {
      scheduleKnowledgeEditorUpdate();
    }
  },
  { immediate: false }
);

watch(
  () => knowledgeForm.base_type,
  (value) => {
    const type = normalizeBaseType(value);
    if (type !== 'vector') {
      knowledgeForm.embedding_model = '';
      return;
    }
    if (!knowledgeForm.embedding_model && embeddingModels.value.length) {
      knowledgeForm.embedding_model = embeddingModels.value[0];
    }
  }
);

</script>