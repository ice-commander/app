import type { Cond, Json, Node, Pred, Series, State, Text } from './types'

/**
 * The evaluation half of `ic-view`, ported value for value from
 * app/src/common/ic-view/src/expr.rs. Function names follow the Rust ones so
 * the two can be read side by side; any change there belongs here too.
 */

const NAMESPACES = ['state', 'view', 'host', 'data', 'arg'] as const
type Namespace = (typeof NAMESPACES)[number]

export function splitReference(raw: string): [Namespace, string] | null {
  const at = raw.indexOf('.')
  if (at <= 0) return null
  const namespace = raw.slice(0, at)
  const key = raw.slice(at + 1)
  if (key === '') return null
  if (!(NAMESPACES as readonly string[]).includes(namespace)) return null
  return [namespace as Namespace, key]
}

function dig(value: Json | undefined, key: string): Json | undefined {
  let held = value
  for (const step of key.split('.')) {
    if (Array.isArray(held)) {
      held = /^[0-9]+$/.test(step) ? held[Number(step)] : undefined
    } else if (held !== null && typeof held === 'object') {
      held = Object.prototype.hasOwnProperty.call(held, step)
        ? (held as Record<string, Json>)[step]
        : undefined
    } else {
      return undefined
    }
    if (held === undefined) return undefined
  }
  return held
}

export function lookup(state: State, reference: string): Json | undefined {
  const split = splitReference(reference)
  if (!split) return undefined
  const [namespace, key] = split
  if (namespace === 'arg') return dig(state.arg, key)
  const table = state[namespace]
  return Object.prototype.hasOwnProperty.call(table, key) ? table[key] : undefined
}

export function asText(value: Json | undefined): string {
  if (value === undefined || value === null) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'boolean' || typeof value === 'number') return String(value)
  return JSON.stringify(value)
}

export function isTruthy(value: Json | undefined): boolean {
  if (value === undefined || value === null) return false
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  if (typeof value === 'string') return value !== '' && value !== 'false' && value !== '0'
  if (Array.isArray(value)) return value.length > 0
  return Object.keys(value).length > 0
}

export function valuesEqual(left: Json | undefined, right: Json | undefined): boolean {
  const l = left ?? null
  const r = right ?? null
  if (typeof l === 'number' && typeof r === 'number') return l === r
  if (l === null) return asText(r) === ''
  if (r === null) return asText(l) === ''
  if (Array.isArray(l) || Array.isArray(r) || typeof l === 'object' || typeof r === 'object') {
    return JSON.stringify(l) === JSON.stringify(r)
  }
  return asText(l) === asText(r)
}

/** A string that names a namespace is read from the state; anything else is itself. */
export function operand(raw: Json, state: State): Json {
  if (typeof raw === 'string' && splitReference(raw)) {
    return lookup(state, raw) ?? null
  }
  return raw
}

export function evaluate(predicate: Pred | undefined | null, state: State): boolean {
  if (predicate === undefined || predicate === null) return true
  if (typeof predicate === 'boolean') return predicate
  if ('eq' in predicate) {
    const values = predicate.eq.map((o) => operand(o, state))
    return values.length >= 2 && values.every((v) => valuesEqual(v, values[0]))
  }
  if ('ne' in predicate) {
    const values = predicate.ne.map((o) => operand(o, state))
    return values.length >= 2 && !values.every((v) => valuesEqual(v, values[0]))
  }
  if ('all' in predicate) return predicate.all.every((p) => evaluate(p, state))
  if ('any' in predicate) return predicate.any.some((p) => evaluate(p, state))
  if ('not' in predicate) return !evaluate(predicate.not, state)
  if ('truthy' in predicate) return isTruthy(operand(predicate.truthy, state))
  if ('empty' in predicate) return asText(operand(predicate.empty, state)) === ''
  if ('has_stored' in predicate) return state.stored_secrets.includes(predicate.has_stored)
  if ('touched' in predicate) return state.touched.includes(predicate.touched)
  if ('one_of' in predicate) {
    const value = operand(predicate.one_of.value, state)
    return predicate.one_of.of.some((candidate) => valuesEqual(value, operand(candidate, state)))
  }
  return false
}

export function visible(predicate: Pred | undefined | null, state: State): boolean {
  return evaluate(predicate, state)
}

function isCases<T>(conditional: Cond<T>): conditional is { cases: { when: Pred; then: T }[]; else?: Cond<T> } {
  return (
    typeof conditional === 'object' &&
    conditional !== null &&
    !Array.isArray(conditional) &&
    Array.isArray((conditional as { cases?: unknown }).cases)
  )
}

