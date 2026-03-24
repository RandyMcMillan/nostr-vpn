import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const requestedScenario = (process.env.TAURI_E2E_SCENARIO || 'all').trim().toLowerCase()
const scenarioFiles = {
  smoke: 'smoke.mjs',
  'join-request': 'join-request.mjs',
}

const scenarioOrder =
  requestedScenario === 'all'
    ? ['smoke', 'join-request']
    : scenarioFiles[requestedScenario]
      ? [requestedScenario]
      : null

if (!scenarioOrder) {
  console.error(
    `unknown TAURI_E2E_SCENARIO=${requestedScenario}; expected one of: all, smoke, join-request`,
  )
  process.exit(1)
}

for (const scenario of scenarioOrder) {
  const scriptPath = path.join(__dirname, scenarioFiles[scenario])
  console.log(`[tauri-e2e] running scenario: ${scenario}`)

  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [scriptPath], {
      stdio: 'inherit',
      env: process.env,
    })
    child.on('error', reject)
    child.on('exit', (code, signal) => {
      if (signal) {
        reject(new Error(`scenario ${scenario} exited via signal ${signal}`))
        return
      }
      resolve(code ?? 1)
    })
  })

  if (exitCode !== 0) {
    process.exit(exitCode)
  }
}
