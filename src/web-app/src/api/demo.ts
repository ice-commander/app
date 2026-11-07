import type { Backend, BackendEvents, TerminalCallbacks, TerminalSession } from './backend'
import type { Connection, Drive, FileContent, Level, Operation, PanelState, Side } from './types'
import type {
  ConnectionKinds,
  Document,
  Field,
  Json,
  State,
  ViewerAnswer,
  ViewerClosed,
  ViewerOpened,
  ViewSnapshot,
} from '../lib/view/types'
import { asText, evaluate, isTruthy, resolve } from '../lib/view/expr'
import ftpDocument from '../assets/demo-plugins/ftp.json'
import sftpDocument from '../assets/demo-plugins/sftp.json'
import webdavDocument from '../assets/demo-plugins/webdav.json'
import ftpIcon from '../assets/demo-plugins/ic-ftp-fs/ftp.svg?inline'
import sftpIcon from '../assets/demo-plugins/ic-sftp-fs/sftp.svg?inline'
import webdavIcon from '../assets/demo-plugins/ic-webdav-fs/webdav.svg?inline'

interface DemoNode {
  name: string
  is_dir: boolean
  size: number
  modified: string
  children?: DemoNode[]
  content?: string
  archive?: boolean
  media?: string
}

const icePhoto = 'ice-photo.jpg'
const introVideo = 'intro.mp4'

let mediaBase = ''

export function setDemoMediaBase(base: string): void {
  mediaBase = base === '' ? '' : new URL(base, document.baseURI).href
}

