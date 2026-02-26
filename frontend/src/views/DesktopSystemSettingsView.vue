<template>
  <div class="portal-shell desktop-system-shell">
    <UserTopbar
      :title="t('desktop.system.title')"
      :subtitle="t('desktop.system.subtitle')"
      :hide-chat="true"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section">
            <div class="desktop-system-layout" v-loading="loading">
              <aside class="desktop-system-sidebar">
                <div class="desktop-system-sidebar-title">{{ t('desktop.system.title') }}</div>
                <div class="desktop-system-sidebar-nav">
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'model' }"
                    @click="setSection('model')"
                  >
                    <i class="fa-solid fa-robot" aria-hidden="true"></i>
                    <span>{{ t('desktop.system.llm') }}</span>
                  </button>
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'containers' }"
                    @click="setSection('containers')"
                  >
                    <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
                    <span>{{ t('desktop.settings.containers') }}</span>
                  </button>
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'remote' }"
                    @click="setSection('remote')"
                  >
                    <i class="fa-solid fa-link" aria-hidden="true"></i>
                    <span>{{ t('desktop.system.remote.title') }}</span>
                  </button>
                </div>
                <div class="desktop-system-sidebar-foot">
                  <p>{{ currentSection.description }}</p>
                </div>
              </aside>

              <section class="desktop-system-content">
                <header class="desktop-system-header">
                  <div class="desktop-system-header-meta">
                    <h3>{{ currentSection.title }}</h3>
                    <p>{{ currentSection.description }}</p>
                  </div>
                  <div class="desktop-system-header-actions">
                    <template v-if="activeSection === 'model'">
                      <el-button type="primary" plain @click="addModel">
                        {{ t('desktop.system.modelAdd') }}
                      </el-button>
                      <el-button type="primary" :loading="savingModel" @click="saveModelSettings">
                        {{ t('desktop.common.save') }}
                      </el-button>
                    </template>
                    <template v-else-if="activeSection === 'containers'">
                      <el-button type="primary" plain @click="addContainer">
                        {{ t('desktop.containers.add') }}
                      </el-button>
                      <el-button type="primary" :loading="savingContainers" @click="saveContainerSettings">
                        {{ t('desktop.common.save') }}
                      </el-button>
                    </template>
                  </div>
                </header>

                <div v-show="activeSection === 'model'" class="desktop-system-panel">
                  <el-card>
                    <el-form label-position="top" class="desktop-form-grid">
                      <el-form-item :label="t('desktop.system.language')">
                        <el-select v-model="language" class="desktop-full-width">
                          <el-option
                            v-for="item in supportedLanguages"
                            :key="item"
                            :label="getLanguageLabel(item)"
                            :value="item"
                          />
                        </el-select>
                      </el-form-item>
                      <el-form-item :label="t('desktop.system.defaultModel')">
                        <el-select v-model="defaultModel" class="desktop-full-width" filterable allow-create>
                          <el-option
                            v-for="item in modelRows"
                            :key="item.key"
                            :label="item.key || t('desktop.system.modelUnnamed')"
                            :value="item.key"
                          />
                        </el-select>
                      </el-form-item>
                    </el-form>
                  </el-card>

                  <el-card>
                    <el-table :data="modelRows" border>
                      <el-table-column :label="t('desktop.system.modelKey')" width="200">
                        <template #default="{ row }">
                          <el-input v-model="row.key" />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.baseUrl')" min-width="260">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.base_url"
                            :placeholder="t('desktop.system.baseUrlPlaceholder')"
                          />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.apiKey')" min-width="220">
                        <template #default="{ row }">
                          <el-input v-model="row.api_key" show-password />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.modelName')" min-width="220">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.model"
                            :placeholder="t('desktop.system.modelNamePlaceholder')"
                          />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.common.actions')" width="120" align="center">
                        <template #default="{ row }">
                          <el-button link type="danger" @click="removeModel(row)">
                            {{ t('desktop.common.remove') }}
                          </el-button>
                        </template>
                      </el-table-column>
                    </el-table>
                    <p class="desktop-settings-hint">{{ t('desktop.system.llmHint') }}</p>
                  </el-card>
                </div>

                <div v-show="activeSection === 'containers'" class="desktop-system-panel">
                  <el-card>
                    <el-form label-position="top">
                      <el-form-item :label="t('desktop.containers.defaultWorkspace')">
                        <el-input
                          v-model="workspaceRoot"
                          :placeholder="t('desktop.containers.pathPlaceholder')"
                        >
                          <template #append>
                            <el-button
                              :disabled="!canBrowseLocalPaths"
                              @click="openPathPickerForWorkspace"
                            >
                              {{ t('desktop.common.browse') }}
                            </el-button>
                          </template>
                        </el-input>
                        <p class="desktop-settings-hint">{{ t('desktop.containers.defaultHint') }}</p>
                      </el-form-item>
                    </el-form>

                    <el-table :data="containerRows" border>
                      <el-table-column
                        prop="container_id"
                        :label="t('desktop.containers.id')"
                        width="120"
                      />
                      <el-table-column :label="t('desktop.containers.path')">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.root"
                            :placeholder="t('desktop.containers.pathPlaceholder')"
                            @input="syncWorkspaceFromContainer(row)"
                          >
                            <template #append>
                              <el-button
                                :disabled="!canBrowseLocalPaths"
                                @click="openPathPickerForContainer(row.container_id, row.root)"
                              >
                                {{ t('desktop.common.browse') }}
                              </el-button>
                            </template>
                          </el-input>
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.seed.cloudWorkspaceId')" min-width="220">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.cloud_workspace_id"
                            :placeholder="t('desktop.seed.cloudWorkspacePlaceholder')"
                          />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.seed.syncStatus')" min-width="280">
                        <template #default="{ row }">
                          <div class="desktop-seed-cell">
                            <el-tag size="small" :type="seedStatusTagType(row.seed_status)">
                              {{ seedStatusLabel(row.seed_status) }}
                            </el-tag>
                            <template v-if="row.seed_job">
                              <el-progress
                                :percentage="Number(row.seed_job.progress.percent || 0)"
                                :status="row.seed_job.status === 'failed' ? 'exception' : undefined"
                                :stroke-width="8"
                              />
                              <div class="desktop-seed-meta">
                                <span>
                                  {{ row.seed_job.progress.processed_files }} / {{ row.seed_job.progress.total_files }}
                                </span>
                                <span v-if="row.seed_job.progress.speed_bps > 0">
                                  {{ formatSeedSpeed(row.seed_job.progress.speed_bps) }}
                                </span>
                                <span v-if="row.seed_job.progress.eta_seconds !== null">
                                  ETA {{ formatSeedEta(row.seed_job.progress.eta_seconds) }}
                                </span>
                              </div>
                              <p v-if="row.seed_job.error" class="desktop-seed-error">
                                {{ row.seed_job.error }}
                              </p>
                            </template>
                          </div>
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.common.actions')" width="220" align="center">
                        <template #default="{ row }">
                          <div class="desktop-container-actions">
                            <el-button
                              link
                              type="primary"
                              :loading="seedActionForContainer(row.container_id) === 'start'"
                              :disabled="isSeedActionBusy(row.container_id)"
                              @click="startSeedForContainer(row)"
                            >
                              {{ t('desktop.seed.start') }}
                            </el-button>
                            <el-button
                              v-if="row.seed_job?.status === 'running'"
                              link
                              :loading="seedActionForContainer(row.container_id) === 'pause'"
                              :disabled="isSeedActionBusy(row.container_id)"
                              @click="pauseSeedForContainer(row)"
                            >
                              {{ t('desktop.seed.pause') }}
                            </el-button>
                            <el-button
                              v-if="row.seed_job?.status === 'paused'"
                              link
                              :loading="seedActionForContainer(row.container_id) === 'resume'"
                              :disabled="isSeedActionBusy(row.container_id)"
                              @click="resumeSeedForContainer(row)"
                            >
                              {{ t('desktop.seed.resume') }}
                            </el-button>
                            <el-button
                              v-if="row.seed_job && row.seed_job.status !== 'done' && row.seed_job.status !== 'canceled'"
                              link
                              type="warning"
                              :loading="seedActionForContainer(row.container_id) === 'cancel'"
                              :disabled="isSeedActionBusy(row.container_id)"
                              @click="cancelSeedForContainer(row)"
                            >
                              {{ t('desktop.seed.cancel') }}
                            </el-button>
                            <el-button
                              v-if="row.container_id !== 1"
                              link
                              type="danger"
                              @click="removeContainer(row.container_id)"
                            >
                              {{ t('desktop.common.remove') }}
                            </el-button>
                            <span
                              v-else
                              class="desktop-container-fixed"
                            >
                              {{ t('desktop.containers.fixed') }}
                            </span>
                          </div>
                        </template>
                      </el-table-column>
                    </el-table>
                  </el-card>
                </div>

                <div v-show="activeSection === 'remote'" class="desktop-system-panel">
                  <el-card>
                    <template #header>
                      <div class="desktop-settings-title-row">
                        <span>{{ t('desktop.system.remote.title') }}</span>
                        <span class="desktop-settings-remote-state" :class="{ connected: remoteConnected }">
                          {{
                            remoteConnected
                              ? t('desktop.system.remote.connected')
                              : t('desktop.system.remote.disconnected')
                          }}
                        </span>
                      </div>
                    </template>

                    <el-form label-position="top" class="desktop-form-grid">
                      <el-form-item :label="t('desktop.system.remote.serverBaseUrl')">
                        <el-input
                          v-model="remoteServerBaseUrl"
                          :placeholder="t('desktop.system.remote.serverPlaceholder')"
                        />
                      </el-form-item>
                    </el-form>

                    <div class="desktop-settings-remote-actions">
                      <el-button
                        type="primary"
                        :loading="connectingRemote"
                        @click="connectRemoteServer"
                      >
                        {{ t('desktop.system.remote.connect') }}
                      </el-button>
                      <el-button :disabled="!remoteConnected" @click="disconnectRemoteServer">
                        {{ t('desktop.system.remote.disconnect') }}
                      </el-button>
                    </div>
                    <p class="desktop-settings-hint">{{ t('desktop.system.remote.hint') }}</p>
                  </el-card>
                </div>
              </section>
            </div>
          </section>
        </div>
      </section>
    </main>

    <el-dialog
      v-model="pathPickerVisible"
      :title="t('desktop.containers.pathPickerTitle')"
      width="760px"
      class="desktop-path-picker-dialog"
    >
      <div class="desktop-path-picker" v-loading="pathPickerLoading">
        <div class="desktop-path-picker-toolbar">
          <el-button plain :disabled="!pathPickerParentPath" @click="openPathPickerParent">
            <i class="fa-solid fa-arrow-up" aria-hidden="true"></i>
            <span>{{ t('desktop.containers.pathPickerUp') }}</span>
          </el-button>
          <el-input :model-value="pathPickerCurrentPath" readonly />
        </div>

        <div v-if="pathPickerRoots.length" class="desktop-path-picker-roots">
          <el-button
            v-for="root in pathPickerRoots"
            :key="root"
            text
            @click="openPathPickerDirectory(root)"
          >
            {{ root }}
          </el-button>
        </div>

        <div class="desktop-path-picker-list">
          <button
            v-for="item in pathPickerItems"
            :key="item.path"
            type="button"
            class="desktop-path-picker-item"
            @click="openPathPickerDirectory(item.path)"
          >
            <i class="fa-solid fa-folder" aria-hidden="true"></i>
            <span>{{ item.name }}</span>
          </button>
          <div v-if="!pathPickerLoading && !pathPickerItems.length" class="desktop-path-picker-empty">
            {{ t('desktop.containers.pathPickerEmpty') }}
          </div>
        </div>
      </div>

      <template #footer>
        <el-button @click="pathPickerVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button
          type="primary"
          :disabled="!pathPickerCurrentPath"
          @click="applyPathPickerSelection"
        >
          {{ t('desktop.containers.pathPickerUseCurrent') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import { useRoute, useRouter } from 'vue-router';

import {
  controlDesktopSeedJob,
  fetchDesktopSettings,
  listDesktopDirectories,
  listDesktopSeedJobs,
  startDesktopSeedJob,
  updateDesktopSettings,
  type DesktopDirectoryEntry,
  type DesktopDirectoryListData,
  type DesktopRemoteGatewaySettings,
  type DesktopSeedJob
} from '@/api/desktop';
import {
  clearDesktopRemoteApiBaseOverride,
  getDesktopRemoteApiBaseOverride,
  getDesktopLocalToken,
  isDesktopRemoteAuthMode,
  setDesktopRemoteApiBaseOverride
} from '@/config/desktop';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n, getLanguageLabel, setLanguage } from '@/i18n';

type SectionKey = 'model' | 'containers' | 'remote';

type ModelRow = {
  key: string;
  base_url: string;
  api_key: string;
  model: string;
  raw: Record<string, unknown>;
};

type ContainerRow = {
  container_id: number;
  root: string;
  cloud_workspace_id: string;
  seed_status: string;
  seed_job: DesktopSeedJob | null;
};

type PathPickerTarget =
  | { kind: 'workspace' }
  | { kind: 'container'; containerId: number };

type SeedActionKind = 'start' | 'pause' | 'resume' | 'cancel';

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    key,
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    raw: { ...raw }
  }));

