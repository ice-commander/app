import { useCallback, useEffect, useRef, useState } from 'react'
import { tr } from "./lib/i18n";
import * as api from './api/client'
import type { Connection, Drive, Side } from './api/types'
import { Panel, type PanelHandle } from './components/Panel'
import { ConnectionsDialog } from './components/ConnectionsDialog'
import { FileViewer } from './components/FileViewer'

// `?inline` forces a data-URI (the plain import is emitted as a separate /app-logo.svg
// file because index.html also references it as a favicon, and that file isn't served by
// the embedded web UI). Inlining matches every other icon and fixes the broken header logo.
import appLogo from './assets/app-logo.svg?inline'
import settingIcon from './assets/setting.svg'
import doneIcon from './assets/done.svg'
import afternoonIcon from './assets/afternoon.svg'
import nightIcon from './assets/night.svg'
import connectIcon from './assets/connect.svg'

type ModalState = {
  type: 'none' | 'new-folder' | 'rename' | 'delete' | 'help' | 'settings' | 'quit' | 'connections'
  value?: string
}

export interface AppOptions {
  /** Grab F1–F10 / Tab / Delete on `window`. Disable when the app is embedded on a
   *  host page (the website's live demo) so it doesn't hijack the page's keys —
   *  every F-key stays reachable via the bottom bar buttons. Default: true. */
  captureKeys?: boolean
  /** Mounted inside a host page rather than owning the viewport: the window
   *  chrome fills its container (see `.app-window.embedded`). Default: false. */
  embedded?: boolean
}

