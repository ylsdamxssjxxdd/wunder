const { performance } = require('node:perf_hooks')

const POST_FIRST_FRAME_DELAY_MS = 10

// ready-to-show is Electron's first rendered frame, not DOM/network readiness.
// Keep this gate independent of bridge readiness so a slow runtime cannot delay paint.
const waitForStartupFrame = (window, { timeoutMs = 15000 } = {}) =>
  new Promise((resolve, reject) => {
    let delayTimer
    let deadlineTimer
    const cleanup = () => {
      clearTimeout(delayTimer)
      clearTimeout(deadlineTimer)
      window.removeListener('ready-to-show', onFrame)
      window.removeListener('closed', onClosed)
    }
    const finish = (ready) => {
      cleanup()
      resolve(ready)
    }
    const onClosed = () => finish(false)
    const onFrame = () => {
      const deadline = performance.now() + POST_FIRST_FRAME_DELAY_MS
      const afterFrame = () => {
        const remaining = deadline - performance.now()
        if (remaining > 0) {
          delayTimer = setTimeout(afterFrame, Math.ceil(remaining))
          return
        }
        finish(!window.isDestroyed())
      }
      delayTimer = setTimeout(afterFrame, POST_FIRST_FRAME_DELAY_MS)
    }
    window.once('ready-to-show', onFrame)
    window.once('closed', onClosed)
    deadlineTimer = setTimeout(() => {
      cleanup()
      reject(new Error('Desktop startup frame timed out'))
    }, timeoutMs)
    if (window.isDestroyed()) finish(false)
  })

module.exports = { POST_FIRST_FRAME_DELAY_MS, waitForStartupFrame }