// A field the host left out arrives as JSON null, which serde reads back as absent.
export function resolve<T>(conditional: Cond<T> | undefined | null, state: State): T | undefined {
  if (conditional === undefined || conditional === null) return undefined
  if (!isCases(conditional)) return conditional
  for (const branch of conditional.cases) {
    if (evaluate(branch.when, state)) return branch.then
  }
  return resolve(conditional.else, state)
}

export type Translate = (key: string) => string | undefined

export function textOf(chosen: Text, translate: Translate): string {
  if (typeof chosen === 'string') return chosen
  if ('literal' in chosen) return chosen.literal
  return translate(chosen.tr) ?? chosen.en
}

/** A stored value may itself be a `Text`, so a placeholder can carry a translation. */
export function display(value: Json | undefined, translate: Translate): string {
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    const held = value as Record<string, Json>
    if (typeof held.tr === 'string' && typeof held.en === 'string') {
      return textOf({ tr: held.tr, en: held.en }, translate)
    }
    if (typeof held.literal === 'string') return held.literal
  }
  return asText(value)
}

export function substitute(template: string, state: State, translate: Translate): string {
  let rendered = ''
  let rest = template
  for (;;) {
    const open = rest.indexOf('{')
    if (open < 0) break
    rendered += rest.slice(0, open)
    const tail = rest.slice(open + 1)
    const close = tail.indexOf('}')
    if (close < 0) {
      rendered += '{'
      rest = tail
      continue
    }
    const key = tail.slice(0, close)
    const found = lookup(state, key)
    if (found !== undefined) {
      rendered += display(found, translate)
    } else if (splitReference(key)) {
      // a namespaced reference with nothing behind it renders as nothing
    } else if (Object.prototype.hasOwnProperty.call(state.state, key)) {
      rendered += display(state.state[key], translate)
    } else {
      rendered += `{${key}}`
    }
    rest = tail.slice(close + 1)
  }
  return rendered + rest
}

export function resolveText(
  conditional: Cond<Text> | undefined,
  state: State,
  translate: Translate,
): string | undefined {
  const chosen = resolve(conditional, state)
  if (chosen === undefined) return undefined
  const text = textOf(chosen, translate)
  return text.includes('{') ? substitute(text, state, translate) : text
}

export function choiceIndex(node: Node, current: Json | undefined): number | undefined {
  const options = node.options ?? []
  const insensitive = node.decode?.case_insensitive ?? false
  const matches = (candidate: Json) =>
    valuesEqual(candidate, current) ||
    (insensitive && asText(candidate).toLowerCase() === asText(current).toLowerCase())
  const at = options.findIndex((option) => matches(option.value))
  if (at >= 0) return at
  const fallback = node.decode?.unknown
  if (fallback === undefined) return undefined
  const other = options.findIndex((option) => valuesEqual(option.value, fallback))
  return other >= 0 ? other : undefined
}

export function seriesOf(state: State, key: string): Series[] {
  const raw = state.data[key]
  if (!Array.isArray(raw)) return []
  return raw as unknown as Series[]
}

export function seriesBounds(series: Series[], node: Node): [number, number] {
  let low = node.min ?? Number.MAX_VALUE
  let high = node.max ?? -Number.MAX_VALUE
  if (node.min === undefined || node.max === undefined) {
    for (const line of series) {
      for (const value of line.values ?? []) {
        if (node.min === undefined && value < low) low = value
        if (node.max === undefined && value > high) high = value
      }
    }
  }
  if (!Number.isFinite(low) || low === Number.MAX_VALUE) low = 0
  if (!Number.isFinite(high) || high === -Number.MAX_VALUE) high = low + 1
  if (Math.abs(high - low) < Number.EPSILON) high = low + 1
  return [low, high]
}

export function walk(node: Node, visit: (node: Node) => void): void {
  visit(node)
  for (const child of node.children ?? []) walk(child, visit)
}

/** What a node writes to, for a reply that names widgets by node id. */
export function bindOfNode(form: Node, id: string): string | undefined {
  let found: string | undefined
  walk(form, (node) => {
    if (node.id === id && node.bind !== undefined) found = node.bind
  })
  return found
}

export function fieldOf(document: { fields?: { bind: string }[] }, bind: string) {
  return document.fields?.find((field) => field.bind === bind)
}
