/**
 * 工作目录拖拽上传的统一目录遍历。
 *
 * 只负责把 DataTransfer 的 FileSystemEntry 树展开成「文件 + 目录」清单：
 * - 不做文件数量裁剪，上传端按有界窗口分批投递，避免整目录后段文件被静默丢弃；
 * - 目录本身也进入清单，空目录才能在云端落地；
 * - 深度防御只针对异常的递归结构，正常目录不会被截断。
 */

export type WorkspaceDropDirectoryReader = {
  readEntries: (
    successCallback: (entries: WorkspaceDropEntry[]) => void,
    errorCallback?: (reason: DOMException) => void
  ) => void;
};

export type WorkspaceDropEntry = {
  name?: string;
  isFile?: boolean;
  isDirectory?: boolean;
  file?: (
    successCallback: (file: File) => void,
    errorCallback?: (reason: DOMException) => void
  ) => void;
  createReader?: () => WorkspaceDropDirectoryReader;
};

export type WorkspaceDroppedFile = {
  file: File;
  relativePath: string;
};

export type WorkspaceDroppedTree = {
  files: WorkspaceDroppedFile[];
  /** 拖入目录的相对路径（不含根），按遍历顺序去重前的全量列表。 */
  directories: string[];
  /** file()/readEntries() 报错而无法读取的条目数。 */
  unreadable: number;
  /** 因超过深度防御上限被跳过的目录数。 */
  depthSkipped: number;
};

const DEFAULT_MAX_DEPTH = 64;

/**
 * 展开拖入的条目树。返回全部文件和目录，不做数量上限。
 */
export const collectWorkspaceDropTree = async (
  entries: WorkspaceDropEntry[],
  options: { maxDepth?: number } = {}
): Promise<WorkspaceDroppedTree> => {
  const parsedDepth = Number(options.maxDepth);
  const maxDepth = Number.isFinite(parsedDepth) && parsedDepth > 0
    ? Math.floor(parsedDepth)
    : DEFAULT_MAX_DEPTH;
  const files: WorkspaceDroppedFile[] = [];
  const directories: string[] = [];
  let unreadable = 0;
  let depthSkipped = 0;

  const readAllEntries = async (
    reader: WorkspaceDropDirectoryReader
  ): Promise<WorkspaceDropEntry[] | null> => {
    const collected: WorkspaceDropEntry[] = [];
    while (true) {
      const batch = await new Promise<WorkspaceDropEntry[] | null>((resolve) => {
        try {
          reader.readEntries(
            (items) => resolve(items || []),
            () => resolve(null)
          );
        } catch {
          resolve(null);
        }
      });
      if (batch === null) return null;
      if (!batch.length) return collected;
      collected.push(...batch);
    }
  };

  const walk = async (list: WorkspaceDropEntry[], depth: number, prefix: string): Promise<void> => {
    for (const entry of list) {
      if (!entry) continue;
      const name = String(entry.name || '');
      if (entry.isFile) {
        if (!entry.file) {
          unreadable += 1;
          continue;
        }
        const file = await new Promise<File | null>((resolve) => {
          try {
            entry.file?.((value) => resolve(value || null), () => resolve(null));
          } catch {
            resolve(null);
          }
        });
        if (!file) {
          unreadable += 1;
          continue;
        }
        files.push({
          file,
          relativePath: prefix ? `${prefix}/${name || file.name}` : ''
        });
        continue;
      }
      if (!entry.isDirectory || !name) continue;
      const nextPrefix = prefix ? `${prefix}/${name}` : name;
      directories.push(nextPrefix);
      if (depth >= maxDepth) {
        depthSkipped += 1;
        continue;
      }
      const reader = entry.createReader?.();
      if (!reader) {
        unreadable += 1;
        continue;
      }
      // readEntries 每次最多返回约 100 项，必须循环读到空，否则大目录会漏条目。
      const children = await readAllEntries(reader);
      if (children === null) {
        unreadable += 1;
        continue;
      }
      if (children.length) await walk(children, depth + 1, nextPrefix);
    }
  };

  await walk(entries, 0, '');
  return { files, directories, unreadable, depthSkipped };
};

/**
 * 找出没有任何文件后代的目录（空目录 / 只含空目录的目录）。
 * 只返回最深一层的空目录：后端 create_dir_all 会顺带补出祖先目录，
 * 这样既不会漏建子目录，也不会为同一条空目录链重复建目录。
 */
export const resolveEmptyDropDirectories = (
  directories: string[],
  files: WorkspaceDroppedFile[]
): string[] => {
  if (!directories.length) return [];
  const withFiles = new Set<string>();
  files.forEach((item) => {
    const path = String(item.relativePath || '').trim();
    if (!path) return;
    let index = path.lastIndexOf('/');
    while (index > 0) {
      const directory = path.slice(0, index);
      if (withFiles.has(directory)) break;
      withFiles.add(directory);
      index = directory.lastIndexOf('/');
    }
  });
  const empty = new Set(directories.filter((directory) => !withFiles.has(directory)));
  if (!empty.size) return [];
  const emptyParents = new Set<string>();
  empty.forEach((directory) => {
    if (!directory.includes('/')) return;
    const parent = directory.slice(0, directory.lastIndexOf('/'));
    if (empty.has(parent)) emptyParents.add(parent);
  });
  return Array.from(empty).filter((directory) => !emptyParents.has(directory));
};
