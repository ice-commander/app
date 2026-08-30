import { useCallback, useEffect, useReducer, useState } from 'react'
import { tr } from "../lib/i18n";
import * as api from '../api/client'
import type { Connection, Drive, FileEntry, Level, Operation, PanelState, Side, Tab } from '../api/types'
import { atRoot, formatSize } from '../lib/format'
import { driveIcon, driveSection, SECTION_ORDER } from '../lib/driveIcons'
import { Breadcrumbs } from './Breadcrumbs'
import { FileList, type ViewMode } from './FileList'
import { SelectSource } from './SelectSource'
import { Terminal } from './Terminal'

import starIcon from '../assets/star.svg'
import emptyStarIcon from '../assets/empty-star.svg'
import terminalIcon from '../assets/unix-console.svg'
import processesIcon from '../assets/processes.svg'
import expandIcon from '../assets/expand.svg'
import collapseIcon from '../assets/collapse.svg'
import folderIcon from '../assets/folder.svg'
import arrowUpIcon from '../assets/arrow-up.svg'
import arrowLeftIcon from '../assets/arrow-left.svg'
import arrowRightIcon from '../assets/arrow-right.svg'
import refreshIcon from '../assets/refresh.svg'
import editPencilIcon from '../assets/edit-pencil.svg'
import deleteFolderIcon from '../assets/delete-folder.svg'
import listIconsIcon from '../assets/list-icons.svg'
import mediumIconsIcon from '../assets/medium-icons.svg'
import smallIconsIcon from '../assets/small-icons.svg'

interface DataState {
  levels: Level[]
  path: string
  entries: FileEntry[]
  showing_selector: boolean
  tabs: Tab[]
  active_tab: number
  loading: boolean
  error: string | null
}

type Action =
  | { type: 'loading' }
  | { type: 'loaded'; state: PanelState }
  | { type: 'error'; message: string }

function reducer(s: DataState, a: Action): DataState {
  switch (a.type) {
    case 'loading':
      return { ...s, loading: true, error: null }
    case 'loaded':
      return {
        levels: a.state.levels,
        path: a.state.path,
        entries: a.state.entries,
        showing_selector: a.state.showing_selector,
        // A single implicit tab when the backend has no tab model (empty/absent).
        tabs: a.state.tabs && a.state.tabs.length > 0 ? a.state.tabs : [{ id: 0, title: '/' }],
        active_tab: a.state.active_tab ?? 0,
        loading: false,
        error: null,
      }
    case 'error':
      return { ...s, loading: false, error: a.message }
  }
}

// GTK has three modes, the web three; `tiles` is web-only and rides on GTK's `large`.
const GTK_TO_WEB: Record<string, ViewMode> = { list: 'list', small: 'medium', large: 'medium' }
const WEB_TO_GTK: Record<ViewMode, string> = { list: 'list', medium: 'large', tiles: 'large' }

export interface PanelHandle {
  reload: () => void
  applyState: (s: PanelState) => void
  /** Set the view mode from a GTK-side change (mirrored over WS); no REST echo. */
  applyViewMode: (mode: string) => void
  selectedName: () => string | null
  selectedPath: () => string | null
  selectedIsDir: () => boolean
  /** Paths of the whole multi-selection, in click order. */
  selectedPaths: () => string[]
  /** Type-ahead: cursor to the next entry starting with `ch`; true if one matched. */
  quickJump: (ch: string) => boolean
  currentPath: () => string
  showingSelector: () => boolean
}

interface PanelProps {
  side: Side
  active: boolean
  drives: Drive[]
  /** Saved connections, for opening one in the edit form. */
  connections: Connection[]
  onActivate: () => void
  onNewFolder: () => void
  onRename: () => void
  onDelete: () => void
  onToggleFavorite: (key: string) => void
  favoritesOnly: boolean
  onSetFavoritesOnly: (value: boolean) => void
  /** Persist a connection from the panel's form; `connect` = also mount it here. */
  onSaveConnection: (conn: Connection, connect: boolean) => void
  onDeleteConnection: (name: string) => void
  onOpenFile: (path: string) => void
  terminalOpen: boolean
  terminalExpanded: boolean
  onToggleTerminalExpand: () => void
  onCloseTerminal: () => void
  ref?: (h: PanelHandle | null) => void
}

const isLocal = (d: Drive) => d.kind === 'root' || d.kind === 'home' || d.kind === 'drive'

