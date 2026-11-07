import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { adopt, tr } from "./lib/i18n";
import { DialogScope, type Dialogs } from './lib/dialogs'
import { beingTypedInto, holdFocus, holdPage, KeyScope, useEscape, type KeyTarget } from './lib/keyScope'
import * as api from './api/client'
import type { Connection, Drive, Side } from './api/types'
import { Panel, type PanelHandle } from './components/Panel'
import { ConnectionsDialog } from './components/ConnectionsDialog'
import { FileViewer } from './components/FileViewer'
import { PluginViewer } from './components/view/PluginViewer'

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

type Question = {
  text: string
  body?: string
  kind: 'notice' | 'confirm' | 'password'
  settle: (answer: string | null) => void
}

const plainFKey = (e: KeyboardEvent) =>
  /^F([1-9]|10)$/.test(e.key) && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey

const HOST_ERRORS: Record<string, string> = {
  'no saved connections to export': 'conn_manager.export_empty',
  'wrong password': 'conn_manager.import_wrong_password',
  'not an ice-commander connections file': 'conn_manager.import_malformed',
}

const worded = (message: string) => (HOST_ERRORS[message] === undefined ? message : tr(HOST_ERRORS[message]))

export interface AppOptions {
  /** Grab F1–F10 / Tab / Delete on `window`. Not consulted when `embedded`: the
   *  keys are then taken only while focus is inside the window, so the host
   *  page keeps its own. Default: true. */
  captureKeys?: boolean
  /** Mounted inside a host page rather than owning the viewport: the window
   *  chrome fills its container (see `.app-window.embedded`) and handles keys
   *  only while it holds focus; Escape hands focus back to the page. Default: false. */
  embedded?: boolean
}

