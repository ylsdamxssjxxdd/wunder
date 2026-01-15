import { defineStore } from 'pinia';

import {
  createWunderWorkspaceDir,
  deleteWunderWorkspaceEntry,
  fetchWunderWorkspaceContent,
  uploadWunderWorkspace
} from '@/api/workspace';
import { isDemoMode, loadDemoWorkspaceState, saveDemoWorkspaceState } from '@/utils/demo';

const DEFAULT_TREE_DEPTH = 3;

const buildDemoWorkspaceState = () => ({
  folders: [],
  files: [],
  activePath: ''
});

const normalizeDemoWorkspaceState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildDemoWorkspaceState();
  }
  return {
    folders: Array.isArray(value.folders) ? value.folders : [],
    files: Array.isArray(value.files) ? value.files : [],
    activePath: typeof value.activePath === 'string' ? value.activePath : ''
  };
};

const getDemoWorkspaceState = () => normalizeDemoWorkspaceState(loadDemoWorkspaceState());

const persistDemoWorkspaceState = (state) => saveDemoWorkspaceState(state);

const syncDemoWorkspaceCache = ({ folders, files, activePath }) => {
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

const normalizeWorkspacePath = (value) =>
  String(value || '').replace(/\\/g, '/').replace(/^\/+/, '').trim();

const joinWorkspacePath = (basePath, name) =>
  [normalizeWorkspacePath(basePath), String(name || '').trim()]
    .filter(Boolean)
    .join('/');

const getFileExtension = (name) => {
  const trimmed = String(name || '').trim();
  if (!trimmed) return '';
  const dotIndex = trimmed.lastIndexOf('.');
  if (dotIndex === -1 || dotIndex === trimmed.length - 1) {
    return '';
  }
  return trimmed.slice(dotIndex + 1).toLowerCase();
};

const mapFolderEntries = (entries = []) =>
  entries
    .filter((entry) => entry?.type === 'dir')
    .map((entry) => ({
      id: entry.path || '',
      name: entry.name || entry.path || '未命名目录',
      path: entry.path || '',
      children: mapFolderEntries(entry.children || [])
    }));

const mapFileEntries = (entries = []) =>
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
    folders: [],
    files: [],
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
      const payload = data || {};
      const folders = mapFolderEntries(payload.entries || []);
      this.folders = [
        {
          id: '',
          name: '根目录',
          path: '',
          children: folders
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
      const payload = data || {};
      this.activePath = normalizeWorkspacePath(payload.path || normalized);
      this.files = mapFileEntries(payload.entries || []);
      syncDemoWorkspaceCache({ files: this.files, activePath: this.activePath });
      return this.files;
    },
    async createFolder(payload) {
      const name = String(payload?.name || '').trim();
      if (!name) {
        return null;
      }
      const targetPath = joinWorkspacePath(this.activePath, name);
      const { data } = await createWunderWorkspaceDir({ path: targetPath });
      await this.loadFolders();
      return data;
    },
    async uploadFile(file, targetPath = null) {
      const formData = new FormData();
      formData.append('path', normalizeWorkspacePath(targetPath ?? this.activePath));
      formData.append('files', file, file.name);
      const { data } = await uploadWunderWorkspace(formData);
      await this.loadFiles(targetPath ?? this.activePath);
      return data;
    },
    async deleteFile(path) {
      const targetPath = normalizeWorkspacePath(path);
      if (!targetPath) {
        return;
      }
      await deleteWunderWorkspaceEntry({ path: targetPath });
      await this.loadFiles(this.activePath);
    }
  }
});
