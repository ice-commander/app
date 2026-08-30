import { useEffect, useRef } from 'react'
import { tr } from "../lib/i18n";
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import * as api from '../api/client'
import type { Side } from '../api/types'
import closeWhiteIcon from '../assets/close-white.svg'
import expandIcon from '../assets/expand.svg'
import collapseIcon from '../assets/collapse.svg'

interface Props {
  side: Side
  expanded: boolean
  onToggleExpand: () => void
  onClose: () => void
}

// A pure I/O view over the app's per-panel terminal: streams the PTY output into xterm
// and forwards keystrokes back over the WebSocket. The shell itself lives in the GTK app.
export function Terminal({ side, expanded, onToggleExpand, onClose }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const refitRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return

    let term: XTerm | null = null
    let session: ReturnType<typeof api.openTerminal> | null = null
    let ro: ResizeObserver | null = null
    let disposed = false

    const start = () => {
      if (disposed || !host) return

      const t = new XTerm({
        fontFamily: 'var(--font-mono, monospace)',
        fontSize: 15,
        fontWeight: 500,
        lineHeight: 1.2,
        cursorBlink: true,
        theme: { background: '#0e0f11', foreground: '#e6e6e6', cursor: '#e6e6e6' },
      })
      term = t
      const fit = new FitAddon()
      fitRef.current = fit
      t.loadAddon(fit)
      t.open(host)
      fit.fit()

      // Transport-agnostic terminal session (real PTY over WS, or the demo shell).
      const s = api.openTerminal(side, { onData: (data) => t.write(data) })
      session = s
      t.onData((d) => s.send(d))

      // Keep the far-side PTY sized to xterm so full-screen TUIs (mc/vim) fill the view.
      const refit = () => { try { fit.fit(); s.resize(t.cols, t.rows) } catch { /* not ready */ } }
      refitRef.current = refit
      refit()

      // Terminal copy/paste bindings (parity with the GTK terminal). Mouse selection
      // is built into xterm; these add the keyboard shortcuts. Clipboard access needs
      // a secure context (localhost counts; plain-http LAN does not — then it no-ops).
      t.attachCustomKeyEventHandler((e) => {
        if (e.type !== 'keydown' || !e.ctrlKey || !e.shiftKey) return true
        if (e.code === 'KeyC') {
          const sel = t.getSelection()
          if (sel) { e.preventDefault(); navigator.clipboard?.writeText(sel).catch(() => {}) }
          return false
        }
        if (e.code === 'KeyV') {
          e.preventDefault()
          // term.paste wraps in bracketed-paste markers when the app enabled that mode.
          navigator.clipboard?.readText().then((text) => { if (text) t.paste(text) }).catch(() => {})
          return false
        }
        return true
      })

      t.focus()

      ro = new ResizeObserver(() => refit())
      ro.observe(host)
    }

    // Measure the cell only once the monospace webfont is actually available. Google
    // Fonts loads JetBrains Mono asynchronously (display=swap); if xterm opens first it
    // measures the wider fallback font and every glyph lands in an over-wide cell — the
    // visible gaps between characters. Start on both resolve and reject (fallback font).
    document.fonts.load('500 14px "JetBrains Mono"').then(start, start)

    return () => {
      disposed = true
      ro?.disconnect()
      session?.close()
      term?.dispose()
      fitRef.current = null
      refitRef.current = null
    }
  }, [side])

  // re-fit when toggling expand/collapse (container size changed)
  useEffect(() => {
    const id = setTimeout(() => { refitRef.current?.() }, 60)
    return () => clearTimeout(id)
  }, [expanded])

  return (
    <div className="web-terminal">
      <div className="web-terminal-header">
        <div className="web-terminal-title">
          <div className="terminal-indicator"></div>
          <span>Terminal ({side === 'left' ? 'Left' : 'Right'})</span>
        </div>
        <div className="web-terminal-actions">
          <button className="web-terminal-btn" onClick={onToggleExpand} title={expanded ? 'Collapse to panel' : 'Expand across both panels'}>
            <img src={expanded ? collapseIcon : expandIcon} alt={expanded ? 'collapse' : 'expand'} style={{ width: '14px', height: '14px' }} />
          </button>
          <button className="web-terminal-btn" onClick={onClose} title={tr("webpult.close_f9")}>
            <img src={closeWhiteIcon} alt="x" style={{ width: '12px', height: '12px' }} />
          </button>
        </div>
      </div>
      <div className="web-terminal-body" ref={hostRef}></div>
    </div>
  )
}
