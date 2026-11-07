import { createContext, useContext, useEffect, useRef } from 'react'

export interface KeyTarget {
  addEventListener(type: 'keydown', listener: (e: KeyboardEvent) => void, capture?: boolean): void
  removeEventListener(type: 'keydown', listener: (e: KeyboardEvent) => void, capture?: boolean): void
}

export const KeyScope = createContext<KeyTarget | null>(window)

const layers = new WeakMap<KeyTarget, object[]>()

interface EscapeOptions {
  open?: boolean
  scope?: KeyTarget | null
  inPanel?: boolean
}

export function useEscape(onEscape: () => void, { open = true, scope: own, inPanel = false }: EscapeOptions = {}): void {
  const inherited = useContext(KeyScope)
  const scope = own === undefined ? inherited : own
  const latest = useRef(onEscape)
  useEffect(() => {
    latest.current = onEscape
  })
  useEffect(() => {
    if (scope === null || !open) return
    const stack = layers.get(scope) ?? []
    layers.set(scope, stack)
    const layer = {}
    stack.push(layer)
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || stack[stack.length - 1] !== layer) return
      // A panel's layer leaves the terminals uncovered, and Escape typed into one belongs to its shell.
      if (inPanel && e.target instanceof Element && e.target.closest('.xterm') !== null) return
      e.preventDefault()
      e.stopPropagation()
      latest.current()
    }
    scope.addEventListener('keydown', onKey, true)
    return () => {
      scope.removeEventListener('keydown', onKey, true)
      stack.splice(stack.indexOf(layer), 1)
    }
  }, [scope, open, inPanel])
}

export function beingTypedInto(): boolean {
  const focused = document.activeElement as HTMLElement | null
  const tag = focused?.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || focused?.isContentEditable === true
}

const SCROLLING = new Set([' ', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Home', 'End'])
const PRESSED = new Set(['button', 'checkbox', 'color', 'file', 'image', 'reset', 'submit'])

function usesKey(el: EventTarget | null, key: string): boolean {
  const paging = key === 'PageUp' || key === 'PageDown'
  if (el instanceof HTMLSelectElement) return true
  if (el instanceof HTMLInputElement) {
    if (el.type === 'range') return true
    if (el.type === 'radio') return key === ' ' || key.startsWith('Arrow')
    return PRESSED.has(el.type) ? key === ' ' : !paging
  }
  if (el instanceof HTMLTextAreaElement && el.readOnly) return key.startsWith('Arrow')
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLMediaElement) return !paging
  if (el instanceof HTMLElement && el.isContentEditable) return !paging
  return key === ' ' && el instanceof HTMLButtonElement
}

export function holdPage(e: KeyboardEvent): void {
  if (!SCROLLING.has(e.key) || e.altKey || e.metaKey || e.defaultPrevented || usesKey(e.target, e.key)) return
  e.preventDefault()
  const text = e.target
  if (!(text instanceof HTMLTextAreaElement)) return
  // Chrome pages a textarea by aligning the caret to the viewport top, and past its edge scrolls the page instead.
  if (e.key === 'Home') text.scrollTop = 0
  else if (e.key === 'End') text.scrollTop = text.scrollHeight
  else text.scrollTop += (e.key === 'PageUp' || (e.key === ' ' && e.shiftKey) ? -1 : 1) * text.clientHeight
}

export function holdFocus(root: HTMLElement): () => void {
  let held = true
  const regain = () => {
    if (held && document.activeElement === document.body) root.focus({ preventScroll: true })
  }
  const enter = () => {
    held = true
  }
  const leave = (e: FocusEvent) => {
    const left = e.target as Element
    // A control being removed (still attached when this fires) or disabled drops focus to body; that is not leaving.
    queueMicrotask(() => {
      held = !left.isConnected || left.matches(':disabled') || root.contains(document.activeElement)
      regain()
    })
  }
  const keep = new MutationObserver(regain)
  root.addEventListener('focusin', enter)
  root.addEventListener('focusout', leave)
  keep.observe(root, { childList: true, subtree: true })
  root.focus({ preventScroll: true })
  return () => {
    keep.disconnect()
    root.removeEventListener('focusin', enter)
    root.removeEventListener('focusout', leave)
  }
}