const now = (): string => {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

const dir = (name: string, children: DemoNode[] = [], modified = '2026-06-28 14:12'): DemoNode => ({
  name,
  is_dir: true,
  size: 0,
  modified,
  children,
})

const text = (name: string, content: string, modified = '2026-06-30 09:41'): DemoNode => ({
  name,
  is_dir: false,
  size: content.length,
  modified,
  content,
})

const blob = (name: string, size: number, modified = '2026-05-17 18:03'): DemoNode => ({
  name,
  is_dir: false,
  size,
  modified,
})

const archive = (name: string, size: number, children: DemoNode[]): DemoNode => ({
  name,
  is_dir: false,
  size,
  modified: '2026-06-25 21:30',
  children,
  archive: true,
})

const media = (name: string, size: number, file: string, modified = '2026-06-29 12:05'): DemoNode => ({
  name,
  is_dir: false,
  size,
  modified,
  media: file,
})

const HOME_README = `# Ice Commander — live demo

Everything in this window is a sandbox running in your browser.
Nothing is uploaded anywhere, and it all resets on reload.
Every F-key is also a button on the bar at the bottom.

Things to try:

  * Select ice-photo.jpg and press F3 — the viewer shows it.
    intro.mp4 plays there too.
  * Double-click photos.tar.gz — archives browse like folders.
  * Click the flag left of the path in the other panel and open
    the USB stick. Then select a file here and press F5 to copy
    it there, or F6 to move it.
  * Press F9 — a terminal opens (try: ls, cd, cat, help).
  * The same flag leads to the demo FTP server, and
    "＋ Add connection" saves an FTP, SFTP or WebDAV connection.
    Nothing goes over the network: connecting opens a folder
    that lives in your browser.
  * F4 opens this file in the editor. Go ahead, edit it.

The desktop application does the same with your own disks on
Linux, macOS and Windows; FTP, SFTP, WebDAV and archives come
to it as plugins.
`

const MAIN_RS = `use adw::prelude::*;

mod api;
mod panels;

fn main() {
    let app = adw::Application::builder()
        .application_id("com.icecommander.gtkapp")
        .build();

    app.connect_activate(|app| {
        let window = panels::build_main_window(app);
        window.present();
    });

    app.run();
}
`

const API_RS = `//! REST + WebSocket contract — the same one this demo mocks in your browser.
//!
//! Commands arrive over REST, state flows back as 'panel_updated' pushes:
//!
//!   POST /api/panel/{side}/enter      { name }
//!   POST /api/panel/{dst_side}/copy   { src_side, paths }
//!   GET  /api/panel/{side}/file/stream?path=…   ← streams the media you
//!                                                  are viewing right now
//!
//! The desktop app serves it, the web UI speaks it, and this sandbox mocks it.

pub enum ApiCmd {
    Enter { side: PanelSide, name: String },
    Copy { src: PanelSide, dst: PanelSide, paths: Vec<String> },
    StreamFile { side: PanelSide, path: String },
}
`

const PANELS_RS = `use gtk::prelude::*;

/// Two panels, one focus — the commander way.
pub struct DualPane {
    pub left: Panel,
    pub right: Panel,
    pub active: Side,
}

impl DualPane {
    pub fn swap(&mut self) {
        self.active = match self.active {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
    }
}
`

const LICENSE = `MIT License

Copyright (c) 2026 Ice Commander Project

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND.
`

const CONNECTION_README = `This connection lives only in the sandbox: nothing here went over the network.
Files you copy here with F5 stay in your browser until you reload the page.
`

function localTree(): DemoNode {
  return dir('/', [
    dir('bin', [blob('bash', 1183224), blob('ls', 142136)]),
    dir('etc', [text('hostname', 'ice-demo\n'), text('hosts', '127.0.0.1 localhost\n::1       localhost\n')]),
    dir('home', [
      dir('demo', [
        dir('Documents', [
          text(
            'notes.md',
            '# Notes\n\n- ship the landing page\n- record a demo GIF\n- release 0.9\n\n## Ideas\n\n- per-file conflict prompts: done!\n- media streaming over F3: you are looking at it\n',
          ),
          text(
            'meeting.txt',
            'Standup 2026-07-02\n\n- web demo embedded on the site\n- F3 opens photos and videos now\n- the Processes popup shows running copies\n',
          ),
          blob('report.pdf', 320113),
          blob('invoice.docx', 48770),
        ]),
        dir('Pictures', [
          dir('iceland-2025', [blob('DSC_0117.nef', 24312411), blob('DSC_0118.nef', 23998650)]),
          media('screenshot.png', 2411002, icePhoto),
        ]),
        dir('Music', [blob('mix.mp3', 8412930)]),
        dir('projects', [
          dir('ice-commander', [
            dir('src', [text('main.rs', MAIN_RS), text('api.rs', API_RS), text('panels.rs', PANELS_RS)]),
            text(
              'Cargo.toml',
              '[package]\nname = "ice-commander"\nversion = "0.8.144"\nedition = "2021"\n\n[dependencies]\ngtk = { package = "gtk4", version = "0.9" }\nadw = { package = "libadwaita", version = "0.7" }\ntokio = { version = "1", features = ["full"] }\n',
            ),
            text(
              'README.md',
              '# ice-commander\n\nDual-pane file manager.\n\nEverything you can open with F3 here is editable with F4 —\nthis whole project tree lives in your browser memory.\n',
            ),
            text('LICENSE', LICENSE),
            text('.gitignore', 'target/\nnode_modules/\ndist/\n*.log\n'),
          ]),
        ]),
        archive('photos.tar.gz', 1403211776, [
          dir('2025', [
            dir('iceland', [blob('DSC_0117.NEF', 24312411), blob('DSC_0118.NEF', 23998650)]),
            dir('kyiv', [media('IMG_2201.jpg', 3211776, icePhoto), media('IMG_2202.jpg', 2998431, icePhoto)]),
          ]),
        ]),
        archive('backup.zip', 44040192, [
          dir('config', [text('app.toml', '[ui]\ntheme = "dark"\nlanguage = "en"\n')]),
          blob('data.db', 41943040),
        ]),
        media('ice-photo.jpg', 13850, icePhoto),
        media('intro.mp4', 425852, introVideo),
        text('README.md', HOME_README),
        text('todo.txt', 'try the FTP source\nenter an archive\nopen the terminal (F9)\n'),
      ]),
    ]),
    dir('run', [dir('media', [dir('demo', [usbTree()])])]),
    dir('usr', [dir('share', [dir('icons', [])])]),
  ])
}

function usbTree(): DemoNode {
  return dir('USB', [
    dir('movies', [media('demo.mp4', 425852, introVideo)]),
    blob('firmware.bin', 2097152),
    text('README.txt', 'A removable USB stick. Copy things here with F5.\n'),
  ])
}

function ftpTree(): DemoNode {
  return dir('/', [
    dir('pub', [
      dir('releases', [
        blob('ice-commander-gtk_0.8.144-1_amd64.deb', 6329004),
        blob('ice-commander-gtk-0.8.144-1.x86_64.rpm', 7014676),
        blob('ice-commander-gtk-0.8.144-1-x86_64.pkg.tar.zst', 8674803),
        blob('ice-commander-gtk-0.8.144-1-win64.exe', 27327943),
        blob('ice-commander-gtk-0.8.144-1-mac.dmg', 18159761),
      ]),
      dir('docs', [
        text(
          'index.html',
          '<!doctype html>\n<html>\n  <body>\n    <h1>Ice Commander docs</h1>\n    <p>This file lives on the demo FTP server. Edit it with F4 —\n    the sandbox keeps it in memory until you reload.</p>\n  </body>\n</html>\n',
        ),
      ]),
    ]),
    text('welcome.msg', 'Welcome to the Ice Commander demo FTP server.\nEvery byte here is imaginary.\n'),
  ])
}

function ensureDirs(root: DemoNode, segs: string[]): DemoNode {
  let node = root
  for (const seg of segs) {
    node.children ||= []
    let next = node.children.find((c) => c.name === seg)
    if (!next) {
      next = dir(seg)
      node.children.push(next)
    }
    node = next
  }
  return node
}

interface Frame {
  sourceKey: string
  mount: string
  root: DemoNode
  rel: string[]
}

interface DemoPanel {
  frames: Frame[]
  showingSelector: boolean
  history: Frame[][]
  future: Frame[][]
}

interface Source {
  tree: DemoNode
  mount: string
  initial: string[]
  latency: number
  label?: string
  icon?: string
}

interface DemoTab {
  id: number
  panel: DemoPanel
}

interface LevelRef {
  frame: number
  segment: number | null
}

interface Summary {
  fmt: string
  fallback?: Summary
}

interface KindField extends Field {
  derive?: 'never' | 'while_untouched' | 'commit_only' | 'always'
  empty_as_absent?: boolean
  keep_when_inapplicable?: boolean
  on_parse_error?: 'reject' | 'default'
}

interface KindDocument extends Document {
  fields?: KindField[]
  summary?: Summary
  opens_at?: string
}

type SavedConnection = Connection & { stored_secrets?: string[] }

const KINDS = [
  { id: 'ftp', label: 'FTP', document: ftpDocument as KindDocument },
  { id: 'sftp', label: 'SFTP', document: sftpDocument as KindDocument },
  { id: 'webdav', label: 'WebDAV', document: webdavDocument as KindDocument },
]

const PLUGIN_ASSETS = new Map([
  ['ic-ftp-fs/ftp.svg', ftpIcon],
  ['ic-sftp-fs/sftp.svg', sftpIcon],
  ['ic-webdav-fs/webdav.svg', webdavIcon],
])

const NOT_AN_EXPORT = 'not an ice-commander connections file'

const hostFacts = () => ({ kind: 'web', locale: 'en', can: ['copy'], width_dp: 0, height_dp: 0 })

const kindOf = (id: string): KindDocument | undefined => KINDS.find((k) => k.id === id)?.document

const valueOf = (record: Connection, bind: string): string =>
  bind === 'name' ? record.name : bind === 'folder' ? (record.folder ?? '') : (record.settings?.[bind] ?? '')

const secretsOf = (record: Connection): string[] =>
  (kindOf(record.kind)?.fields ?? []).filter((f) => f.secret === true).map((f) => f.bind)

const storedSecrets = (record: Connection): string[] => secretsOf(record).filter((bind) => valueOf(record, bind) !== '')

const connectionKey = (record: Connection): string => `conn://${record.kind}/${record.name}`

const sortedKeys = (settings: Record<string, string>): Record<string, string> =>
  Object.fromEntries(Object.entries(settings).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))