const buildModelPayload = (row: ModelRow): Record<string, unknown> => {
  const output: Record<string, unknown> = { ...row.raw };

  const setText = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (cleaned) {
      output[key] = cleaned;
    } else {
      delete output[key];
    }
  };

  setText('base_url', row.base_url);
  setText('api_key', row.api_key);
  setText('model', row.model);

  return output;
};

const normalizeSection = (value: unknown): SectionKey => {
  const cleaned = String(value || '').trim().toLowerCase();
  if (cleaned === 'containers') {
    return 'containers';
  }
  if (cleaned === 'remote') {
    return 'remote';
  }
  return 'model';
};

const normalizeSeedStatus = (value: unknown): string => {
  const cleaned = String(value || '').trim().toLowerCase();
  if (['running', 'paused', 'done', 'failed', 'canceled', 'idle'].includes(cleaned)) {
    return cleaned;
  }
  return 'idle';
};

const toFiniteNumber = (value: unknown, fallback = 0): number => {
  const num = Number(value);
  return Number.isFinite(num) ? num : fallback;
};

const parseSeedJob = (raw: unknown): DesktopSeedJob | null => {
  const source = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : null;
  if (!source) {
    return null;
  }

  const jobId = String(source.job_id || '').trim();
  const containerId = Number.parseInt(String(source.container_id || ''), 10);
  if (!jobId || !Number.isFinite(containerId) || containerId <= 0) {
    return null;
  }

  const progressRaw =
    source.progress && typeof source.progress === 'object'
      ? (source.progress as Record<string, unknown>)
      : {};

  return {
    job_id: jobId,
    container_id: containerId,
    local_root: String(source.local_root || '').trim(),
    cloud_workspace_id: String(source.cloud_workspace_id || '').trim(),
    remote_api_base: String(source.remote_api_base || '').trim(),
    stage: String(source.stage || '').trim(),
    status: normalizeSeedStatus(source.status),
    progress: {
      percent: Math.min(100, Math.max(0, toFiniteNumber(progressRaw.percent))),
      processed_files: Math.max(0, Math.round(toFiniteNumber(progressRaw.processed_files))),
      total_files: Math.max(0, Math.round(toFiniteNumber(progressRaw.total_files))),
      processed_bytes: Math.max(0, Math.round(toFiniteNumber(progressRaw.processed_bytes))),
      total_bytes: Math.max(0, Math.round(toFiniteNumber(progressRaw.total_bytes))),
      speed_bps: Math.max(0, toFiniteNumber(progressRaw.speed_bps)),
      eta_seconds:
        progressRaw.eta_seconds === null
          ? null
          : (() => {
              const eta = toFiniteNumber(progressRaw.eta_seconds, -1);
              return eta >= 0 ? Math.round(eta) : null;
            })()
    },
    current_item: String(source.current_item || '').trim() || undefined,
    error: String(source.error || '').trim() || undefined,
    created_at: toFiniteNumber(source.created_at),
    updated_at: toFiniteNumber(source.updated_at),
    started_at:
      source.started_at === null || source.started_at === undefined
        ? null
        : toFiniteNumber(source.started_at),
    finished_at:
      source.finished_at === null || source.finished_at === undefined
        ? null
        : toFiniteNumber(source.finished_at)
  };
};

