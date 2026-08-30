import type { Backend, TerminalCallbacks, TerminalSession } from './backend'
import type { Connection, Drive, FileContent, Operation, PanelState, Side } from './types'
import { HttpBackend } from './http'

/**
 * Facade over the active [`Backend`]. Components import this module and never
 * see a transport — swap the backend and the whole UI follows.
 */
export const backend: Backend = new HttpBackend()

// ── panel state & navigation ──────────────────────────────────────────────────
export const fetchState = (side: Side): Promise<PanelState> => backend.fetchState(side)
export const enter = (side: Side, name: string): Promise<void> => backend.enter(side, name)
export const goUp = (side: Side): Promise<void> => backend.goUp(side)
export const goBack = (side: Side): Promise<void> => backend.goBack(side)
export const goForward = (side: Side): Promise<void> => backend.goForward(side)
export const breadcrumb = (side: Side, level: number): Promise<void> =>
  backend.breadcrumb(side, level)
export const goHome = (side: Side): Promise<void> => backend.goHome(side)

// ── tabs ────────────────────────────────────────────────────────────────────────
export const addTab = (side: Side): Promise<void> => backend.addTab(side)
export const closeTab = (side: Side, id: number): Promise<void> => backend.closeTab(side, id)
export const switchTab = (side: Side, id: number): Promise<void> => backend.switchTab(side, id)

// ── sources ───────────────────────────────────────────────────────────────────
export const getDrives = (): Promise<Drive[]> => backend.getDrives()
export const activateSource = (side: Side, key: string): Promise<void> => backend.activateSource(side, key)
export const getConnections = (): Promise<Connection[]> => backend.getConnections()
export const saveConnection = (connection: Connection): Promise<void> => backend.saveConnection(connection)
export const deleteConnection = (name: string): Promise<void> => backend.deleteConnection(name)
export const setConnectionsDialog = (open: boolean): Promise<void> => backend.setConnectionsDialog(open)
export const exportConnections = (password?: string): Promise<string> => backend.exportConnections(password)
export const importConnections = (data: string, password?: string): Promise<number> =>
  backend.importConnections(data, password)
export const connectTo = (side: Side, connection: Connection): Promise<void> => backend.connectTo(side, connection)

// ── favorites ─────────────────────────────────────────────────────────────────
export const toggleFavorite = (key: string): Promise<void> => backend.toggleFavorite(key)
export const getFavoritesOnly = (): Promise<boolean> => backend.getFavoritesOnly()
export const setFavoritesOnly = (value: boolean): Promise<void> => backend.setFavoritesOnly(value)

// ── files ─────────────────────────────────────────────────────────────────────
export const readFile = (side: Side, path: string): Promise<FileContent> => backend.readFile(side, path)
export const writeFile = (side: Side, path: string, content: string): Promise<void> =>
  backend.writeFile(side, path, content)
export const getFileUrl = (side: Side, path: string): Promise<string> => backend.getFileUrl(side, path)
export const openNative = (side: Side, path: string): Promise<void> => backend.openNative(side, path)
export const closeViewerWindows = (): Promise<void> => backend.closeViewerWindows()
export const uploadFile = (side: Side, path: string, data: Blob): Promise<void> =>
  backend.uploadFile(side, path, data)
export const deleteEntries = (side: Side, paths: string[]): Promise<void> => backend.deleteEntries(side, paths)
export const mkdir = (side: Side, name: string): Promise<void> => backend.mkdir(side, name)
export const renameEntry = (side: Side, oldPath: string, newName: string): Promise<void> =>
  backend.renameEntry(side, oldPath, newName)
export const copyEntries = (srcSide: Side, dstSide: Side, paths: string[]): Promise<void> =>
  backend.copyEntries(srcSide, dstSide, paths)
export const moveEntries = (srcSide: Side, dstSide: Side, paths: string[]): Promise<void> =>
  backend.moveEntries(srcSide, dstSide, paths)

// ── operations ────────────────────────────────────────────────────────────────
export const getOperations = (): Promise<Operation[]> => backend.getOperations()
export const cancelOperation = (id: number): Promise<void> => backend.cancelOperation(id)

// ── terminal ──────────────────────────────────────────────────────────────────
export const openTerminal = (side: Side, callbacks: TerminalCallbacks): TerminalSession =>
  backend.openTerminal(side, callbacks)
export const setTerminalExpanded = (side: Side, expanded: boolean): Promise<void> =>
  backend.setTerminalExpanded(side, expanded)
export const setViewMode = (side: Side, mode: string): Promise<void> =>
  backend.setViewMode(side, mode)

// ── events ────────────────────────────────────────────────────────────────────
/** Subscribe to pushed events; returns an unsubscribe function. */
export function connectWebSocket(
  onPanelUpdated: (side: Side, state: PanelState) => void,
  onTerminalState?: (side: Side, open: boolean) => void,
  onTerminalExpanded?: (side: Side, expanded: boolean) => void,
  onOpenViewer?: (side: Side, path: string, mode: 'view' | 'edit') => void,
  onViewMode?: (side: Side, mode: string) => void,
  onCloseViewer?: (side: Side) => void,
  // NOTE: these are POSITIONAL — the order here must match the call in App.tsx. Two
  // callbacks with compatible shapes have already been swapped by accident once.
  onConnectionsDialog?: (open: boolean) => void,
): () => void {
  return backend.subscribe({ onPanelUpdated, onTerminalState, onTerminalExpanded, onOpenViewer, onViewMode, onCloseViewer, onConnectionsDialog })
}
