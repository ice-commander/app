import { useState } from 'react'
import { tr } from "../lib/i18n";
import type { Connection, Drive } from '../api/types'
import arrowRightIcon from '../assets/arrow-right.svg'
import starIcon from '../assets/star.svg'
import emptyStarIcon from '../assets/empty-star.svg'
import { driveIcon, driveSection, SECTION_ORDER, SECTION_TITLES, type DriveSection } from '../lib/driveIcons'
import { ConnectionForm } from './ConnectionForm'

interface Props {
  drives: Drive[]
  /** Saved connections keyed by name — the records behind the `net` rows. */
  connections: Connection[]
  favoritesOnly: boolean
  onActivate: (key: string) => void
  onToggleFavorite: (key: string) => void
  onSetFavoritesOnly: (value: boolean) => void
  /** Persist a connection; `connect` = also mount it in this panel. */
  onSaveConnection: (conn: Connection, connect: boolean) => void
  onDeleteConnection: (name: string) => void
}

export function SelectSource({ drives, connections, favoritesOnly, onActivate, onToggleFavorite, onSetFavoritesOnly, onSaveConnection, onDeleteConnection }: Props) {
  // `undefined` = closed, `null` = creating a new one, otherwise the record to edit.
  const [editing, setEditing] = useState<Connection | null | undefined>(undefined)
  // Two-step delete: first click arms the button, second confirms.
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)

  const shown = favoritesOnly ? drives.filter((d) => d.is_favorite) : drives

  const grouped: Record<DriveSection, Drive[]> = { local: [], net: [] }
  for (const d of shown) grouped[driveSection(d.kind)].push(d)

  return (
    <div className="select-source-container">
      <h2 className="select-source-title">{tr("selector.title")}</h2>

      {SECTION_ORDER.map((section) =>
        // The net section always renders — it hosts the "add connection" button.
        grouped[section].length === 0 && section !== 'net' ? null : (
          <div key={section}>
            {section !== 'local' && (
              <h2 className="select-source-title" style={{ marginTop: '16px', display: 'flex', alignItems: 'center', gap: '12px' }}>
                {SECTION_TITLES[section]}
                {section === 'net' && (
                  <button
                    className="modal-btn confirm"
                    style={{ padding: '2px 10px', fontSize: '12px' }}
                    title={tr("webpult.save_a_new_ftp_sftp_webdav_connection")}
                    onClick={() => setEditing(null)}
                  >
                    ＋ Add connection
                  </button>
                )}
              </h2>
            )}
            <div className="drives-list-box">
              {grouped[section].map((d) => (
                <div
                  key={d.key}
                  className="drive-row"
                  onClick={() => d.is_online && onActivate(d.key)}
                  title={d.subtitle}
                  style={d.is_online ? undefined : { opacity: 0.45, cursor: 'default' }}
                >
                  <img src={driveIcon(d.icon)} alt={d.name} className="drive-icon" />
                  <div className="drive-info">
                    <span className="drive-name">{d.name}</span>
                    <span className="drive-path">{d.subtitle}</span>
                  </div>
                  {section === 'net' && connections.some((c) => c.name === d.name) && (
                    <button
                      className="drive-favorite-btn"
                      title={tr("webpult.edit_connection")}
                      onClick={(e) => {
                        e.stopPropagation()
                        const conn = connections.find((c) => c.name === d.name)
                        if (conn) setEditing(conn)
                      }}
                    >
                      ✎
                    </button>
                  )}
                  {section === 'net' && (
                    <button
                      className="drive-favorite-btn"
                      title={pendingDelete === d.name ? 'Click again to delete' : 'Delete connection'}
                      style={pendingDelete === d.name ? { color: 'var(--color-error, #e05555)', opacity: 1 } : undefined}
                      onClick={(e) => {
                        e.stopPropagation()
                        if (pendingDelete === d.name) {
                          setPendingDelete(null)
                          onDeleteConnection(d.name)
                        } else {
                          setPendingDelete(d.name)
                        }
                      }}
                      onMouseLeave={() => setPendingDelete((cur) => (cur === d.name ? null : cur))}
                    >
                      {pendingDelete === d.name ? '✓?' : '✕'}
                    </button>
                  )}
                  <button
                    className={`drive-favorite-btn ${d.is_favorite ? 'favorite' : ''}`}
                    title={d.is_favorite ? 'Remove from favorites' : 'Add to favorites'}
                    onClick={(e) => { e.stopPropagation(); onToggleFavorite(d.key) }}
                  >
                    <img src={d.is_favorite ? starIcon : emptyStarIcon} alt="favorite" />
                  </button>
                  {d.is_online && (
                    <div className="drive-arrow">
                      <img src={arrowRightIcon} alt="open" />
                    </div>
                  )}
                </div>
              ))}
              {section === 'net' && grouped[section].length === 0 && (
                <span className="select-source-hint" style={{ padding: '8px 12px', display: 'block' }}>{tr("webpult.no_saved_connections_yet")}</span>
              )}
            </div>
          </div>
        )
      )}

      {shown.length === 0 && (
        <span className="select-source-hint">
          {favoritesOnly ? 'No favorite sources — add some with ★' : 'No sources available'}
        </span>
      )}

      <label className="select-source-option">
        <input
          type="checkbox"
          checked={favoritesOnly}
          onChange={(e) => onSetFavoritesOnly(e.target.checked)}
        />{tr("webpult.show_only_favorite_drives_on_the_toolbar")}</label>

      {editing !== undefined && (
        <ConnectionForm
          // `key` remounts the form when switching records, so its fields reload.
          key={editing?.name ?? '__new__'}
          initial={editing ?? undefined}
          onCancel={() => setEditing(undefined)}
          onSave={(conn, connect) => { setEditing(undefined); onSaveConnection(conn, connect) }}
        />
      )}
    </div>
  )
}
