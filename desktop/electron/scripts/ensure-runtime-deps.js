const fs = require('fs')
const path = require('path')
const { spawnSync } = require('child_process')

const projectDir = path.resolve(__dirname, '..')
const runtimeModule = path.join(projectDir, 'node_modules', 'electron-updater', 'package.json')
const workspaceLink = path.join(projectDir, 'node_modules', 'wunder-workspace')

const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm'

const removeWorkspaceLink = () => {
  if (!fs.existsSync(workspaceLink)) {
    return
  }
  fs.rmSync(workspaceLink, { recursive: true, force: true })
  console.log(`[runtime-deps] removed workspace link: ${workspaceLink}`)
}

const installRuntimeDeps = () => {
  console.log('[runtime-deps] installing local production dependencies')
  const result = spawnSync(npmCommand, ['install', '--omit=dev', '--workspaces=false'], {
    cwd: projectDir,
    stdio: 'inherit'
  })
  if (result.status !== 0) {
    process.exit(result.status || 1)
  }
}

// Keep runtime dependencies local to the Electron app directory so the packaged
// app does not depend on workspace-hoisted modules that electron-builder may skip.
if (!fs.existsSync(runtimeModule)) {
  installRuntimeDeps()
}

removeWorkspaceLink()

if (!fs.existsSync(runtimeModule)) {
  console.error(`[runtime-deps] missing runtime dependency after install: ${runtimeModule}`)
  process.exit(1)
}

console.log('[runtime-deps] local production dependencies are ready')
