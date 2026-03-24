import test from 'node:test'
import assert from 'node:assert/strict'

import { parseAppDeepLink } from './deep-link-actions.js'

test('parseAppDeepLink recognizes invite URLs', () => {
  const invite = 'nvpn://invite/payload-123'
  assert.deepEqual(parseAppDeepLink(invite), {
    type: 'invite',
    invite,
  })
})

test('parseAppDeepLink recognizes debug request and tick URLs', () => {
  assert.deepEqual(parseAppDeepLink('nvpn://debug/request-join'), {
    type: 'request-join',
  })
  assert.deepEqual(parseAppDeepLink('nvpn://debug/tick'), {
    type: 'tick',
  })
})

test('parseAppDeepLink recognizes debug accept URLs', () => {
  assert.deepEqual(
    parseAppDeepLink('nvpn://debug/accept-join?requester=npub1requester'),
    {
      type: 'accept-join',
      requesterNpub: 'npub1requester',
    },
  )
})

test('parseAppDeepLink ignores invalid or incomplete URLs', () => {
  assert.equal(parseAppDeepLink(''), null)
  assert.equal(parseAppDeepLink('https://example.com'), null)
  assert.equal(parseAppDeepLink('nvpn://invite/'), null)
  assert.equal(parseAppDeepLink('nvpn://debug/accept-join'), null)
  assert.equal(parseAppDeepLink('nvpn://debug/unknown'), null)
})