const integer = (text: string): number | undefined => (/^[+-]?\d+$/.test(text.trim()) ? Number(text.trim()) : undefined)

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

function carrySecrets(incoming: Connection, stored: Connection): void {
  for (const bind of secretsOf(incoming)) {
    const kept = valueOf(stored, bind)
    if (valueOf(incoming, bind) === '' && kept !== '') incoming.settings = { ...incoming.settings, [bind]: kept }
  }
}

function fill(template: string, record: Connection): string | undefined {
  let found = false
  let blank = false
  const filled = template.replace(/\{([^}]*)\}/g, (_whole, key: string) => {
    const value = valueOf(record, key)
    found = true
    if (value === '') blank = true
    return value
  })
  return blank || (!found && template.includes('{')) ? undefined : filled
}

function summaryOf(record: Connection): string {
  for (let summary = kindOf(record.kind)?.summary; summary !== undefined; summary = summary.fallback) {
    const filled = fill(summary.fmt, record)
    if (filled !== undefined && filled.trim() !== '') return filled
  }
  return ''
}

function coerce(field: KindField, chosen: Json, fallback: Json | undefined): { text: string } | { problem: string } {
  if (field.type === 'bool') return { text: String(isTruthy(chosen)) }
  const text = asText(chosen)
  if (field.type !== 'integer') return { text }
  let number = integer(text)
  if (number === undefined && field.on_parse_error === 'default') {
    if (fallback === undefined) return { text: '' }
    number = integer(asText(fallback))
  }
  if (number === undefined) return { problem: `${field.bind}: ${JSON.stringify(text)} is not a number` }
  if (number < (field.min ?? -Infinity) || number > (field.max ?? Infinity)) {
    return { problem: `${field.bind}: ${number} is out of range` }
  }
  return { text: String(number) }
}

function commit(document: KindDocument, state: State) {
  const record: Record<string, string> = {}
  const settings: Record<string, string> = {}
  const missing: string[] = []
  const problems: string[] = []
  for (const field of document.fields ?? []) {
    const usable = evaluate(field.when, state)
    if (!usable && field.keep_when_inapplicable !== true) continue
    const raw = state.state[field.bind]
    const present = asText(raw) !== ''
    const derive = field.derive ?? 'never'
    const fallback = resolve(field.default, state)
    const chosen =
      fallback !== undefined && (derive === 'always' || (derive !== 'never' && !present)) ? fallback : (raw ?? null)
    const coerced = coerce(field, chosen, fallback)
    if ('problem' in coerced) {
      problems.push(coerced.problem)
      continue
    }
    if (coerced.text === '' && field.required === true && usable) missing.push(field.bind)
    if (coerced.text === '' && field.empty_as_absent === true) continue
    if (field.scope === 'record') record[field.bind] = coerced.text
    else settings[field.bind] = coerced.text
  }
  return { record, settings, missing: [...missing, ...problems] }
}

function imported(value: unknown): Connection {
  if (!isRecord(value)) throw new Error(NOT_AN_EXPORT)
  const { name, folder, kind, settings = {} } = value
  const textual = isRecord(settings) && Object.values(settings).every((v) => typeof v === 'string')
  if (typeof name !== 'string' || typeof kind !== 'string' || kind === '' || !textual) throw new Error(NOT_AN_EXPORT)
  if (folder !== undefined && folder !== null && typeof folder !== 'string') throw new Error(NOT_AN_EXPORT)
  return {
    name,
    folder: typeof folder === 'string' ? folder : undefined,
    kind,
    settings: sortedKeys(settings as Record<string, string>),
  }
}

const joinPath = (mount: string, rel: string[]): string => {
  const base = mount === '/' ? '' : mount
  return rel.length ? `${base}/${rel.join('/')}` : mount === '/' ? '/' : mount
}

const cloneFrames = (frames: Frame[]): Frame[] => frames.map((f) => ({ ...f, rel: [...f.rel] }))

const restamp = (node: DemoNode): DemoNode => ({ ...node, modified: now(), children: node.children?.map(restamp) })

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

export class DemoBackend implements Backend {
  private sources = new Map<string, Source>()
  private drives: Drive[] = []
  private savedConnections: Connection[] = [
    {
      name: 'ftp.icecommander.com',
      kind: 'ftp',
      settings: { host: 'ftp.icecommander.com', port: '21', user: 'demo' },
    },
  ]
  private favoritesOnly = false
  private listeners = new Set<BackendEvents>()
  private tabs: Record<Side, DemoTab[]>
  private activeTab: Record<Side, number>
  private nextTabId: Record<Side, number>

