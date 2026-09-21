const test = require('node:test')
const assert = require('node:assert/strict')
const { EventEmitter } = require('node:events')
const { performance } = require('node:perf_hooks')
const { POST_FIRST_FRAME_DELAY_MS, waitForStartupFrame } = require('./startupFrame')

const createWindow = () => Object.assign(new EventEmitter(), { isDestroyed: () => false })
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

test('bridge work waits for the first frame and at least 10ms afterwards', async () => {
  const window = createWindow()
  let started = false
  const ready = waitForStartupFrame(window).then((value) => { started = value })
  await sleep(25)
  assert.equal(started, false)
  const frameAt = performance.now()
  window.emit('ready-to-show')
  assert.equal(started, false)
  await ready
  assert.equal(started, true)
  assert.ok(performance.now() - frameAt >= POST_FIRST_FRAME_DELAY_MS)
  assert.deepEqual(window.eventNames(), [])
})

test('closing before or after the frame cancels startup and removes listeners', async () => {
  for (const painted of [false, true]) {
    const window = createWindow()
    const ready = waitForStartupFrame(window)
    if (painted) window.emit('ready-to-show')
    window.emit('closed')
    assert.equal(await ready, false)
    assert.deepEqual(window.eventNames(), [])
  }
})

test('missing first frame reports failure without starting bridge work', async () => {
  const window = createWindow()
  await assert.rejects(waitForStartupFrame(window, { timeoutMs: 15 }), /frame timed out/)
  assert.deepEqual(window.eventNames(), [])
})

test('already destroyed windows do not leave a pending startup', async () => {
  const window = createWindow()
  window.isDestroyed = () => true
  assert.equal(await waitForStartupFrame(window), false)
  assert.deepEqual(window.eventNames(), [])
})
