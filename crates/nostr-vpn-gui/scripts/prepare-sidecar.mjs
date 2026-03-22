#!/usr/bin/env node

import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const args = process.argv.slice(2)
const release = args.includes('--release')
const profile = release ? 'release' : 'debug'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const guiRoot = resolve(scriptDir, '..')
const workspaceRoot = resolve(guiRoot, '..', '..')
const targetTriple = resolveTargetTriple(workspaceRoot)
const exeSuffix = process.platform === 'win32' ? '.exe' : ''
const isWindowsTarget =
  (targetTriple && targetTriple.includes('windows')) || process.platform === 'win32'

const cargoArgs = ['build', '--bin', 'nvpn', '-p', 'nostr-vpn-cli']
if (release) {
  cargoArgs.push('--release')
}
if (targetTriple) {
  cargoArgs.push('--target', targetTriple)
}

execFileSync('cargo', cargoArgs, {
  cwd: workspaceRoot,
  stdio: 'inherit',
})

const buildTargetRoot = targetTriple
  ? resolve(workspaceRoot, 'target', targetTriple, profile)
  : resolve(workspaceRoot, 'target', profile)

const sourceBinary = resolve(buildTargetRoot, `nvpn${exeSuffix}`)
if (!existsSync(sourceBinary)) {
  console.error(`expected nvpn binary at ${sourceBinary}, but it was not found`)
  process.exit(1)
}

const sidecarDir = resolve(guiRoot, 'src-tauri', 'binaries')
mkdirSync(sidecarDir, { recursive: true })

const sidecarName = targetTriple
  ? `nvpn-${targetTriple}${exeSuffix}`
  : `nvpn${exeSuffix}`
const sidecarBinary = resolve(sidecarDir, sidecarName)
copyFileSync(sourceBinary, sidecarBinary)

if (isWindowsTarget) {
  const sourceDll = resolve(buildTargetRoot, 'wintun.dll')
  if (!existsSync(sourceDll)) {
    console.error(`expected wintun.dll at ${sourceDll}, but it was not found`)
    process.exit(1)
  }

  copyFileSync(sourceDll, resolve(sidecarDir, 'wintun.dll'))
}

if (process.platform !== 'win32') {
  chmodSync(sidecarBinary, 0o755)
}

console.log(
  `prepared nvpn sidecar (${profile}${targetTriple ? `, ${targetTriple}` : ''}): ${sidecarBinary}`,
)

function resolveTargetTriple(workspaceRootPath) {
  const envTarget =
    process.env.TAURI_ENV_TARGET_TRIPLE ||
    process.env.CARGO_BUILD_TARGET ||
    process.env.TARGET
  if (envTarget && envTarget.trim().length > 0) {
    return envTarget.trim()
  }

  try {
    const rustcInfo = execFileSync('rustc', ['-vV'], {
      cwd: workspaceRootPath,
      encoding: 'utf8',
    })
    const hostLine = rustcInfo
      .split('\n')
      .find((line) => line.startsWith('host:'))
    if (!hostLine) {
      return null
    }
    const host = hostLine.slice('host:'.length).trim()
    return host.length > 0 ? host : null
  } catch {
    return null
  }
}