  constructor() {
    const local = localTree()
    this.sources.set('local_fs:/', { tree: local, mount: '/', initial: [], latency: 30 })
    this.sources.set('local_fs:~', { tree: local, mount: '/', initial: ['home', 'demo'], latency: 30 })
    this.sources.set('local_fs:/run/media/demo/USB', {
      tree: local,
      mount: '/',
      initial: ['run', 'media', 'demo', 'USB'],
      latency: 60,
    })
    this.sources.set('conn://ftp/ftp.icecommander.com', { tree: ftpTree(), mount: '/', initial: [], latency: 300 })
    this.drives = [
      {
        name: 'System Root',
        key: 'local_fs:/',
        path: '/',
        icon: 'home.svg',
        kind: 'root',
        subtitle: '/',
        is_favorite: true,
        is_online: true,
      },
      {
        name: 'User Home',
        key: 'local_fs:~',
        path: '/home/demo',
        icon: 'at-home.svg',
        kind: 'home',
        subtitle: '/home/demo',
        is_favorite: true,
        is_online: true,
      },
      {
        name: 'USB',
        key: 'local_fs:/run/media/demo/USB',
        path: '/run/media/demo/USB',
        icon: 'ssd.svg',
        kind: 'drive',
        subtitle: '/run/media/demo/USB',
        is_favorite: false,
        is_online: true,
      },
    ]
    this.syncConnections()
    this.tabs = {
      left: [{ id: 0, panel: this.freshPanel('local_fs:~') }],
      right: [{ id: 0, panel: this.freshPanel('local_fs:~') }],
    }
    this.activeTab = { left: 0, right: 0 }
    this.nextTabId = { left: 1, right: 1 }
  }

  private activePanel(side: Side): DemoPanel {
    const tabs = this.tabs[side]
    return (tabs.find((t) => t.id === this.activeTab[side]) ?? tabs[0]).panel
  }
  private tabTitle(panel: DemoPanel): string {
    if (panel.showingSelector) return '/'
    const top = panel.frames[panel.frames.length - 1]
    return top.rel[top.rel.length - 1] || (this.drives.find((d) => d.key === top.sourceKey)?.name ?? '/')
  }
  private freshPanel(key: string): DemoPanel {
    return { frames: [this.baseFrame(key)], showingSelector: false, history: [], future: [] }
  }
  private baseFrame(key: string): Frame {
    const source = this.sources.get(key)!
    return { sourceKey: key, mount: source.mount, root: source.tree, rel: [...source.initial] }
  }
  private top(side: Side): Frame {
    const panel = this.activePanel(side)
    return panel.frames[panel.frames.length - 1]
  }
  private nodeAt(root: DemoNode, segs: string[]): DemoNode | null {
    let node = root
    for (const seg of segs) {
      const next = node.children?.find((c) => c.name === seg)
      if (!next) return null
      node = next
    }
    return node
  }
  private currentDir(side: Side): DemoNode {
    const top = this.top(side)
    return this.nodeAt(top.root, top.rel) ?? top.root
  }
  private latency(side: Side): number {
    return this.sources.get(this.top(side).sourceKey)?.latency ?? 30
  }
  private flatten(panel: DemoPanel): { levels: Level[]; map: LevelRef[] } {
    const levels: Level[] = []
    const map: LevelRef[] = []
    panel.frames.forEach((frame, f) => {
      if (f === 0) {
        const source = this.sources.get(frame.sourceKey)
        levels.push({ name: source?.label ?? '', is_archive: false, label: source?.label, icon: source?.icon })
      } else {
        levels.push({ name: frame.mount.split('/').filter(Boolean).pop() ?? '', is_archive: true })
      }
      map.push({ frame: f, segment: null })
      frame.rel.forEach((seg, i) => {
        levels.push({ name: seg, is_archive: false })
        map.push({ frame: f, segment: i })
      })
    })
    return { levels, map }
  }
  private buildState(side: Side): PanelState {
    const panel = this.activePanel(side)
    const { levels } = this.flatten(panel)
    const top = this.top(side)
    const cur = this.currentDir(side)
    const path = joinPath(top.mount, top.rel)
    return {
      levels,
      path,
      entries: [...(cur.children ?? [])]
        .sort((a, b) => (a.is_dir === b.is_dir ? a.name.localeCompare(b.name) : a.is_dir ? -1 : 1))
        .map((n) => ({
          name: n.name,
          path: path === '/' ? `/${n.name}` : `${path}/${n.name}`,
          is_dir: n.is_dir,
          enterable: n.archive === true,
          size: n.is_dir ? null : n.size,
          modified: n.modified,
        })),
      showing_selector: panel.showingSelector,
      tabs: this.tabs[side].map((t) => ({ id: t.id, title: this.tabTitle(t.panel) })),
      active_tab: this.activeTab[side],
    }
  }
  private notify(side: Side): void {
    const state = this.buildState(side)
    setTimeout(() => {
      for (const l of this.listeners) l.onPanelUpdated?.(side, state)
    }, 0)
  }
  private snapshot(side: Side): void {
    const panel = this.activePanel(side)
    panel.history.push(cloneFrames(panel.frames))
    if (panel.history.length > 64) panel.history.shift()
    panel.future = []
  }
  private async cmd(side: Side, mutate: (panel: DemoPanel) => void): Promise<void> {
    await sleep(this.latency(side))
    mutate(this.activePanel(side))
    this.notify(side)
  }
  private findChild(side: Side, pathOrName: string): { parent: DemoNode; node: DemoNode } | null {
    const cur = this.currentDir(side)
    const top = this.top(side)
    const base = joinPath(top.mount, top.rel)
    const node = cur.children?.find(
      (c) => (base === '/' ? `/${c.name}` : `${base}/${c.name}`) === pathOrName || c.name === pathOrName,
    )
    return node ? { parent: cur, node } : null
  }
  private syncConnections(): void {
    const favorites = new Set(this.drives.filter((d) => d.is_favorite).map((d) => d.key))
    const connections = this.savedConnections.map((c): Drive => {
      const key = connectionKey(c)
      const document = kindOf(c.kind)
      const at = (document?.opens_at ? valueOf(c, document.opens_at) : '').split('/').filter(Boolean)
      const icon = typeof document?.icon === 'string' ? document.icon : 'connect.svg'
      const held = this.sources.get(key)
      const tree = held?.tree ?? dir('/')
      const landing = ensureDirs(tree, at)
      if (!held) landing.children?.push(text('README.txt', CONNECTION_README))
      this.sources.set(key, { tree, mount: '/', initial: at, latency: held?.latency ?? 300, label: c.name, icon })
      return {
        name: c.name,
        key,
        path: '',
        icon,
        kind: 'net',
        subtitle: summaryOf(c),
        is_favorite: favorites.has(key),
        is_online: true,
      }
    })
    this.drives = [...this.drives.filter((d) => d.kind !== 'net'), ...connections]
  }

