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
  assert.match(panel, /downloadWorkspaceDirectoryArchive\(uploadTarget\.value\)/);
});
