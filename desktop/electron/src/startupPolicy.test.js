const test = require('node:test')
const assert = require('node:assert/strict')

const {
  DEFAULT_LOADING_SHELL_DELAY_MS,
  resolveLoadingShellDelayMs
} = require('./startupPolicy')

test('shows the loading shell immediately by default', () => {
  assert.equal(DEFAULT_LOADING_SHELL_DELAY_MS, 0)
  assert.equal(resolveLoadingShellDelayMs(undefined), 0)
})

test('allows an explicit loading shell delay for diagnostics', () => {
  assert.equal(resolveLoadingShellDelayMs('180'), 180)
  assert.equal(resolveLoadingShellDelayMs('-1'), 0)
  assert.equal(resolveLoadingShellDelayMs('invalid'), 0)
})
