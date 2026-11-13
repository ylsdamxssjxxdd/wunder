import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const panel = readFileSync(
  resolve(process.cwd(), 'src/views/messenger/workspace/WorkspaceFilesPanel.vue'),
  'utf8'
);

test('workspace file area exposes explicit upload and download controls', () => {
  assert.match(panel, /ref="uploadInputRef"/);
  assert.match(panel, /@click="openUploadPicker"/);
  assert.match(panel, /@change="handleUploadInputChange"/);
  assert.match(panel, /@click="handleDownloadCurrentDirectory"/);
  assert.match(panel, /@click="enterSelectionMode"/);
  assert.match(panel, /const handleUploadInputChange =/);
  assert.match(panel, /const handleDownloadCurrentDirectory =/);
  // 当前目录下载统一走 archiveDirectory(路径)，由它内部调用打包下载接口。
  assert.match(panel, /archiveDirectory\(uploadTarget\.value,/);
  assert.match(panel, /downloadWorkspaceDirectoryArchive\(path\)/);
});