const parseSeedJobs = (raw: unknown): DesktopSeedJob[] => {
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw
    .map((item) => parseSeedJob(item))
    .filter((item): item is DesktopSeedJob => Boolean(item))
    .sort((left, right) => right.created_at - left.created_at);
};

const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const loading = ref(false);
const savingModel = ref(false);
const savingContainers = ref(false);
const connectingRemote = ref(false);
const seedJobsLoading = ref(false);

const activeSection = ref<SectionKey>('model');

const language = ref('zh-CN');
const supportedLanguages = ref<string[]>(['zh-CN', 'en-US']);
const defaultModel = ref('');
const modelRows = ref<ModelRow[]>([]);

const workspaceRoot = ref('');
const containerRows = ref<ContainerRow[]>([]);

const remoteServerBaseUrl = ref('');
const remoteConnected = ref(false);

const canBrowseLocalPaths = computed(() => true);
const pathPickerVisible = ref(false);
const pathPickerLoading = ref(false);
const pathPickerCurrentPath = ref('');
const pathPickerParentPath = ref('');
const pathPickerRoots = ref<string[]>([]);
const pathPickerItems = ref<DesktopDirectoryEntry[]>([]);
const pathPickerTarget = ref<PathPickerTarget | null>(null);
const seedActionStates = ref<Record<number, SeedActionKind | undefined>>({});

