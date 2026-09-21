const test = require('node:test')
const assert = require('node:assert/strict')

const {
  parseNonNegativeNumber
} = require('./startupPolicy')

test('uses the default for absent or invalid timing values', () => {
  assert.equal(parseNonNegativeNumber(undefined, 10), 10)
  assert.equal(parseNonNegativeNumber('-1', 10), 10)
  assert.equal(parseNonNegativeNumber('invalid', 10), 10)
})

test('accepts zero and positive timing values', () => {
  assert.equal(parseNonNegativeNumber('180', 10), 180)
  assert.equal(parseNonNegativeNumber('0', 10), 0)
})
