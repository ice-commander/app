export type Side = 'left' | 'right'

export interface FileEntry {
  name: string
  path: string
  is_dir: boolean
  size: number | null
  modified: string | null
}

/** One breadcrumb level (flat mirror of the backend's NavPath). Index 0 is the
    provider root; each following level is a path segment. */
export interface Level {
  name: string
  /** This level crosses into an archive → render the ⊞ chip. Never set on the root. */
  is_archive: boolean
  /** Connection name of a mounted remote source at the root (e.g. "myproject.com");
      absent for local roots and archives. */
  label?: string
  /** Icon basename for the labelled source (same names as Drive.icon). */
  icon?: string
}

/** One tab of a side. The strip renders these; `levels`/`entries` on PanelState
    describe whichever tab is `active_tab`. Addressed by stable `id`, never index. */
export interface Tab {
  id: number
  title: string
  icon?: string
}

export interface PanelState {
  levels: Level[]
  /** Absolute path of the current directory within its provider. */
  path: string
  entries: FileEntry[]
  showing_selector: boolean
  /** GTK panel view mode: "list" | "small" | "large". Absent in the demo backend. */
  view_mode?: string
  /** This side's open tabs. Empty/absent → the UI shows a single implicit tab. */
  tabs?: Tab[]
  /** Id of the tab the rest of this state describes. */
  active_tab?: number
}

export interface FileContent {
  path: string
  content: string
  is_binary: boolean
}

/** One running transfer (the Processes popup). */
export interface Operation {
  id: number
  kind: 'copy' | 'move' | string
  current_file: string
  done_bytes: number
  /** 0 = total unknown (REST transfers aren't pre-scanned) — show file counts. */
  total_bytes: number
  done_files: number
  total_files: number
  speed_bps: number
}

export type DriveKind = 'root' | 'home' | 'drive' | 'volume' | 'net'

export interface Drive {
  name: string
  /** Stable activation/favorite key, matching the GTK app (pass to activateSource). */
  key: string
  /** Mount path for local sources (used for tab highlighting); empty for net. */
  path: string
  /** Icon basename, e.g. "ssd.svg". */
  icon: string
  kind: DriveKind
  subtitle: string
  is_favorite: boolean
  is_online: boolean
}

export interface Connection {
  name: string
  protocol: string
  host: string
  port: number
  user: string
  pass?: string
  auth_type?: string
  key_path?: string
  passphrase?: string
  remote_path?: string
  use_tunnel?: boolean
  tunnel_host?: string
  tunnel_port?: number
  tunnel_user?: string
  tunnel_auth_type?: string
  tunnel_pass?: string
  tunnel_key_path?: string
  tunnel_passphrase?: string
}
