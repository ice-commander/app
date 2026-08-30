import { useEffect, useState } from 'react'
import { tr } from '../lib/i18n'
import type { Connection } from '../api/types'

/**
 * The web counterpart of the GTK "Manage FTP/SFTP Connections" dialog: the saved
 * list on the left, one connection on the right — viewed, edited or created.
 *
 * Fields follow the protocol, the way the GTK dialog does: the SSH key rows exist
 * for SFTP only, and only when its authentication type is a key.
 *
 * Passwords are never handed out by the API, so the field stays blank when editing
 * and an untouched blank means "keep the stored one" — the backends merge it back
 * server-side. Typing into it replaces the secret.
 */

interface Props {
  connections: Connection[]
  onSave: (conn: Connection, connect: boolean) => void
  onDelete: (name: string) => void
  onConnect: (conn: Connection) => void
  onExport: (password: string | undefined) => void
  onImport: (data: string, password: string | undefined) => void
  onClose: () => void
}

type Mode = 'view' | 'edit' | 'new'

const DEFAULT_PORTS: Record<string, number> = { ftp: 21, sftp: 22, webdav: 443 }

const ROW: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: '10px',
  marginBottom: '8px',
}
const LABEL: React.CSSProperties = {
  width: '150px',
  flexShrink: 0,
  color: 'var(--text-dim)',
  fontSize: '13px',
  textAlign: 'right',
}

const blank = (): Connection => ({
  name: '',
  protocol: 'ftp',
  host: '',
  port: DEFAULT_PORTS.ftp,
  user: '',
})

