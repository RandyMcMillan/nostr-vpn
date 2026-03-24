export const NETWORK_INVITE_PREFIX = 'nvpn://invite/'
const DEBUG_DEEP_LINK_PREFIX = 'nvpn://debug/'

export const parseAppDeepLink = (value) => {
  const trimmed = value.trim()
  if (!trimmed) {
    return null
  }

  if (trimmed.startsWith(NETWORK_INVITE_PREFIX) && trimmed.length > NETWORK_INVITE_PREFIX.length) {
    return { type: 'invite', invite: trimmed }
  }

  if (!trimmed.startsWith(DEBUG_DEEP_LINK_PREFIX)) {
    return null
  }

  let parsed
  try {
    parsed = new URL(trimmed)
  } catch {
    return null
  }

  if (parsed.protocol !== 'nvpn:' || parsed.hostname !== 'debug') {
    return null
  }

  const action = parsed.pathname.replace(/^\/+/, '')
  switch (action) {
    case 'request-join':
      return { type: 'request-join' }
    case 'accept-join': {
      const requesterNpub = parsed.searchParams.get('requester')?.trim() || ''
      if (!requesterNpub) {
        return null
      }
      return { type: 'accept-join', requesterNpub }
    }
    case 'tick':
      return { type: 'tick' }
    default:
      return null
  }
}
