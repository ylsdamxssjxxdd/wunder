const test = require('node:test')
const assert = require('node:assert/strict')

const {
  isWindows7OrOlder,
  resolveDesktopEffectWindowsDisabledReason,
  shouldDisableElectronHardwareAcceleration
} = require('./desktopCompatibility')

test('detects Windows 7 and older releases only', () => {
  assert.equal(isWindows7OrOlder({ platform: 'win32', release: '6.1.7601' }), true)
  assert.equal(isWindows7OrOlder({ platform: 'win32', release: '6.0.6002' }), true)
  assert.equal(isWindows7OrOlder({ platform: 'win32', release: '10.0.22631' }), false)
  assert.equal(isWindows7OrOlder({ platform: 'linux', release: '6.1.7601' }), false)
})

test('keeps native desktop effect windows enabled on modern Windows by default', () => {
  assert.equal(
    resolveDesktopEffectWindowsDisabledReason({
      env: {},
      platform: 'win32',
      release: '10.0.22631'
    }),
    ''
  )
})

test('disables native desktop effect windows on Windows 7 unless explicitly forced', () => {
  assert.equal(
    resolveDesktopEffectWindowsDisabledReason({
      env: {},
      platform: 'win32',
      release: '6.1.7601'
    }),
    'windows7'
  )
  assert.equal(
    resolveDesktopEffectWindowsDisabledReason({
      env: { WUNDER_ENABLE_DESKTOP_EFFECT_WINDOWS: '1' },
      platform: 'win32',
      release: '6.1.7601'
    }),
    ''
  )
})

test('explicit desktop effect disable env wins over force env', () => {
  assert.equal(
    resolveDesktopEffectWindowsDisabledReason({
      env: {
        WUNDER_DISABLE_DESKTOP_EFFECT_WINDOWS: '1',
        WUNDER_ENABLE_DESKTOP_EFFECT_WINDOWS: '1'
      },
      platform: 'win32',
      release: '10.0.22631'
    }),
    'env'
  )
})

test('hardware acceleration stays enabled on modern Windows unless explicitly disabled', () => {
  assert.equal(
    shouldDisableElectronHardwareAcceleration({
      env: {},
      platform: 'win32',
      release: '10.0.22631'
    }),
    false
  )
  assert.equal(
    shouldDisableElectronHardwareAcceleration({
      env: { WUNDER_DISABLE_GPU: '1' },
      platform: 'win32',
      release: '10.0.22631'
    }),
    true
  )
  assert.equal(
    shouldDisableElectronHardwareAcceleration({
      env: { WUNDER_DESKTOP_RENDERER_COMPAT_MODE: '1' },
      platform: 'win32',
      release: '10.0.22631'
    }),
    true
  )
  assert.equal(
    shouldDisableElectronHardwareAcceleration({
      env: { WUNDER_SIDECAR_RUNTIME: '1' },
      platform: 'win32',
      release: '10.0.22631'
    }),
    false
  )
})