let seedPollingTimer: number | null = null;
let seedPollErrorNotified = false;

const currentSection = computed(() => {
  if (activeSection.value === 'containers') {
    return {
      title: t('desktop.settings.containers'),
      description: t('desktop.containers.subtitle')
    };
  }
  if (activeSection.value === 'remote') {
    return {
      title: t('desktop.system.remote.title'),
      description: t('desktop.system.remote.hint')
    };
  }
  return {
    title: t('desktop.system.llm'),
    description: t('desktop.system.llmHint')
  };
});

const refreshRemoteConnected = () => {
  remoteConnected.value = isDesktopRemoteAuthMode();
};

const sortContainerRows = () => {
  containerRows.value.sort((left, right) => left.container_id - right.container_id);
};

const toContainerRow = (item: Partial<ContainerRow>): ContainerRow => ({
  container_id: Number(item.container_id || 0),
  root: String(item.root || '').trim(),
  cloud_workspace_id: String(item.cloud_workspace_id || '').trim(),
  seed_status: normalizeSeedStatus(item.seed_status),
  seed_job: item.seed_job || null
});

const ensureDefaultContainer = () => {
  const workspace = workspaceRoot.value.trim();
  const first = containerRows.value.find((item) => item.container_id === 1);
  if (!first) {
    containerRows.value.unshift(
      toContainerRow({
        container_id: 1,
        root: workspace
      })
    );
  } else if (workspace) {
    first.root = workspace;
  }
  sortContainerRows();
};

const parseContainerRows = (mountsRaw: unknown, rootsRaw: unknown): ContainerRow[] => {
  const map = new Map<number, ContainerRow>();

  if (Array.isArray(mountsRaw)) {
    mountsRaw.forEach((item) => {
      const containerId = Number.parseInt(String(item?.container_id), 10);
      if (!Number.isFinite(containerId) || containerId <= 0) {
        return;
      }
      map.set(
        containerId,
        toContainerRow({
          container_id: containerId,
          root: String(item?.root || '').trim(),
          cloud_workspace_id: String(item?.cloud_workspace_id || '').trim(),
          seed_status: String(item?.seed_status || 'idle').trim()
        })
      );
    });
  }

  if (Array.isArray(rootsRaw)) {
    rootsRaw.forEach((item) => {
      const containerId = Number.parseInt(String(item?.container_id), 10);
      if (!Number.isFinite(containerId) || containerId <= 0) {
        return;
      }
      const existing = map.get(containerId);
      const root = String(item?.root || '').trim();
      if (existing) {
        if (!existing.root && root) {
          existing.root = root;
        }
      } else {
        map.set(
          containerId,
          toContainerRow({
            container_id: containerId,
            root
          })
        );
      }
    });
  }

  return Array.from(map.values()).sort((left, right) => left.container_id - right.container_id);
};

const parseDirectoryListData = (raw: unknown): DesktopDirectoryListData => {
  const source = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const roots = Array.isArray(source.roots)
    ? source.roots.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  const items = Array.isArray(source.items)
    ? source.items
        .map((item) => {
          const entry = item && typeof item === 'object' ? (item as Record<string, unknown>) : {};
          return {
            name: String(entry.name || '').trim(),
            path: String(entry.path || '').trim()
          };
        })
        .filter((item) => item.name && item.path)
    : [];
  return {
    current_path: String(source.current_path || '').trim(),
    parent_path: String(source.parent_path || '').trim() || null,
    roots,
    items
  };
};

const resolveApiErrorMessage = (error: unknown): string => {
  const message =
    (error as { response?: { data?: { message?: unknown } } })?.response?.data?.message ||
    (error as { message?: unknown })?.message;
  const cleaned = String(message || '').trim();
  return cleaned;
};

const applySeedJobSnapshot = (snapshot: DesktopSeedJob) => {
  const row = containerRows.value.find((item) => item.container_id === snapshot.container_id);
  if (!row) {
    return;
  }
  row.seed_job = snapshot;
  row.seed_status = normalizeSeedStatus(snapshot.status);
};

const applySeedJobs = (jobs: DesktopSeedJob[]) => {
  const latestByContainer = new Map<number, DesktopSeedJob>();
  for (const job of jobs) {
    if (!latestByContainer.has(job.container_id)) {
      latestByContainer.set(job.container_id, job);
    }
  }
  containerRows.value.forEach((row) => {
    const latest = latestByContainer.get(row.container_id);
    if (latest) {
      row.seed_job = latest;
      row.seed_status = normalizeSeedStatus(latest.status);
      return;
    }
    row.seed_job = null;
    row.seed_status = normalizeSeedStatus(row.seed_status);
  });
};

const loadSeedJobs = async (silent = true) => {
  if (seedJobsLoading.value) {
    return;
  }
  seedJobsLoading.value = true;
  try {
    const response = await listDesktopSeedJobs({ limit: 200 });
    const jobs = parseSeedJobs(response?.data?.data?.items);
    applySeedJobs(jobs);
    seedPollErrorNotified = false;
  } catch (error) {
    console.error(error);
    if (!silent || !seedPollErrorNotified) {
      ElMessage.error(t('desktop.seed.loadFailed'));
      seedPollErrorNotified = true;
    }
  } finally {
    seedJobsLoading.value = false;
  }
};

