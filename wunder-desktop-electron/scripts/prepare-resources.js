const fs = require('fs')
const path = require('path')

const repoRoot = path.resolve(__dirname, '..', '..')
const outputRoot = path.resolve(__dirname, '..', 'resources')

const bridgeName = process.platform === 'win32' ? 'wunder-desktop-bridge.exe' : 'wunder-desktop-bridge'
const bridgeSource = process.env.WUNDER_BRIDGE_BIN || path.join(repoRoot, 'target', 'release', bridgeName)
const frontendSource = path.join(repoRoot, 'frontend', 'dist')

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

copyFile(bridgeSource, path.join(outputRoot, bridgeName))
if (process.platform !== 'win32') {
  try {
    fs.chmodSync(path.join(outputRoot, bridgeName), 0o755)
  } catch (err) {
    console.warn('[prepare] failed to chmod bridge binary:', err)
  }
}

copyDir(frontendSource, path.join(outputRoot, 'frontend-dist'))
copyDirIfExists(path.join(repoRoot, 'config'), path.join(outputRoot, 'config'))
copyDirIfExists(path.join(repoRoot, 'prompts'), path.join(outputRoot, 'prompts'))
copyDirIfExists(path.join(repoRoot, 'skills'), path.join(outputRoot, 'skills'))
copyDirIfExists(path.join(repoRoot, 'scripts'), path.join(outputRoot, 'scripts'))

console.log(`[prepare] resources ready at: ${outputRoot}`)
