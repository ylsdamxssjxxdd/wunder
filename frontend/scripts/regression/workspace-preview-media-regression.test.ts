import test from 'node:test';
import assert from 'node:assert/strict';

import {
  normalizeWorkspacePreviewBlob,
  resolveWorkspaceResourcePreviewKind,
  resolveWorkspaceResourceMimeType
} from '../../src/utils/workspaceResourcePreview';

test('音频扩展名都进入 audio 预览分支', () => {
  for (const name of ['voice.mp3', 'clip.WAV', 'loop.flac', 'note.aac', 'talk.ogg', 'talk.oga', 'talk.opus', 'memo.m4a', 'memo.weba']) {
    assert.equal(resolveWorkspaceResourcePreviewKind(name, 1024), 'audio', name);
  }
});

test('音频 MIME 按扩展名解析，未知扩展名不臆造类型', () => {
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'mp3'), 'audio/mpeg');
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'wav'), 'audio/wav');
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'm4a'), 'audio/mp4');
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'opus'), 'audio/ogg');
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'weba'), 'audio/webm');
  assert.equal(resolveWorkspaceResourceMimeType('audio', 'unknown'), '');
});

test('下载端点给出 octet-stream 时按预览类型归一化 blob', () => {
  const raw = new Blob([new Uint8Array([1, 2, 3])], { type: 'application/octet-stream' });

  const audio = normalizeWorkspacePreviewBlob(raw, 'audio', 'mp3');
  assert.equal(audio.type, 'audio/mpeg');
  assert.equal(audio.size, 3);

  const video = normalizeWorkspacePreviewBlob(raw, 'video', 'mp4');
  assert.equal(video.type, 'video/mp4');

  // 已经正确的 blob 不重建，避免多一次内存拷贝。
  const alreadyCorrect = new Blob([new Uint8Array([9])], { type: 'audio/mpeg' });
  assert.equal(normalizeWorkspacePreviewBlob(alreadyCorrect, 'audio', 'mp3'), alreadyCorrect);
});

test('文本类型不被媒体 MIME 覆盖', () => {
  const raw = new Blob(['hello'], { type: 'application/octet-stream' });
  assert.equal(normalizeWorkspacePreviewBlob(raw, 'text', 'txt'), raw);
});