const stopSeedPolling = () => {
  if (seedPollingTimer !== null) {
    window.clearInterval(seedPollingTimer);
    seedPollingTimer = null;
  }
};

const startSeedPolling = () => {
  if (seedPollingTimer !== null) {
    return;
  }
  seedPollingTimer = window.setInterval(() => {
    void loadSeedJobs(true);
  }, 2000);
};

const ensureSeedPolling = () => {
  if (activeSection.value !== 'containers') {
    stopSeedPolling();
    return;
  }
  startSeedPolling();
  void loadSeedJobs(true);
};

const setSeedActionState = (containerId: number, action?: SeedActionKind) => {
  const next = { ...seedActionStates.value };
  if (action) {
    next[containerId] = action;
  } else {
    delete next[containerId];
  }
  seedActionStates.value = next;
};

const isSeedActionBusy = (containerId: number): boolean =>
  Boolean(seedActionStates.value[containerId]);

const seedActionForContainer = (containerId: number): SeedActionKind | '' =>
  seedActionStates.value[containerId] || '';

const runSeedAction = async (
  containerId: number,
  action: SeedActionKind,
  handler: () => Promise<void>
) => {
  if (isSeedActionBusy(containerId)) {
    return;
  }
  setSeedActionState(containerId, action);
  try {
    await handler();
  } finally {
    setSeedActionState(containerId);
  }
};

const resolveSeedAccessToken = (): string => {
  try {
    return String(localStorage.getItem('access_token') || '').trim();
  } catch {
    return '';
  }
};

const resolveSeedRemoteApiBase = (): string => {
  const override = getDesktopRemoteApiBaseOverride().trim();
  if (override) {
    return override;
  }
  return remoteServerBaseUrl.value.trim();
};

const seedStatusLabel = (status: string): string => {
  switch (normalizeSeedStatus(status)) {
    case 'running':
      return t('desktop.seed.status.running');
    case 'paused':
      return t('desktop.seed.status.paused');
    case 'done':
      return t('desktop.seed.status.done');
    case 'failed':
      return t('desktop.seed.status.failed');
    case 'canceled':
      return t('desktop.seed.status.canceled');
    default:
      return t('desktop.seed.status.idle');
  }
};

const seedStatusTagType = (status: string): '' | 'success' | 'warning' | 'danger' | 'info' => {
  switch (normalizeSeedStatus(status)) {
    case 'running':
      return 'warning';
    case 'paused':
      return 'info';
    case 'done':
      return 'success';
    case 'failed':
      return 'danger';
    case 'canceled':
      return 'info';
    default:
      return '';
  }
};

const formatSeedSpeed = (speedBps: number): string => {
  const value = Math.max(0, Number(speedBps) || 0);
  if (value >= 1024 * 1024) {
    return `${(value / 1024 / 1024).toFixed(1)} MB/s`;
  }
  if (value >= 1024) {
    return `${(value / 1024).toFixed(1)} KB/s`;
  }
  return `${Math.round(value)} B/s`;
};

const formatSeedEta = (etaSeconds: number | null): string => {
  if (etaSeconds === null || !Number.isFinite(etaSeconds)) {
    return '--';
  }
  const totalSeconds = Math.max(0, Math.round(etaSeconds));
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) {
    return `${minutes}m ${seconds}s`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return `${hours}h ${remainingMinutes}m`;
};

const ensureLocalPathPicker = (): boolean => {
  if (canBrowseLocalPaths.value) {
    return true;
  }
  ElMessage.warning(t('desktop.containers.pathPickerLocalOnly'));
  return false;
};

const loadPathPickerDirectory = async (path?: string) => {
  pathPickerLoading.value = true;
  try {
    const response = await listDesktopDirectories(path);
    const data = parseDirectoryListData(response?.data?.data);
    pathPickerCurrentPath.value = data.current_path;
    pathPickerParentPath.value = data.parent_path || '';
    pathPickerRoots.value = data.roots;
    pathPickerItems.value = data.items;
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.containers.pathPickerLoadFailed'));
  } finally {
    pathPickerLoading.value = false;
  }
};

const openPathPickerForWorkspace = async () => {
  if (!ensureLocalPathPicker()) {
    return;
  }
  pathPickerTarget.value = { kind: 'workspace' };
  pathPickerVisible.value = true;
  await loadPathPickerDirectory(workspaceRoot.value);
};

const openPathPickerForContainer = async (containerId: number, root: string) => {
  if (!ensureLocalPathPicker()) {
    return;
  }
  pathPickerTarget.value = { kind: 'container', containerId };
  pathPickerVisible.value = true;
  await loadPathPickerDirectory(root || workspaceRoot.value);
};

const openPathPickerDirectory = async (path: string) => {
  const targetPath = String(path || '').trim();
  if (!targetPath) {
    return;
  }
  await loadPathPickerDirectory(targetPath);
};

const openPathPickerParent = async () => {
  if (!pathPickerParentPath.value) {
    return;
  }
  await loadPathPickerDirectory(pathPickerParentPath.value);
};

const applyPathPickerSelection = () => {
  const selected = pathPickerCurrentPath.value.trim();
  if (!selected) {
    return;
  }
  const target = pathPickerTarget.value;
  if (!target) {
    return;
  }

  if (target.kind === 'workspace') {
    workspaceRoot.value = selected;
    ensureDefaultContainer();
  } else {
    const row = containerRows.value.find((item) => item.container_id === target.containerId);
    if (row) {
      row.root = selected;
      if (target.containerId === 1) {
        workspaceRoot.value = selected;
      }
    }
  }

  pathPickerVisible.value = false;
};