export function Panel({ side, active, drives, connections, onActivate, onNewFolder, onRename, onDelete, onToggleFavorite, favoritesOnly, onSetFavoritesOnly, onSaveConnection, onDeleteConnection, onOpenFile, terminalOpen, terminalExpanded, onToggleTerminalExpand, onCloseTerminal, ref }: PanelProps) {
  const [data, dispatch] = useReducer(reducer, {
    levels: [],
    path: '/',
    entries: [],
    showing_selector: false,
    tabs: [{ id: 0, title: '/' }],
    active_tab: 0,
    loading: true,
    error: null,
  })
  // Multi-selection in click order; the last name is the cursor (rename/F3/F4
  // keep working on "the" selected item). Plain click = single, Ctrl toggles,
  // Shift extends a range over the visible entry order.
  const [selected, setSelected] = useState<string[]>([])
  const cursorName = selected.length > 0 ? selected[selected.length - 1] : null
  const [viewMode, setViewMode] = useState<ViewMode>('list')
  const [dropdownOpen, setDropdownOpen] = useState(false)
  // OS → panel drag-and-drop: dropping files uploads them into the current dir.
  const [dropActive, setDropActive] = useState(false)
  const [procOpen, setProcOpen] = useState(false)

  // Processes popup: poll the running-operations snapshot while it is open.
  const [operations, setOperations] = useState<Operation[]>([])
  useEffect(() => {
    if (!procOpen) return
    let live = true
    const tick = () => api.getOperations().then((ops) => { if (live) setOperations(ops) }).catch(() => {})
    tick()
    const t = setInterval(tick, 500)
    return () => { live = false; clearInterval(t) }
  }, [procOpen])

  // close popovers on any outside click (inner interactions stopPropagation)
  useEffect(() => {
    if (!dropdownOpen && !procOpen) return
    const close = () => { setDropdownOpen(false); setProcOpen(false) }
    window.addEventListener('click', close)
    return () => window.removeEventListener('click', close)
  }, [dropdownOpen, procOpen])

  // Adopt the app's view mode carried in a panel state (fresh load / WS update),
  // so the pult reflects whatever the GTK panel is showing. Absent in demo mode.
  const syncViewMode = useCallback((state: PanelState) => {
    if (state.view_mode) setViewMode(GTK_TO_WEB[state.view_mode] ?? 'list')
  }, [])

  const reload = useCallback(async () => {
    dispatch({ type: 'loading' })
    try {
      const state = await api.fetchState(side)
      dispatch({ type: 'loaded', state })
      syncViewMode(state)
      setSelected([])
    } catch (e) {
      dispatch({ type: 'error', message: String(e) })
    }
  }, [side, syncViewMode])

  useEffect(() => { reload() }, [reload])

  const applyState = useCallback((state: PanelState) => {
    dispatch({ type: 'loaded', state })
    syncViewMode(state)
    setSelected([])
  }, [syncViewMode])

  // View switch from the toolbar: apply locally, then drive the GTK app over REST.
  // A GTK-mirrored change comes back via applyViewMode instead (no REST echo).
  const changeViewMode = useCallback((m: ViewMode) => {
    setViewMode(m)
    api.setViewMode(side, WEB_TO_GTK[m]).catch(() => {})
  }, [side])

  const handle: PanelHandle = {
    reload,
    applyState,
    applyViewMode: (mode: string) => setViewMode(GTK_TO_WEB[mode] ?? 'list'),
    selectedName: () => cursorName,
    selectedPath: () => data.entries.find((e) => e.name === cursorName)?.path ?? null,
    selectedIsDir: () => data.entries.find((e) => e.name === cursorName)?.is_dir ?? false,
    selectedPaths: () =>
      selected
        .map((n) => data.entries.find((e) => e.name === n)?.path)
        .filter((p): p is string => p !== undefined),
    quickJump: (ch) => {
      if (data.showing_selector || data.entries.length === 0) return false
      const needle = ch.toLowerCase()
      const names = data.entries.map((e) => e.name)
      const start = cursorName ? names.indexOf(cursorName) + 1 : 0
      for (let off = 0; off < names.length; off++) {
        const name = names[(start + off) % names.length]
        if (name.toLowerCase().startsWith(needle)) {
          setSelected([name])
          return true
        }
      }
      return false
    },
    currentPath: () => data.path,
    showingSelector: () => data.showing_selector,
  }
  if (ref) ref(handle)

  // fire-and-forget navigation — WS panel_updated drives applyState
  const fire = (fn: () => void) => { dispatch({ type: 'loading' }); fn() }

  const path = data.path
  const showSel = data.showing_selector

  // A mounted remote source names itself via the root level (label). Without this
  // check a remote path like "/" prefix-matches the local root drive and the tab
  // lies ("System Root" on an FTP mount).
  const remoteMount = !showSel ? data.levels[0]?.label : undefined
  const remoteMountIcon = data.levels[0]?.icon

  // the active local drive = longest local mount path that prefixes the current path
  const localDrives = drives.filter(isLocal)
  const activeDrive = showSel || remoteMount
    ? null
    : localDrives
        .filter((d) => d.path && (path === d.path || path.startsWith(d.path.endsWith('/') ? d.path : d.path + '/')))
        .sort((a, b) => b.path.length - a.path.length)[0] ?? null
  const activeDriveKey = activeDrive?.key
    ?? (remoteMount ? drives.find((d) => d.kind === 'net' && d.name === remoteMount)?.key ?? null : null)

  // dropdown lists the full unified list (favorites filter keeps the active source visible)
  const dropdownDrives = favoritesOnly
    ? drives.filter((d) => d.is_favorite || d.key === activeDriveKey)
    : drives
  const dropdownSections = SECTION_ORDER
    .map((section) => ({ section, items: dropdownDrives.filter((d) => driveSection(d.kind) === section) }))
    .filter((s) => s.items.length > 0)

  return (
    <div className={`panel ${active ? 'active' : ''} ${terminalExpanded ? 'term-expanded' : ''}`} onClick={onActivate}>
      {/* Tabs — one entry per tab; the active one hosts the source dropdown, each tab
          in the strip IS its drive selector (mirrors the GTK app). "+" adds a tab. */}
      <div className="panel-tabs">
        {data.tabs.map((tab) => {
          const isActive = tab.id === data.active_tab
          const canClose = data.tabs.length > 1
          const eject = canClose && (
            <span
              className="tab-eject"
              title={tr("webpult.close_tab")}
              onClick={(e) => { e.stopPropagation(); setDropdownOpen(false); fire(() => api.closeTab(side, tab.id)) }}
            >
              ×
            </span>
          )
          if (!isActive) {
            return (
              <button
                key={tab.id}
                className="panel-tab"
                onClick={(e) => { e.stopPropagation(); fire(() => api.switchTab(side, tab.id)) }}
                title={tab.title}
              >
                <span>{tab.title}</span>
                {eject}
              </button>
            )
          }
          const tabIcon = remoteMount ? remoteMountIcon : activeDrive?.icon
          return (
            <div key={tab.id} className="tab-dropdown-wrapper" onClick={(e) => e.stopPropagation()}>
              <button
                className={`panel-tab active ${dropdownOpen ? 'active-dropdown' : ''}`}
                onClick={(e) => { e.stopPropagation(); setDropdownOpen((o) => !o) }}
              >
                <img src={tabIcon ? driveIcon(tabIcon) : folderIcon} alt="" style={{ width: '14px', height: '14px', objectFit: 'contain' }} />
                <span>{remoteMount ?? activeDrive?.name ?? tab.title}</span>
                <span className="panel-tab-chevron">▼</span>
                {eject}
              </button>
              {dropdownOpen && (
                <div className="dropdown-menu">
                  {dropdownSections.map(({ section, items }, si) => (
                    <div key={section}>
                      {si > 0 && (
                        <div style={{ height: '1px', background: 'var(--border, #2a2d31)', margin: '4px 0' }} />
                      )}
                      {items.map((dd) => (
                        <button
                          key={dd.key}
                          className={`dropdown-item ${dd.key === activeDriveKey ? 'active' : ''}`}
                          disabled={!dd.is_online}
                          onClick={() => { setDropdownOpen(false); if (dd.is_online) fire(() => api.activateSource(side, dd.key)) }}
                          title={dd.subtitle}
                          style={dd.is_online ? undefined : { opacity: 0.45 }}
                        >
                          <img src={driveIcon(dd.icon)} alt="" style={{ width: '16px', height: '16px', objectFit: 'contain' }} />
                          <span>{dd.name}</span>
                          <span
                            className="dropdown-star"
                            title={dd.is_favorite ? 'Remove from favorites' : 'Add to favorites'}
                            onClick={(e) => { e.stopPropagation(); onToggleFavorite(dd.key) }}
                            style={{ marginLeft: 'auto', cursor: 'pointer', display: 'inline-flex', alignItems: 'center' }}
                          >
                            <img src={dd.is_favorite ? starIcon : emptyStarIcon} alt="favorite" style={{ width: '14px', height: '14px' }} />
                          </span>
                        </button>
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )
        })}
        <button
          className="add-tab-btn"
          title={tr("webpult.new_tab")}
          onClick={(e) => { e.stopPropagation(); fire(() => api.addTab(side)) }}
        >
          +
        </button>
      </div>

      {/* Toolbar — two rows, like the GTK app */}
      <div className="panel-toolbar">
        {/* Row 1: navigation + address (breadcrumbs get the whole row) */}
        <div className="toolbar-nav-row">
          <div className="nav-buttons-group">
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); fire(() => api.goUp(side)) }} disabled={showSel || atRoot(data.levels)} title={tr("webpult.up_directory")}>
              <img src={arrowUpIcon} alt={tr("webpult.up")} />
            </button>
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); fire(() => api.goBack(side)) }} title={tr("registry.back_tooltip")}>
              <img src={arrowLeftIcon} alt={tr("registry.back_tooltip")} />
            </button>
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); fire(() => api.goForward(side)) }} title={tr("webpult.forward")}>
              <img src={arrowRightIcon} alt={tr("webpult.forward")} />
            </button>
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); reload() }} title={tr("fm.refresh")}>
              <img src={refreshIcon} alt={tr("fm.refresh")} />
            </button>
          </div>

          {/* Breadcrumbs */}
          <div className="breadcrumbs-container">
            {showSel ? (
              <span style={{ color: 'var(--text-dim)' }}>{tr("selector.title")}</span>
            ) : (
              <Breadcrumbs levels={data.levels} onBreadcrumb={(l) => fire(() => api.breadcrumb(side, l))} onStart={() => fire(() => api.goHome(side))} />
            )}
          </div>
        </div>

        {/* Row 2: file operations (left) · views / processes / terminal (right) */}
        <div className="toolbar-ops-row">
          <div className="ops-buttons">
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); onNewFolder() }} disabled={showSel} title={tr("webpult.new_folder")}>
              <img src={folderIcon} alt="new-folder" style={{ filter: 'hue-rotate(240deg)' }} />
            </button>
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); onRename() }} disabled={showSel || !cursorName} title={tr("fm.context.rename")}>
              <img src={editPencilIcon} alt="rename" />
            </button>
            <button className="nav-btn" onClick={(e) => { e.stopPropagation(); onDelete() }} disabled={showSel || selected.length === 0} title={tr("common_forms.delete")}>
              <img src={deleteFolderIcon} alt="delete" />
            </button>
          </div>

          <div className="ops-right">
            {/* Process manager */}
            <div className="proc-wrapper" onClick={(e) => e.stopPropagation()}>
              <button className={`nav-btn ${procOpen ? 'active' : ''}`} onClick={() => setProcOpen((o) => !o)} title={tr("selector.btn_processes")}>
                <img src={processesIcon} alt="processes" />
              </button>
              {procOpen && (
                <div className="dropdown-menu proc-popover">
                  {operations.length === 0 ? (
                    <div className="proc-empty">{tr("webpult.no_active_operations")}</div>
                  ) : (
                    operations.map((op) => (
                      <div key={op.id} className="proc-op">
                        <div className="proc-op-head">
                          <span className="proc-op-kind">{op.kind === 'move' ? 'Moving' : 'Copying'}</span>
                          <span className="proc-op-file" title={op.current_file}>{op.current_file || '…'}</span>
                          <button
                            className="proc-op-cancel"
                            title={tr("webpult.cancel_this_operation")}
                            onClick={() => api.cancelOperation(op.id).catch(() => {})}
                          >
                            ✕
                          </button>
                        </div>
                        <div className="proc-op-bar">
                          <div
                            className="proc-op-bar-fill"
                            style={{
                              width: op.total_bytes > 0
                                ? `${Math.min(100, (op.done_bytes / op.total_bytes) * 100)}%`
                                : op.total_files > 0
                                  ? `${Math.min(100, (op.done_files / op.total_files) * 100)}%`
                                  : '100%',
                            }}
                          />
                        </div>
                        <div className="proc-op-stats">
                          <span>
                            {op.total_bytes > 0
                              ? `${formatSize(op.done_bytes)} / ${formatSize(op.total_bytes)}`
                              : `${op.done_files} / ${op.total_files} files`}
                          </span>
                          {op.speed_bps > 0 && <span>{formatSize(op.speed_bps)}/s</span>}
                        </div>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>

            {/* Terminal open/close + expand (when open) */}
            {terminalOpen && (
              <button className="nav-btn" onClick={(e) => { e.stopPropagation(); onToggleTerminalExpand() }} title={terminalExpanded ? 'Collapse terminal' : 'Expand terminal across both panels'}>
                <img src={terminalExpanded ? collapseIcon : expandIcon} alt="expand" />
              </button>
            )}
            <button className={`nav-btn ${terminalOpen ? 'active' : ''}`} onClick={(e) => { e.stopPropagation(); onCloseTerminal() }} title={tr("webpult.terminal_f9")}>
              <img src={terminalIcon} alt="terminal" />
            </button>

            <span className="tb-sep" />

            {/* View switches — far right, like the GTK toolbar */}
            <div className="view-buttons-group">
              <button className={`view-btn ${viewMode === 'list' ? 'active' : ''}`} onClick={(e) => { e.stopPropagation(); changeViewMode('list') }} title={tr("fm.list_view")}>
                <img src={listIconsIcon} alt="list" style={{ filter: 'opacity(0.8)' }} />
              </button>
              <button className={`view-btn ${viewMode === 'medium' ? 'active' : ''}`} onClick={(e) => { e.stopPropagation(); changeViewMode('medium') }} title={tr("webpult.grid_icons")}>
                <img src={mediumIconsIcon} alt="grid" style={{ filter: 'opacity(0.8)' }} />
              </button>
              <button className={`view-btn ${viewMode === 'tiles' ? 'active' : ''}`} onClick={(e) => { e.stopPropagation(); changeViewMode('tiles') }} title={tr("webpult.tiles_view")}>
                <img src={smallIconsIcon} alt="tiles" style={{ filter: 'opacity(0.8)' }} />
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* View content area (drop target: dropping OS files uploads them here) */}
      <div
        className={`panel-view-content ${dropActive ? 'drop-target' : ''}`}
        onDragOver={(e) => {
          if (showSel || !e.dataTransfer.types.includes('Files')) return
          e.preventDefault()
          e.dataTransfer.dropEffect = 'copy'
          setDropActive(true)
        }}
        onDragLeave={(e) => {
          if (e.currentTarget.contains(e.relatedTarget as Node)) return
          setDropActive(false)
        }}
        onDrop={async (e) => {
          setDropActive(false)
          if (showSel || e.dataTransfer.files.length === 0) return
          e.preventDefault()
          const files = [...e.dataTransfer.files]
          dispatch({ type: 'loading' })
          for (const f of files) {
            const target = path === '/' ? `/${f.name}` : `${path}/${f.name}`
            try {
              await api.uploadFile(side, target, f)
            } catch (err) {
              alert(`Upload of ${f.name} failed: ${err}`)
              break
            }
          }
          reload()
        }}
      >
        {showSel ? (
          <SelectSource
            drives={drives}
            connections={connections}
            favoritesOnly={favoritesOnly}
            onActivate={(key) => fire(() => api.activateSource(side, key))}
            onToggleFavorite={onToggleFavorite}
            onSetFavoritesOnly={onSetFavoritesOnly}
            onSaveConnection={onSaveConnection}
            onDeleteConnection={onDeleteConnection}
          />
        ) : (
          <FileList
            entries={data.entries}
            viewMode={viewMode}
            selectedNames={selected}
            showUp={!atRoot(data.levels)}
            onSelect={(name, mods) => {
              if (mods.shift && selected.length > 0) {
                const names = data.entries.map((e) => e.name)
                const a = names.indexOf(selected[selected.length - 1])
                const b = names.indexOf(name)
                if (a !== -1 && b !== -1) {
                  const [lo, hi] = a < b ? [a, b] : [b, a]
                  setSelected(names.slice(lo, hi + 1))
                  return
                }
              }
              if (mods.ctrl) {
                setSelected((cur) => (cur.includes(name) ? cur.filter((n) => n !== name) : [...cur, name]))
              } else {
                setSelected((cur) => (cur.length === 1 && cur[0] === name ? [] : [name]))
              }
            }}
            onOpenDir={(name) => fire(() => api.enter(side, name))}
            onOpenFile={(entry) => onOpenFile(entry.path)}
            onUp={() => fire(() => api.goUp(side))}
          />
        )}
      </div>

      {/* Status info bar */}
      <div className="panel-footer-info">
        {data.error ? (
          <span style={{ color: 'var(--accent-red, #e06c75)' }}>{data.error}</span>
        ) : data.loading ? (
          <span>Loading…</span>
        ) : showSel ? (
          <span>Sources available: {drives.length}</span>
        ) : (
          <span>
            {selected.length > 1
              ? `Selected: ${selected.length} items`
              : cursorName
                ? `Selected: ${cursorName}`
                : `${data.entries.length} item(s)`}
          </span>
        )}
      </div>

      {/* Per-panel terminal (xterm over WS → the app's PTY) */}
      {terminalOpen && (
        <Terminal
          side={side}
          expanded={terminalExpanded}
          onToggleExpand={onToggleTerminalExpand}
          onClose={onCloseTerminal}
        />
      )}
    </div>
  )
}