export function ConnectionsDialog({ connections, onSave, onDelete, onConnect, onExport, onImport, onClose }: Props) {
  const [selected, setSelected] = useState<string | null>(connections[0]?.name ?? null)
  const [mode, setMode] = useState<Mode>('view')
  const [draft, setDraft] = useState<Connection>(connections[0] ?? blank())
  const [pass, setPass] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [tunnelPass, setTunnelPass] = useState('')
  const [tunnelPassphrase, setTunnelPassphrase] = useState('')
  const [portTouched, setPortTouched] = useState(false)

  // Follow the selection while viewing; never clobber a draft being edited.
  useEffect(() => {
    if (mode !== 'view') return
    const found = connections.find((c) => c.name === selected)
    if (found) setDraft(found)
  }, [selected, connections, mode])

  const readOnly = mode === 'view'
  const isSftp = draft.protocol === 'sftp'
  const usesKey = isSftp && (draft.auth_type ?? 'password') === 'key'
  const valid = draft.name.trim() !== '' && draft.host.trim() !== ''

  const set = <K extends keyof Connection>(k: K, v: Connection[K]) =>
    setDraft((d) => ({ ...d, [k]: v }))

  const switchProtocol = (p: string) => {
    setDraft((d) => ({ ...d, protocol: p, port: portTouched ? d.port : (DEFAULT_PORTS[p] ?? 21) }))
  }

  const startNew = () => {
    setMode('new')
    setSelected(null)
    setDraft(blank())
    setPass('')
    setPassphrase('')
    setPortTouched(false)
  }

  const cancel = () => {
    setMode('view')
    setPass('')
    setPassphrase('')
    const first = connections.find((c) => c.name === selected) ?? connections[0]
    if (first) {
      setSelected(first.name)
      setDraft(first)
    }
  }

  const save = (connect: boolean) => {
    if (!valid) return
    const out: Connection = {
      ...draft,
      name: draft.name.trim(),
      host: draft.host.trim(),
      // Blank = keep whatever is stored; the backend merges the sealed secret.
      pass: pass === '' ? undefined : pass,
      ...(usesKey ? { passphrase: passphrase === '' ? undefined : passphrase } : {}),
      ...(draft.use_tunnel
        ? {
            tunnel_pass: tunnelPass === '' ? undefined : tunnelPass,
            tunnel_passphrase: tunnelPassphrase === '' ? undefined : tunnelPassphrase,
          }
        : {}),
    }
    onSave(out, connect)
    setSelected(out.name)
    setMode('view')
    setPass('')
    setPassphrase('')
  }

  const field = (
    label: string,
    value: string,
    onChange: (v: string) => void,
    opts: { placeholder?: string; type?: string; width?: string } = {}
  ) => (
    <div style={ROW}>
      <span style={LABEL}>{label}</span>
      <input
        type={opts.type ?? 'text'}
        className="modal-input"
        style={{ flex: 1, width: opts.width }}
        value={value}
        placeholder={opts.placeholder}
        disabled={readOnly}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  )

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
                <span className="select-source-hint" style={{ padding: '6px 4px', display: 'block' }}>
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
                    setDraft(c)
                    setPass('')
                    setPassphrase('')
                  }}
                >
                  <div className="drive-info">
                    <span className="drive-name">{c.name}</span>
                    <span className="drive-path">{c.protocol.toUpperCase()}</span>
                  </div>
                </div>
              ))}
            </div>
            <button
              className="modal-btn confirm"
              style={{ width: '100%', marginTop: '8px' }}
              onClick={startNew}
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
                  ? tr('webpult.edit_connection')
                  : tr('conn_manager.view_connection')}
            </div>

            {connections.length === 0 && mode === 'view' ? (
              <span className="select-source-hint">{tr('webpult.no_saved_connections_yet')}</span>
            ) : (
              <>
                {field(tr('conn_manager.name'), draft.name, (v) => set('name', v))}

                <div style={ROW}>
                  <span style={LABEL}>{tr('conn_manager.protocol')}</span>
                  <select
                    className="modal-input"
                    style={{ flex: 1 }}
                    value={draft.protocol}
                    // The name keys a stored record, and the protocol decides which
                    // fields mean anything — both stay put once saved.
                    disabled={readOnly || mode === 'edit'}
                    onChange={(e) => switchProtocol(e.target.value)}
                  >
                    <option value="ftp">FTP</option>
                    <option value="sftp">SFTP</option>
                    <option value="webdav">WebDAV</option>
                  </select>
                </div>

                {isSftp && (
                  <div style={ROW}>
                    <span style={LABEL}>{tr('conn_manager.auth_type')}</span>
                    <select
                      className="modal-input"
                      style={{ flex: 1 }}
                      value={draft.auth_type ?? 'password'}
                      disabled={readOnly}
                      onChange={(e) => set('auth_type', e.target.value)}
                    >
                      <option value="password">{tr('conn_manager.auth_password')}</option>
                      <option value="key">{tr('conn_manager.auth_key')}</option>
                    </select>
                  </div>
                )}

                <div style={ROW}>
                  <span style={LABEL}>{tr('conn_manager.host')}</span>
                  <input
                    type="text"
                    className="modal-input"
                    style={{ flex: 1 }}
                    value={draft.host}
                    disabled={readOnly}
                    onChange={(e) => set('host', e.target.value)}
                  />
                  <input
                    type="number"
                    className="modal-input"
                    style={{ width: '90px' }}
                    value={draft.port}
                    disabled={readOnly}
                    onChange={(e) => {
                      setPortTouched(true)
                      set('port', Number(e.target.value))
                    }}
                  />
                </div>

                {field(tr('conn_manager.username'), draft.user, (v) => set('user', v))}

                {!usesKey &&
                  field(
                    tr('conn_manager.password'),
                    pass,
                    setPass,
                    {
                      type: 'password',
                      placeholder: readOnly
                        ? '••••••'
                        : mode === 'edit'
                          ? tr('conn_manager.keep_stored_password')
                          : undefined,
                    }
                  )}

                {field(tr('webpult.remote_path'), draft.remote_path ?? '', (v) => set('remote_path', v), {
                  placeholder: '/',
                })}

                {usesKey && (
                  <>
                    {field(tr('conn_manager.key_path'), draft.key_path ?? '', (v) => set('key_path', v))}
                    {field(tr('conn_manager.passphrase'), passphrase, setPassphrase, {
                      type: 'password',
                      placeholder: readOnly ? '' : tr('conn_manager.keep_stored_password'),
                    })}
                  </>
                )}

                {/* SSH tunnel — the same optional block the GTK dialog carries. */}
                <div style={{ ...ROW, marginTop: '10px' }}>
                  <span style={LABEL}>{tr('conn_manager.use_tunnel')}</span>
                  <input
                    type="checkbox"
                    checked={draft.use_tunnel ?? false}
                    disabled={readOnly}
                    onChange={(e) => set('use_tunnel', e.target.checked)}
                  />
                </div>

                {draft.use_tunnel && (
                  <>
                    <div style={ROW}>
                      <span style={LABEL}>{tr('conn_manager.tunnel_host')}</span>
                      <input
                        type="text" className="modal-input" style={{ flex: 1 }}
                        value={draft.tunnel_host ?? ''} disabled={readOnly}
                        onChange={(e) => set('tunnel_host', e.target.value)}
                      />
                      <input
                        type="number" className="modal-input" style={{ width: '90px' }}
                        value={draft.tunnel_port ?? 22} disabled={readOnly}
                        onChange={(e) => set('tunnel_port', Number(e.target.value))}
                      />
                    </div>
                    {field(tr('conn_manager.tunnel_user'), draft.tunnel_user ?? '', (v) => set('tunnel_user', v))}
                    <div style={ROW}>
                      <span style={LABEL}>{tr('conn_manager.auth_type')}</span>
                      <select
                        className="modal-input" style={{ flex: 1 }}
                        value={draft.tunnel_auth_type ?? 'password'} disabled={readOnly}
                        onChange={(e) => set('tunnel_auth_type', e.target.value)}
                      >
                        <option value="password">{tr('conn_manager.auth_password')}</option>
                        <option value="key">{tr('conn_manager.auth_key')}</option>
                      </select>
                    </div>
                    {(draft.tunnel_auth_type ?? 'password') === 'key'
                      ? <>
                          {field(tr('conn_manager.key_path'), draft.tunnel_key_path ?? '', (v) => set('tunnel_key_path', v))}
                          {field(tr('conn_manager.passphrase'), tunnelPassphrase, setTunnelPassphrase, {
                            type: 'password',
                            placeholder: readOnly ? '' : tr('conn_manager.keep_stored_password'),
                          })}
                        </>
                      : field(tr('conn_manager.password'), tunnelPass, setTunnelPass, {
                          type: 'password',
                          placeholder: readOnly ? '••••••' : tr('conn_manager.keep_stored_password'),
                        })}
                  </>
                )}
              </>
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

          <button className="modal-btn cancel" onClick={mode === 'view' ? onClose : cancel}>
            {tr('account.cancel')}
          </button>

          {mode === 'view' && selected && (
            <>
              <button
                className="modal-btn cancel"
                onClick={() => {
                  if (confirm(tr('conn_manager.delete_confirm'))) onDelete(selected)
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
                  const c = connections.find((x) => x.name === selected)
                  if (c) {
                    onConnect(c)
                    onClose()
                  }
                }}
              >
                {tr('webpult.connect')}
              </button>
            </>
          )}

          {mode !== 'view' && (
            <>
              <button className="modal-btn confirm" disabled={!valid} onClick={() => save(false)}>
                {tr('common_forms.save')}
              </button>
              <button className="modal-btn confirm" disabled={!valid} onClick={() => save(true)}>
                {tr('common_forms.save')} &amp; {tr('webpult.connect')}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
