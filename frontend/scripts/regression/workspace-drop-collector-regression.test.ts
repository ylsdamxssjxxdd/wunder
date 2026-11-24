// AI生成
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  collectWorkspaceClipboardFiles,
  collectWorkspaceDropTree,
  resolveEmptyDropDirectories,
  type WorkspaceDropEntry,
  type WorkspaceDroppedFile
} from '../../src/utils/workspaceDrop';

type MockNode =
  | { name: string; kind: 'file' }
  | { name: string; kind: 'dir'; children: MockNode[] };

const mockFile = (name: string): File => ({ name, size: 1 } as unknown as File);

const toEntry = (node: MockNode): WorkspaceDropEntry => {
  if (node.kind === 'file') {
    return {
      name: node.name,
      isFile: true,
      file: (success) => success(mockFile(node.name))
    };
  }
  return {
    name: node.name,
    isDirectory: true,
    createReader: () => {
      const pending = [...node.children];
      return {
        readEntries: (success) => {
          // 模拟浏览器每次最多返回 100 项的分批行为。
          success(pending.splice(0, 100).map(toEntry));
        }
      };
    }
  };
};

test('深层目录不会被六层深度上限截断', async () => {
  let node: MockNode = { name: 'leaf.txt', kind: 'file' };
  for (let depth = 0; depth < 10; depth += 1) {
    node = { name: `d${depth}`, kind: 'dir', children: [node] };
  }
  const tree = await collectWorkspaceDropTree([toEntry(node)]);
  assert.equal(tree.depthSkipped, 0);
  assert.equal(tree.files.length, 1);
  assert.equal(
    tree.files[0].relativePath,
    'd9/d8/d7/d6/d5/d4/d3/d2/d1/d0/leaf.txt'
  );
  assert.equal(tree.directories.length, 10);
});

test('大目录分批读取后文件不丢失', async () => {
  const children: MockNode[] = [];
  for (let index = 0; index < 250; index += 1) {
    children.push({ name: `file-${index}.txt`, kind: 'file' });
  }
  const tree = await collectWorkspaceDropTree([
    toEntry({ name: 'big', kind: 'dir', children })
  ]);
  assert.equal(tree.files.length, 250);
  assert.equal(tree.files[249].relativePath, 'big/file-249.txt');
});

test('空目录只返回最深一层，交给 create_dir_all 补齐祖先', () => {
  const directories = [
    'proj',
    'proj/docs',
    'proj/docs/empty',
    'proj/src',
    'proj/src/empty/nested'
  ];
  const files: WorkspaceDroppedFile[] = [
    { file: mockFile('a.ts'), relativePath: 'proj/src/a.ts' }
  ];
  assert.deepEqual(resolveEmptyDropDirectories(directories, files), [
    'proj/docs/empty',
    'proj/src/empty/nested'
  ]);
});

test('无法读取的条目会被计数而不是静默忽略', async () => {
  const tree = await collectWorkspaceDropTree([
    { name: 'broken.txt', isFile: true, file: (_success, error) => error?.(new DOMException('denied')) }
  ]);
  assert.equal(tree.files.length, 0);
  assert.equal(tree.unreadable, 1);
});

test('剪贴板图片和文档从文件项提取且不重复', () => {
  const image = mockFile('image.png');
  const document = mockFile('notes.pdf');
  const tree = collectWorkspaceClipboardFiles({
    files: [image, document],
    items: [
      { kind: 'file', getAsFile: () => image },
      { kind: 'string', getAsFile: () => null },
      { kind: 'file', getAsFile: () => document }
    ]
  } as unknown as DataTransfer);
  assert.deepEqual(tree.files.map((item) => item.relativePath), ['image.png', 'notes.pdf']);
  assert.deepEqual(tree.directories, []);
});

test('剪贴板只有 files 时使用文件名并忽略空列表', () => {
  const transfer = { files: [mockFile('report.docx')], items: [] } as unknown as DataTransfer;
  assert.equal(collectWorkspaceClipboardFiles(transfer).files[0].relativePath, 'report.docx');
  assert.equal(collectWorkspaceClipboardFiles(null).files.length, 0);
});
