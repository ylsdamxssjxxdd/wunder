const fs = require('fs')
const path = require('path')

// Allow staging builds to point back to the real repo root.
const repoRoot = process.env.WUNDER_REPO_ROOT
  ? path.resolve(process.env.WUNDER_REPO_ROOT)
  : path.resolve(__dirname, '..', '..', '..')
const outputRoot = path.resolve(__dirname, '..', 'resources')
const electronProjectRoot = path.resolve(__dirname, '..')
const skipRuntimeDepsCopy = process.env.WUNDER_SKIP_RUNTIME_DEPS_COPY === '1'
const includeCli = process.env.WUNDER_INCLUDE_CLI !== '0'
const extraRuntimeFiles = String(process.env.WUNDER_EXTRA_RUNTIME_FILES || '')
  .split(path.delimiter)
  .map((item) => item.trim())
  .filter((item) => item)
const extraRuntimeRoots = String(process.env.WUNDER_EXTRA_RUNTIME_ROOTS || '')
  .split(path.delimiter)
  .map((item) => item.trim())
  .filter((item) => item)

const bridgeName = process.platform === 'win32' ? 'wunder-desktop-bridge.exe' : 'wunder-desktop-bridge'
const cliName = process.platform === 'win32' ? 'wunder-cli.exe' : 'wunder-cli'
const bridgeSource = process.env.WUNDER_BRIDGE_BIN || path.join(repoRoot, 'target', 'release', bridgeName)
const cliSource = process.env.WUNDER_CLI_BIN || path.join(repoRoot, 'target', 'release', cliName)
const frontendSource = process.env.WUNDER_FRONTEND_DIST || path.join(repoRoot, 'frontend', 'dist')
const desktopPreconfigSource = path.join(repoRoot, 'docs', '分发', '预配置文件.yml')
const buildIconIcoSource = path.join(__dirname, '..', 'build', 'icon.ico')
const fallbackIconIcoSource = path.join(__dirname, '..', 'assets', 'icon.ico')
const iconIcoSource = fs.existsSync(buildIconIcoSource) ? buildIconIcoSource : fallbackIconIcoSource
const buildIconPngSource = path.join(__dirname, '..', 'build', 'icon.png')
const fallbackIconPngSource = path.join(__dirname, '..', 'assets', 'icon.png')
const iconPngSource = fs.existsSync(buildIconPngSource) ? buildIconPngSource : fallbackIconPngSource
const linuxIconSetDir = path.join(__dirname, '..', 'build', 'icons')
const linuxIconSetSizes = [16, 24, 32, 48, 64, 96, 128, 256, 512]
const runtimeNodeModulesSource = path.join(electronProjectRoot, 'node_modules')
const runtimeDepsOutputRoot = path.join(outputRoot, 'runtime-deps')

const copyDirWithFilter = (src, dest, filter) => {
  fs.cpSync(src, dest, {
    recursive: true,
    filter: (source) => filter(source)
  })
}

const copyDir = (src, dest) => {
  fs.cpSync(src, dest, { recursive: true })
}

const copyDirIfExists = (src, dest) => {
  if (!fs.existsSync(src)) {
    console.warn(`[prepare] skip missing: ${src}`)
    return
  }
  copyDir(src, dest)
}

const copyRuntimeNodeModules = () => {
  if (skipRuntimeDepsCopy) {
    console.log('[prepare] skip runtime deps copy: WUNDER_SKIP_RUNTIME_DEPS_COPY=1')
    return
  }
  if (!fs.existsSync(runtimeNodeModulesSource)) {
    console.warn(`[prepare] skip runtime deps: ${runtimeNodeModulesSource}`)
    return
  }
  copyDirWithFilter(runtimeNodeModulesSource, runtimeDepsOutputRoot, (source) => {
    const relative = path.relative(runtimeNodeModulesSource, source)
    if (!relative) {
      return true
    }
    const normalized = relative.replace(/\\/g, '/')
    if (normalized === '.bin' || normalized.startsWith('.bin/')) {
      return false
    }
    if (normalized === '.package-lock.json') {
      return false
    }
    if (normalized === 'wunder-workspace' || normalized.startsWith('wunder-workspace/')) {
      return false
    }
    return true
  })
  console.log(`[prepare] copied runtime deps to: ${runtimeDepsOutputRoot}`)
}

