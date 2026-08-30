import type { Connection, Drive, FileContent, Operation, PanelState, Side } from './types'

/**
 * The single seam between the web UI and whatever fulfills it.
 *
 * Components never talk to a transport: no fetch, no WebSocket, no URLs. They
 * call command methods (promises) and subscribe to pushed events (callbacks).
 * How commands travel and where events come from is an implementation detail:
 *
 * `HttpBackend` (api/http.ts) is the implementation: REST requests and WebSocket
 * streams to an ice-commander instance running with `--webui`.
 *
 * Keep this interface in lockstep with the app's REST/WS contract (see
 * src/gtk-app/src/api.rs); it is the frontend mirror of that contract.
 */

/** Server-pushed events. Register via [`Backend.subscribe`]; unused ones may be omitted. */
export interface BackendEvents {
  /** A panel changed (navigation, mutation, or a mirrored GTK action) — apply the new state. */
  onPanelUpdated?(side: Side, state: PanelState): void
  /** The GTK side opened/closed a panel terminal — mirror it. */
  onTerminalState?(side: Side, open: boolean): void
  /** A panel terminal was expanded across both panels (or collapsed back). */
  onTerminalExpanded?(side: Side, expanded: boolean): void
  /** The app asks the web UI to open its file viewer (remote F3/F4). */
  onOpenViewer?(side: Side, path: string, mode: 'view' | 'edit'): void
  /** A native viewer window closed in the GTK app — close the web overlay to match. */
  onCloseViewer?(side: Side): void
  /** The app opened or closed its own connections dialog — mirror the web overlay. */
  onConnectionsDialog?(open: boolean): void
  /** The GTK side switched a panel's file view ("list" | "small" | "large"). */
  onViewMode?(side: Side, mode: string): void
}

/** Callbacks for one live terminal. `onData` receives raw PTY output (VT100 bytes for xterm). */
export interface TerminalCallbacks {
  onData(data: Uint8Array): void
  /** The session ended on the far side (socket closed, shell exited). */
  onClose?(): void
}

/** A live terminal session. Keystrokes go in via `send`; output arrives on the callbacks. */
export interface TerminalSession {
  send(data: string): void
  /** Tell the far-side PTY the xterm dimensions, so full-screen TUIs (mc/vim) fill the view. */
  resize(cols: number, rows: number): void
  close(): void
}

export interface Backend {
  // ── panel state & navigation ────────────────────────────────────────────────
  fetchState(side: Side): Promise<PanelState>
  enter(side: Side, name: string): Promise<void>
  goUp(side: Side): Promise<void>
  goBack(side: Side): Promise<void>
  goForward(side: Side): Promise<void>
  /** Jump to a breadcrumb by its flat level index (0 = provider root). */
  breadcrumb(side: Side, level: number): Promise<void>
  /** Show the Select Source page in this panel. */
  goHome(side: Side): Promise<void>

  // ── tabs ──────────────────────────────────────────────────────────────────────
  /** Open a new tab (focused) on this side. */
  addTab(side: Side): Promise<void>
  /** Close the tab with `id` (no-op on the last tab). */
  closeTab(side: Side, id: number): Promise<void>
  /** Make the tab with `id` the active one. */
  switchTab(side: Side, id: number): Promise<void>

  // ── sources ─────────────────────────────────────────────────────────────────
  getDrives(): Promise<Drive[]>
  /** Open a source (drive / connection) by its unified key in the given panel. */
  activateSource(side: Side, key: string): Promise<void>
  getConnections(): Promise<Connection[]>
  /** Persist a connection (upsert by name) — it appears in getDrives() as kind 'net'
   *  and mounts via activateSource. Does not mount by itself. */
  saveConnection(connection: Connection): Promise<void>
  /** Remove a saved connection by name. */
  deleteConnection(name: string): Promise<void>
  setConnectionsDialog(open: boolean): Promise<void>
  exportConnections(password?: string): Promise<string>
  importConnections(data: string, password?: string): Promise<number>
  connectTo(side: Side, connection: Connection): Promise<void>

  // ── favorites ───────────────────────────────────────────────────────────────
  /** `key` is the unified drive key (e.g. "local_fs:/"). */
  toggleFavorite(key: string): Promise<void>
  getFavoritesOnly(): Promise<boolean>
  setFavoritesOnly(value: boolean): Promise<void>

  // ── files ───────────────────────────────────────────────────────────────────
  readFile(side: Side, path: string): Promise<FileContent>
  writeFile(side: Side, path: string, content: string): Promise<void>
  /** URL suitable for <img src>/<video src> — how the bytes arrive is the
   *  backend's business (REST stream endpoint, data URI, blob…). */
  getFileUrl(side: Side, path: string): Promise<string>
  /** Also open the file in the app's native GTK viewer window (mirror a web F3/F4). */
  openNative(side: Side, path: string): Promise<void>
  /** Close the app's native viewer windows (mirror the web overlay closing). */
  closeViewerWindows(): Promise<void>
  /** Raw-bytes upload (drag-and-drop from the OS). `path` is the full target
   *  file path in the panel's provider — works on remote mounts too. */
  uploadFile(side: Side, path: string, data: Blob): Promise<void>
  deleteEntries(side: Side, paths: string[]): Promise<void>
  mkdir(side: Side, name: string): Promise<void>
  renameEntry(side: Side, oldPath: string, newName: string): Promise<void>
  copyEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void>
  moveEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void>

  // ── operations (Processes popup) ────────────────────────────────────────────
  /** Snapshot of running transfers; the popup polls while open. */
  getOperations(): Promise<Operation[]>
  /** Abort a running transfer — same effect as its Cancel button. */
  cancelOperation(id: number): Promise<void>

  // ── terminal ────────────────────────────────────────────────────────────────
  /** Open a terminal for a panel. The transport (WS, fake shell, …) is hidden here. */
  openTerminal(side: Side, callbacks: TerminalCallbacks): TerminalSession
  /** Expand the panel's terminal across both panels (or collapse). Echoes back as an event. */
  setTerminalExpanded(side: Side, expanded: boolean): Promise<void>
  /** Switch the panel's file view ("list" | "small" | "large"). Echoes back as an event. */
  setViewMode(side: Side, mode: string): Promise<void>

  // ── events ──────────────────────────────────────────────────────────────────
  /** Start receiving pushed events. Returns an unsubscribe function. */
  subscribe(events: BackendEvents): () => void
}
