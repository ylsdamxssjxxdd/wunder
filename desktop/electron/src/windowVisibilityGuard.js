const DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS = 1200

const normalizeNonNegativeNumber = (value, fallbackValue) => {
  const parsed = Number(value)
  if (Number.isFinite(parsed) && parsed >= 0) {
    return parsed
  }
  return fallbackValue
}

const normalizeNow = (value) => {
  const parsed = Number(value)
  if (Number.isFinite(parsed) && parsed >= 0) {
    return parsed
  }
  return Date.now()
}

const createWindowVisibilityGuard = (options = {}) => {
  const minimizeRestoreCooldownMs = normalizeNonNegativeNumber(
    options.minimizeRestoreCooldownMs,
    DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS
  )
  let lastManualMinimizeAt = 0

  const markManualMinimize = (now) => {
    lastManualMinimizeAt = normalizeNow(now)
    return lastManualMinimizeAt
  }

  const clearManualMinimize = () => {
    lastManualMinimizeAt = 0
  }

  const isWithinRecentMinimizeWindow = (now) => {
    if (!lastManualMinimizeAt || minimizeRestoreCooldownMs === 0) {
      return false
    }
    return normalizeNow(now) - lastManualMinimizeAt < minimizeRestoreCooldownMs
  }

  const isAutoShowBlocked = (options = {}) => {
    if (options.explicit === true) {
      return false
    }
    return isWithinRecentMinimizeWindow(options.now)
  }

  const shouldRestoreAfterHiddenCapture = (options = {}) => {
    if (options.window && typeof options.window.isMinimized === 'function' && options.window.isMinimized()) {
      return false
    }
    return !isAutoShowBlocked(options)
  }

  return {
    minimizeRestoreCooldownMs,
    markManualMinimize,
    clearManualMinimize,
    isAutoShowBlocked,
    isWithinRecentMinimizeWindow,
    shouldRestoreAfterHiddenCapture,
    getLastManualMinimizeAt: () => lastManualMinimizeAt
  }
}

module.exports = {
  DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS,
  createWindowVisibilityGuard
}
