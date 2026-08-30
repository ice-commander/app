import { useEffect, useState } from 'react'
import { tr } from "../lib/i18n";
import * as api from '../api/client'
import type { Side } from '../api/types'

interface Props {
  side: Side
  path: string
  /** 'view' opens read-only (F3); 'edit' opens editable (F4). */
  mode: 'view' | 'edit'
  onClose: () => void
  onSaved: () => void
}

type State =
  | { kind: 'loading' }
  | { kind: 'binary' }
  | { kind: 'text'; content: string }
  | { kind: 'image'; url: string }
  | { kind: 'video'; url: string }
  | { kind: 'audio'; url: string }
  | { kind: 'error'; message: string }

const IMAGE_EXT = ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico']
const VIDEO_EXT = ['mp4', 'm4v', 'webm', 'mov']
const AUDIO_EXT = ['mp3', 'ogg', 'wav', 'flac']

function mediaKind(name: string): 'image' | 'video' | 'audio' | null {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (IMAGE_EXT.includes(ext)) return 'image'
  if (VIDEO_EXT.includes(ext)) return 'video'
  if (AUDIO_EXT.includes(ext)) return 'audio'
  return null
}

export function FileViewer({ side, path, mode, onClose, onSaved }: Props) {
  const [state, setState] = useState<State>({ kind: 'loading' })
  const [editing, setEditing] = useState(mode === 'edit')
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const name = path.split('/').pop() || path

  useEffect(() => {
    let alive = true
    setState({ kind: 'loading' })

    // Media opens via the backend's streamed URL (REST stream endpoint on a real
    // app, data URI in the sandbox); everything else goes to the text viewer.
    const kind = mode === 'view' ? mediaKind(name) : null
    if (kind) {
      api.getFileUrl(side, path)
        .then((url) => {
          if (!alive) return
          setState(url ? { kind, url } : { kind: 'binary' })
        })
        .catch((e) => { if (alive) setState({ kind: 'error', message: String(e) }) })
    } else {
      api.readFile(side, path)
        .then((f) => { if (alive) setState(f.is_binary ? { kind: 'binary' } : { kind: 'text', content: f.content }) })
        .catch((e) => { if (alive) setState({ kind: 'error', message: String(e) }) })
    }
    return () => { alive = false }
  }, [side, path, mode, name])

  // Escape closes the viewer (unless there are unsaved edits)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        if (!dirty || confirm('Discard unsaved changes?')) onClose()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [dirty, onClose])

  const save = async () => {
    if (state.kind !== 'text') return
    setSaving(true)
    try {
      await api.writeFile(side, path, state.content)
      setDirty(false)
      onSaved()
    } catch (e) {
      setState({ kind: 'error', message: String(e) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-card"
        onClick={(e) => e.stopPropagation()}
        // Sized relative to the app window (the overlay), not the page viewport —
        // embedded on the website the widget is far smaller than the viewport,
        // and vw/vh units would push the dialog past the window edges.
        style={{ width: '86%', maxWidth: '1000px', height: '86%', display: 'flex', flexDirection: 'column' }}
      >
        <h3 className="modal-title" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span style={{ fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.5px', color: 'var(--text-dim)', border: '1px solid var(--border, #2a2d31)', borderRadius: '4px', padding: '1px 6px', fontWeight: 600 }}>
            {editing ? 'Edit' : 'View'}
          </span>
          <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {name}{dirty ? ' •' : ''}
          </span>
          <span style={{ fontSize: '11px', color: 'var(--text-dim)', fontWeight: 400 }}>{path}</span>
        </h3>

        {state.kind === 'loading' && <p style={{ color: 'var(--text-muted)' }}>Loading…</p>}
        {state.kind === 'binary' && (
          <p style={{ color: 'var(--text-muted)' }}>Binary or oversized file — not shown in the text viewer.</p>
        )}
        {state.kind === 'error' && <p style={{ color: 'var(--accent-red, #e06c75)' }}>{state.message}</p>}
        {state.kind === 'image' && (
          <div style={{ flex: 1, minHeight: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: '8px', background: 'var(--bg-app, #0e0f11)', border: '1px solid var(--border, #2a2d31)', borderRadius: '6px', overflow: 'hidden' }}>
            <img src={state.url} alt={name} style={{ maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }} />
          </div>
        )}
        {state.kind === 'video' && (
          <div style={{ flex: 1, minHeight: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: '8px', background: 'var(--bg-app, #0e0f11)', border: '1px solid var(--border, #2a2d31)', borderRadius: '6px', overflow: 'hidden' }}>
            <video src={state.url} controls autoPlay style={{ maxWidth: '100%', maxHeight: '100%' }} />
          </div>
        )}
        {state.kind === 'audio' && (
          <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: '8px' }}>
            <audio src={state.url} controls autoPlay style={{ width: '90%' }} />
          </div>
        )}
        {state.kind === 'text' && (
          <textarea
            value={state.content}
            readOnly={!editing}
            onChange={(e) => { setState({ kind: 'text', content: e.target.value }); setDirty(true) }}
            spellCheck={false}
            style={{
              flex: 1, width: '100%', resize: 'none', marginTop: '8px', padding: '10px',
              fontFamily: 'var(--font-mono, monospace)', fontSize: '13px', lineHeight: 1.5,
              background: 'var(--bg-app, #0e0f11)', color: 'var(--text-main, #e6e6e6)',
              border: '1px solid var(--border, #2a2d31)', borderRadius: '6px',
            }}
          />
        )}

        <div className="modal-buttons" style={{ marginTop: '12px' }}>
          <button className="modal-btn cancel" onClick={onClose}>{tr("editor.close")}</button>
          {state.kind === 'text' && !editing && (
            <button className="modal-btn confirm" onClick={() => setEditing(true)}>{tr("conn_manager.edit_btn")}</button>
          )}
          {state.kind === 'text' && editing && (
            <button className="modal-btn confirm" onClick={save} disabled={saving || !dirty}>
              {saving ? 'Saving…' : 'Save'}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
