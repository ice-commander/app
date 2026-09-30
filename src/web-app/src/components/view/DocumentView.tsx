import { useEffect, useState } from 'react'
import type { Action, Document, Intent, Json, Node, Series, State, Text } from '../../lib/view/types'
import {
  asText,
  choiceIndex,
  display,
  isTruthy,
  resolve,
  resolveText,
  seriesBounds,
  seriesOf,
  textOf,
  visible,
  walk,
  type Translate,
} from '../../lib/view/expr'

/**
 * Renders a plugin's declarative document. The application holds no table of
 * protocols or fields: every row, label, condition and option below comes from
 * the document the plugin sent.
 */

export interface DocumentViewProps {
  document: Document
  /** The four namespaces the predicates read, as the host last reported them. */
  state: State
  /** What the widgets currently hold, keyed by bind. */
  values: Record<string, Json>
  onChange: (node: Node, bind: string, value: Json) => void
  onIntent: (intent: Intent | undefined, node: Node | Action) => void
  translate: Translate
  /** Where a plugin-supplied `asset:` icon is served from. */
  assetUrl: (name: string) => string
  /**
   * Where a picture named in the document is served from — `file:<path>` for
   * something on the filesystem the window was opened on, `part:<name>` for
   * something the plugin makes. Undefined when there is nothing to show.
   */
  pictureUrl?: (src: string) => string | undefined
  disabled?: boolean
}

const ROW: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: '10px',
  marginBottom: '8px',
}
const LABEL: React.CSSProperties = {
  width: '130px',
  flexShrink: 0,
  color: 'var(--text-dim)',
  fontSize: '13px',
  textAlign: 'right',
}

function roleStyle(role: string | undefined): React.CSSProperties {
  switch (role) {
    case 'title1':
      return { fontSize: '20px', fontWeight: 700 }
    case 'title2':
      return { fontSize: '17px', fontWeight: 600 }
    case 'heading':
      return { fontSize: '14px', fontWeight: 600 }
    case 'caption':
    case 'dim':
      return { color: 'var(--text-dim)', fontSize: '12px' }
    case 'mono':
      return { fontFamily: 'var(--mono, monospace)' }
    case 'error':
      return { color: 'var(--danger, #e05252)' }
    default:
      return {}
  }
}

function buttonClass(role: string | undefined): string {
  if (role === 'primary') return 'btn btn-primary'
  if (role === 'destructive') return 'btn btn-danger'
  if (role === 'row') return 'view-row'
  if (role === 'row_selected') return 'view-row selected'
  return 'btn'
}

function bringIntoView(row: HTMLButtonElement | null) {
  row?.scrollIntoView({ block: 'center' })
}

const NAMED_KEYS: Record<string, string> = {
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
  ' ': 'space',
  Enter: 'return',
  Escape: 'escape',
  Backspace: 'backspace',
  Delete: 'delete',
  Tab: 'tab',
  Home: 'home',
  End: 'end',
  PageUp: 'page_up',
  PageDown: 'page_down',
}

function pressedAsWritten(e: KeyboardEvent): string {
  let said = ''
  if (e.ctrlKey) said += 'ctrl+'
  if (e.altKey) said += 'alt+'
  if (e.shiftKey) said += 'shift+'
  return said + (NAMED_KEYS[e.key] ?? e.key.toLowerCase())
}

function beingTypedInto(): boolean {
  const focused = document.activeElement as HTMLElement | null
  const tag = focused?.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || focused?.isContentEditable === true
}

