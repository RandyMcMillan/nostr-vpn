import { accessSync, constants, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { setTimeout as delay } from 'node:timers/promises'

export function log(message) {
  console.log(`[tauri-e2e] ${message}`)
}

export function assertExecutable(filePath) {
  try {
    accessSync(filePath, constants.X_OK)
  } catch (error) {
    throw new Error(`required executable missing or not executable: ${filePath} (${String(error)})`)
  }
}

export function assertTunAvailable() {
  try {
    accessSync('/dev/net/tun', constants.R_OK | constants.W_OK)
  } catch (error) {
    throw new Error(`/dev/net/tun is unavailable; run with NET_ADMIN and tun device (${String(error)})`)
  }
}

export async function runChecked(cmd, args, options = {}) {
  const {
    cwd = process.cwd(),
    env = process.env,
    timeoutMs = 20_000,
  } = options

  return await new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let stderr = ''
    let timedOut = false

    const timeout = setTimeout(() => {
      timedOut = true
      child.kill('SIGKILL')
    }, timeoutMs)

    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })
    child.on('error', (error) => {
      clearTimeout(timeout)
      reject(new Error(`failed to spawn ${cmd}: ${error.message}`))
    })
    child.on('exit', (code, signal) => {
      clearTimeout(timeout)
      if (timedOut) {
        reject(new Error(`command timed out: ${cmd} ${args.join(' ')}`))
        return
      }

      if (code !== 0) {
        reject(
          new Error(
            `command failed (${code ?? signal}): ${cmd} ${args.join(' ')}\nstdout:\n${stdout}\nstderr:\n${stderr}`,
          ),
        )
        return
      }

      resolve({ stdout, stderr })
    })
  })
}

export function spawnManaged(name, cmd, args, options = {}) {
  const child = spawn(cmd, args, {
    cwd: options.cwd || process.cwd(),
    env: options.env || process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
  })

  const meta = {
    name,
    process: child,
    stdout: '',
    stderr: '',
    exited: false,
    exitCode: null,
    exitSignal: null,
  }

  child.stdout.on('data', (chunk) => {
    const text = chunk.toString()
    meta.stdout += text
    process.stdout.write(`[${name}] ${text}`)
  })

  child.stderr.on('data', (chunk) => {
    const text = chunk.toString()
    meta.stderr += text
    process.stderr.write(`[${name}] ${text}`)
  })

  child.on('exit', (code, signal) => {
    meta.exited = true
    meta.exitCode = code
    meta.exitSignal = signal
  })

  child.on('error', (error) => {
    meta.stderr += `\nspawn error: ${error.message}`
  })

  return meta
}

export async function stopManaged(meta, signal = 'SIGINT', timeoutMs = 15_000) {
  if (meta.exited) {
    return
  }

  meta.process.kill(signal)
  const started = Date.now()
  while (!meta.exited && Date.now() - started < timeoutMs) {
    await delay(100)
  }

  if (!meta.exited) {
    meta.process.kill('SIGKILL')
    throw new Error(`failed to stop ${meta.name} via ${signal} within ${timeoutMs}ms`)
  }
}

export async function waitForProcessOutput(meta, matcher, description, timeoutMs = 30_000) {
  const started = Date.now()

  while (Date.now() - started < timeoutMs) {
    if (meta.exited) {
      throw new Error(
        `${meta.name} exited before ${description} (code=${meta.exitCode}, signal=${meta.exitSignal})\nstdout:\n${meta.stdout}\nstderr:\n${meta.stderr}`,
      )
    }

    if (matcher.test(meta.stdout) || matcher.test(meta.stderr)) {
      return
    }

    await delay(200)
  }

  throw new Error(
    `timed out waiting for ${description} from ${meta.name}\nstdout:\n${meta.stdout}\nstderr:\n${meta.stderr}`,
  )
}

export function npubFromConfig(configPath) {
  const content = readFileSync(configPath, 'utf8')
  const nostrSection = content.split('[node]')[0] || content
  const match = nostrSection.match(/^public_key\s*=\s*"([^"]+)"/m)
  if (!match) {
    throw new Error(`could not parse nostr public_key from ${configPath}`)
  }

  return match[1]
}

export function extractJsonDocument(raw) {
  const start = raw.indexOf('{')
  const end = raw.lastIndexOf('}')
  if (start < 0 || end < start) {
    throw new Error(`command output did not include JSON document: ${raw}`)
  }
  return raw.slice(start, end + 1)
}

