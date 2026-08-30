import { copyFileSync, existsSync, mkdirSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const root = join(__dirname, '..', '..')

const src = join(__dirname, '..', 'dist')
const dst = join(root, 'gtk-app', 'assets', 'webui')

mkdirSync(dst, { recursive: true })

for (const file of ['bundle.js', 'style.css']) {
  const from = join(src, file)
  const to = join(dst, file)
  if (!existsSync(from)) {
    console.error(`Missing: ${from}`)
    process.exit(1)
  }
  copyFileSync(from, to)
  console.log(`Copied: ${file} → gtk-app/assets/webui/${file}`)
}

console.log('post-build: done')
