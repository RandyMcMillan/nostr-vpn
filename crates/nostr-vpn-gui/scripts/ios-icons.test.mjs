import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const sourceDir = path.resolve(scriptDir, '../src-tauri/icons/ios')
const bundledDir = path.resolve(
  scriptDir,
  '../src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset',
)

const listPngs = async (dir) =>
  (await readdir(dir))
    .filter((entry) => entry.endsWith('.png'))
    .sort((left, right) => left.localeCompare(right))

test('bundled iOS AppIcon catalog matches the committed iOS source icons', async () => {
  const [sourceFiles, bundledFiles] = await Promise.all([listPngs(sourceDir), listPngs(bundledDir)])

  assert.deepEqual(
    bundledFiles,
    sourceFiles,
    'generated iOS app icon filenames drifted from src-tauri/icons/ios',
  )

  await Promise.all(
    sourceFiles.map(async (file) => {
      const [sourceBytes, bundledBytes] = await Promise.all([
        readFile(path.join(sourceDir, file)),
        readFile(path.join(bundledDir, file)),
      ])

      assert.deepEqual(
        bundledBytes,
        sourceBytes,
        `generated iOS app icon ${file} differs from src-tauri/icons/ios/${file}`,
      )
    }),
  )
})
