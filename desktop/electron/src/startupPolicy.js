const DEFAULT_LOADING_SHELL_DELAY_MS = 0

const parseNonNegativeNumber = (raw, fallbackValue) => {
  const parsed = Number(raw)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallbackValue
}

// A loading surface is cheaper than a blank native window, especially on legacy hardware.
const resolveLoadingShellDelayMs = (rawValue) =>
  parseNonNegativeNumber(rawValue, DEFAULT_LOADING_SHELL_DELAY_MS)

module.exports = {
  DEFAULT_LOADING_SHELL_DELAY_MS,
  parseNonNegativeNumber,
  resolveLoadingShellDelayMs
}
