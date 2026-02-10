import { defineStore } from 'pinia';

import {
  createWunderWorkspaceDir,
  deleteWunderWorkspaceEntry,
  fetchWunderWorkspaceContent,
  uploadWunderWorkspace
} from '@/api/workspace';
import { t } from '@/i18n';
import { isDemoMode, loadDemoWorkspaceState, saveDemoWorkspaceState } from '@/utils/demo';

const DEFAULT_TREE_DEPTH = 3;

type WorkspaceEntry = {
  type?: string;
  name?: string;
  path?: string;
  size?: number;
  updated_time?: string;
  children?: WorkspaceEntry[];
};

type WorkspaceFolder = {
  id: string;
  name: string;
  path: string;
  children: WorkspaceFolder[];
};

type WorkspaceFile = {
  name: string;
  path: string;
  size: number;
  updated_time: string;
  extension: string;
};

type DemoWorkspaceState = {
  folders: WorkspaceFolder[];
  files: WorkspaceFile[];
  activePath: string;
};

type DemoWorkspacePatch = {
  folders?: WorkspaceFolder[];
  files?: WorkspaceFile[];
  activePath?: string;
};

const buildDemoWorkspaceState = (): DemoWorkspaceState => ({
  folders: [],
  files: [],
  activePath: ''
});

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const normalizeDemoWorkspaceState = (value: unknown): DemoWorkspaceState => {
  const source = asRecord(value);
  return {
    folders: Array.isArray(source.folders) ? (source.folders as WorkspaceFolder[]) : [],
    files: Array.isArray(source.files) ? (source.files as WorkspaceFile[]) : [],
    activePath: typeof source.activePath === 'string' ? source.activePath : ''
  };
};

const getDemoWorkspaceState = () => normalizeDemoWorkspaceState(loadDemoWorkspaceState());

const persistDemoWorkspaceState = (state: DemoWorkspaceState) => saveDemoWorkspaceState(state);

const syncDemoWorkspaceCache = ({ folders, files, activePath }: DemoWorkspacePatch = {}) => {
  if (!isDemoMode()) return;
  const state = getDemoWorkspaceState();
  if (Array.isArray(folders)) {
    state.folders = folders;
  }
  if (Array.isArray(files)) {
    state.files = files;
  }
  if (typeof activePath === 'string') {
    state.activePath = activePath;
  }
  persistDemoWorkspaceState(state);
};

const normalizeWorkspacePath = (value: unknown) =>
  String(value || '').replace(/\\/g, '/').replace(/^\/+/, '').trim();

const joinWorkspacePath = (basePath: unknown, name: unknown) =>
  [normalizeWorkspacePath(basePath), String(name || '').trim()]
    .filter(Boolean)
    .join('/');

const getFileExtension = (name: unknown) => {
  const trimmed = String(name || '').trim();
  if (!trimmed) return '';
  const dotIndex = trimmed.lastIndexOf('.');
  if (dotIndex === -1 || dotIndex === trimmed.length - 1) {
    return '';
  }
  return trimmed.slice(dotIndex + 1).toLowerCase();
};

const mapFolderEntries = (entries: WorkspaceEntry[] = []): WorkspaceFolder[] =>
  entries
    .filter((entry) => entry?.type === 'dir')
    .map((entry) => ({
      id: entry.path || '',
      name: entry.name || entry.path || t('workspace.folder.unnamed'),
      path: entry.path || '',
      children: mapFolderEntries(entry.children || [])
    }));

const mapFileEntries = (entries: WorkspaceEntry[] = []): WorkspaceFile[] =>
  entries
    .filter((entry) => entry?.type === 'file')
    .map((entry) => {
      const extension = getFileExtension(entry.name);
      return {
        name: entry.name || '',
        path: entry.path || '',
        size: Number(entry.size) || 0,
        updated_time: entry.updated_time || '',
        extension
      };
    });

export const useWorkspaceStore = defineStore('workspace', {
  state: () => ({
    folders: [] as WorkspaceFolder[],
    files: [] as WorkspaceFile[],
    activePath: '',
    loading: false
  }),
  actions: {
    async loadFolders() {
      const rootPath = '';
      const { data } = await fetchWunderWorkspaceContent({
        path: rootPath,
        include_content: true,
        depth: DEFAULT_TREE_DEPTH,
        sort_by: 'name',
        order: 'asc'
      });
      const payload = asRecord(data);
      const folderEntries = Array.isArray(payload.entries)
        ? mapFolderEntries(payload.entries as WorkspaceEntry[])
        : [];
      this.folders = [
        {
          id: '',
          name: t('workspace.folder.root'),
          path: '',
          children: folderEntries
        }
      ];
      syncDemoWorkspaceCache({ folders: this.folders });
      return this.folders;
    },
    async loadFiles(path = '') {
      const normalized = normalizeWorkspacePath(path);
      const { data } = await fetchWunderWorkspaceContent({
        path: normalized,
        include_content: true,
        depth: 1,
        sort_by: 'name',
        order: 'asc'
      });
      const payload = asRecord(data);
      this.activePath = normalizeWorkspacePath(payload.path || normalized);
      this.files = Array.isArray(payload.entries)
        ? mapFileEntries(payload.entries as WorkspaceEntry[])
        : [];
      syncDemoWorkspaceCache({ files: this.files, activePath: this.activePath });
      return this.files;
    },
    async createFolder(payload: { name?: string } | null | undefined) {
      const name = String(payload?.name || '').trim();
      if (!name) {
        return null;
      }
      const targetPath = joinWorkspacePath(this.activePath, name);
      const { data } = await createWunderWorkspaceDir({ path: targetPath });
      await this.loadFolders();
      return data;
    },
    async uploadFile(file: File, targetPath: string | null = null) {
      const formData = new FormData();
      formData.append('path', normalizeWorkspacePath(targetPath ?? this.activePath));
      formData.append('files', file, file.name);
      const { data } = await uploadWunderWorkspace(formData);
      await this.loadFiles(targetPath ?? this.activePath);
      return data;
    },
    async deleteFile(path: string) {
      const targetPath = normalizeWorkspacePath(path);
      if (!targetPath) {
        return;
      }
      await deleteWunderWorkspaceEntry({ path: targetPath });
      await this.loadFiles(this.activePath);
    }
  }
});