  async fetchState(side: Side): Promise<PanelState> {
    await sleep(this.latency(side))
    return this.buildState(side)
  }
  async enter(side: Side, name: string): Promise<void> {
    await this.cmd(side, (panel) => {
      const node = this.currentDir(side).children?.find((c) => c.name === name)
      if (node) {
        this.snapshot(side)
        if (node.is_dir) this.top(side).rel.push(name)
        else if (node.archive && node.children) {
          const top = this.top(side)
          const base = joinPath(top.mount, top.rel)
          panel.frames.push({
            sourceKey: top.sourceKey,
            mount: base === '/' ? `/${name}` : `${base}/${name}`,
            root: node,
            rel: [],
          })
        }
      }
    })
  }
  async goUp(side: Side): Promise<void> {
    await this.cmd(side, (panel) => {
      const top = this.top(side)
      if (top.rel.length > 0) {
        this.snapshot(side)
        top.rel.pop()
      } else if (panel.frames.length > 1) {
        this.snapshot(side)
        panel.frames.pop()
      }
    })
  }
  async goBack(side: Side): Promise<void> {
    await this.cmd(side, (panel) => {
      const prev = panel.history.pop()
      if (prev) {
        panel.future.push(cloneFrames(panel.frames))
        panel.frames = prev
        panel.showingSelector = false
      }
    })
  }
  async goForward(side: Side): Promise<void> {
    await this.cmd(side, (panel) => {
      const next = panel.future.pop()
      if (next) {
        panel.history.push(cloneFrames(panel.frames))
        panel.frames = next
        panel.showingSelector = false
      }
    })
  }
  async breadcrumb(side: Side, level: number): Promise<void> {
    await this.cmd(side, (panel) => {
      const { map } = this.flatten(panel)
      if (level < 0 || level >= map.length) return
      const { frame, segment } = map[level]
      this.snapshot(side)
      panel.frames = panel.frames.slice(0, frame + 1)
      const f = panel.frames[frame]
      f.rel = segment === null ? [] : f.rel.slice(0, segment + 1)
      panel.showingSelector = false
    })
  }
  async goHome(side: Side): Promise<void> {
    await this.cmd(side, (panel) => {
      panel.showingSelector = true
    })
  }

  async addTab(side: Side): Promise<void> {
    const id = this.nextTabId[side]++
    this.tabs[side].push({ id, panel: this.freshPanel('local_fs:~') })
    this.activeTab[side] = id
    this.notify(side)
  }
  async closeTab(side: Side, id: number): Promise<void> {
    const tabs = this.tabs[side]
    if (tabs.length <= 1) return
    const idx = tabs.findIndex((t) => t.id === id)
    if (idx < 0) return
    tabs.splice(idx, 1)
    if (this.activeTab[side] === id) this.activeTab[side] = tabs[Math.min(idx, tabs.length - 1)].id
    this.notify(side)
  }
  async switchTab(side: Side, id: number): Promise<void> {
    if (this.tabs[side].some((t) => t.id === id)) {
      this.activeTab[side] = id
      this.notify(side)
    }
  }

