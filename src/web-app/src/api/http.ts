import type { Backend, BackendEvents, TerminalCallbacks, TerminalSession } from './backend'
import type { Connection, Drive, FileContent, Operation, PanelState, Side } from './types'

/**
 * The real backend: REST + WebSocket to an ice-commander instance running with
 * `--webui`. This is the only file (besides the demo twin) that knows the
 * transport — URLs, fetch and WebSocket never leak past the Backend interface.
 */
export class HttpBackend implements Backend {
  private async json<T>(url: string, init?: RequestInit): Promise<T> {
    const res = await fetch(url, init)
    if (!res.ok) {
      // Carry the server's own message: handlers answer `{ "error": "..." }`, and
      // callers branch on it (an encrypted import replies `needs-password`). A bare
      // status code told them nothing.
      let detail = ''
      try {
        const body = await res.text()
        const parsed = JSON.parse(body) as { error?: string }
        detail = parsed.error ?? body
      } catch {
        /* not JSON — the status line is all we have */
      }
      throw new Error(detail !== '' ? detail : `${res.status} ${res.statusText}`)
    }
    return res.json()
  }

  private post(url: string, body?: unknown): Promise<unknown> {
    return this.json(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  }

  // ── panel state & navigation ────────────────────────────────────────────────
  fetchState(side: Side): Promise<PanelState> {
    return this.json(`/api/panel/${side}/state`)
  }
  async enter(side: Side, name: string): Promise<void> {
    await this.post(`/api/panel/${side}/enter`, { name })
  }
  async goUp(side: Side): Promise<void> {
    await this.post(`/api/panel/${side}/up`)
  }
  async goBack(side: Side): Promise<void> {
    await this.post(`/api/panel/${side}/back`)
  }
  async goForward(side: Side): Promise<void> {
    await this.post(`/api/panel/${side}/forward`)
  }
  async breadcrumb(side: Side, level: number): Promise<void> {
    await this.post(`/api/panel/${side}/breadcrumb`, { level })
  }
  async goHome(side: Side): Promise<void> {
    await this.post(`/api/panel/${side}/home`)
  }

  // ── tabs ──────────────────────────────────────────────────────────────────────
  async addTab(side: Side): Promise<void> {
    await this.post(`/api/panel/${side}/tabs/add`)
  }
  async closeTab(side: Side, id: number): Promise<void> {
    await this.post(`/api/panel/${side}/tabs/${id}/close`)
  }
  async switchTab(side: Side, id: number): Promise<void> {
    await this.post(`/api/panel/${side}/tabs/${id}/activate`)
  }

  // ── sources ─────────────────────────────────────────────────────────────────
  getDrives(): Promise<Drive[]> {
    return this.json('/api/drives')
  }
  async activateSource(side: Side, key: string): Promise<void> {
    await this.post(`/api/panel/${side}/activate`, { key })
  }
  getConnections(): Promise<Connection[]> {
    return this.json('/api/connections')
  }
  async saveConnection(connection: Connection): Promise<void> {
    await this.post('/api/connections', connection)
  }
  async deleteConnection(name: string): Promise<void> {
    await this.json(`/api/connections/${encodeURIComponent(name)}`, { method: 'DELETE' })
  }
  async setConnectionsDialog(open: boolean): Promise<void> {
    await this.post('/api/connections/dialog', { open })
  }
  async exportConnections(password?: string): Promise<string> {
    const r = await this.post('/api/connections/export', { password })
    return (r as { data: string }).data
  }
  async importConnections(data: string, password?: string): Promise<number> {
    const r = await this.post('/api/connections/import', { data, password })
    return (r as { imported: number }).imported
  }
  async connectTo(side: Side, connection: Connection): Promise<void> {
    await this.post('/api/connect', { side, connection })
  }

  // ── favorites ───────────────────────────────────────────────────────────────
  async toggleFavorite(key: string): Promise<void> {
    await this.post('/api/favorites/toggle', { path: key })
  }
  async getFavoritesOnly(): Promise<boolean> {
    const r = await this.json<{ value: boolean }>('/api/favorites/only')
    return r.value
  }
  async setFavoritesOnly(value: boolean): Promise<void> {
    await this.post('/api/favorites/only', { value })
  }

  // ── files ───────────────────────────────────────────────────────────────────
  readFile(side: Side, path: string): Promise<FileContent> {
    return this.json(`/api/panel/${side}/file?path=${encodeURIComponent(path)}`)
  }
  async writeFile(side: Side, path: string, content: string): Promise<void> {
    await this.json(`/api/panel/${side}/file`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, content }),
    })
  }
  async getFileUrl(side: Side, path: string): Promise<string> {
    return `/api/panel/${side}/file/stream?path=${encodeURIComponent(path)}`
  }
  async uploadFile(side: Side, path: string, data: Blob): Promise<void> {
    await this.json(`/api/panel/${side}/upload?path=${encodeURIComponent(path)}`, {
      method: 'POST',
      body: data,
    })
  }
  async deleteEntries(side: Side, paths: string[]): Promise<void> {
    await this.post(`/api/panel/${side}/delete`, { paths })
  }
  async mkdir(side: Side, name: string): Promise<void> {
    await this.post(`/api/panel/${side}/mkdir`, { name })
  }
  async openNative(side: Side, path: string): Promise<void> {
    await this.post(`/api/panel/${side}/open-native`, { path })
  }
  async closeViewerWindows(): Promise<void> {
    await this.post('/api/windows/close-extra')
  }
  async renameEntry(side: Side, oldPath: string, newName: string): Promise<void> {
    await this.post(`/api/panel/${side}/rename`, { old_path: oldPath, new_name: newName })
  }
  async copyEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void> {
    await this.post(`/api/panel/${dstSide}/copy`, { src_side: srcSide, paths })
  }
  async moveEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void> {
    await this.post(`/api/panel/${dstSide}/move`, { src_side: srcSide, paths })
  }

  // ── operations ──────────────────────────────────────────────────────────────
  getOperations(): Promise<Operation[]> {
    return this.json('/api/operations')
  }
  async cancelOperation(id: number): Promise<void> {
    await this.post(`/api/operations/${id}/cancel`)
  }

  // ── terminal ────────────────────────────────────────────────────────────────
  openTerminal(side: Side, callbacks: TerminalCallbacks): TerminalSession {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const ws = new WebSocket(`${proto}//${location.host}/api/panel/${side}/terminal/ws`)
    ws.binaryType = 'arraybuffer'
    // xterm fits on mount — before the socket finishes connecting — so that first resize
    // would be lost. Buffer the latest pre-open resize and flush it on `open`, so the PTY
    // gets the real dimensions the instant the shell is reachable.
    let pendingResize: string | null = null
    ws.onopen = () => { if (pendingResize !== null) { ws.send(pendingResize); pendingResize = null } }
    ws.onmessage = (e) => callbacks.onData(new Uint8Array(e.data as ArrayBuffer))
    ws.onclose = () => callbacks.onClose?.()
    return {
      // Keystrokes go as binary; the server routes binary → PTY input and reserves text
      // frames for control messages (resize), so the two never collide.
      send: (data) => { if (ws.readyState === WebSocket.OPEN) ws.send(new TextEncoder().encode(data)) },
      resize: (cols, rows) => {
        const msg = JSON.stringify({ resize: { cols, rows } })
        if (ws.readyState === WebSocket.OPEN) ws.send(msg)
        else pendingResize = msg
      },
      close: () => ws.close(),
    }
  }
  async setTerminalExpanded(side: Side, expanded: boolean): Promise<void> {
    await this.post(`/api/panel/${side}/terminal/expand`, { expanded })
  }
  async setViewMode(side: Side, mode: string): Promise<void> {
    await this.post(`/api/panel/${side}/view-mode`, { mode })
  }

  // ── events ──────────────────────────────────────────────────────────────────
  subscribe(events: BackendEvents): () => void {
    let closed = false
    let ws: WebSocket | null = null

    const connect = () => {
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      ws = new WebSocket(`${proto}//${location.host}/api/ws`)
      ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data)
          if (msg.event === 'panel_updated') events.onPanelUpdated?.(msg.side, msg.state)
          else if (msg.event === 'terminal_state') events.onTerminalState?.(msg.side, msg.open)
          else if (msg.event === 'terminal_expanded') events.onTerminalExpanded?.(msg.side, msg.expanded)
          else if (msg.event === 'open_viewer') events.onOpenViewer?.(msg.side, msg.path, msg.mode)
          else if (msg.event === 'close_viewer') events.onCloseViewer?.(msg.side)
          else if (msg.event === 'open_connections') events.onConnectionsDialog?.(true)
          else if (msg.event === 'close_connections') events.onConnectionsDialog?.(false)
          else if (msg.event === 'view_mode') events.onViewMode?.(msg.side, msg.mode)
        } catch { /* ignore malformed */ }
      }
      ws.onclose = () => {
        if (!closed) setTimeout(connect, 2000)
      }
    }
    connect()

    return () => {
      closed = true
      ws?.close()
    }
  }
}
