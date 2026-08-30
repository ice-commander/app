import type { Level } from '../api/types'

export function formatSize(size: number | null): string {
  if (size == null) return ''
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let s = size
  let i = 0
  while (s >= 1024 && i < units.length - 1) {
    s /= 1024
    i++
  }
  return `${i === 0 ? s : s.toFixed(1)} ${units[i]}`
}

/** True when at the base provider root — only the root level, nothing pushed. */
export function atRoot(levels: Level[]): boolean {
  return levels.length <= 1
}
