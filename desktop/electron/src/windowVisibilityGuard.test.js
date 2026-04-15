const test = require('node:test')
const assert = require('node:assert/strict')

const {
  DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS,
  createWindowVisibilityGuard
} = require('./windowVisibilityGuard')

test('blocks implicit auto-show shortly after minimize', () => {
  const guard = createWindowVisibilityGuard({ minimizeRestoreCooldownMs: 1000 })
  guard.markManualMinimize(100)

  assert.equal(guard.isAutoShowBlocked({ now: 250 }), true)
  assert.equal(guard.isAutoShowBlocked({ now: 1100 }), false)
})

test('explicit restore bypasses recent minimize protection', () => {
  const guard = createWindowVisibilityGuard({
    minimizeRestoreCooldownMs: DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS
  })
  guard.markManualMinimize(500)

  assert.equal(guard.isAutoShowBlocked({ now: 700, explicit: true }), false)
})

test('hidden capture restore stays suppressed while window remains minimized', () => {
  const guard = createWindowVisibilityGuard({ minimizeRestoreCooldownMs: 1000 })

  assert.equal(
    guard.shouldRestoreAfterHiddenCapture({
      now: 100,
      window: {
        isMinimized: () => true
      }
    }),
    false
  )
})

test('clearing minimize mark re-enables automatic restore', () => {
  const guard = createWindowVisibilityGuard({ minimizeRestoreCooldownMs: 1000 })
  guard.markManualMinimize(100)
  guard.clearManualMinimize()

  assert.equal(guard.isAutoShowBlocked({ now: 150 }), false)
  assert.equal(guard.shouldRestoreAfterHiddenCapture({ now: 150 }), true)
})
