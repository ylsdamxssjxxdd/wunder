const parseNonNegativeNumber = (raw, fallbackValue) => {
  const parsed = Number(raw)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallbackValue
}

module.exports = {
  parseNonNegativeNumber
}