const applySettingsData = (data: Record<string, any>) => {
  const loadedLanguages = Array.isArray(data.supported_languages)
    ? data.supported_languages.map((item: unknown) => String(item || '').trim()).filter(Boolean)
    : [];
  supportedLanguages.value = loadedLanguages.length ? loadedLanguages : ['zh-CN', 'en-US'];
  language.value = String(data.language || supportedLanguages.value[0] || 'zh-CN');

  const llm = data.llm || {};
  defaultModel.value = String(llm.default || '').trim();
  modelRows.value = parseModelRows((llm.models as Record<string, Record<string, unknown>>) || {});
  if (!modelRows.value.length) {
    addModel();
  }
  if (!defaultModel.value) {
    defaultModel.value = modelRows.value[0]?.key || '';
  }

  workspaceRoot.value = String(data.workspace_root || '').trim();
  containerRows.value = parseContainerRows(data.container_mounts, data.container_roots);
  ensureDefaultContainer();

  remoteServerBaseUrl.value = String(data.remote_gateway?.server_base_url || '').trim();
  refreshRemoteConnected();

  ensureSeedPolling();
};

const setSection = (section: SectionKey) => {
  activeSection.value = section;
  const nextQuery = { ...route.query, section };
  router.replace({ path: '/desktop/system', query: nextQuery });
};

watch(
  () => route.query.section,
  (value) => {
    activeSection.value = normalizeSection(value);
  },
  { immediate: true }
);

watch(
  () => activeSection.value,
  () => {
    ensureSeedPolling();
  }
);

const addModel = () => {
  modelRows.value.push({
    key: '',
    base_url: '',
    api_key: '',
    model: '',
    raw: {}
  });
};

const removeModel = (target: ModelRow) => {
  modelRows.value = modelRows.value.filter((item) => item !== target);
  if (!modelRows.value.some((item) => item.key.trim() === defaultModel.value.trim())) {
    defaultModel.value = modelRows.value[0]?.key || '';
  }
};

const addContainer = () => {
  const maxId = containerRows.value.reduce((max, item) => Math.max(max, item.container_id), 1);
  containerRows.value.push(
    toContainerRow({
      container_id: maxId + 1
    })
  );
  sortContainerRows();
};

const removeContainer = (containerId: number) => {
  if (containerId === 1) {
    return;
  }
  containerRows.value = containerRows.value.filter((item) => item.container_id !== containerId);
};

const syncWorkspaceFromContainer = (row: ContainerRow) => {
  if (row.container_id === 1) {
    workspaceRoot.value = String(row.root || '').trim();
  }
};

