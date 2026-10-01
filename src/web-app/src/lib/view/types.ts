/**
 * The declarative document a plugin sends, mirrored from the Rust `ic-view`
 * crate (app/src/common/ic-view/src/doc.rs). The wire format is that crate's
 * serde output, so these types follow its tagging exactly:
 *   Cond<T>  — untagged: either `{ cases: [...], else?: ... }` or a bare T
 *   Pred     — untagged: either a boolean or a one-key operator object
 *   PredOp   — externally tagged, snake_case
 *   Intent   — internally tagged on `do`, snake_case
 */

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }

export type Text = string | { tr: string; en: string } | { literal: string }

export interface Case<T> {
  when: Pred
  then: T
}

export type Cond<T> = T | { cases: Case<T>[]; else?: Cond<T> }

export type Pred =
  | boolean
  | { eq: Json[] }
  | { ne: Json[] }
  | { all: Pred[] }
  | { any: Pred[] }
  | { not: Pred }
  | { truthy: Json }
  | { empty: Json }
  | { has_stored: string }
  | { touched: string }
  | { one_of: { value: Json; of: Json[] } }

export type Intent =
  | { do: 'submit' }
  | { do: 'connect' }
  | { do: 'close' }
  | { do: 'revert' }
  | { do: 'set'; keys: Record<string, Json> }
  | { do: 'pick'; node: string }
  | { do: 'emit'; node: string }
  | { do: string }

export type NodeKind =
  | 'view'
  | 'column'
  | 'row'
  | 'group'
  | 'separator'
  | 'text'
  | 'icon'
  | 'input'
  | 'switch'
  | 'choice'
  | 'button'
  | 'table'
  | 'chart'
  | 'image'
  | 'media'
  | 'canvas'
  | 'slider'

export type InputVariant = 'text' | 'masked' | 'masked_reveal' | 'integer' | 'path' | 'multiline'

export interface Choice {
  value: Json
  label?: Text
}

export interface Column {
  key: string
  title?: Text
  width?: number
}

export interface Series {
  label?: Text
  color?: [number, number, number, number]
  values: number[]
}

export interface Node {
  t: string
  id?: string
  bind?: string
  children?: Node[]
  title?: Cond<Text>
  subtitle?: Cond<Text>
  text?: Cond<Text>
  placeholder?: Cond<Text>
  tooltip?: Cond<Text>
  caption?: Cond<Text>
  label?: Cond<Text>
  icon?: Cond<Text>
  role?: Cond<Text>
  visible?: Pred
  sensitive?: Pred
  chrome?: 'bare' | 'row'
  variant?: InputVariant
  emit?: 'commit' | 'change'
  scroll?: 'none' | 'vertical' | 'both'
  surface?: 'embedded' | 'dialog' | 'window' | 'panel'
  options?: Choice[]
  columns?: Column[]
  rows_key?: string
  series_key?: string
  min?: number
  max?: number
  decode?: { case_insensitive?: boolean; unknown?: Json }
  picker?: { mode?: 'file' | 'folder'; title?: Text }
  intent?: Cond<Intent>
  accel?: string
  weight?: number
  width?: number
  height?: number
  spacing?: number
  padding?: number
  margin_top?: number
  debounce_ms?: number
  refresh_ms?: number
  selectable?: boolean
  read_only?: boolean
  wrap?: boolean
  /** Where a picture or a piece of media is: `file:<path>` or `part:<name>`. */
  src?: Cond<Text>
  fit?: 'contain' | 'cover' | 'actual' | 'width'
  zoom?: boolean
  autoplay?: boolean
  media?: 'audio' | 'video'
  /** Where a slider reads its position, in `data`. Without it a slider follows
   *  what the state holds under its `bind`, which is what a form wants. */
  value_key?: string
  /** How far a slider moves in one step. Absent leaves it to the browser. */
  step?: number
}

export interface Field {
  bind: string
  type?: 'text' | 'integer' | 'bool' | 'path'
  required?: boolean
  secret?: boolean
  scope?: 'settings' | 'record'
  when?: Pred
  default?: Cond<Json>
  min?: number
  max?: number
}

export interface Action {
  id: string
  label?: Cond<Text>
  role?: Cond<Text>
  visible?: Pred
  sensitive?: Pred
  intent?: Cond<Intent>
}

/** A key that reaches the plugin as `activate` of `node`, with nothing on screen to press. */
export interface Key {
  accel: string
  node: string
}

export interface Document {
  schema: number
  kind: string
  label?: Text
  icon?: Text
  identity?: string
  immutable_after_create?: string[]
  fields?: Field[]
  data?: Record<string, Json>
  form: Node
  actions?: Action[]
  keys?: Key[]
}

/** The namespaces a reference may address, as `ic-view` names them. */
export interface State {
  state: Record<string, Json>
  view: Record<string, Json>
  host: Record<string, Json>
  data: Record<string, Json>
  /** Whatever the view was opened with, whole: `arg` alone is not flat. */
  arg: Json
  stored_secrets: string[]
  touched: string[]
}

export const EMPTY_STATE: State = {
  state: {},
  view: {},
  host: {},
  data: {},
  arg: null,
  stored_secrets: [],
  touched: [],
}

/** One connection kind a plugin declared. */
export interface ConnectionKind {
  id: string
  label: string
  document: Document
  /** Whether the plugin behind it hears what the form does. */
  events?: boolean
}

/** What `GET /api/connections/kinds` answers. */
export interface ConnectionKinds {
  /** The host's own facts, so `host.*` reads the same here as it does natively. */
  host: { kind: string; locale: string; can: string[] }
  kinds: ConnectionKind[]
}

export interface ViewerOpened {
  instance: number
  viewer: string
  document: Document
}

export interface ViewerAnswer {
  answer: Record<string, Json>
  document: Document | null
}

export interface ViewerClosed {
  closed: boolean
  document?: Document | null
}

/** What the host answers after opening a view or handing it an event. */
export interface ViewSnapshot {
  view: string
  document: Document
  state: State
  put: Record<string, Json> | null
  clipboard: string | null
}