async function http(base, method, endpoint, body) {
  const response = await fetch(`${base}${endpoint}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })

  const json = await response.json().catch(() => ({}))
  if (!response.ok || json.value?.error) {
    const detail = json.value?.message || JSON.stringify(json)
    throw new Error(`${method} ${endpoint} failed: ${detail}`)
  }

  return json
}

export async function executeScript(base, sessionId, script, args = []) {
  const response = await http(base, 'POST', `/session/${sessionId}/execute/sync`, {
    script,
    args,
  })
  return response.value
}

export async function waitForDriverReady(base, timeoutMs = 20_000) {
  const started = Date.now()

  while (Date.now() - started < timeoutMs) {
    try {
      const status = await fetch(`${base}/status`)
      if (status.ok) {
        return
      }
    } catch {
      // Keep polling.
    }

    await delay(250)
  }

  throw new Error(`tauri-driver at ${base} did not become ready`)
}

function elementId(value) {
  return value?.['element-6066-11e4-a52e-4f735466cecf'] || value?.ELEMENT
}

export async function createSession(base, appPath) {
  const payload = {
    capabilities: {
      alwaysMatch: {
        browserName: 'wry',
        'tauri:options': {
          application: appPath,
        },
      },
    },
  }

  const response = await http(base, 'POST', '/session', payload)
  const sessionId = response.value?.sessionId || response.sessionId
  if (!sessionId) {
    throw new Error(`missing webdriver session id: ${JSON.stringify(response)}`)
  }

  return sessionId
}

export async function deleteSession(base, sessionId) {
  await http(base, 'DELETE', `/session/${sessionId}`).catch(() => {})
}

export async function find(base, sessionId, selector) {
  const response = await http(base, 'POST', `/session/${sessionId}/element`, {
    using: 'css selector',
    value: selector,
  })

  const id = elementId(response.value)
  if (!id) {
    throw new Error(`missing element id for selector ${selector}`)
  }

  return id
}

export async function findAll(base, sessionId, selector) {
  const response = await http(base, 'POST', `/session/${sessionId}/elements`, {
    using: 'css selector',
    value: selector,
  })

  return (response.value || []).map((entry) => elementId(entry)).filter(Boolean)
}

export async function getText(base, sessionId, id) {
  const response = await http(base, 'GET', `/session/${sessionId}/element/${id}/text`)
  return String(response.value || '')
}

export async function getRect(base, sessionId, id) {
  const response = await http(base, 'GET', `/session/${sessionId}/element/${id}/rect`)
  return response.value
}

export async function textForSelector(base, sessionId, selector) {
  const id = await find(base, sessionId, selector)
  return await getText(base, sessionId, id)
}

export async function screenshot(base, sessionId) {
  const response = await http(base, 'GET', `/session/${sessionId}/screenshot`)
  return response.value
}

export async function captureScreenshot(base, sessionId, screenshotPath) {
  const screenshotBase64 = await screenshot(base, sessionId)
  mkdirSync(path.dirname(screenshotPath), { recursive: true })
  writeFileSync(screenshotPath, Buffer.from(screenshotBase64, 'base64'))
}

export async function setWindowRect(base, sessionId, width, height) {
  await http(base, 'POST', `/session/${sessionId}/window/rect`, {
    x: 0,
    y: 0,
    width,
    height,
  })
}

export async function source(base, sessionId) {
  const response = await http(base, 'GET', `/session/${sessionId}/source`)
  return response.value || ''
}

export async function click(base, sessionId, id) {
  try {
    await http(base, 'POST', `/session/${sessionId}/element/${id}/click`, {})
  } catch (error) {
    if (!/element click intercepted/i.test(String(error))) {
      throw error
    }

    log(`webdriver click intercepted for ${id}; retrying with DOM click`)
    await executeScript(
      base,
      sessionId,
      'arguments[0].click(); return true;',
      [{ 'element-6066-11e4-a52e-4f735466cecf': id }],
    )
  }
}

export async function clickSelector(base, sessionId, selector) {
  const id = await find(base, sessionId, selector)
  await click(base, sessionId, id)
  return id
}

export async function clear(base, sessionId, id) {
  await http(base, 'POST', `/session/${sessionId}/element/${id}/clear`, {})
}

export async function sendKeys(base, sessionId, id, text) {
  await http(base, 'POST', `/session/${sessionId}/element/${id}/value`, {
    text,
    value: [...text],
  })
}

export async function typeInto(base, sessionId, selector, text, options = {}) {
  const id = await find(base, sessionId, selector)
  if (options.clearFirst ?? true) {
    await clear(base, sessionId, id)
  }
  if (text.length > 0) {
    await sendKeys(base, sessionId, id, text)
  }
  return id
}

export async function isPresent(base, sessionId, selector) {
  try {
    await find(base, sessionId, selector)
    return true
  } catch {
    return false
  }
}

export async function waitUntil(fn, description, timeoutMs = 40_000) {
  const started = Date.now()
  while (Date.now() - started < timeoutMs) {
    const value = await fn()
    if (value) {
      return value
    }

    await delay(250)
  }

  throw new Error(`timed out waiting for ${description}`)
}

export async function pageContains(base, sessionId, pattern) {
  const html = (await source(base, sessionId)).replace(/\s+/g, ' ')
  return pattern.test(html)
}

export async function waitForSelectorText(
  base,
  sessionId,
  selector,
  pattern,
  description,
  timeoutMs = 40_000,
) {
  return await waitUntil(
    async () => {
      try {
        const text = await textForSelector(base, sessionId, selector)
        return pattern.test(text) ? text : false
      } catch {
        return false
      }
    },
    description,
    timeoutMs,
  )
}