export default function App({ captureKeys = true, embedded = false }: AppOptions = {}) {
  const [root, setRoot] = useState<HTMLDivElement | null>(null)
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
  // No plugin claimed the open file — the built-in viewer draws it instead.
  const [plain, setPlain] = useState(false)
  const [favoritesOnly, setFavoritesOnly] = useState(false)

  // alert/confirm/prompt would freeze the whole host page; questions open inside the window instead.
  const [question, setQuestion] = useState<Question | null>(null)
  const asked = useRef<Question | null>(null)
  const reply = useRef<HTMLInputElement | null>(null)
  const dialogs = useMemo<Dialogs>(() => {
    const ask = (text: string, kind: Question['kind'], body?: string) =>
      new Promise<string | null>((resolve) => {
        const back = document.activeElement
        const open: Question = {
          text,
          body,
          kind,
          settle: (answer) => {
            asked.current = null
            setQuestion(null)
            if (back instanceof HTMLElement && back.isConnected) back.focus({ preventScroll: true })
            resolve(answer)
          },
        }
        asked.current = open
        setQuestion(open)
      })
    return {
      notice: (text, body) => ask(text, 'notice', body).then(() => {}),
      confirm: (text) => ask(text, 'confirm').then((answer) => answer !== null),
      password: (title, body) => ask(title, 'password', body),
    }
  }, [])

  const showViewer = useCallback((side: Side, path: string, mode: 'view' | 'edit') => {
    setPlain(false)
    setViewer({ side, path, mode })
  }, [])

  const closeViewer = useCallback(() => {
    setViewer(null)
    api.closeViewerWindows().catch(() => {})
  }, [])

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

  // The host owns the language and holds whatever dictionaries plugins
  // registered, so the built-in ones are only a fallback until this lands.
  const [lang, setLang] = useState('en')
  useEffect(() => {
    api
      .fetchTranslations()
      .then(({ lang: served, keys }) => {
        adopt(served, keys)
        setLang(served)
      })
      .catch(() => {})
  }, [])

  const handleToggleFavorite = useCallback(async (key: string) => {
    await api.toggleFavorite(key)
    refreshDrives()
  }, [refreshDrives])

  // Unset means the right panel, as gtk-app/src/settings/page_interface.rs decides it.
  const [openTarget, setOpenTarget] = useState('right')
  const refreshSettings = useCallback(() => {
    api
      .fetchSettings()
      .then((settings) => {
        const target = settings['ui.open_connection_target']
        setOpenTarget(typeof target === 'string' ? target : 'right')
      })
      .catch(() => {})
  }, [])

  useEffect(() => { refreshSettings() }, [refreshSettings])

  const connectSide: Side =
    openTarget === 'left' || openTarget === 'right'
      ? openTarget
      : openTarget === 'opposite'
        ? other(activePanel)
        : activePanel

  const handleSetFavoritesOnly = useCallback(async (value: boolean) => {
    setFavoritesOnly(value)
    await api.setFavoritesOnly(value)
    refreshDrives()
  }, [refreshDrives])

  // The host committed and stored the record, and mounted it when asked; all
  // that is left here is to reload the lists it appears in.
  const handleConnectionSaved = useCallback(
    (side: Side, _name: string, connected: boolean) => {
      refreshDrives()
      if (connected) void (side === 'left' ? leftRef : rightRef).current?.reload()
    },
    [refreshDrives],
  )

  const handleConnectTo = useCallback(async (side: Side, conn: Connection) => {
    try {
      await api.connectTo(side, conn)
    } catch (e) {
      void dialogs.notice(tr('fm.load_failed'), e instanceof Error ? e.message : String(e))
    }
    void refOf(side)?.reload()
  }, [dialogs])

  // Export lands as a downloaded file; the browser cannot write one directly, so the
  // JSON comes back through the API and is handed to a temporary object URL.
  const handleExportConnections = useCallback(async (password?: string) => {
    let data: string
    try {
      data = await api.exportConnections(password)
    } catch (e) {
      void dialogs.notice(tr('conn_manager.export_failed'), worded(e instanceof Error ? e.message : String(e)))
      return
    }
    const url = URL.createObjectURL(new Blob([data], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'ice-commander-connections.json'
    a.click()
    URL.revokeObjectURL(url)
    void dialogs.notice(
      tr('conn_manager.export_done_title'),
      tr(password === undefined ? 'conn_manager.export_done_plain' : 'conn_manager.export_done_encrypted'),
    )
  }, [dialogs])

  const handleImportConnections = useCallback(async (data: string, password?: string) => {
    const attempt = (pw?: string) =>
      api.importConnections(data, pw).then(
        () => null,
        (e: unknown) => (e instanceof Error ? e.message : String(e)),
      )
    let failure = await attempt(password)
    // An encrypted file answers `needs-password`; ask once and retry with it.
    if (failure !== null && failure.includes('needs-password') && password === undefined) {
      const pw =
        (await dialogs.password(tr('conn_manager.import_password_title'), tr('conn_manager.import_password_body'))) ?? ''
      if (pw === '') return
      failure = await attempt(pw)
    }
    if (failure !== null) {
      void dialogs.notice(tr('conn_manager.import_failed'), worded(failure))
      return
    }
    refreshDrives()
  }, [refreshDrives, dialogs])

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
      (side, path, mode) => showViewer(side as Side, path, mode),
      // mirror GTK panel view-mode switch → web
      (side, mode) => refOf(side as Side)?.applyViewMode(mode),
      // GTK closed a native viewer window → close the web overlay (state only — no
      // command back, so the echo can't loop)
      () => setViewer(null),
      // The app opened/closed its own connections dialog → mirror it here. State only,
      // for the same reason: sending the command back would bounce forever.
      (open) => {
        if (open) refreshSettings()
        setModal((m) => (open ? { type: 'connections' } : (m.type === 'connections' ? { type: 'none' } : m)))
      },
    )
    return disconnect
  }, [showViewer, refreshSettings])

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
    if (!p) { void dialogs.notice('Select a file first.'); return }
    if (h.selectedIsDir()) { void dialogs.notice('Cannot open a folder — pick a file.'); return }
    showViewer(activePanel, p, mode)
    // mirror into the GTK app: open the same file in a native viewer window
    api.openNative(activePanel, p).catch(() => {})
  }

  const handleCopy = useCallback(async () => {
    const src = refOf(activePanel)
    const dst = refOf(other(activePanel))
    const paths = src?.selectedPaths() ?? []
    if (paths.length === 0) { void dialogs.notice('Select an item to copy first.'); return }
    if (dst?.showingSelector()) { void dialogs.notice('Destination panel is on the Select Source page.'); return }
    await api.copyEntries(activePanel, other(activePanel), paths)
    src?.reload(); dst?.reload()
  }, [activePanel, dialogs])

  const handleMove = useCallback(async () => {
    const src = refOf(activePanel)
    const dst = refOf(other(activePanel))
    const paths = src?.selectedPaths() ?? []
    if (paths.length === 0) { void dialogs.notice('Select an item to move first.'); return }
    if (dst?.showingSelector()) { void dialogs.notice('Destination panel is on the Select Source page.'); return }
    await api.moveEntries(activePanel, other(activePanel), paths)
    src?.reload(); dst?.reload()
  }, [activePanel, dialogs])

  // ── keyboard ────────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!embedded || root === null) return
    return holdFocus(root)
  }, [embedded, root])

  // Registered once, before any viewer's Escape listener, so an open question gets every key first.
  useEffect(() => {
    const scope: KeyTarget | null = embedded ? root : window
    if (scope === null) return
    const onKey = (e: KeyboardEvent) => {
      const open = asked.current
      if (open === null) return
      e.stopImmediatePropagation()
      if (embedded) holdPage(e)
      if (plainFKey(e)) e.preventDefault()
      if (e.key === 'Escape') open.settle(null)
      else if (e.key === 'Enter' && e.target instanceof HTMLInputElement && e.target === reply.current) open.settle(e.target.value)
      else return
      // Otherwise the keypress that follows reaches the control focus went back to, and Enter clicks it.
      e.preventDefault()
    }
    scope.addEventListener('keydown', onKey, true)
    return () => scope.removeEventListener('keydown', onKey, true)
  }, [embedded, root])

  useEscape(closeModal, {
    open: modal.type !== 'none' && modal.type !== 'connections',
    scope: embedded ? root : window,
  })

  useEffect(() => {
    const scope: KeyTarget | null = embedded ? root : captureKeys ? window : null
    if (scope === null) return
    const onKey = (e: KeyboardEvent) => {
      if (embedded) holdPage(e)
      if (plainFKey(e)) e.preventDefault()
      // A focused field owns the keyboard, and so does the terminal.
      if (beingTypedInto()) return
      if (modal.type !== 'none' || viewer) return
      switch (e.key) {
        case 'F1': e.preventDefault(); setModal({ type: 'help' }); break
        case 'F2': e.preventDefault(); triggerRename(); break
        case 'F3': e.preventDefault(); openViewer('view'); break
        case 'F4': e.preventDefault(); openViewer('edit'); break
        case 'F5': e.preventDefault(); handleCopy(); break
        case 'F6': e.preventDefault(); handleMove(); break
        case 'F7': e.preventDefault(); setModal({ type: 'new-folder', value: '' }); break
        case 'F8': case 'Delete': e.preventDefault(); triggerDelete(); break
        case 'F9': e.preventDefault(); if (!e.repeat) toggleTerminal(activePanel); break
        case 'F10': e.preventDefault(); setModal({ type: 'quit' }); break
        case 'Tab': e.preventDefault(); setActivePanel((p) => (p === 'left' ? 'right' : 'left')); break
        case 'Escape': if (embedded) (e.target as HTMLElement).blur(); break
        default: {
          // Type-ahead: a printable character jumps the active panel's cursor
          // to the next entry starting with it (repeats cycle, wrap-around).
          if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey && e.key !== ' ') {
            if (refOf(activePanel)?.quickJump(e.key)) e.preventDefault()
          }
        }
      }
    }
    scope.addEventListener('keydown', onKey)
    return () => scope.removeEventListener('keydown', onKey)
  }, [captureKeys, embedded, root, modal.type, viewer, activePanel, handleCopy, handleMove, toggleTerminal])

  return (
    <KeyScope value={embedded ? root : window}>
      <DialogScope value={dialogs}>
        <div
          ref={setRoot}
          tabIndex={embedded ? -1 : undefined}
          lang={lang}
          className={`app-window ${theme === 'light' ? 'light-theme' : ''} ${embedded ? 'embedded' : ''}`}
        >
          {/* Header */}
          <header className="window-header">
            <div className="header-left">
              <img src={appLogo} alt={tr("webpult.logo")} className="window-logo" />
              <button className="window-icon-btn" onClick={() => setModal({ type: 'settings' })} title={tr("settings.window_title")}>
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
                  refreshSettings()
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
              onConnectionSaved={(name, connected) => handleConnectionSaved('left', name, connected)}
              onDeleteConnection={handleDeleteConnection}
              onOpenFile={(path) => showViewer('left', path, 'view')}
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
              onConnectionSaved={(name, connected) => handleConnectionSaved('right', name, connected)}
              onDeleteConnection={handleDeleteConnection}
              onOpenFile={(path) => showViewer('right', path, 'view')}
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
              side={connectSide}
              onSaved={(name, connected) => handleConnectionSaved(connectSide, name, connected)}
              onDelete={handleDeleteConnection}
              onConnect={(conn) => handleConnectTo(connectSide, conn)}
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
                <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>{tr("webpult.permanently_delete")} <strong>{modal.value}</strong>? This action cannot be undone.
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

          {viewer &&
            (viewer.mode === 'view' && !plain ? (
              <PluginViewer
                side={viewer.side}
                path={viewer.path}
                onClose={closeViewer}
                onUnclaimed={() => setPlain(true)}
              />
            ) : (
              <FileViewer
                side={viewer.side}
                path={viewer.path}
                mode={viewer.mode}
                onClose={closeViewer}
                onSaved={() => refOf(viewer.side)?.reload()}
              />
            ))}

          {question && (
            <div className="modal-overlay" onClick={() => question.settle(null)}>
              <div className="modal-card" onClick={(e) => e.stopPropagation()}>
                <h3 className="modal-title">{question.text}</h3>
                {question.body !== undefined && (
                  <p style={{ fontSize: '13px', color: 'var(--text-muted)', whiteSpace: 'pre-line' }}>{question.body}</p>
                )}
                {question.kind === 'password' && (
                  <input ref={reply} type="password" className="modal-input" autoComplete="off" autoFocus />
                )}
                <div className="modal-buttons">
                  {question.kind !== 'notice' && (
                    <button className="modal-btn cancel" onClick={() => question.settle(null)}>{tr("common.cancel")}</button>
                  )}
                  <button
                    className="modal-btn confirm"
                    autoFocus={question.kind !== 'password'}
                    onClick={() => question.settle(reply.current?.value ?? '')}
                  >
                    {tr("common.ok")}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </DialogScope>
    </KeyScope>
  )
}
