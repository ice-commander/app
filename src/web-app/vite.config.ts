import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Dev: proxy REST + WebSocket to a running `--headless --webui` app (default port 7878).
// Override the target with VITE_API_TARGET if the app runs elsewhere.
const API_TARGET = process.env.VITE_API_TARGET ?? 'http://localhost:7878'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: API_TARGET,
        changeOrigin: true,
        ws: true,
      },
    },
  },
  build: {
    // Single self-contained bundle.js + style.css, served by the Rust app via
    // include_bytes!("../assets/webui/{bundle.js,style.css}"). All SVG assets are
    // inlined as data URIs so the output is exactly two files (no hashed assets).
    assetsInlineLimit: 100 * 1024 * 1024,
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        entryFileNames: 'bundle.js',
        chunkFileNames: 'bundle.js',
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith('.css')) return 'style.css'
          return assetInfo.name ?? 'asset'
        },
      },
    },
  },
})
