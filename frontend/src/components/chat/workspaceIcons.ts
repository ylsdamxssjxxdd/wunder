type WorkspaceIconTheme = {
  file?: string;
  fileExtensions?: Record<string, unknown>;
  fileNames?: Record<string, unknown>;
  iconDefinitions?: Record<string, { iconPath?: string } | unknown>;
};

export type WorkspaceThemeIconResolver = {
  resolveFileIconPath: (entryName: string, extension: string) => string;
};

const WORKSPACE_ICON_BASE = `${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}vscode-icons`;
const WORKSPACE_ICON_PATH_RE = /^(\.\.\/|\.\/)+/;

const FALLBACK_EXTENSION_ICON_ENTRIES: Array<[string, string]> = [
  ['7z', '_f_zip'],
  ['aac', '_f_audio'],
  ['adoc', '_f_asciidoc'],
  ['astro', '_f_astro'],
  ['avi', '_f_video'],
  ['bash', '_f_shell'],
  ['bat', '_f_bat'],
  ['bmp', '_f_image'],
  ['bz2', '_f_zip'],
  ['c', '_f_c'],
  ['cc', '_f_cpp'],
  ['cfg', '_f_config'],
  ['cjs', '_f_js'],
  ['clj', '_f_clojure'],
  ['cljc', '_f_clojure'],
  ['cljs', '_f_clojure'],
  ['cmd', '_f_bat'],
  ['coffee', '_f_coffeescript'],
  ['conf', '_f_config'],
  ['cpp', '_f_cpp'],
  ['cs', '_f_csharp'],
  ['css', '_f_css'],
  ['csv', '_f_text'],
  ['cts', '_f_typescript'],
  ['cxx', '_f_cpp'],
  ['dart', '_f_dartlang'],
  ['db', '_f_db'],
  ['doc', '_f_word'],
  ['docx', '_f_word'],
  ['env', '_f_dotenv'],
  ['erb', '_f_erb'],
  ['erl', '_f_erlang'],
  ['ex', '_f_elixir'],
  ['exs', '_f_elixir'],
  ['fish', '_f_shell'],
  ['flac', '_f_audio'],
  ['fs', '_f_fsharp'],
  ['fsi', '_f_fsharp'],
  ['fsx', '_f_fsharp'],
  ['gif', '_f_image'],
  ['go', '_f_go'],
  ['gql', '_f_graphql'],
  ['gradle', '_f_gradle'],
  ['graphql', '_f_graphql'],
  ['groovy', '_f_groovy'],
  ['gz', '_f_zip'],
  ['h', '_f_h'],
  ['hpp', '_f_hpp'],
  ['htm', '_f_html'],
  ['html', '_f_html'],
  ['ini', '_f_tune'],
  ['jar', '_f_jar'],
  ['java', '_f_java'],
  ['jpeg', '_f_image'],
  ['jpg', '_f_image'],
  ['js', '_f_js'],
  ['json', '_f_json'],
  ['jsx', '_f_reactjs'],
  ['kt', '_f_kotlin'],
  ['kts', '_f_kotlin'],
  ['less', '_f_less'],
  ['lock', '_f_lock'],
  ['log', '_f_log'],
  ['lua', '_f_lua'],
  ['m4a', '_f_audio'],
  ['md', '_f_markdown'],
  ['mdx', '_f_mdx'],
  ['mjs', '_f_js'],
  ['mkd', '_f_markdown'],
  ['mkv', '_f_video'],
  ['mov', '_f_video'],
  ['mp3', '_f_audio'],
  ['mp4', '_f_video'],
  ['mts', '_f_typescript'],
  ['nim', '_f_nim'],
  ['nimble', '_f_nimble'],
  ['ogg', '_f_audio'],
  ['pdf', '_f_pdf'],
  ['php', '_f_php'],
  ['phtml', '_f_php'],
  ['pl', '_f_perl'],
  ['pm', '_f_perl'],
  ['png', '_f_image'],
  ['postcss', '_f_postcss'],
  ['ppt', '_f_powerpoint'],
  ['pptx', '_f_powerpoint'],
  ['proto', '_f_protobuf'],
  ['ps1', '_f_powershell'],
  ['py', '_f_python'],
  ['pyi', '_f_python'],
  ['pyw', '_f_python'],
  ['r', '_f_r'],
  ['rar', '_f_zip'],
  ['rb', '_f_ruby'],
  ['rmd', '_f_rmd'],
  ['rs', '_f_rust'],
  ['rst', '_f_markdown'],
  ['sass', '_f_sass'],
  ['sc', '_f_scala'],
  ['scala', '_f_scala'],
  ['scss', '_f_scss'],
  ['sh', '_f_shell'],
  ['sql', '_f_sql'],
  ['sqlite', '_f_sqlite'],
  ['styl', '_f_stylus'],
  ['stylus', '_f_stylus'],
  ['svelte', '_f_svelte'],
  ['svg', '_f_svg'],
  ['swift', '_f_swift'],
  ['tar', '_f_zip'],
  ['tex', '_f_tex'],
  ['tgz', '_f_zip'],
  ['toml', '_f_toml'],
  ['ts', '_f_typescript'],
  ['tsv', '_f_text'],
  ['tsx', '_f_reactts'],
  ['txt', '_f_text'],
  ['vb', '_f_vb'],
  ['vue', '_f_vue'],
  ['wav', '_f_audio'],
  ['webm', '_f_video'],
  ['webp', '_f_image'],
  ['xhtml', '_f_html'],
  ['xls', '_f_excel'],
  ['xlsx', '_f_excel'],
  ['xml', '_f_xml'],
  ['xsd', '_f_xml'],
  ['xsl', '_f_xml'],
  ['xslt', '_f_xml'],
  ['xz', '_f_zip'],
  ['yaml', '_f_yaml'],
  ['yml', '_f_yaml'],
  ['zip', '_f_zip'],
  ['zsh', '_f_shell']
];

