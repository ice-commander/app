import { useState } from 'react'
import { tr } from "../lib/i18n";
import type { Connection } from '../api/types'

/**
 * Create/edit form for a saved connection (FTP / SFTP / WebDAV) — the web
 * counterpart of the GTK connections dialog, kept to the essential fields.
 * Tunnel options stay GTK-only for now.
 */

interface Props {
  /** Pre-filled values when editing an existing connection. */
  initial?: Connection
  onCancel: () => void
  /** `connect` = also mount it in the active panel right away. */
  onSave: (conn: Connection, connect: boolean) => void
}

const DEFAULT_PORTS: Record<string, number> = { ftp: 21, sftp: 22, webdav: 443 }

const ROW: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }
const LABEL: React.CSSProperties = { width: '110px', flexShrink: 0, color: 'var(--text-dim)', fontSize: '13px', textAlign: 'right' }

export function ConnectionForm({ initial, onCancel, onSave }: Props) {
  const [protocol, setProtocol] = useState(initial?.protocol ?? 'ftp')
  const [name, setName] = useState(initial?.name ?? '')
  const [host, setHost] = useState(initial?.host ?? '')
  const [port, setPort] = useState<number>(initial?.port ?? DEFAULT_PORTS.ftp)
  const [portTouched, setPortTouched] = useState(initial !== undefined)
  const [user, setUser] = useState(initial?.user ?? '')
  const [pass, setPass] = useState(initial?.pass ?? '')
  const [remotePath, setRemotePath] = useState(initial?.remote_path ?? '')
  const [authType, setAuthType] = useState(initial?.auth_type ?? 'password')
  const [keyPath, setKeyPath] = useState(initial?.key_path ?? '')
  const [passphrase, setPassphrase] = useState(initial?.passphrase ?? '')

  const switchProtocol = (p: string) => {
    setProtocol(p)
    if (!portTouched) setPort(DEFAULT_PORTS[p] ?? 21)
  }

  const valid = name.trim() !== '' && host.trim() !== ''
  const sftpKey = protocol === 'sftp' && authType === 'key'

  const build = (): Connection => ({
    name: name.trim(),
    protocol,
    host: host.trim(),
    port,
    user,
    pass: pass === '' ? undefined : pass,
    remote_path: remotePath.trim() === '' ? undefined : remotePath.trim(),
    ...(protocol === 'sftp'
      ? {
          auth_type: authType,
          key_path: sftpKey && keyPath.trim() !== '' ? keyPath.trim() : undefined,
          passphrase: sftpKey && passphrase !== '' ? passphrase : undefined,
        }
      : {}),
  })

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal-card" style={{ minWidth: '420px' }} onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">{initial ? 'Edit Connection' : 'New Connection'}</h3>

        <div style={ROW}>
          <span style={LABEL}>{tr("conn_manager.protocol")}</span>
          <select
            className="modal-input"
            style={{ flex: 1 }}
            value={protocol}
            onChange={(e) => switchProtocol(e.target.value)}
            disabled={initial !== undefined}
          >
            <option value="ftp">FTP</option>
            <option value="sftp">SFTP</option>
            <option value="webdav">WebDAV</option>
          </select>
        </div>

        <div style={ROW}>
          <span style={LABEL}>{tr("find.hdr_name")}</span>
          <input
            type="text" className="modal-input" style={{ flex: 1 }} autoFocus
            placeholder="myproject.com" value={name} onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div style={ROW}>
          <span style={LABEL}>{protocol === 'webdav' ? 'URL' : 'Host'}</span>
          <input
            type="text" className="modal-input" style={{ flex: 1 }}
            placeholder={protocol === 'webdav' ? 'https://server/dav' : 'server.example.com'}
            value={host} onChange={(e) => setHost(e.target.value)}
          />
          {protocol !== 'webdav' && (
            <input
              type="number" className="modal-input" style={{ width: '80px' }}
              value={port}
              onChange={(e) => { setPortTouched(true); setPort(Number(e.target.value) || 0) }}
            />
          )}
        </div>

        <div style={ROW}>
          <span style={LABEL}>{tr("webpult.user")}</span>
          <input
            type="text" className="modal-input" style={{ flex: 1 }}
            value={user} onChange={(e) => setUser(e.target.value)}
          />
        </div>

        {protocol === 'sftp' && (
          <div style={ROW}>
            <span style={LABEL}>{tr("webpult.auth")}</span>
            <select
              className="modal-input" style={{ flex: 1 }}
              value={authType} onChange={(e) => setAuthType(e.target.value)}
            >
              <option value="password">{tr("webpult.password")}</option>
              <option value="key">{tr("webpult.private_key")}</option>
            </select>
          </div>
        )}

        {sftpKey ? (
          <>
            <div style={ROW}>
              <span style={LABEL}>{tr("webpult.key_path")}</span>
              <input
                type="text" className="modal-input" style={{ flex: 1 }}
                placeholder="~/.ssh/id_ed25519" value={keyPath} onChange={(e) => setKeyPath(e.target.value)}
              />
            </div>
            <div style={ROW}>
              <span style={LABEL}>{tr("webpult.passphrase")}</span>
              <input
                type="password" className="modal-input" style={{ flex: 1 }}
                value={passphrase} onChange={(e) => setPassphrase(e.target.value)}
              />
            </div>
          </>
        ) : (
          <div style={ROW}>
            <span style={LABEL}>{tr("webpult.password")}</span>
            <input
              type="password" className="modal-input" style={{ flex: 1 }}
              value={pass} onChange={(e) => setPass(e.target.value)}
            />
          </div>
        )}

        <div style={ROW}>
          <span style={LABEL}>{tr("webpult.remote_path")}</span>
          <input
            type="text" className="modal-input" style={{ flex: 1 }}
            placeholder="/" value={remotePath} onChange={(e) => setRemotePath(e.target.value)}
          />
        </div>

        <div className="modal-buttons">
          <button className="modal-btn cancel" onClick={onCancel}>{tr("account.cancel")}</button>
          <button className="modal-btn confirm" disabled={!valid} onClick={() => valid && onSave(build(), false)}>{tr("common_forms.save")}</button>
          <button className="modal-btn confirm" disabled={!valid} onClick={() => valid && onSave(build(), true)}>
            Save &amp; Connect
          </button>
        </div>
      </div>
    </div>
  )
}
