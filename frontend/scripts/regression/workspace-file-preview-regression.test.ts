import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const panel = readFileSync(resolve(process.cwd(), 'src/views/messenger/workspace/WorkspaceFilesPanel.vue'), 'utf8');
const dialog = readFileSync(resolve(process.cwd(), 'src/views/messenger/workspace/WorkspaceFilePreviewDialog.vue'), 'utf8');

test('workspace HTML files render in a sandboxed iframe with rewritten local resources', () => {
  assert.match(panel, /HTML_PREVIEW_EXTENSIONS = new Set\(\['html', 'htm', 'xhtml'\]\)/);
  assert.match(panel, /buildWorkspaceHtmlPreviewDocument/);
  assert.match(panel, /preview\.previewKind = 'html'/);
  assert.match(dialog, /previewKind === 'html'/);
  assert.match(dialog, /sandbox="previewKind === 'html' \? 'allow-scripts' : undefined"/);
});