const EXTRA_ALLOWED_ICON_IDS = [
  '_f_babel',
  '_f_bun',
  '_f_cargo',
  '_f_composer',
  '_f_docker',
  '_f_editorconfig',
  '_f_eslint',
  '_f_git',
  '_f_go_package',
  '_f_jsconfig',
  '_f_maven',
  '_f_npm',
  '_f_pip',
  '_f_pnpm',
  '_f_poetry',
  '_f_prettier',
  '_f_pypi',
  '_f_rollup',
  '_f_stylelint',
  '_f_tsconfig',
  '_f_vite',
  '_f_webpack',
  '_f_yarn'
];

const normalizeIconKey = (value: string | undefined | null): string =>
  String(value || '').trim().toLowerCase();

const normalizeThemeIconPath = (iconPath: string | undefined): string => {
  const rawPath = String(iconPath || '');
  if (!rawPath) {
    return '';
  }
  return `${WORKSPACE_ICON_BASE}/${rawPath.replace(WORKSPACE_ICON_PATH_RE, '')}`;
};

const buildWorkspaceThemeIconResolver = async (): Promise<WorkspaceThemeIconResolver> => {
  const themeModule = await import('@/assets/vscode-icons-theme.json');
  const theme = ((themeModule.default ?? themeModule) || {}) as WorkspaceIconTheme;
  const iconDefinitions = (theme.iconDefinitions || {}) as Record<string, { iconPath?: string }>;
  const fileExtensionIconMap = new Map(
    Object.entries(theme.fileExtensions || {}).map(([key, value]) => [
      normalizeIconKey(key),
      String(value || '')
    ])
  );
  const fallbackExtensionIconMap = new Map<string, string>(
    FALLBACK_EXTENSION_ICON_ENTRIES.filter(([, iconId]) => Boolean(iconDefinitions[iconId]))
  );
  const fileNameIconMap = new Map(
    Object.entries(theme.fileNames || {}).map(([key, value]) => [normalizeIconKey(key), String(value || '')])
  );
  const defaultFileIconId = String(theme.file || '');
  const allowedIconIds = new Set(
    [
      defaultFileIconId,
      ...fallbackExtensionIconMap.values(),
      ...EXTRA_ALLOWED_ICON_IDS.filter((iconId) => Boolean(iconDefinitions[iconId]))
    ].filter(Boolean)
  );

  const resolveThemeIconPath = (iconId: string, fallbackId = defaultFileIconId): string => {
    const resolvedId = iconId && allowedIconIds.has(iconId) ? iconId : '';
    const rawPath = resolvedId ? iconDefinitions[resolvedId]?.iconPath : '';
    const normalizedPath = normalizeThemeIconPath(rawPath);
    if (normalizedPath) {
      return normalizedPath;
    }
    if (fallbackId && fallbackId !== iconId) {
      return resolveThemeIconPath(fallbackId, '');
    }
    return '';
  };

  const resolveFileIconId = (entryName: string, extension: string): string => {
    const nameKey = normalizeIconKey(entryName);
    if (nameKey && fileNameIconMap.has(nameKey)) {
      return fileNameIconMap.get(nameKey) || defaultFileIconId;
    }
    const extensionKey = normalizeIconKey(extension);
    if (extensionKey) {
      if (fileExtensionIconMap.has(extensionKey)) {
        return fileExtensionIconMap.get(extensionKey) || defaultFileIconId;
      }
      if (fallbackExtensionIconMap.has(extensionKey)) {
        return fallbackExtensionIconMap.get(extensionKey) || defaultFileIconId;
      }
    }
    return defaultFileIconId;
  };

  return {
    resolveFileIconPath(entryName: string, extension: string): string {
      const iconId = resolveFileIconId(entryName, extension);
      return (
        resolveThemeIconPath(iconId, defaultFileIconId) ||
        resolveThemeIconPath(defaultFileIconId, '')
      );
    }
  };
};

let workspaceThemeIconResolverPromise: Promise<WorkspaceThemeIconResolver> | null = null;

export const loadWorkspaceThemeIconResolver = (): Promise<WorkspaceThemeIconResolver> => {
  workspaceThemeIconResolverPromise ??= buildWorkspaceThemeIconResolver();
  return workspaceThemeIconResolverPromise;
};
