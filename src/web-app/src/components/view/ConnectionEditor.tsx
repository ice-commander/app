import { useEffect, useMemo, useRef, useState } from 'react'
import { tr, trOptional } from '../../lib/i18n'
import * as api from '../../api/client'
import type { Connection, Side } from '../../api/types'
import type {
  Action,
  ConnectionKinds,
  Document,
  Intent,
  Json,
  Node,
  State,
  ViewSnapshot,
} from '../../lib/view/types'
import { asText, bindOfNode, resolve, valuesEqual } from '../../lib/view/expr'
import { DocumentView } from './DocumentView'

/**
 * The connection editor, drawn entirely from what the plugins declared. The
 * browser knows no protocol, no port and no field list: it renders the
 * document, collects the binds and hands them back for the host to commit.
 * A kind whose plugin listens also gets its gestures forwarded, and what the
 * plugin answers is written back into the form.
 */

/** The record as the host hands it out: the secrets stay behind, only their binds are named. */
type SavedConnection = Connection & { stored_secrets?: string[] }

interface Props {
  /** The record being edited; absent when creating a new connection. */
  initial?: SavedConnection
  side: Side
  onCancel: () => void
  onSaved: (name: string, connected: boolean) => void
  /** Rendered inside an existing dialog rather than as one of its own. */
  inline?: boolean
}

/** The one bind a record keeps under another name, as the host reads it natively. */
function fromRecord(record: Record<string, Json> | undefined, bind: string): Json | undefined {
  const settings = record?.settings as Record<string, Json> | undefined
  return settings?.[bind]
}

/** Every bind arrives as text, the way the host seeds a form from a record. */
function seeded(document: Document, from?: SavedConnection) {
  const values: Record<string, Json> = {}
  const record = from as unknown as Record<string, Json> | undefined
  for (const field of document.fields ?? []) {
    if (field.secret === true) continue
    const text = asText(fromRecord(record, field.bind))
    if (text !== '') values[field.bind] = text
  }
  return values
}

/**
 * Which secret binds the host already holds a value for. The record carries no
 * secret out of the host, so the server names them in `stored_secrets`.
 */
function storedSecrets(document: Document, from?: SavedConnection) {
  if (from === undefined) return []
  const named = new Set(from.stored_secrets ?? [])
  const record = from as unknown as Record<string, Json>
  const binds: string[] = []
  for (const field of document.fields ?? []) {
    if (field.secret !== true) continue
    if (named.has(field.bind) || asText(fromRecord(record, field.bind)) !== '') binds.push(field.bind)
  }
  return binds
}

/** A connection form keeps no session on the host, so its answer carries the redescribe too. */
type FormAnswer = ViewSnapshot & { redescribe?: boolean }

/** What a form event may carry: a secret bind stays in the browser until the user saves. */
function withoutSecrets(values: Record<string, Json>, document: Document | null) {
  const secret = new Set(
    (document?.fields ?? []).filter((field) => field.secret === true).map((field) => field.bind),
  )
  const carried: Record<string, Json> = {}
  for (const [bind, value] of Object.entries(values)) {
    if (!secret.has(bind)) carried[bind] = value
  }
  return carried
}

