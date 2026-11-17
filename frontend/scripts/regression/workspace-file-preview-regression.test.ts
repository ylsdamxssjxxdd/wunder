import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const panel = readFileSync(resolve(process.cwd(), 'src/views/messenger/workspace/WorkspaceFilesPanel.vue'), 'utf8');
const loader = readFileSync(resolve(process.cwd(), 'src/views/messenger/workspace/workspaceDocumentPreview.ts'), 'utf8');
const dialog = readFileSync(resolve(process.cwd(), 'src/components/workspace/WorkspaceDocumentDialog.vue'), 'utf8');
const controller = readFileSync(
  resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerInstall.ts'),
  'utf8'
);
const dialogsHost = readFileSync(
  resolve(process.cwd(), 'src/views/messenger/sections/MessengerDialogsHost.vue'),
  'utf8'
);

test('workspace HTML files render in a sandboxed iframe with rewritten local resources', () => {
  assert.match(panel, /loadWorkspaceDocumentPreview/);
  assert.match(loader, /buildWorkspaceHtmlPreviewDocument/);
  assert.match(loader, /isWorkspaceHtmlDocumentPath/);
  assert.match(dialog, /previewKind === 'html'/);
  assert.match(dialog, /sandbox="allow-scripts"/);
});

test('markdown documents can switch between source editing and rendered preview', () => {
  assert.match(dialog, /isWorkspaceMarkdownDocumentPath/);
  assert.match(dialog, /renderMarkdown/);
  assert.match(dialog, /workspace\.preview\.modePreview/);
  assert.match(dialog, /workspace\.preview\.modeEdit/);
});

test('chat-referenced workspace documents open the same editable dialog as the files area', () => {
  assert.match(controller, /loadWorkspaceDocumentPreview/);
  assert.match(controller, /ctx\.handleResourcePreviewSave/);
  assert.match(controller, /saveWunderWorkspaceFile/);
  assert.match(dialogsHost, /WorkspaceDocumentDialog/);
  assert.match(dialogsHost, /handleResourcePreviewSave/);
});
