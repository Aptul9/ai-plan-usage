// Copies the runtime UI files from ../ui into ./ui-dist for Tauri's
// frontendDist. Strips Electron-only files and node_modules. Runs from the
// apps/tauri/ directory (set by tauri.conf.json build.beforeBuildCommand /
// beforeDevCommand).
const fs = require('fs')
const path = require('path')

const ROOT = __dirname
const SRC = path.resolve(ROOT, '..', 'ui')
const DST = path.resolve(ROOT, 'ui-dist')

const KEEP_FILES = [
  'popover.html',
  'popover.css',
  'popover.js',
  'settings.html',
  'settings.js',
  'icon-renderer.html',
  'icon-renderer.js',
  'tauri-shim.js',
]

function rmrf(p) {
  if (!fs.existsSync(p)) return
  for (const entry of fs.readdirSync(p)) {
    const child = path.join(p, entry)
    const stat = fs.lstatSync(child)
    if (stat.isDirectory()) {
      rmrf(child)
      fs.rmdirSync(child)
    } else {
      fs.unlinkSync(child)
    }
  }
}

function copyDir(src, dst) {
  fs.mkdirSync(dst, { recursive: true })
  for (const entry of fs.readdirSync(src)) {
    const sChild = path.join(src, entry)
    const dChild = path.join(dst, entry)
    const stat = fs.lstatSync(sChild)
    if (stat.isDirectory()) {
      copyDir(sChild, dChild)
    } else {
      fs.copyFileSync(sChild, dChild)
    }
  }
}

function main() {
  if (!fs.existsSync(SRC)) {
    console.error(`sync-ui: source ui dir not found at ${SRC}`)
    process.exit(1)
  }
  fs.mkdirSync(DST, { recursive: true })
  rmrf(DST)
  fs.mkdirSync(DST, { recursive: true })

  for (const name of KEEP_FILES) {
    const src = path.join(SRC, name)
    const dst = path.join(DST, name)
    if (!fs.existsSync(src)) {
      console.warn(`sync-ui: skipping missing ${name}`)
      continue
    }
    fs.copyFileSync(src, dst)
  }
  const srcAssets = path.join(SRC, 'assets')
  if (fs.existsSync(srcAssets)) {
    copyDir(srcAssets, path.join(DST, 'assets'))
  }
  console.log(`sync-ui: copied ${KEEP_FILES.length} files + assets/ to ${DST}`)
}

main()
