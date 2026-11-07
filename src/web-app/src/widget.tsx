import { createRoot } from 'react-dom/client'
import App, { type AppOptions } from './App.tsx'
import { setDemoMediaBase } from './api/demo'
import './index.css'

export function mountIceCommanderDemo(
  el: HTMLElement,
  opts: AppOptions & { mediaBase?: string } = {},
): () => void {
  setDemoMediaBase(opts.mediaBase ?? '')
  const root = createRoot(el)
  root.render(<App captureKeys={opts.captureKeys ?? false} embedded={opts.embedded ?? true} />)
  return () => root.unmount()
}
