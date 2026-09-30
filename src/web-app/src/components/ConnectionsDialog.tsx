import { useState } from 'react'
import { tr } from '../lib/i18n'
import type { Connection, Side } from '../api/types'
import { ConnectionEditor } from './view/ConnectionEditor'

/**
 * The web counterpart of the GTK "Manage Connections" dialog: the saved list on
 * the left, one connection on the right.
 *
 * Nothing here knows a protocol. The fields, their labels, their defaults and
 * the conditions that show or hide them all come from the document its plugin
 * declared; this file only owns the chrome around it.
 */

interface Props {
  connections: Connection[]
  /** Which panel a "save & connect" mounts into. */
  side: Side
  onDelete: (name: string) => void
  onConnect: (conn: Connection) => void
  onExport: (password: string | undefined) => void
  onImport: (data: string, password: string | undefined) => void
  onSaved: (name: string, connected: boolean) => void
  onClose: () => void
}

type Mode = 'view' | 'edit' | 'new'

export function ConnectionsDialog({
  connections,
  side,
  onDelete,
  onConnect,
  onExport,
  onImport,
  onSaved,
  onClose,
}: Props) {
  const [selected, setSelected] = useState<string | null>(connections[0]?.name ?? null)
  const [mode, setMode] = useState<Mode>('view')

  const record = connections.find((c) => c.name === selected)

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-card"
        // `.modal-card` caps every dialog at 400px; this one is a two-column layout and
        // needs the room, so the cap is lifted here rather than for all modals.
        style={{ width: 'min(1120px, 94vw)', maxWidth: 'min(1120px, 94vw)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">{tr('conn_manager.title')}</h3>

        <div style={{ display: 'flex', gap: '16px', alignItems: 'stretch' }}>
          {/* ── saved list ────────────────────────────────────────────────── */}
          <div style={{ width: '260px', flexShrink: 0 }}>
            <div style={{ color: 'var(--text-dim)', fontSize: '12px', marginBottom: '6px' }}>
              {tr('conn_manager.saved_connections')}
            </div>
            <div style={{ maxHeight: '48vh', overflowY: 'auto' }}>
              {connections.length === 0 && (
                <span
                  className="select-source-hint"
                  style={{ padding: '6px 4px', display: 'block' }}
                >
                  {tr('webpult.no_saved_connections_yet')}
                </span>
              )}
              {connections.map((c) => (
                <div
                  key={c.name}
                  className="drive-row"
                  style={
                    c.name === selected && mode !== 'new'
                      ? { background: 'var(--bg-hover, rgba(255,255,255,0.07))' }
                      : undefined
                  }
                  onClick={() => {
                    setMode('view')
                    setSelected(c.name)
                  }}
                >
                  <div className="drive-info">
                    <span className="drive-name">{c.name}</span>
                    <span className="drive-path">{c.kind.toUpperCase()}</span>
                  </div>
                </div>
              ))}
            </div>
            <button
              className="modal-btn confirm"
              style={{ width: '100%', marginTop: '8px' }}
              onClick={() => setMode('new')}
            >
              + {tr('conn_manager.new_connection')}
            </button>
          </div>

          {/* ── the one connection ────────────────────────────────────────── */}
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ color: 'var(--text-dim)', fontSize: '12px', marginBottom: '6px' }}>
              {mode === 'new'
                ? tr('conn_manager.new_connection')
                : mode === 'edit'
                  ? tr('conn_manager.edit_title')
                  : tr('conn_manager.view_title')}
            </div>
            {mode === 'view' && record === undefined && (
              <span className="select-source-hint">{tr('conn_manager.saved_connections')}</span>
            )}
            {mode === 'view' && record !== undefined && <Summary record={record} />}
            {mode !== 'view' && (
              <ConnectionEditor
                // remounting on the record keeps a half-typed draft from leaking across
                key={mode === 'new' ? '__new__' : (record?.name ?? '__none__')}
                inline
                initial={mode === 'new' ? undefined : record}
                side={side}
                onCancel={() => setMode('view')}
                onSaved={(name, connected) => {
                  setMode('view')
                  setSelected(name)
                  onSaved(name, connected)
                  if (connected) onClose()
                }}
              />
            )}
          </div>
        </div>

        {/* ── actions ───────────────────────────────────────────────────────── */}
        <div className="modal-buttons">
          {/* Export/Import sit on the left, like the GTK dialog's pair. */}
          <div style={{ display: 'flex', gap: '8px', marginRight: 'auto' }}>
            <button
              className="modal-btn cancel"
              disabled={connections.length === 0}
              title={tr('conn_manager.export_password_body')}
              onClick={() => {
                const pw = prompt(tr('conn_manager.export_password_title')) ?? ''
                onExport(pw === '' ? undefined : pw)
              }}
            >
              {tr('conn_manager.export')}
            </button>
            <button
              className="modal-btn cancel"
              onClick={() => {
                const input = document.createElement('input')
                input.type = 'file'
                input.accept = '.json,application/json'
                input.onchange = async () => {
                  const file = input.files?.[0]
                  if (!file) return
                  onImport(await file.text(), undefined)
                }
                input.click()
              }}
            >
              {tr('conn_manager.import')}
            </button>
          </div>

          {mode === 'view' && (
            <>
              <button className="modal-btn cancel" onClick={onClose}>
                {tr('account.cancel')}
              </button>
              {record !== undefined && (
                <>
                  <button
                    className="modal-btn cancel"
                    onClick={() => {
                      if (confirm(tr('conn_manager.delete_confirm'))) onDelete(record.name)
                    }}
                  >
                    {tr('conn_manager.delete')}
                  </button>
                  <button className="modal-btn confirm" onClick={() => setMode('edit')}>
                    {tr('conn_manager.edit')}
                  </button>
                  <button
                    className="modal-btn confirm"
                    onClick={() => {
                      onConnect(record)
                      onClose()
                    }}
                  >
                    {tr('webpult.connect')}
                  </button>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

/** What a saved record looks like while only being looked at. */
function Summary({ record }: { record: Connection }) {
  const shown: [string, string][] = Object.entries(record)
    .filter(([key, value]) => key !== 'settings' && value !== null && value !== undefined && value !== '')
    .map(([key, value]) => [key, String(value)])
  const extra = Object.entries(record.settings ?? {}).map(
    ([key, value]) => [key, String(value)] as [string, string],
  )
  return (
    <div style={{ fontSize: '13px' }}>
      {[...shown, ...extra].map(([key, value]) => (
        <div key={key} style={{ display: 'flex', gap: '10px', marginBottom: '4px' }}>
          <span
            style={{
              width: '150px',
              flexShrink: 0,
              textAlign: 'right',
              color: 'var(--text-dim)',
            }}
          >
            {key}
          </span>
          <span style={{ minWidth: 0, overflowWrap: 'anywhere' }}>{value}</span>
        </div>
      ))}
    </div>
  )
}
