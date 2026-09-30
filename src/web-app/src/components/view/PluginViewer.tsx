import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { trOptional } from '../../lib/i18n'
import * as api from '../../api/client'
import type { Side } from '../../api/types'
import type { Action, Document, Intent, Json, Node, State } from '../../lib/view/types'
import { bindOfNode, resolveText, walk } from '../../lib/view/expr'
import { DocumentView } from './DocumentView'


interface Props {
  side: Side
  path: string
  onClose: () => void
  onUnclaimed: () => void
}

function beside(opened: string, name: string): string {
  const at = opened.lastIndexOf('/')
  return at < 0 ? name : `${opened.slice(0, at + 1)}${name}`
}

function asDescribed(doc: Document): State {
  return {
    state: {},
    view: {},
    host: {},
    data: doc.data ?? {},
    arg: null,
    stored_secrets: [],
    touched: [],
  }
}

export function PluginViewer({ side, path, onClose, onUnclaimed }: Props) {
  const [open, setOpen] = useState<{ instance: number; document: Document } | null>(null)
  const [redrawn, setRedrawn] = useState(0)
  const [values, setValues] = useState<Record<string, Json>>({})
  const [overlay, setOverlay] = useState<{ view: Record<string, Json>; data: Record<string, Json> }>(
    { view: {}, data: {} },
  )
  const [files, setFiles] = useState<Record<string, string>>({})
  const [failed, setFailed] = useState<string | null>(null)
  const name = path.split('/').pop() || path
  const unclaimed = useRef(onUnclaimed)
  unclaimed.current = onUnclaimed
  const released = useRef(false)
  const announced = useRef<number | null>(null)

  const urlsFor = useCallback(
    async (doc: Document): Promise<Record<string, string>> => {
      const named = new Set<string>()
      const described = asDescribed(doc)
      walk(doc.form, (node) => {
        const src = resolveText(node.src, described, trOptional)
        if (src !== undefined && src.startsWith('file:')) named.add(src.slice('file:'.length))
      })
      const found = await Promise.all(
        [...named].map(
          async (wanted) => [wanted, await api.getFileUrl(side, beside(path, wanted))] as const,
        ),
      )
      return Object.fromEntries(found)
    },
    [side, path],
  )

  useEffect(() => {
    let alive = true
    let opened: number | null = null
    released.current = false
    void (async () => {
      try {
        const answered = await api.openViewer(side, path)
        if (answered === null) {
          if (alive) unclaimed.current()
          return
        }
        opened = answered.instance
        const urls = await urlsFor(answered.document)
        if (!alive) {
          void api.closeViewer(answered.instance, true)
          return
        }
        setFiles(urls)
        setOpen({ instance: answered.instance, document: answered.document })
      } catch (e) {
        if (alive) setFailed((e as Error).message)
      }
    })()
    return () => {
      alive = false
      if (opened !== null && !released.current) void api.closeViewer(opened, true)
    }
  }, [side, path, urlsFor])

  const state: State = useMemo(
    () => ({
      state: values,
      view: overlay.view,
      host: {},
      data: { ...(open?.document.data ?? {}), ...overlay.data },
      arg: null,
      stored_secrets: [],
      touched: [],
    }),
    [values, overlay, open],
  )

  const apply = (keys: [string, Json][]) => {
    setValues((held) => {
      const next = { ...held }
      for (const [key, value] of keys) {
        if (key.startsWith('state.')) next[key.slice('state.'.length)] = value
      }
      return next
    })
    setOverlay((held) => {
      const view = { ...held.view }
      const data = { ...held.data }
      for (const [key, value] of keys) {
        if (key.startsWith('view.')) view[key.slice('view.'.length)] = value
        if (key.startsWith('data.')) data[key.slice('data.'.length)] = value
      }
      return { view, data }
    })
  }

  const draw = async (instance: number, document: Document) => {
    setFiles(await urlsFor(document))
    setOpen({ instance, document })
    setRedrawn((held) => held + 1)
    setOverlay({ view: {}, data: {} })
  }

  const send = async (event: Record<string, unknown>) => {
    if (open === null) return
    const instance = open.instance
    try {
      const answered = await api.sendViewerEvent(instance, event)
      const drawn = answered.document ?? open.document
      if (answered.document !== null) await draw(instance, answered.document)
      const reply = answered.answer ?? {}
      const put = reply.put as Record<string, Json> | undefined
      if (put !== undefined) {
        setValues((held) => {
          const next = { ...held }
          for (const [node, value] of Object.entries(put)) {
            const bind = bindOfNode(drawn.form, node)
            if (bind !== undefined) next[bind] = value
          }
          return next
        })
      }
      const set = reply.set as Record<string, Json> | undefined
      if (set !== undefined) apply(Object.entries(set))
      if (typeof reply.clipboard === 'string' && navigator.clipboard !== undefined) {
        await navigator.clipboard.writeText(reply.clipboard)
      }
      if (reply.close === true) void leave()
    } catch (e) {
      setFailed((e as Error).message)
    }
  }

  const leave = async () => {
    if (open === null) {
      onClose()
      return
    }
    const instance = open.instance
    try {
      const answered = await api.closeViewer(instance)
      if (!answered.closed) {
        if (answered.document != null) await draw(instance, answered.document)
        return
      }
      released.current = true
    } catch {
    }
    onClose()
  }

  const change = (node: Node, bind: string, value: Json) => {
    if (bind === '') return
    const next = { ...values, [bind]: value }
    setValues(next)
    if (node.emit !== 'change' || node.id === undefined) return
    void send({ type: 'change', node: node.id, bind, value, values: next })
  }

  const activate = (at: string) => {
    void send({ type: 'activate', node: at, values })
  }

  const act = (intent: Intent | undefined, from: Node | Action) => {
    switch (intent?.do) {
      case 'close':
        void leave()
        break
      case 'emit':
        activate((intent as { node: string }).node)
        break
      case 'set':
        apply(Object.entries((intent as { keys: Record<string, Json> }).keys))
        break
      default:
        if ('t' in from && from.id !== undefined) activate(from.id)
        break
    }
  }

  const pictureUrl = (src: string): string | undefined => {
    if (open === null) return undefined
    if (src.startsWith('part:'))
      return api.viewerPartUrl(open.instance, src.slice('part:'.length), redrawn)
    if (src.startsWith('file:')) return files[src.slice('file:'.length)]
    return undefined
  }

  const leaving = useRef(leave)
  leaving.current = leave
  const sending = useRef(send)
  sending.current = send

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      void leaving.current()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])

  useEffect(() => {
    if (open === null || announced.current === open.instance) return
    announced.current = open.instance
    void sending.current({ type: 'opened' })
  }, [open])

  if (open === null && failed === null) return null

  return (
    <div className="modal-overlay" onClick={() => void leave()}>
      <div
        className="modal-card"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '86%',
          maxWidth: '1000px',
          height: '86%',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <h3 className="modal-title" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span
            style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
          >
            {name}
          </span>
          <span style={{ fontSize: '11px', color: 'var(--text-dim)', fontWeight: 400 }}>{path}</span>
        </h3>

        {failed !== null && <div style={{ color: 'var(--danger, #e05252)' }}>{failed}</div>}
        {open !== null && (
          <DocumentView
            document={open.document}
            state={state}
            values={values}
            onChange={change}
            onIntent={act}
            translate={trOptional}
            assetUrl={(asset) => `/api/plugin-assets/${encodeURIComponent(asset)}`}
            pictureUrl={pictureUrl}
          />
        )}
      </div>
    </div>
  )
}