  async getDrives(): Promise<Drive[]> {
    await sleep(30)
    return this.drives.map((d) => ({ ...d }))
  }
  async activateSource(side: Side, key: string): Promise<void> {
    const source = this.sources.get(key)
    if (!source) return
    await sleep(source.latency)
    const panel = this.activePanel(side)
    this.snapshot(side)
    panel.frames = [this.baseFrame(key)]
    panel.showingSelector = false
    this.notify(side)
  }
  async getConnections(): Promise<Connection[]> {
    await sleep(30)
    return this.savedConnections.map((c): SavedConnection => {
      const stored = storedSecrets(c)
      const settings = { ...c.settings }
      for (const bind of stored) settings[bind] = ''
      return {
        name: c.name,
        folder: c.folder,
        kind: c.kind,
        settings: Object.keys(settings).length > 0 ? settings : undefined,
        stored_secrets: stored.length > 0 ? stored : undefined,
      }
    })
  }
  async saveConnection(connection: Connection): Promise<void> {
    await sleep(60)
    const at = this.savedConnections.findIndex((c) => c.name === connection.name)
    const existing = at >= 0 ? this.savedConnections[at] : undefined
    const given = { ...connection.settings }
    const record: Connection = {
      name: connection.name,
      folder: connection.folder ?? existing?.folder,
      kind: connection.kind.toLowerCase(),
      settings: Object.keys(given).length > 0 ? given : { ...existing?.settings },
    }
    if (existing) carrySecrets(record, existing)
    record.settings = sortedKeys(record.settings ?? {})
    if (at >= 0) this.savedConnections[at] = record
    else this.savedConnections.push(record)
    this.syncConnections()
  }
  async deleteConnection(name: string): Promise<void> {
    await sleep(40)
    if (!this.savedConnections.some((c) => c.name === name)) {
      throw new Error(`no saved connection named ${JSON.stringify(name)}`)
    }
    this.savedConnections = this.savedConnections.filter((c) => c.name !== name)
    this.syncConnections()
  }
  async setConnectionsDialog(_open: boolean): Promise<void> {}
  async exportConnections(password?: string): Promise<string> {
    await sleep(60)
    if (this.savedConnections.length === 0) throw new Error('no saved connections to export')
    if (password) throw new Error('the sandbox cannot encrypt an export: leave the password empty')
    const connections = this.savedConnections.map((c) => ({
      folder: c.folder,
      kind: c.kind,
      name: c.name,
      settings: Object.keys(c.settings ?? {}).length > 0 ? c.settings : undefined,
    }))
    return JSON.stringify({ connections, encrypted: false, ic_connections: 1 }, null, 2)
  }
  async importConnections(data: string, password?: string): Promise<number> {
    await sleep(60)
    let file: unknown
    try {
      file = JSON.parse(data)
    } catch {
      throw new Error(NOT_AN_EXPORT)
    }
    if (!isRecord(file) || !('ic_connections' in file)) throw new Error(NOT_AN_EXPORT)
    if (file.encrypted === true) {
      throw new Error(password ? 'the sandbox cannot decrypt an encrypted export' : 'needs-password')
    }
    if (!Array.isArray(file.connections)) throw new Error(NOT_AN_EXPORT)
    for (const record of file.connections.map(imported)) {
      const at = this.savedConnections.findIndex((c) => c.name === record.name)
      if (at >= 0) this.savedConnections[at] = record
      else this.savedConnections.push(record)
    }
    this.syncConnections()
    return this.savedConnections.length
  }
  async connectTo(side: Side, connection: Connection): Promise<void> {
    const record = this.savedConnections.find((c) => c.name === connection.name)
    if (!record) throw new Error(`no saved connection named ${JSON.stringify(connection.name)}`)
    await this.activateSource(side, connectionKey(record))
  }

  async fetchConnectionKinds(): Promise<ConnectionKinds> {
    await sleep(30)
    return {
      host: hostFacts(),
      kinds: KINDS.map(({ id, label, document }) => ({ id, label, document, events: false })),
    }
  }
  async submitConnectionForm(form: {
    kind: string
    values: Record<string, Json>
    touched: string[]
    editing?: string
    connect?: Side
  }): Promise<{ ok: boolean; name?: string; missing?: string[] }> {
    await sleep(60)
    const kind = form.kind.toLowerCase()
    const document = kindOf(kind)
    if (!document) throw new Error(`no plugin serves ${kind} connections`)
    const at = this.savedConnections.findIndex((c) => c.name === form.editing)
    const previous = at >= 0 ? this.savedConnections[at] : undefined
    const committed = commit(document, {
      state: form.values,
      view: {},
      host: {},
      data: {},
      arg: null,
      stored_secrets: previous ? storedSecrets(previous) : [],
      touched: form.touched,
    })
    if (committed.missing.length > 0) return { ok: false, missing: committed.missing }
    const record: Connection = {
      name: committed.record.name ?? '',
      folder: previous?.folder ?? committed.record.folder,
      kind,
      settings: committed.settings,
    }
    if (previous) carrySecrets(record, previous)
    record.settings = sortedKeys(record.settings ?? {})
    if (record.name.trim() === '') throw new Error('a connection needs a name')
    if (this.savedConnections.some((c, i) => c.name === record.name && i !== at)) {
      throw new Error(`a connection named ${JSON.stringify(record.name)} already exists`)
    }
    const key = connectionKey(record)
    const renamed = previous && this.sources.get(connectionKey(previous))
    if (renamed && !this.sources.has(key)) this.sources.set(key, { ...renamed })
    if (at >= 0) this.savedConnections[at] = record
    else this.savedConnections.push(record)
    this.syncConnections()
    if (form.connect) await this.activateSource(form.connect, key)
    return { ok: true, name: record.name }
  }
  async sendConnectionFormEvent(kind: string, _event: Record<string, unknown>): Promise<ViewSnapshot> {
    throw new Error(`no plugin listens to the ${kind} form in the sandbox`)
  }
  async fetchPluginViews(): Promise<{ id: string; title: string }[]> {
    return []
  }
  async openPluginView(id: string, _argument?: string): Promise<ViewSnapshot> {
    throw new Error(`${id} cannot describe itself`)
  }
  async sendPluginViewEvent(id: string, _event: Record<string, unknown>): Promise<ViewSnapshot> {
    throw new Error(`${id} is not open`)
  }
  async closePluginView(_id: string): Promise<void> {}
  async openViewer(_side: Side, _path: string): Promise<ViewerOpened | null> {
    return null
  }
  async sendViewerEvent(_instance: number, _event: Record<string, unknown>): Promise<ViewerAnswer> {
    return { answer: {}, document: null }
  }
  viewerPartUrl(_instance: number, _name: string, _drawn?: number): string {
    return ''
  }
  assetUrl(name: string): string {
    return PLUGIN_ASSETS.get(name) ?? ''
  }
  async closeViewer(_instance: number, _force?: boolean): Promise<ViewerClosed> {
    return { closed: true }
  }
  async fetchTranslations(): Promise<{ lang: string; keys: Record<string, string> }> {
    return { lang: 'en', keys: {} }
  }

