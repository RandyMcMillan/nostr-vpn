import test from 'node:test'
import assert from 'node:assert/strict'

import {
  nodeNameDnsPreview,
  normalizeNodeNameDnsLabel,
  normalizeNodeNameDnsSuffix,
} from './node-name.js'

test('normalizeNodeNameDnsLabel lowercases and turns whitespace into dashes', () => {
  assert.equal(normalizeNodeNameDnsLabel('Martti iPhone 12 Pro'), 'martti-iphone-12-pro')
})

test('normalizeNodeNameDnsLabel collapses punctuation into a single dash', () => {
  assert.equal(normalizeNodeNameDnsLabel("Martti's   iPhone!!!"), 'martti-s-iphone')
})

test('normalizeNodeNameDnsSuffix preserves dotted suffixes', () => {
  assert.equal(normalizeNodeNameDnsSuffix('  Mesh.Home  '), 'mesh.home')
})

test('nodeNameDnsPreview combines the sanitized label with the suffix', () => {
  assert.equal(nodeNameDnsPreview('My Pocket Router', 'nvpn'), 'my-pocket-router.nvpn')
})
