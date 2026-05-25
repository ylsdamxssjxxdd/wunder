const parseWindowsRelease = (value) => {
  const parts = String(value || '')
    .split('.')
    .map((part) => Number.parseInt(part, 10))
  const major = Number.isFinite(parts[0]) ? parts[0] : 0
  const minor = Number.isFinite(parts[1]) ? parts[1] : 0
  return { major, minor }
}

const isWindows7OrOlder = ({ platform = process.platform, release = '' } = {}) => {
  if (platform !== 'win32') {
    return false
  }
  const { major, minor } = parseWindowsRelease(release)
  return major > 0 && (major < 6 || (major === 6 && minor <= 1))
}

const isEnvEnabled = (env, key) => String((env || {})[key] || '').trim() === '1'

const resolveDesktopEffectWindowsDisabledReason = ({
  env = process.env,
  platform = process.platform,
  release = ''
} = {}) => {
  if (isEnvEnabled(env, 'WUNDER_DISABLE_DESKTOP_EFFECT_WINDOWS')) {
    return 'env'
  }
  if (isEnvEnabled(env, 'WUNDER_ENABLE_DESKTOP_EFFECT_WINDOWS')) {
    return ''
  }
  if (isWindows7OrOlder({ platform, release })) {
    return 'windows7'
  }
  return ''
}

const shouldDisableElectronHardwareAcceleration = ({
  env = process.env,
  platform = process.platform,
  release = ''
} = {}) =>
  isEnvEnabled(env, 'WUNDER_DISABLE_GPU') ||
  isEnvEnabled(env, 'WUNDER_DESKTOP_RENDERER_COMPAT_MODE') ||
  isWindows7OrOlder({ platform, release })

module.exports = {
  isWindows7OrOlder,
  parseWindowsRelease,
  resolveDesktopEffectWindowsDisabledReason,
  shouldDisableElectronHardwareAcceleration
}
