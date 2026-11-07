import { defineConfig, normalizePath, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import type { AtRule, Plugin as CssPlugin } from 'postcss'
import { fileURLToPath } from 'node:url'

const DEMO_TRANSPORT = '\0ic-widget:demo-transport'
const DEMO_BACKEND = normalizePath(fileURLToPath(new URL('./src/api/demo.ts', import.meta.url)))

const demoBackend: Plugin = {
  name: 'ic-widget:demo-backend',
  enforce: 'pre',
  resolveId(source, importer) {
    if (source === './http' && importer?.endsWith('/src/api/client.ts')) return DEMO_TRANSPORT
  },
  load(id) {
    if (id === DEMO_TRANSPORT) {
      return `export { DemoBackend as HttpBackend } from ${JSON.stringify(DEMO_BACKEND)}`
    }
  },
}

const englishOnly: Plugin = {
  name: 'ic-widget:english-only',
  enforce: 'pre',
  transform(code, id) {
    if (!id.endsWith('/src/lib/i18n.ts')) return
    const english = code.replace(`'../i18n/*.json'`, `'../i18n/en.json'`)
    if (english === code) this.error('src/lib/i18n.ts no longer globs ../i18n/*.json; update vite.widget.config.ts')
    return english
  },
}

// Not revert-layer: unlayered, it falls back to the host's layers. revert drops svg/img presentation attributes, hence the exclusions.
const HOST_RESET = `
:where(.app-window) {
  all: initial;
  visibility: inherit;
  pointer-events: inherit;
  -webkit-text-size-adjust: inherit;
  text-size-adjust: inherit;
  outline: revert;
  outline-offset: revert;
  --accent: initial;
  --accent-red: initial;
  --added-bg: initial;
  --bg-hover: initial;
  --border: initial;
  --color-error: initial;
  --danger: initial;
  --mono: initial;
  --removed-bg: initial;
}
:where(.app-window) :where(:not(svg, svg *, [width], [height])),
:where(.app-window) ::before,
:where(.app-window) ::after,
:where(.app-window) ::placeholder {
  all: revert;
}
:where(.app-window) :where(svg, [width], [height]) {
  display: revert;
  vertical-align: revert;
  max-width: revert;
}
:where(.app-window) ::-webkit-inner-spin-button,
:where(.app-window) ::-webkit-outer-spin-button {
  all: revert;
}
`

const scopedToWindow: CssPlugin = {
  postcssPlugin: 'ic-widget:scoped-to-window',
  Once(root, { parse }) {
    const keyframes = new Set<string>()
    root.walkAtRules(/keyframes$/, (at) => {
      keyframes.add(at.params)
      at.params = `app-window-${at.params}`
    })
    root.walkDecls(/^animation(-name)?$/, (decl) => {
      decl.value = decl.value.replace(/[\w-]+/g, (word) => (keyframes.has(word) ? `app-window-${word}` : word))
    })
    root.walkRules((rule) => {
      if (rule.parent?.type === 'atrule' && (rule.parent as AtRule).name.endsWith('keyframes')) return
      rule.selectors = rule.selectors.map((selector) =>
        selector === ':root'
          ? ':where(.app-window)'
          : selector.includes('.app-window')
            ? selector
            : `.app-window ${selector}`,
      )
    })
    const file = root.source?.input.file
    if (file?.endsWith('/src/index.css')) root.prepend(parse(HOST_RESET, { from: file }))
  },
}

export default defineConfig({
  plugins: [demoBackend, englishOnly, react()],
  // Library builds leave process.env.NODE_ENV as written, and React reads it at runtime.
  define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  publicDir: false,
  css: { postcss: { plugins: [scopedToWindow] } },
  build: {
    outDir: '../../bin/demo-widget',
    emptyOutDir: true,
    lib: { entry: 'src/widget.tsx', formats: ['es'], fileName: 'demo-widget', cssFileName: 'demo-widget' },
    rolldownOptions: {
      // One file: the site imports it from a blob: URL, against which no chunk import resolves.
      output: { minify: true, comments: false, codeSplitting: false },
    },
    license: { fileName: 'demo-widget.licenses.md' },
  },
})
