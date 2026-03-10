import { copyFileSync, existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const crateDir = path.resolve(__dirname, '..')
const repoRoot = path.resolve(crateDir, '..', '..')
const targetDir = path.join(repoRoot, 'target', 'debug')

execFileSync('cargo', ['build', '-p', 'roughneck-node', '--manifest-path', path.join(repoRoot, 'Cargo.toml')], {
  cwd: repoRoot,
  stdio: 'inherit',
})

const candidates = {
  darwin: path.join(targetDir, 'libroughneck_node.dylib'),
  linux: path.join(targetDir, 'libroughneck_node.so'),
  win32: path.join(targetDir, 'roughneck_node.dll'),
}

const source = candidates[process.platform]
if (!source) {
  throw new Error(`unsupported platform: ${process.platform}`)
}
if (!existsSync(source)) {
  throw new Error(`built addon not found at ${source}`)
}

copyFileSync(source, path.join(crateDir, 'roughneck.node'))