  async toggleFavorite(key: string): Promise<void> {
    await sleep(20)
    const drive = this.drives.find((d) => d.key === key)
    if (drive) drive.is_favorite = !drive.is_favorite
  }
  async getFavoritesOnly(): Promise<boolean> {
    return this.favoritesOnly
  }
  async setFavoritesOnly(value: boolean): Promise<void> {
    this.favoritesOnly = value
  }

  async fetchSettings(): Promise<Record<string, unknown>> {
    return {}
  }

  async readFile(side: Side, path: string): Promise<FileContent> {
    await sleep(this.latency(side))
    const content = this.findChild(side, path)?.node.content
    return { path, content: content ?? '', is_binary: content === undefined }
  }
  async writeFile(side: Side, path: string, content: string): Promise<void> {
    await this.cmd(side, () => {
      const hit = this.findChild(side, path)
      if (hit) {
        hit.node.content = content
        hit.node.size = content.length
        hit.node.modified = now()
      }
    })
  }
  async getFileUrl(side: Side, path: string): Promise<string> {
    await sleep(this.latency(side))
    const file = this.findChild(side, path)?.node.media
    return file && mediaBase ? new URL(file, mediaBase).href : ''
  }
  async deleteEntries(side: Side, paths: string[]): Promise<void> {
    await this.cmd(side, () => {
      const cur = this.currentDir(side)
      for (const p of paths) {
        const hit = this.findChild(side, p)
        if (hit && cur.children) cur.children = cur.children.filter((c) => c !== hit.node)
      }
    })
  }
  async uploadFile(side: Side, path: string, data: Blob): Promise<void> {
    const content = await data
      .text()
      .then((t) => (/�/.test(t) ? undefined : t))
      .catch(() => undefined)
    await this.cmd(side, () => {
      const name = path.split('/').filter(Boolean).pop() ?? 'upload'
      const cur = this.currentDir(side)
      cur.children ||= []
      const existing = cur.children.find((c) => c.name === name)
      if (existing) {
        existing.size = data.size
        existing.modified = now()
        existing.content = content
      } else {
        cur.children.push({ name, is_dir: false, size: data.size, modified: now(), content })
      }
    })
  }
  async openNative(_side: Side, _path: string): Promise<void> {}
  async closeViewerWindows(): Promise<void> {}
  async mkdir(side: Side, name: string): Promise<void> {
    await this.cmd(side, () => {
      const cur = this.currentDir(side)
      cur.children ||= []
      if (!cur.children.some((c) => c.name === name)) cur.children.push(dir(name, [], now()))
    })
  }
  async renameEntry(side: Side, oldPath: string, newName: string): Promise<void> {
    await this.cmd(side, () => {
      const hit = this.findChild(side, oldPath)
      if (hit) {
        hit.node.name = newName
        hit.node.modified = now()
      }
    })
  }
  async copyEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void> {
    await this.transfer(srcSide, dstSide, paths, false)
  }
  async moveEntries(srcSide: Side, dstSide: Side, paths: string[]): Promise<void> {
    await this.transfer(srcSide, dstSide, paths, true)
  }
  private async transfer(src: Side, dst: Side, paths: string[], move: boolean): Promise<void> {
    await sleep(Math.max(this.latency(src), this.latency(dst)))
    const from = this.currentDir(src)
    const to = this.currentDir(dst)
    to.children ||= []
    for (const p of paths) {
      const hit = this.findChild(src, p)
      if (hit) {
        to.children = to.children.filter((c) => c.name !== hit.node.name)
        to.children.push(restamp(hit.node))
        if (move && from.children) from.children = from.children.filter((c) => c !== hit.node)
      }
    }
    this.notify(src)
    this.notify(dst)
  }

  private demoOpStart = Date.now()
  async getOperations(): Promise<Operation[]> {
    await sleep(40)
    const total = 44040192
    const speed = 3.2e6
    const done = Math.min((Date.now() - this.demoOpStart) / 1000 * speed, total)
    if (done >= total) this.demoOpStart = Date.now()
    return [
      {
        id: 1,
        kind: 'copy',
        current_file: 'backup.zip',
        done_bytes: Math.floor(done),
        total_bytes: total,
        done_files: done >= total ? 1 : 0,
        total_files: 1,
        speed_bps: speed,
      },
    ]
  }
  async cancelOperation(_id: number): Promise<void> {
    await sleep(20)
    this.demoOpStart = Date.now()
  }

  openTerminal(side: Side, callbacks: TerminalCallbacks): TerminalSession {
    return new DemoShell(this, side, callbacks)
  }
  async setTerminalExpanded(side: Side, expanded: boolean): Promise<void> {
    setTimeout(() => {
      for (const l of this.listeners) l.onTerminalExpanded?.(side, expanded)
    }, 0)
  }
  async setViewMode(_side: Side, _mode: string): Promise<void> {}

  subscribe(events: BackendEvents): () => void {
    this.listeners.add(events)
    return () => {
      this.listeners.delete(events)
    }
  }

  get localTree(): DemoNode {
    return this.sources.get('local_fs:/')!.tree
  }
  resolveShellPath(root: DemoNode, segs: string[]): DemoNode | null {
    return this.nodeAt(root, segs)
  }
}