const copyFile = (src, dest) => {
  fs.mkdirSync(path.dirname(dest), { recursive: true })
  fs.copyFileSync(src, dest)
}

const copyExtraRuntimeFiles = () => {
  for (const source of extraRuntimeFiles) {
    if (!fs.existsSync(source)) {
      console.warn(`[prepare] skip missing extra runtime file: ${source}`)
      continue
    }
    const targetPath = path.join(outputRoot, path.basename(source))
    copyFile(source, targetPath)
    console.log(`[prepare] copied extra runtime file: ${targetPath}`)
  }
}

const copyExtraRuntimeRoots = () => {
  for (const source of extraRuntimeRoots) {
    if (!fs.existsSync(source)) {
      console.warn(`[prepare] skip missing extra runtime root: ${source}`)
      continue
    }
    const stat = fs.statSync(source)
    if (!stat.isDirectory()) {
      console.warn(`[prepare] extra runtime root is not a directory: ${source}`)
      continue
    }
    for (const entry of fs.readdirSync(source)) {
      const sourcePath = path.join(source, entry)
      const targetPath = path.join(outputRoot, entry)
      fs.cpSync(sourcePath, targetPath, { recursive: true, force: true })
      console.log(`[prepare] merged extra runtime entry: ${targetPath}`)
    }
  }
}

const ensureLinuxIconSet = () => {
  if (!fs.existsSync(iconPngSource)) {
    console.warn('[prepare] skip linux icon set: icon.png not found')
    return
  }
  fs.rmSync(linuxIconSetDir, { recursive: true, force: true })
  fs.mkdirSync(linuxIconSetDir, { recursive: true })
  for (const size of linuxIconSetSizes) {
    copyFile(iconPngSource, path.join(linuxIconSetDir, `${size}x${size}.png`))
  }
}

if (!fs.existsSync(bridgeSource)) {
  console.error(`[prepare] bridge binary not found: ${bridgeSource}`)
  console.error('[prepare] build it first: cargo build --release --bin wunder-desktop-bridge')
  process.exit(1)
}

if (!fs.existsSync(frontendSource)) {
  console.error(`[prepare] frontend dist not found: ${frontendSource}`)
  console.error('[prepare] build it first: npm run build --workspace wunder-frontend')
  process.exit(1)
}

fs.rmSync(outputRoot, { recursive: true, force: true })
fs.mkdirSync(outputRoot, { recursive: true })
ensureLinuxIconSet()

copyFile(bridgeSource, path.join(outputRoot, bridgeName))
if (includeCli && fs.existsSync(cliSource)) {
  copyFile(cliSource, path.join(outputRoot, cliName))
}
if (fs.existsSync(iconIcoSource)) {
  copyFile(iconIcoSource, path.join(outputRoot, 'icon.ico'))
}
if (fs.existsSync(iconPngSource)) {
  copyFile(iconPngSource, path.join(outputRoot, 'icon.png'))
}
if (process.platform !== 'win32') {
  try {
    fs.chmodSync(path.join(outputRoot, bridgeName), 0o755)
  } catch (err) {
    console.warn('[prepare] failed to chmod bridge binary:', err)
  }
  if (includeCli && fs.existsSync(path.join(outputRoot, cliName))) {
    try {
      fs.chmodSync(path.join(outputRoot, cliName), 0o755)
    } catch (err) {
      console.warn('[prepare] failed to chmod cli binary:', err)
    }
  }
}

copyDir(frontendSource, path.join(outputRoot, 'frontend-dist'))
copyRuntimeNodeModules()
copyExtraRuntimeFiles()
copyExtraRuntimeRoots()
if (fs.existsSync(iconPngSource)) {
  copyFile(iconPngSource, path.join(outputRoot, 'frontend-dist', 'desktop-icon.png'))
}
copyDirIfExists(path.join(repoRoot, 'config'), path.join(outputRoot, 'config'))
if (fs.existsSync(desktopPreconfigSource)) {
  copyFile(desktopPreconfigSource, path.join(outputRoot, 'config', 'wunder.desktop.preconfig.yaml'))
}
copyDirIfExists(path.join(repoRoot, 'prompts'), path.join(outputRoot, 'prompts'))
copyDirIfExists(path.join(repoRoot, 'skills'), path.join(outputRoot, 'skills'))
copyDirIfExists(path.join(repoRoot, 'scripts'), path.join(outputRoot, 'scripts'))

console.log(`[prepare] resources ready at: ${outputRoot}`)