export function ConnectionEditor({ initial, side, onCancel, onSaved, inline }: Props) {
  const wanted = initial?.kind?.toLowerCase() ?? ''
  const [offered, setOffered] = useState<ConnectionKinds | null>(null)
  const [failed, setFailed] = useState<string | null>(null)
  const [kindId, setKindId] = useState(wanted)
  const [doc, setDoc] = useState<Document | null>(null)
  const [values, setValues] = useState<Record<string, Json>>({})
  const [overlay, setOverlay] = useState<{ view: Record<string, Json>; data: Record<string, Json> }>(
    { view: {}, data: {} },
  )
  const [stored, setStored] = useState<string[]>([])
  const [touched, setTouched] = useState<string[]>([])
  const [missing, setMissing] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const [round, setRound] = useState(0)
  const waiting = useRef<Record<string, number>>({})
  const chosen = useRef(wanted)
  /** Prepared once per record: whatever the form before it had in flight is no longer its own. */
  const turn = useRef(0)
  const record = useRef(initial)
  record.current = initial
  const recordId = initial === undefined ? '' : initial.name
  const creating = initial === undefined

  /** Picking a kind drops whatever the previous one still had in flight. */
  const choose = (id: string) => {
    for (const timer of Object.values(waiting.current)) window.clearTimeout(timer)
    waiting.current = {}
    chosen.current = id
    setKindId(id)
  }

  useEffect(() => {
    api
      .fetchConnectionKinds()
      .then((answered) => {
        setOffered(answered)
        if (wanted === '' && answered.kinds.length > 0) choose(answered.kinds[0].id)
      })
      .catch((e: Error) => setFailed(e.message))
  }, [wanted])

  const kind = offered?.kinds.find((entry) => entry.id === kindId)

  /** `host.*` reads here what it reads natively, the `can_<name>` flags included. */
  const facts = useMemo(() => {
    const host = offered?.host
    if (host === undefined) return {} as Record<string, Json>
    const seededFacts: Record<string, Json> = { ...host }
    for (const name of host.can ?? []) seededFacts[`can_${name}`] = true
    return seededFacts
  }, [offered])

  /** `sent` is what the request carried, so only what the plugin altered is written back. */
  const send = async (
    target: string,
    form: Document,
    sent: Record<string, Json>,
    event: Record<string, unknown>,
  ) => {
    const mine = turn.current
    if (target !== chosen.current) return
    try {
      const answered: FormAnswer = await api.sendConnectionFormEvent(target, event)
      if (target !== chosen.current || mine !== turn.current) return
      const again = answered.redescribe === true
      const drawn = again ? answered.document : form
      if (again) setDoc(answered.document)
      setOverlay((held) => ({
        view: { ...held.view, ...answered.state.view },
        data: { ...held.data, ...answered.state.data },
      }))
      setValues((held) => {
        const next = { ...held }
        for (const [bind, value] of Object.entries(answered.state.state)) {
          if (!valuesEqual(sent[bind], value)) next[bind] = value
        }
        for (const [node, value] of Object.entries(answered.put ?? {})) {
          const bind = bindOfNode(drawn.form, node)
          if (bind !== undefined) next[bind] = value
        }
        return next
      })
      if (answered.clipboard !== null && navigator.clipboard !== undefined) {
        await navigator.clipboard.writeText(answered.clipboard)
      }
    } catch (e) {
      if (target !== chosen.current || mine !== turn.current) return
      setFailed((e as Error).message)
    }
  }

  const later = (node: string, delay: number, run: () => void) => {
    window.clearTimeout(waiting.current[node])
    if (delay <= 0) {
      run()
      return
    }
    waiting.current[node] = window.setTimeout(run, delay)
  }

  useEffect(
    () => () => {
      for (const timer of Object.values(waiting.current)) window.clearTimeout(timer)
    },
    [],
  )

  // A fresh kind starts from the record being edited plus the declared defaults.
  useEffect(() => {
    if (!kind) return
    for (const timer of Object.values(waiting.current)) window.clearTimeout(timer)
    waiting.current = {}
    turn.current += 1
    const from = record.current
    const start = seeded(kind.document, from)
    const secrets = storedSecrets(kind.document, from)
    const withDefaults: State = {
      state: start,
      view: { mode: from === undefined ? 'new' : 'edit' },
      host: facts,
      data: kind.document.data ?? {},
      arg: null,
      stored_secrets: secrets,
      touched: [],
    }
    for (const field of kind.document.fields ?? []) {
      if (start[field.bind] !== undefined) continue
      const fallback = resolve(field.default, withDefaults)
      if (fallback !== undefined) start[field.bind] = fallback
    }
    setDoc(kind.document)
    setOverlay({ view: {}, data: kind.document.data ?? {} })
    setValues(start)
    setStored(secrets)
    setTouched([])
    setMissing([])
    if (kind.events === true) {
      const carried = withoutSecrets(start, kind.document)
      void send(kind.id, kind.document, carried, {
        type: 'opened',
        values: carried,
        touched: [],
      })
    }
  }, [kind, recordId, round, facts])

  const state: State = useMemo(
    () => ({
      state: values,
      view: { mode: creating ? 'new' : 'edit', ...overlay.view },
      host: facts,
      data: overlay.data,
      arg: null,
      stored_secrets: stored,
      touched,
    }),
    [values, touched, overlay, creating, facts, stored],
  )

  const immutable = useMemo(() => {
    if (creating) return new Set<string>()
    return new Set(doc?.immutable_after_create ?? [])
  }, [doc, creating])

  const change = (node: Node, bind: string, value: Json) => {
    if (bind === '' || immutable.has(bind)) return
    const next = { ...values, [bind]: value }
    const marked = touched.includes(bind) ? touched : [...touched, bind]
    setValues(next)
    setTouched(marked)
    if (kind?.events !== true || node.emit !== 'change' || node.id === undefined) return
    const at = node.id
    const form = doc ?? kind.document
    const carried = withoutSecrets(next, form)
    const target = kind.id
    const mine = turn.current
    later(at, node.debounce_ms ?? 0, () => {
      if (mine !== turn.current) return
      void send(target, form, carried, {
        type: 'change',
        node: at,
        bind,
        value,
        values: carried,
        touched: marked,
      })
    })
  }

  const submit = async (connect: boolean) => {
    if (!kind) return
    setSaving(true)
    setMissing([])
    try {
      const answered = await api.submitConnectionForm({
        kind: kind.id,
        values,
        touched,
        editing: initial?.name,
        connect: connect ? side : undefined,
      })
      if (answered.ok) onSaved(answered.name ?? '', connect)
      else setMissing(answered.missing ?? [])
    } catch (e) {
      setFailed((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  const activate = (at: string) => {
    if (kind?.events !== true) return
    const form = doc ?? kind.document
    const carried = withoutSecrets(values, form)
    void send(kind.id, form, carried, {
      type: 'activate',
      node: at,
      values: carried,
      touched,
    })
  }

  const act = (intent: Intent | undefined, from: Node | Action) => {
    switch (intent?.do) {
      case 'submit':
        void submit(false)
        break
      case 'connect':
        void submit(true)
        break
      case 'close':
        onCancel()
        break
      case 'revert':
        setRound((again) => again + 1)
        break
      case 'emit':
        activate((intent as { node: string }).node)
        break
      case 'set': {
        const keys = Object.entries((intent as { keys: Record<string, Json> }).keys)
        setValues((held) => {
          const next = { ...held }
          for (const [key, value] of keys) {
            if (key.startsWith('state.')) next[key.slice('state.'.length)] = value
          }
          return next
        })
        setOverlay((held) => {
          const view = { ...held.view }
          for (const [key, value] of keys) {
            if (key.startsWith('view.')) view[key.slice('view.'.length)] = value
          }
          return { ...held, view }
        })
        break
      }
      // A picker is the host's own gesture; a browser has no panel to pick from.
      case 'pick':
        break
      default:
        // An intent the host does not know reaches the plugin as the node's own activation.
        if ('t' in from && from.id !== undefined) activate(from.id)
        break
    }
  }

  const framed = (body: React.ReactNode) =>
    inline === true ? (
      <div>{body}</div>
    ) : (
      <div className="modal-overlay" onClick={onCancel}>
        <div
          className="modal-card"
          style={{ width: 'min(620px, 94vw)' }}
          onClick={(e) => e.stopPropagation()}
        >
          <h3 style={{ marginTop: 0 }}>
            {initial === undefined
              ? tr('conn_manager.add_new_connection')
              : tr('conn_manager.edit_title')}
          </h3>
          {body}
        </div>
      </div>
    )

  if (failed !== null) {
    return framed(<div style={{ color: 'var(--danger, #e05252)' }}>{failed}</div>)
  }
  if (offered === null) return null
  if (offered.kinds.length === 0) {
    return framed(<div>{tr('conn_manager.no_kinds')}</div>)
  }

  return framed(
    <>

        <label style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }}>
          <span
            style={{
              width: '130px',
              flexShrink: 0,
              textAlign: 'right',
              color: 'var(--text-dim)',
              fontSize: '13px',
            }}
          >
            {tr('conn_manager.protocol')}
          </span>
          <select
            value={kindId}
            disabled={initial !== undefined}
            onChange={(e) => choose(e.target.value)}
            style={{ flex: 1 }}
          >
            {offered.kinds.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.label}
              </option>
            ))}
          </select>
        </label>

        {doc && (
          <DocumentView
            document={doc}
            state={state}
            values={values}
            onChange={change}
            onIntent={act}
            translate={trOptional}
            assetUrl={(name) => `/api/plugin-assets/${encodeURIComponent(name)}`}
            disabled={saving}
          />
        )}

        {missing.length > 0 && (
          <div style={{ color: 'var(--danger, #e05252)', fontSize: '13px', marginTop: '8px' }}>
            {`${tr('conn_manager.still_needed')} ${missing
              .map((bind) => trOptional(`conn_manager.field_${bind}`) ?? bind)
              .join(', ')}`}
          </div>
        )}

      <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end', marginTop: '14px' }}>
        <button className="btn" onClick={onCancel} disabled={saving}>
          {tr('conn_manager.cancel_edit')}
        </button>
        <button className="btn" onClick={() => void submit(false)} disabled={saving}>
          {tr('conn_manager.save_connection')}
        </button>
        <button className="btn btn-primary" onClick={() => void submit(true)} disabled={saving}>
          {tr('conn_manager.connect_btn')}
        </button>
      </div>
    </>,
  )
}
