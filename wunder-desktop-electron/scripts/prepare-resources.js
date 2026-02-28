const fs = require('fs')
const path = require('path')

const repoRoot = path.resolve(__dirname, '..', '..')
const outputRoot = path.resolve(__dirname, '..', 'resources')

const bridgeName = process.platform === 'win32' ? 'wunder-desktop-bridge.exe' : 'wunder-desktop-bridge'
const bridgeSource = process.env.WUNDER_BRIDGE_BIN || path.join(repoRoot, 'target', 'release', bridgeName)
const frontendSource = path.join(repoRoot, 'frontend', 'dist')
const buildIconIcoSource = path.join(__dirname, '..', 'build', 'icon.ico')
const fallbackIconIcoSource = path.join(__dirname, '..', 'assets', 'icon.ico')
const iconIcoSource = fs.existsSync(buildIconIcoSource) ? buildIconIcoSource : fallbackIconIcoSource
const buildIconPngSource = path.join(__dirname, '..', 'build', 'icon.png')
const fallbackIconPngSource = path.join(__dirname, '..', 'assets', 'icon.png')
const iconPngSource = fs.existsSync(buildIconPngSource) ? buildIconPngSource : fallbackIconPngSource
const linuxIconSetDir = path.join(__dirname, '..', 'build', 'icons')
const linuxIconSetSizes = [16, 24, 32, 48, 64, 96, 128, 256, 512]

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

const copyFile = (src, dest) => {
  fs.mkdirSync(path.dirname(dest), { recursive: true })
  fs.copyFileSync(src, dest)
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
  console.error('[prepare] build it first: (cd frontend && npm run build)')
  process.exit(1)
}

fs.rmSync(outputRoot, { recursive: true, force: true })
fs.mkdirSync(outputRoot, { recursive: true })
ensureLinuxIconSet()

copyFile(bridgeSource, path.join(outputRoot, bridgeName))
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
}

copyDir(frontendSource, path.join(outputRoot, 'frontend-dist'))
if (fs.existsSync(iconPngSource)) {
  copyFile(iconPngSource, path.join(outputRoot, 'frontend-dist', 'desktop-icon.png'))
}
copyDirIfExists(path.join(repoRoot, 'config'), path.join(outputRoot, 'config'))
copyDirIfExists(path.join(repoRoot, 'prompts'), path.join(outputRoot, 'prompts'))
copyDirIfExists(path.join(repoRoot, 'skills'), path.join(outputRoot, 'skills'))
copyDirIfExists(path.join(repoRoot, 'scripts'), path.join(outputRoot, 'scripts'))

console.log(`[prepare] resources ready at: ${outputRoot}`)
