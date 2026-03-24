const ASCII_ALNUM = /^[A-Za-z0-9]$/

export const normalizeNodeNameDnsLabel = (value) => {
  let label = ''
  let previousDash = false

  for (const ch of value) {
    if (ASCII_ALNUM.test(ch)) {
      label += ch.toLowerCase()
      previousDash = false
    } else if (!previousDash) {
      label += '-'
      previousDash = true
    }
  }

  label = label.replace(/^-+/, '').replace(/-+$/, '')
  if (!label) {
    return ''
  }

  if (label.length > 63) {
    label = label.slice(0, 63).replace(/-+$/, '')
  }

  return label
}

export const normalizeNodeNameDnsSuffix = (value) =>
  value
    .trim()
    .replace(/\.+$/, '')
    .split('.')
    .map(normalizeNodeNameDnsLabel)
    .filter(Boolean)
    .join('.')

export const nodeNameDnsPreview = (name, suffix) => {
  const label = normalizeNodeNameDnsLabel(name)
  if (!label) {
    return ''
  }

  const normalizedSuffix = normalizeNodeNameDnsSuffix(suffix)
  return normalizedSuffix ? `${label}.${normalizedSuffix}` : label
}