const encoder = new TextEncoder()
const C_USER = '\x1b[38;5;75m'
const C_PATH = '\x1b[38;5;178m'
const C_DIR = '\x1b[1;34m'
const C_RESET = '\x1b[0m'

class DemoShell implements TerminalSession {
  private buffer = ''
  private cwd = ['home', 'demo']
  private closed = false
  private backend: DemoBackend
  private cb: TerminalCallbacks

  constructor(backend: DemoBackend, _side: Side, cb: TerminalCallbacks) {
    this.backend = backend
    this.cb = cb
    setTimeout(() => {
      if (this.closed) return
      this.out('Ice Commander demo shell — this is a sandbox, not a real machine.\r\n')
      this.out('Try: help, ls, cd, cat, echo, pwd, clear\r\n\r\n')
      this.prompt()
    }, 60)
  }

  private out(s: string): void {
    if (!this.closed) this.cb.onData(encoder.encode(s))
  }
  private cwdLabel(): string {
    const abs = '/' + this.cwd.join('/')
    return abs === '/home/demo' || abs.startsWith('/home/demo/') ? '~' + abs.slice(10) : abs
  }
  private prompt(): void {
    this.out(`${C_USER}demo@ice${C_RESET}:${C_PATH}${this.cwdLabel()}${C_RESET}$ `)
  }

  send(data: string): void {
    let escape: 'esc' | 'csi' | 'ss3' | null = null
    for (const ch of data) {
      if (escape === 'esc') escape = ch === '[' ? 'csi' : ch === 'O' ? 'ss3' : null
      else if (escape === 'csi') escape = ch >= '@' && ch <= '~' ? null : 'csi'
      else if (escape === 'ss3') escape = null
      else if (ch === '\x1b') escape = 'esc'
      else if (ch === '\r') {
        this.out('\r\n')
        this.exec(this.buffer.trim())
        this.buffer = ''
      } else if (ch === '\x7f') {
        if (this.buffer.length > 0) {
          this.buffer = this.buffer.slice(0, -1)
          this.out('\b \b')
        }
      } else if (ch === '\x03') {
        this.buffer = ''
        this.out('^C\r\n')
        this.prompt()
      } else if (ch >= ' ') {
        this.buffer += ch
        this.out(ch)
      }
    }
  }

  private node(segs: string[]): DemoNode | null {
    return this.backend.resolveShellPath(this.backend.localTree, segs)
  }
  private resolve(path: string): string[] | null {
    let segs: string[]
    if (path.startsWith('/')) segs = path.split('/').filter(Boolean)
    else if (path === '~' || path.startsWith('~/')) segs = ['home', 'demo', ...path.slice(1).split('/').filter(Boolean)]
    else segs = [...this.cwd, ...path.split('/').filter(Boolean)]
    const out: string[] = []
    for (const s of segs) {
      if (s === '.') continue
      else if (s === '..') out.pop()
      else out.push(s)
    }
    return this.node(out) ? out : null
  }

  private exec(line: string): void {
    if (line) {
      const [cmd = '', ...args] = line.split(/\s+/)
      switch (cmd) {
        case 'help':
          this.out('Available: ls [path], cd [path], cat <file>, pwd, echo <text>, whoami, uname, clear, help\r\n')
          break
        case 'pwd':
          this.out('/' + this.cwd.join('/') + '\r\n')
          break
        case 'whoami':
          this.out('demo\r\n')
          break
        case 'uname':
          this.out('IceOS demo 1.0 wasm64\r\n')
          break
        case 'clear':
          this.out('\x1b[2J\x1b[H')
          break
        case 'echo':
          this.out(args.join(' ') + '\r\n')
          break
        case 'ls': {
          const segs = args[0] ? this.resolve(args[0]) : this.cwd
          const node = segs ? this.node(segs) : null
          if (!node) this.out(`ls: ${args[0]}: no such file or directory\r\n`)
          else if (!node.is_dir) this.out(node.name + '\r\n')
          else {
            const names = [...(node.children ?? [])]
              .sort((a, b) => (a.is_dir === b.is_dir ? a.name.localeCompare(b.name) : a.is_dir ? -1 : 1))
              .map((c) => (c.is_dir ? `${C_DIR}${c.name}${C_RESET}` : c.name))
            this.out(names.join('  ') + (names.length ? '\r\n' : ''))
          }
          break
        }
        case 'cd': {
          if (!args[0] || args[0] === '~') {
            this.cwd = ['home', 'demo']
            break
          }
          const segs = this.resolve(args[0])
          const node = segs ? this.node(segs) : null
          if (!node) this.out(`cd: ${args[0]}: no such file or directory\r\n`)
          else if (!node.is_dir) this.out(`cd: ${args[0]}: not a directory\r\n`)
          else this.cwd = segs!
          break
        }
        case 'cat': {
          if (!args[0]) {
            this.out('cat: missing operand\r\n')
            break
          }
          const segs = this.resolve(args[0])
          const node = segs ? this.node(segs) : null
          if (!node) this.out(`cat: ${args[0]}: no such file or directory\r\n`)
          else if (node.is_dir) this.out(`cat: ${args[0]}: is a directory\r\n`)
          else if (node.content === undefined) this.out(`cat: ${args[0]}: binary file\r\n`)
          else this.out(node.content.replaceAll('\n', '\r\n'))
          break
        }
        default:
          this.out(`${cmd}: command not found (this demo shell knows: help, ls, cd, cat, echo, pwd)\r\n`)
      }
    }
    this.prompt()
  }

  resize(_cols: number, _rows: number): void {}
  close(): void {
    this.closed = true
    this.cb.onClose?.()
  }
}