export default function App({ captureKeys = true, embedded = false }: AppOptions = {}) {
  // ?theme=light lets automation (demo screenshots) capture both themes.
  const [theme, setTheme] = useState<'dark' | 'light'>(() =>
    new URLSearchParams(window.location.search).get('theme') === 'light' ? 'light' : 'dark',
  )
  const [activePanel, setActivePanel] = useState<Side>('left')
  const [drives, setDrives] = useState<Drive[]>([])
  // Saved connections, kept alongside drives: the badge counts them and the source
  // selector needs the full records (host/port/user) to open one for editing —
  // `Drive` carries only a display name and a key.
  const [connections, setConnections] = useState<Connection[]>([])
  const [modal, setModal] = useState<ModalState>({ type: 'none' })
  const [viewer, setViewer] = useState<{ side: Side; path: string; mode: 'view' | 'edit' } | null>(null)
  const [favoritesOnly, setFavoritesOnly] = useState(false)

  // Real per-panel terminals (xterm over WebSocket → the app's PTY). Each side is
  // independent; one can be expanded to span both panels.
  const [termOpen, setTermOpen] = useState<{ left: boolean; right: boolean }>({ left: false, right: false })
  const [expandedSide, setExpandedSide] = useState<Side | null>(null)

  const toggleTerminal = useCallback((side: Side) => {
    setTermOpen((p) => ({ ...p, [side]: !p[side] }))
    setExpandedSide((cur) => (cur === side ? null : cur))
  }, [])

  // Expand/collapse mirrors to GTK; the resulting state echoes back over /api/ws.
  const toggleTerminalExpand = useCallback((side: Side) => {
    const willExpand = expandedSide !== side
    setExpandedSide(willExpand ? side : null)
    api.setTerminalExpanded(side, willExpand).catch(() => {})
  }, [expandedSide])

  const leftRef = useRef<PanelHandle | null>(null)
  const rightRef = useRef<PanelHandle | null>(null)

  const refOf = (s: Side) => (s === 'left' ? leftRef.current : rightRef.current)
  const other = (s: Side): Side => (s === 'left' ? 'right' : 'left')

  const closeModal = () => setModal({ type: 'none' })

  // ── data ──────────────────────────────────────────────────────────────────
  const refreshDrives = useCallback(() => {
    api.getDrives().then(setDrives).catch(() => setDrives([]))
    api.getConnections().then(setConnections).catch(() => setConnections([]))
  }, [])

  useEffect(() => { refreshDrives() }, [refreshDrives])

  useEffect(() => {
    api.getFavoritesOnly().then(setFavoritesOnly).catch(() => {})
  }, [])

  const handleToggleFavorite = useCallback(async (key: string) => {
    await api.toggleFavorite(key)
    refreshDrives()
  }, [refreshDrives])

  const handleSetFavoritesOnly = useCallback(async (value: boolean) => {
    setFavoritesOnly(value)
    await api.setFavoritesOnly(value)
    refreshDrives()
  }, [refreshDrives])

  // Persist a connection; optionally mount it in the panel that opened the form.
  const handleSaveConnection = useCallback(async (side: Side, conn: Connection, connect: boolean) => {
    await api.saveConnection(conn)
    refreshDrives()
    if (connect) await api.connectTo(side, conn)
  }, [refreshDrives])

  const handleConnectTo = useCallback(async (side: Side, conn: Connection) => {
    await api.connectTo(side, conn)
  }, [])

  // Export lands as a downloaded file; the browser cannot write one directly, so the
  // JSON comes back through the API and is handed to a temporary object URL.
  const handleExportConnections = useCallback(async (password?: string) => {
    const data = await api.exportConnections(password)
    const url = URL.createObjectURL(new Blob([data], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'ice-commander-connections.json'
    a.click()
    URL.revokeObjectURL(url)
  }, [])

  const handleImportConnections = useCallback(async (data: string, password?: string) => {
    try {
      await api.importConnections(data, password)
    } catch (e) {
      // An encrypted file answers `needs-password`; ask once and retry with it.
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('needs-password') && password === undefined) {
        const pw = prompt(tr('conn_manager.import_password_title')) ?? ''
        if (pw === '') return
        await api.importConnections(data, pw)
      } else {
        alert(msg)
        return
      }
    }
    refreshDrives()
  }, [refreshDrives])

  const handleDeleteConnection = useCallback(async (name: string) => {
    await api.deleteConnection(name)
    refreshDrives()
  }, [refreshDrives])

  useEffect(() => {
    const disconnect = api.connectWebSocket(
      (side, state) => { refOf(side as Side)?.applyState(state) },
      // mirror GTK terminal open/close → web
      (side, open) => {
        setTermOpen((p) => ({ ...p, [side]: open }))
        if (!open) setExpandedSide((cur) => (cur === side ? null : cur))
      },
      // mirror GTK terminal expand/collapse → web
      (side, expanded) => setExpandedSide(expanded ? (side as Side) : null),
      // remote F3/F4 — the app asks us to open the viewer
      (side, path, mode) => setViewer({ side: side as Side, path, mode }),
      // mirror GTK panel view-mode switch → web
      (side, mode) => refOf(side as Side)?.applyViewMode(mode),
      // GTK closed a native viewer window → close the web overlay (state only — no
      // command back, so the echo can't loop)
      () => setViewer(null),
      // The app opened/closed its own connections dialog → mirror it here. State only,
      // for the same reason: sending the command back would bounce forever.
      (open) => setModal((m) => (open ? { type: 'connections' } : (m.type === 'connections' ? { type: 'none' } : m))),
    )
    return disconnect
  }, [])

  // ── file operations ─────────────────────────────────────────────────────────
  const handleNewFolder = useCallback(async () => {
    const name = modal.value?.trim()
    if (!name) return
    closeModal()
    await api.mkdir(activePanel, name)
    refOf(activePanel)?.reload()
  }, [modal.value, activePanel])

  const handleRenameConfirm = useCallback(async () => {
    const newName = modal.value?.trim()
    const h = refOf(activePanel)
    const oldPath = h?.selectedPath()
    if (!newName || !oldPath) return
    closeModal()
    await api.renameEntry(activePanel, oldPath, newName)
    h?.reload()
  }, [modal.value, activePanel])

  const handleDeleteConfirm = useCallback(async () => {
    const h = refOf(activePanel)
    const paths = h?.selectedPaths() ?? []
    if (paths.length === 0) return
    closeModal()
    await api.deleteEntries(activePanel, paths)
    h?.reload()
  }, [activePanel])

  const triggerRename = () => {
    const h = refOf(activePanel)
    const name = h?.selectedName()
    if (!name) return
    setModal({ type: 'rename', value: name })
  }

  const triggerDelete = () => {
    const h = refOf(activePanel)
    const paths = h?.selectedPaths() ?? []
    if (paths.length === 0) return
    const label = paths.length > 1 ? `${paths.length} items` : h!.selectedName()!
    setModal({ type: 'delete', value: label })
  }

  // F3 = view (read-only), F4 = edit — open the selected file in the FileViewer
  const openViewer = (mode: 'view' | 'edit') => {
    const h = refOf(activePanel)
    if (!h || h.showingSelector()) return
    const p = h.selectedPath()
    if (!p) { alert('Select a file first.'); return }
    if (h.selectedIsDir()) { alert('Cannot open a folder — pick a file.'); return }
    setViewer({ side: activePanel, path: p, mode })
    // mirror into the GTK app: open the same file in a native viewer window
    api.openNative(activePanel, p).catch(() => {})
  }

  const handleCopy = useCallback(async () => {
    const src = refOf(activePanel)
    const dst = refOf(other(activePanel))
    const paths = src?.selectedPaths() ?? []
    if (paths.length === 0) { alert('Select an item to copy first.'); return }
    if (dst?.showingSelector()) { alert('Destination panel is on the Select Source page.'); return }
    await api.copyEntries(activePanel, other(activePanel), paths)
    src?.reload(); dst?.reload()
  }, [activePanel])

  const handleMove = useCallback(async () => {
    const src = refOf(activePanel)
    const dst = refOf(other(activePanel))
    const paths = src?.selectedPaths() ?? []
    if (paths.length === 0) { alert('Select an item to move first.'); return }
    if (dst?.showingSelector()) { alert('Destination panel is on the Select Source page.'); return }
    await api.moveEntries(activePanel, other(activePanel), paths)
    src?.reload(); dst?.reload()
  }, [activePanel])

  // ── keyboard ────────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!captureKeys) return
    const onKey = (e: KeyboardEvent) => {
      // While the terminal is focused, let all keys flow to the shell (incl. F-keys for
      // TUI apps); use the terminal's × button or the footer F9 to close it.
      if (modal.type !== 'none' || viewer || document.activeElement?.classList.contains('xterm-helper-textarea')) return
      switch (e.key) {
        case 'F1': e.preventDefault(); setModal({ type: 'help' }); break
        case 'F2': e.preventDefault(); triggerRename(); break
        case 'F3': e.preventDefault(); openViewer('view'); break
        case 'F4': e.preventDefault(); openViewer('edit'); break
        case 'F5': e.preventDefault(); handleCopy(); break
        case 'F6': e.preventDefault(); handleMove(); break
        case 'F7': e.preventDefault(); setModal({ type: 'new-folder', value: '' }); break
        case 'F8': case 'Delete': e.preventDefault(); triggerDelete(); break
        case 'F9': e.preventDefault(); toggleTerminal(activePanel); break
        case 'F10': e.preventDefault(); setModal({ type: 'quit' }); break
        case 'Tab': e.preventDefault(); setActivePanel((p) => (p === 'left' ? 'right' : 'left')); break
        default: {
          // Type-ahead: a printable character jumps the active panel's cursor
          // to the next entry starting with it (repeats cycle, wrap-around).
          // Skip when any input owns the keyboard (forms, filter, rename).
          const tag = document.activeElement?.tagName
          if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') break
          if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey && e.key !== ' ') {
            if (refOf(activePanel)?.quickJump(e.key)) e.preventDefault()
          }
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [captureKeys, modal.type, viewer, activePanel, handleCopy, handleMove, toggleTerminal])

  return (
    <div className={`app-window ${theme === 'light' ? 'light-theme' : ''} ${embedded ? 'embedded' : ''}`}>
      {/* Header */}
      <header className="window-header">
        <div className="header-left">
          <img src={appLogo} alt={tr("webpult.logo")} className="window-logo" />
          <button className="window-icon-btn" onClick={() => setModal({ type: 'settings' })} title={tr("settings_title")}>
            <img src={settingIcon} alt="settings" />
          </button>
          <img src={doneIcon} alt="ready" style={{ width: '16px', height: '16px' }} />
        </div>
        <div className="header-title">Ice Commander - Dual-Pane File Manager</div>
        <div className="header-right">
          <button
            className="window-icon-btn theme-toggle-btn"
            onClick={() => setTheme((p) => (p === 'dark' ? 'light' : 'dark'))}
            title={`Switch to ${theme === 'dark' ? 'Light' : 'Dark'} Theme`}
            style={{ marginRight: '4px' }}
          >
            <img src={theme === 'dark' ? afternoonIcon : nightIcon} alt="theme" style={{ width: '18px', height: '18px' }} />
          </button>
          <div
            className="connections-badge"
            style={{ cursor: 'pointer' }}
            title={tr("selector.btn_connections")}
            onClick={() => {
              refreshDrives()
              setModal({ type: 'connections' })
              // Ask the app to raise its own dialog; in the standalone web server this
              // is a no-op, since the browser owns the only UI there.
              void api.setConnectionsDialog(true).catch(() => {})
            }}
          >
            <img src={connectIcon} alt="connections" />
            <span>{tr("webpult.connections")} ({connections.length})</span>
          </div>
          <div className="window-controls">
            <div className="control-dot minimize" title={tr("webpult.minimize")}></div>
            <div className="control-dot maximize" title={tr("webpult.maximize")}></div>
            <div className="control-dot close" onClick={() => setModal({ type: 'quit' })} title={tr("editor.close")}></div>
          </div>
        </div>
      </header>

      {/* Panels */}
      <div className="panels-container">
        <Panel
          side="left"
          active={activePanel === 'left'}
          drives={drives}
          connections={connections}
          onActivate={() => setActivePanel('left')}
          onNewFolder={() => setModal({ type: 'new-folder', value: '' })}
          onRename={triggerRename}
          onDelete={triggerDelete}
          onToggleFavorite={handleToggleFavorite}
          favoritesOnly={favoritesOnly}
          onSetFavoritesOnly={handleSetFavoritesOnly}
          onSaveConnection={(conn, connect) => handleSaveConnection('left', conn, connect)}
          onDeleteConnection={handleDeleteConnection}
          onOpenFile={(path) => setViewer({ side: 'left', path, mode: 'view' })}
          terminalOpen={termOpen.left}
          terminalExpanded={expandedSide === 'left'}
          onToggleTerminalExpand={() => toggleTerminalExpand('left')}
          onCloseTerminal={() => toggleTerminal('left')}
          ref={(h) => { leftRef.current = h }}
        />
        <Panel
          side="right"
          active={activePanel === 'right'}
          drives={drives}
          connections={connections}
          onActivate={() => setActivePanel('right')}
          onNewFolder={() => setModal({ type: 'new-folder', value: '' })}
          onRename={triggerRename}
          onDelete={triggerDelete}
          onToggleFavorite={handleToggleFavorite}
          favoritesOnly={favoritesOnly}
          onSetFavoritesOnly={handleSetFavoritesOnly}
          onSaveConnection={(conn, connect) => handleSaveConnection('right', conn, connect)}
          onDeleteConnection={handleDeleteConnection}
          onOpenFile={(path) => setViewer({ side: 'right', path, mode: 'view' })}
          terminalOpen={termOpen.right}
          terminalExpanded={expandedSide === 'right'}
          onToggleTerminalExpand={() => toggleTerminalExpand('right')}
          onCloseTerminal={() => toggleTerminal('right')}
          ref={(h) => { rightRef.current = h }}
        />
      </div>

      {/* F-key bar */}
      <footer className="bottom-fkey-bar">
        <button className="fkey-btn" onClick={() => setModal({ type: 'help' })}><span className="fkey-btn-num">F1</span>{tr("webpult.help")}</button>
        <button className="fkey-btn" onClick={triggerRename}><span className="fkey-btn-num">F2</span>{tr("fm.context.rename")}</button>
        <button className="fkey-btn" onClick={() => openViewer('view')}><span className="fkey-btn-num">F3</span>{tr("editor.view")}</button>
        <button className="fkey-btn" onClick={() => openViewer('edit')}><span className="fkey-btn-num">F4</span>{tr("conn_manager.edit_btn")}</button>
        <button className="fkey-btn" onClick={handleCopy}><span className="fkey-btn-num">F5</span>{tr("fm.action_copy")}</button>
        <button className="fkey-btn" onClick={handleMove}><span className="fkey-btn-num">F6</span>{tr("fm.action_move")}</button>
        <button className="fkey-btn" onClick={() => setModal({ type: 'new-folder', value: '' })}><span className="fkey-btn-num">F7</span>{tr("webpult.newfolder")}</button>
        <button className="fkey-btn" onClick={triggerDelete}><span className="fkey-btn-num">F8</span>{tr("common_forms.delete")}</button>
        <button className="fkey-btn" onClick={() => toggleTerminal(activePanel)}><span className="fkey-btn-num">F9</span>{tr("resources.terminal")}</button>
        <button className="fkey-btn" onClick={() => setModal({ type: 'quit' })}><span className="fkey-btn-num">F10</span>{tr("webpult.quit")}</button>
      </footer>

      {/* Modals */}
      {modal.type === 'new-folder' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <h3 className="modal-title">{tr("fm.create_dir_title")}</h3>
            <input
              type="text" className="modal-input" value={modal.value || ''} autoFocus
              onChange={(e) => setModal({ ...modal, value: e.target.value })}
              onKeyDown={(e) => e.key === 'Enter' && handleNewFolder()}
              placeholder={tr("webpult.folder_name")}
            />
            <div className="modal-buttons">
              <button className="modal-btn cancel" onClick={closeModal}>{tr("account.cancel")}</button>
              <button className="modal-btn confirm" onClick={handleNewFolder}>{tr("fm.create_btn")}</button>
            </div>
          </div>
        </div>
      )}

      {modal.type === 'connections' && (
        <ConnectionsDialog
          connections={connections}
          onClose={() => {
            closeModal()
            void api.setConnectionsDialog(false).catch(() => {})
          }}
          onSave={(conn, connect) => handleSaveConnection(activePanel, conn, connect)}
          onDelete={handleDeleteConnection}
          onConnect={(conn) => handleConnectTo(activePanel, conn)}
          onExport={handleExportConnections}
          onImport={handleImportConnections}
        />
      )}

      {modal.type === 'rename' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <h3 className="modal-title">{tr("fm.rename_title")}</h3>
            <input
              type="text" className="modal-input" value={modal.value || ''} autoFocus
              onChange={(e) => setModal({ ...modal, value: e.target.value })}
              onKeyDown={(e) => e.key === 'Enter' && handleRenameConfirm()}
              placeholder={tr("webpult.new_name")}
            />
            <div className="modal-buttons">
              <button className="modal-btn cancel" onClick={closeModal}>{tr("account.cancel")}</button>
              <button className="modal-btn confirm" onClick={handleRenameConfirm}>{tr("fm.context.rename")}</button>
            </div>
          </div>
        </div>
      )}

      {modal.type === 'delete' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <h3 className="modal-title">{tr("webpult.delete_item")}</h3>
            <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>{tr("webpult.permanently_delete")}<strong>{modal.value}</strong>? This action cannot be undone.
            </p>
            <div className="modal-buttons">
              <button className="modal-btn cancel" onClick={closeModal}>{tr("account.cancel")}</button>
              <button className="modal-btn danger" onClick={handleDeleteConfirm}>{tr("common_forms.delete")}</button>
            </div>
          </div>
        </div>
      )}

      {modal.type === 'help' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()} style={{ maxWidth: '450px' }}>
            <h3 className="modal-title">{tr("help.keyboard_shortcuts")}</h3>
            <div className="help-modal-content">
              {[
                ['Select File / Folder', 'Left Click'],
                ['Navigate / Open', 'Double Click'],
                ['Switch active panel', 'TAB'],
                ['Rename selected item', 'F2'],
                ['Copy / Move to other panel', 'F5 / F6'],
                ['Create new folder', 'F7'],
                ['Delete selected item', 'F8 / Delete'],
                ['Toggle terminal', 'F9'],
              ].map(([desc, key]) => (
                <div className="help-item" key={key}>
                  <span className="help-item-desc">{desc}</span>
                  <span className="help-item-key">{key}</span>
                </div>
              ))}
            </div>
            <div className="modal-buttons">
              <button className="modal-btn confirm" onClick={closeModal}>{tr("webpult.got_it")}</button>
            </div>
          </div>
        </div>
      )}

      {modal.type === 'settings' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <h3 className="modal-title">{tr("webpult.ice_commander_web_config")}</h3>
            <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>
              Remote control of the running GTK application over REST + WebSocket.
            </p>
            <div className="modal-buttons">
              <button className="modal-btn confirm" onClick={closeModal}>{tr("editor.close")}</button>
            </div>
          </div>
        </div>
      )}

      {modal.type === 'quit' && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()} style={{ textAlign: 'center', gap: '12px' }}>
            <h3 className="modal-title">{tr("webpult.disconnect_session")}</h3>
            <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>{tr("webpult.close_this_web_control_session")}</p>
            <div className="modal-buttons" style={{ justifyContent: 'center' }}>
              <button className="modal-btn confirm" onClick={closeModal}>{tr("webpult.stay")}</button>
            </div>
          </div>
        </div>
      )}

      {viewer && (
        <FileViewer
          side={viewer.side}
          path={viewer.path}
          mode={viewer.mode}
          onClose={() => { setViewer(null); api.closeViewerWindows().catch(() => {}) }}
          onSaved={() => refOf(viewer.side)?.reload()}
        />
      )}
    </div>
  )
}