function useAccelerators(props: DocumentViewProps) {
  const { document: doc, state, onIntent, disabled } = props
  useEffect(() => {
    const bound = new Map<string, Node>()
    walk(doc.form, (node) => {
      if (node.accel === undefined || node.t !== 'button') return
      if (!visible(node.visible, state) || !visible(node.sensitive, state)) return
      bound.set(node.accel.toLowerCase(), node)
    })
    if (bound.size === 0) return
    const onKey = (e: KeyboardEvent) => {
      if (disabled === true || beingTypedInto()) return
      const node = bound.get(pressedAsWritten(e))
      if (node === undefined) return
      e.preventDefault()
      e.stopPropagation()
      onIntent(resolve(node.intent, state), node)
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [doc, state, onIntent, disabled])
}

function px(value: number | undefined): string | undefined {
  return value === undefined ? undefined : `${value}px`
}

export function DocumentView(props: DocumentViewProps) {
  const doc = props.document
  const offered = (doc.actions ?? []).filter((action) => visible(action.visible, props.state))
  useAccelerators(props)
  return (
    <div
      className="view-document"
      style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}
    >
      <NodeView node={doc.form} {...props} />
      {offered.length > 0 && (
        <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end', marginTop: '12px' }}>
          {offered.map((action) => (
            <button
              key={action.id}
              className={buttonClass(resolveText(action.role, props.state, props.translate))}
              disabled={props.disabled || !visible(action.sensitive, props.state)}
              onClick={() => props.onIntent(resolve(action.intent, props.state), action)}
            >
              {resolveText(action.label, props.state, props.translate) ?? action.id}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function NodeView({ node, ...rest }: DocumentViewProps & { node: Node }) {
  const { state, values, translate, onChange, onIntent, assetUrl, pictureUrl } = rest
  if (!visible(node.visible, state)) return null
  const live = !rest.disabled && visible(node.sensitive, state)
  const text = (conditional: Parameters<typeof resolveText>[0]) =>
    resolveText(conditional, state, translate)
  const held = node.bind === undefined ? undefined : values[node.bind]
  const grows = node.weight !== undefined && node.weight > 0
  const lead: React.CSSProperties = {
    marginTop: px(node.margin_top),
    flex: grows ? `${node.weight} 1 0` : undefined,
    minHeight: grows ? 0 : undefined,
  }
  const kids = node.children ?? []

  switch (node.t) {
    case 'view':
    case 'column': {
      const scrolls = node.scroll !== undefined && node.scroll !== 'none'
      return (
        <div
          style={{
            ...lead,
            display: 'flex',
            flexDirection: 'column',
            gap: px(node.spacing),
            padding: px(node.padding),
            // A box that says it scrolls, scrolls — a playlist is a column.
            overflowY: scrolls ? 'auto' : undefined,
            overflowX: node.scroll === 'both' ? 'auto' : undefined,
            flex: node.t === 'view' ? '1 1 auto' : lead.flex,
            minHeight: node.t === 'view' || scrolls || grows ? 0 : undefined,
          }}
        >
          {kids.map((child, at) => (
            <NodeView key={child.id ?? at} node={child} {...rest} />
          ))}
        </div>
      )
    }

    case 'row':
      return (
        <div
          style={{ ...lead, display: 'flex', alignItems: 'center', gap: px(node.spacing) ?? '10px' }}
        >
          {kids.map((child, at) => (
            <div
              key={child.id ?? at}
              style={{
                flex:
                  child.width !== undefined
                    ? `0 0 ${child.width}px`
                    : child.weight === undefined
                      ? '0 0 auto'
                      : `${child.weight} 1 0`,
                minWidth: 0,
              }}
            >
              <NodeView node={child} {...rest} />
            </div>
          ))}
        </div>
      )

    case 'group':
      return (
        <fieldset
          style={{
            ...lead,
            border: '1px solid var(--border, #3a3a3a)',
            borderRadius: '6px',
            padding: '10px 12px',
            minWidth: 0,
          }}
        >
          {text(node.title) !== undefined && (
            <legend style={{ padding: '0 6px', fontSize: '13px', color: 'var(--text-dim)' }}>
              {text(node.title)}
            </legend>
          )}
          {kids.map((child, at) => (
            <NodeView key={child.id ?? at} node={child} {...rest} />
          ))}
        </fieldset>
      )

    case 'separator':
      return <hr style={{ ...lead, border: 0, borderTop: '1px solid var(--border, #3a3a3a)' }} />

    case 'text':
      return (
        <div
          style={{
            ...lead,
            ...roleStyle(text(node.role)),
            whiteSpace: node.wrap ? 'pre-wrap' : 'nowrap',
            overflow: node.wrap ? undefined : 'hidden',
            textOverflow: node.wrap ? undefined : 'ellipsis',
          }}
          title={text(node.tooltip)}
        >
          {text(node.text)}
        </div>
      )

    case 'icon': {
      const name = text(node.icon)
      if (name === undefined || !name.startsWith('asset:')) return null
      // A `theme:` name is a GTK icon name; the browser has no such set.
      const size = node.width ?? 16
      return (
        <img
          src={assetUrl(name.slice('asset:'.length))}
          alt=""
          width={size}
          height={size}
          style={lead}
        />
      )
    }

    case 'image':
    case 'media': {
      const named = text(node.src)
      const where = named === undefined ? undefined : pictureUrl?.(named)
      if (where === undefined) {
        // A picture is named, never carried, so there is nothing to draw
        // until the host says where it is.
        return (
          <div style={{ ...lead, opacity: 0.6 }}>{named?.replace(/^(file|part):/, '') ?? ''}</div>
        )
      }
      const room: React.CSSProperties = {
        ...lead,
        maxWidth: '100%',
        objectFit: node.fit === 'cover' ? 'cover' : node.fit === 'actual' ? 'none' : 'contain',
        width: node.fit === 'width' ? '100%' : px(node.width),
        height: px(node.height),
      }
      if (node.t === 'media') {
        return node.media === 'audio' ? (
          <audio src={where} controls autoPlay={node.autoplay} style={{ ...lead, width: '100%' }} />
        ) : (
          <video src={where} controls autoPlay={node.autoplay} style={room} />
        )
      }
      return <img src={where} alt="" style={room} />
    }

    case 'input':
      return (
        <Input
          node={node}
          value={asText(held)}
          live={live}
          title={text(node.title)}
          placeholder={text(node.placeholder)}
          tooltip={text(node.tooltip)}
          lead={lead}
          onChange={(next) => onChange(node, node.bind ?? '', next)}
        />
      )

    case 'slider': {
      const low = node.min ?? 0
      const high = node.max !== undefined && node.max > low ? node.max : low + 1
      // Where it sits is the plugin's to say while something is going on, and
      // the state's otherwise — the same rule a table's rows follow.
      const shown =
        node.value_key !== undefined ? Number(state.data[node.value_key] ?? low) : Number(held ?? low)
      return (
        <input
          type="range"
          min={low}
          max={high}
          step={node.step ?? (high - low) / 100}
          value={Number.isFinite(shown) ? shown : low}
          disabled={!live || node.read_only}
          title={text(node.tooltip)}
          style={{ ...lead, width: node.width === undefined ? '100%' : px(node.width) }}
          onChange={(e) => onChange(node, node.bind ?? '', Number(e.target.value))}
        />
      )
    }

    case 'switch':
      return (
        <label style={{ ...ROW, ...lead }} title={text(node.tooltip)}>
          <span style={LABEL}>{text(node.title)}</span>
          <input
            type="checkbox"
            checked={isTruthy(held)}
            disabled={!live || node.read_only}
            onChange={(e) => onChange(node, node.bind ?? '', e.target.checked)}
          />
        </label>
      )

    case 'choice': {
      const options = node.options ?? []
      const at = choiceIndex(node, held)
      return (
        <label style={{ ...ROW, ...lead }} title={text(node.tooltip)}>
          <span style={LABEL}>{text(node.title)}</span>
          <select
            value={at === undefined ? '' : String(at)}
            disabled={!live || node.read_only}
            onChange={(e) => {
              const picked = options[Number(e.target.value)]
              if (picked) onChange(node, node.bind ?? '', picked.value)
            }}
            style={{ flex: 1, minWidth: 0 }}
          >
            {at === undefined && <option value="">{asText(held)}</option>}
            {options.map((option, index) => (
              <option key={index} value={String(index)}>
                {option.label === undefined
                  ? asText(option.value)
                  : textOf(option.label as Text, translate)}
              </option>
            ))}
          </select>
        </label>
      )
    }

    case 'button': {
      const role = text(node.role)
      return (
        <button
          ref={role === 'row_selected' ? bringIntoView : undefined}
          className={buttonClass(role)}
          style={lead}
          disabled={!live}
          title={text(node.tooltip)}
          onClick={() => onIntent(resolve(node.intent, state), node)}
        >
          {text(node.label) ?? text(node.text) ?? node.id}
        </button>
      )
    }

    case 'table': {
      const columns = node.columns ?? []
      const raw = node.rows_key === undefined ? undefined : state.data[node.rows_key]
      const rows = Array.isArray(raw) ? (raw as Record<string, Json>[]) : []
      return (
        <div style={{ ...lead, overflow: 'auto', maxHeight: px(node.height) }}>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px' }}>
            <thead>
              <tr>
                {columns.map((column) => (
                  <th
                    key={column.key}
                    style={{
                      textAlign: 'left',
                      color: 'var(--text-dim)',
                      borderBottom: '1px solid var(--border, #3a3a3a)',
                      padding: '4px 6px',
                      width: px(column.width),
                    }}
                  >
                    {column.title === undefined
                      ? column.key
                      : textOf(column.title as Text, translate)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, at) => (
                <tr key={at}>
                  {columns.map((column) => (
                    <td key={column.key} style={{ padding: '3px 6px' }}>
                      {display(row[column.key], translate)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    }

    case 'chart': {
      const series = node.series_key === undefined ? [] : seriesOf(state, node.series_key)
      return (
        <div style={lead}>
          <Chart node={node} series={series} />
          {text(node.caption) !== undefined && (
            <div style={{ color: 'var(--text-dim)', fontSize: '12px' }}>{text(node.caption)}</div>
          )}
        </div>
      )
    }

    default:
      return null
  }
}

function Input(props: {
  node: Node
  value: string
  live: boolean
  title?: string
  placeholder?: string
  tooltip?: string
  lead: React.CSSProperties
  onChange: (value: Json) => void
}) {
  const { node, value, live, onChange } = props
  const [revealed, setRevealed] = useState(false)
  const variant = node.variant ?? 'text'
  const masked = variant === 'masked' || (variant === 'masked_reveal' && !revealed)

  if (variant === 'multiline') {
    return (
      <div style={props.lead} title={props.tooltip}>
        {props.title !== undefined && (
          <div style={{ ...LABEL, width: 'auto', textAlign: 'left', marginBottom: '4px' }}>
            {props.title}
          </div>
        )}
        <textarea
          value={value}
          placeholder={props.placeholder}
          disabled={!live || node.read_only}
          rows={Math.max(3, Math.round((node.height ?? 90) / 18))}
          style={{ width: '100%', resize: 'vertical' }}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    )
  }

  return (
    <label style={{ ...ROW, ...props.lead }} title={props.tooltip}>
      <span style={LABEL}>{props.title}</span>
      <input
        value={value}
        placeholder={props.placeholder}
        disabled={!live || node.read_only}
        style={{ flex: 1, minWidth: 0 }}
        type={masked ? 'password' : variant === 'integer' ? 'number' : 'text'}
        min={node.min}
        max={node.max}
        onChange={(e) => onChange(e.target.value)}
      />
      {variant === 'masked_reveal' && (
        <button
          type="button"
          className="btn"
          onClick={() => setRevealed(!revealed)}
          style={{ flexShrink: 0 }}
        >
          {revealed ? '●' : '○'}
        </button>
      )}
    </label>
  )
}

const HUES = ['#4aa3df', '#5ac878', '#e0c14a', '#c36ad0']

function Chart({ node, series }: { node: Node; series: Series[] }) {
  const width = node.width ?? 320
  const height = node.height ?? 90
  const [low, high] = seriesBounds(series, node)
  const span = high - low
  return (
    <svg
      width="100%"
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      role="img"
    >
      {series.map((line, index) => {
        const points = line.values ?? []
        if (points.length < 2) return null
        const step = width / (points.length - 1)
        const path = points
          .map((value, at) => {
            const y = height - ((value - low) / span) * height
            return `${at === 0 ? 'M' : 'L'}${(at * step).toFixed(1)},${y.toFixed(1)}`
          })
          .join(' ')
        const stroke = line.color
          ? `rgba(${Math.round(line.color[0] * 255)},${Math.round(line.color[1] * 255)},${Math.round(line.color[2] * 255)},${line.color[3]})`
          : HUES[index % HUES.length]
        return <path key={index} d={path} fill="none" strokeWidth={1.5} stroke={stroke} />
      })}
    </svg>
  )
}