watch(workspaceRoot, (value) => {
  const first = containerRows.value.find((item) => item.container_id === 1);
  if (first) {
    first.root = String(value || '').trim();
  }
});

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveModelSettings = async () => {
  const models: Record<string, Record<string, unknown>> = {};

  for (const row of modelRows.value) {
    const key = row.key.trim();
    if (!key) {
      ElMessage.warning(t('desktop.system.modelKeyRequired'));
      return;
    }
    if (models[key]) {
      ElMessage.warning(t('desktop.system.modelKeyDuplicate', { key }));
      return;
    }
    models[key] = buildModelPayload(row);
  }

  const currentDefaultModel = defaultModel.value.trim() || Object.keys(models)[0] || '';
  if (!currentDefaultModel) {
    ElMessage.warning(t('desktop.system.defaultModelRequired'));
    return;
  }
  if (!models[currentDefaultModel]) {
    ElMessage.warning(t('desktop.system.defaultModelMissing'));
    return;
  }

  const defaultModelConfig = models[currentDefaultModel] || {};
  const defaultBaseUrl = String(defaultModelConfig.base_url || '').trim();
  const defaultModelName = String(defaultModelConfig.model || '').trim();
  if (!defaultBaseUrl || !defaultModelName) {
    ElMessage.warning(t('desktop.system.defaultModelConfigRequired'));
    return;
  }

  const selectedLanguage = language.value.trim();
  if (!selectedLanguage) {
    ElMessage.warning(t('desktop.system.languageRequired'));
    return;
  }

  savingModel.value = true;
  try {
    const response = await updateDesktopSettings({
      language: selectedLanguage,
      llm: {
        default: currentDefaultModel,
        models
      }
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
    setLanguage(language.value, { force: true });
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingModel.value = false;
  }
};

const saveContainerSettings = async () => {
  const workspace = workspaceRoot.value.trim();
  if (!workspace) {
    ElMessage.warning(t('desktop.containers.workspaceRequired'));
    return;
  }

  const normalized = containerRows.value
    .map((item) => ({
      container_id: Number.parseInt(String(item.container_id), 10),
      root: String(item.root || '').trim(),
      cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
    }))
    .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0);

  const defaultContainer = normalized.find((item) => item.container_id === 1);
  if (defaultContainer) {
    defaultContainer.root = workspace;
  } else {
    normalized.unshift({ container_id: 1, root: workspace, cloud_workspace_id: '' });
  }

  for (const item of normalized) {
    if (!item.root) {
      ElMessage.warning(t('desktop.containers.pathRequired', { id: item.container_id }));
      return;
    }
  }

  savingContainers.value = true;
  try {
    const response = await updateDesktopSettings({
      workspace_root: workspace,
      container_mounts: normalized
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingContainers.value = false;
  }
};

const startSeedForContainer = async (row: ContainerRow) => {
  const localRoot = row.root.trim();
  if (!localRoot) {
    ElMessage.warning(t('desktop.containers.pathRequired', { id: row.container_id }));
    return;
  }

  const accessToken = resolveSeedAccessToken();
  if (!accessToken) {
    ElMessage.warning(t('desktop.seed.accessTokenRequired'));
    return;
  }

  const remoteApiBase = resolveSeedRemoteApiBase();
  if (!remoteApiBase) {
    ElMessage.warning(t('desktop.seed.remoteBaseRequired'));
    return;
  }

  await runSeedAction(row.container_id, 'start', async () => {
    try {
      const response = await startDesktopSeedJob({
        container_id: row.container_id,
        access_token: accessToken,
        local_root: localRoot,
        remote_api_base: remoteApiBase,
        cloud_workspace_id: row.cloud_workspace_id.trim() || undefined
      });
      const snapshot = parseSeedJob(response?.data?.data);
      if (snapshot) {
        applySeedJobSnapshot(snapshot);
      }
      ensureSeedPolling();
      ElMessage.success(t('desktop.seed.startSuccess', { id: row.container_id }));
    } catch (error) {
      console.error(error);
      const detail = resolveApiErrorMessage(error);
      ElMessage.error(detail || t('desktop.seed.startFailed', { id: row.container_id }));
    }
  });
};

const controlSeedForContainer = async (
  row: ContainerRow,
  action: 'pause' | 'resume' | 'cancel'
) => {
  const jobId = row.seed_job?.job_id?.trim() || '';
  if (!jobId) {
    ElMessage.warning(t('desktop.seed.jobMissing', { id: row.container_id }));
    return;
  }

  const actionMessages: Record<typeof action, { success: string; failed: string }> = {
    pause: {
      success: 'desktop.seed.pauseSuccess',
      failed: 'desktop.seed.pauseFailed'
    },
    resume: {
      success: 'desktop.seed.resumeSuccess',
      failed: 'desktop.seed.resumeFailed'
    },
    cancel: {
      success: 'desktop.seed.cancelSuccess',
      failed: 'desktop.seed.cancelFailed'
    }
  };

  await runSeedAction(row.container_id, action, async () => {
    try {
      const response = await controlDesktopSeedJob({
        job_id: jobId,
        action
      });
      const snapshot = parseSeedJob(response?.data?.data);
      if (snapshot) {
        applySeedJobSnapshot(snapshot);
      }
      ensureSeedPolling();
      ElMessage.success(t(actionMessages[action].success, { id: row.container_id }));
    } catch (error) {
      console.error(error);
      const detail = resolveApiErrorMessage(error);
      ElMessage.error(detail || t(actionMessages[action].failed, { id: row.container_id }));
    }
  });
};

const pauseSeedForContainer = async (row: ContainerRow) => {
  await controlSeedForContainer(row, 'pause');
};

const resumeSeedForContainer = async (row: ContainerRow) => {
  await controlSeedForContainer(row, 'resume');
};

const cancelSeedForContainer = async (row: ContainerRow) => {
  await controlSeedForContainer(row, 'cancel');
};

const connectRemoteServer = async () => {
  const rawUrl = remoteServerBaseUrl.value.trim();
  if (!rawUrl) {
    ElMessage.warning(t('desktop.system.remote.serverRequired'));
    return;
  }

  const normalizedApiBase = setDesktopRemoteApiBaseOverride(rawUrl);
  if (!normalizedApiBase) {
    ElMessage.warning(t('desktop.system.remote.serverInvalid'));
    return;
  }

  connectingRemote.value = true;
  try {
    const payload: { remote_gateway: DesktopRemoteGatewaySettings } = {
      remote_gateway: {
        enabled: true,
        server_base_url: rawUrl
      }
    };
    await updateDesktopSettings(payload);

    try {
      localStorage.removeItem('access_token');
    } catch {
      // ignore localStorage failures
    }

    refreshRemoteConnected();
    ElMessage.success(t('desktop.system.remote.connectSuccess'));
    router.push('/login');
  } catch (error) {
    clearDesktopRemoteApiBaseOverride();
    console.error(error);
    ElMessage.error(t('desktop.system.remote.connectFailed'));
  } finally {
    connectingRemote.value = false;
  }
};

const disconnectRemoteServer = async () => {
  connectingRemote.value = true;
  try {
    await updateDesktopSettings({
      remote_gateway: {
        enabled: false,
        server_base_url: ''
      }
    });

    clearDesktopRemoteApiBaseOverride();
    const localToken = getDesktopLocalToken();
    if (localToken) {
      try {
        localStorage.setItem('access_token', localToken);
      } catch {
        // ignore localStorage failures
      }
    }

    remoteServerBaseUrl.value = '';
    refreshRemoteConnected();
    ElMessage.success(t('desktop.system.remote.disconnectSuccess'));
    router.push('/desktop/home');
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.remote.disconnectFailed'));
  } finally {
    connectingRemote.value = false;
  }
};

onMounted(() => {
  refreshRemoteConnected();
  void loadSettings();
});

onBeforeUnmount(() => {
  stopSeedPolling();
});
</script>

<style scoped>
.desktop-system-shell {
  --desktop-input-bg: rgba(255, 255, 255, 0.06);
  --desktop-table-header-bg: rgba(255, 255, 255, 0.05);
  --desktop-table-row-hover-bg: rgba(255, 255, 255, 0.04);
}

:root[data-user-theme='light'] .desktop-system-shell {
  --desktop-input-bg: rgba(15, 23, 42, 0.04);
  --desktop-table-header-bg: rgba(15, 23, 42, 0.05);
  --desktop-table-row-hover-bg: rgba(15, 23, 42, 0.03);
}

.desktop-system-shell .portal-main-scroll {
  min-height: 0;
}

.desktop-system-shell .portal-section {
  flex: 1;
  min-height: 0;
}

.desktop-system-layout {
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  gap: 16px;
  min-height: 100%;
  height: 100%;
  align-items: stretch;
}

.desktop-system-sidebar {
  box-sizing: border-box;
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 100%;
}

.desktop-system-sidebar-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--portal-muted);
  padding: 4px 8px;
}

.desktop-system-sidebar-nav {
  display: grid;
  gap: 8px;
}

.desktop-system-sidebar-item {
  width: 100%;
  border: 1px solid transparent;
  background: transparent;
  color: var(--portal-text);
  padding: 10px 12px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.desktop-system-sidebar-item:hover {
  border-color: var(--portal-border);
  background: rgba(255, 255, 255, 0.04);
}

.desktop-system-sidebar-item.active {
  border-color: rgba(var(--portal-primary-rgb), 0.5);
  background: rgba(var(--portal-primary-rgb), 0.15);
}

.desktop-system-sidebar-foot {
  margin-top: auto;
  border: 1px dashed var(--portal-border);
  border-radius: 10px;
  background: rgba(var(--portal-primary-rgb), 0.08);
  padding: 10px 12px;
}

.desktop-system-sidebar-foot p {
  margin: 0;
  color: var(--portal-muted);
  font-size: 12px;
  line-height: 1.5;
}

:root[data-user-theme='light'] .desktop-system-sidebar-item:hover {
  background: rgba(15, 23, 42, 0.03);
}

:root[data-user-theme='light'] .desktop-system-sidebar-item.active {
  background: rgba(var(--portal-primary-rgb), 0.12);
}

:root[data-user-theme='light'] .desktop-system-sidebar-foot {
  background: rgba(var(--portal-primary-rgb), 0.06);
}

.desktop-system-content {
  min-width: 0;
  display: grid;
  gap: 16px;
  align-content: start;
}

.desktop-system-header {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 14px 16px;
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: flex-start;
  position: sticky;
  top: 12px;
  z-index: 4;
}

.desktop-system-header-meta h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
}

.desktop-system-header-meta p {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.desktop-system-panel {
  display: grid;
  gap: 16px;
}

.desktop-form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 12px;
}

.desktop-full-width {
  width: 100%;
}

.desktop-settings-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.desktop-settings-remote-state {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-settings-remote-state.connected {
  color: #22c55e;
}

.desktop-settings-remote-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.desktop-settings-hint {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-fixed {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
}

.desktop-seed-cell {
  display: grid;
  gap: 8px;
}

.desktop-seed-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  color: var(--portal-muted);
  font-size: 12px;
}

.desktop-seed-error {
  margin: 0;
  color: #f97316;
  font-size: 12px;
  line-height: 1.45;
  word-break: break-word;
}

:root[data-user-theme='light'] .desktop-seed-error {
  color: #ea580c;
}

.desktop-path-picker {
  display: grid;
  gap: 12px;
}

.desktop-path-picker-toolbar {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 10px;
  align-items: center;
}

.desktop-path-picker-roots {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.desktop-path-picker-list {
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-panel);
  max-height: 360px;
  overflow: auto;
  padding: 8px;
  display: grid;
  gap: 6px;
}

.desktop-path-picker-item {
  width: 100%;
  border: 1px solid transparent;
  background: transparent;
  color: var(--portal-text);
  border-radius: 8px;
  padding: 8px 10px;
  display: flex;
  align-items: center;
  gap: 10px;
  text-align: left;
  cursor: pointer;
}

.desktop-path-picker-item:hover {
  border-color: var(--portal-border);
  background: rgba(var(--portal-primary-rgb), 0.1);
}

.desktop-path-picker-empty {
  padding: 18px 12px;
  text-align: center;
  color: var(--portal-muted);
  font-size: 12px;
}

.desktop-system-shell :deep(.el-card) {
  border: 1px solid var(--portal-border);
  background: var(--portal-panel);
  color: var(--portal-text);
}

.desktop-system-shell :deep(.el-card__header) {
  border-bottom: 1px solid var(--portal-border);
}

.desktop-system-shell :deep(.el-input__wrapper),
.desktop-system-shell :deep(.el-select__wrapper),
.desktop-system-shell :deep(.el-textarea__inner) {
  background: var(--desktop-input-bg);
  box-shadow: 0 0 0 1px var(--portal-border) inset;
}

.desktop-system-shell :deep(.el-form-item__label),
.desktop-system-shell :deep(.el-input__inner),
.desktop-system-shell :deep(.el-select__placeholder),
.desktop-system-shell :deep(.el-textarea__inner) {
  color: var(--portal-text);
}

.desktop-system-shell :deep(.el-input__inner::placeholder),
.desktop-system-shell :deep(.el-textarea__inner::placeholder) {
  color: var(--portal-muted);
}

.desktop-system-shell :deep(.el-table) {
  --el-table-bg-color: transparent;
  --el-table-tr-bg-color: transparent;
  --el-table-header-bg-color: var(--desktop-table-header-bg);
  --el-table-border-color: var(--portal-border);
  --el-table-text-color: var(--portal-text);
  --el-table-header-text-color: var(--portal-muted);
}

.desktop-system-shell :deep(.el-table__row:hover > td.el-table__cell) {
  background: var(--desktop-table-row-hover-bg);
}

:root[data-user-theme='light'] .desktop-settings-remote-state.connected {
  color: #16a34a;
}

@media (max-width: 1100px) {
  .desktop-system-layout {
    grid-template-columns: 1fr;
  }

  .desktop-system-sidebar {
    min-height: 0;
    height: auto;
  }

  .desktop-system-sidebar-nav {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .desktop-system-header {
    position: static;
  }
}

@media (max-width: 760px) {
  .desktop-system-sidebar-nav {
    grid-template-columns: 1fr;
  }

  .desktop-system-header {
    flex-direction: column;
  }

  .desktop-system-header-actions {
    width: 100%;
    justify-content: flex-start;
  }

  .desktop-path-picker-toolbar {
    grid-template-columns: 1fr;
  }
}
</style>
