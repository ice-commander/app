// Extension-based file icons, ported from the GTK app so web and desktop agree on
// file types, colors, and labels:
//   fm-ui/src/icon_generator.rs (FileType + colors + SVG template)
//   fm-ui/src/utils.rs::get_file_icon (extension → type/text mapping)
//
// A colored "page with a folded corner" SVG with the extension written on it,
// generated on demand and cached — mirroring the Rust GLOBAL_SVG_CACHE. Returned
// as a `data:` URI for use in <img src>. The one intentional divergence: the REST
// API carries no Unix permissions, so the GTK "executable → BIN" fallback for
// unknown extensions isn't available here (such files get the generic icon).

type FileType = 'executable' | 'developer' | 'media' | 'photo' | 'document' | 'archive' | 'configText'

// (bg, border, fold, text) — identical hex values to FileType::colors() in Rust.
const COLORS: Record<FileType, [string, string, string, string]> = {
  executable: ['#f78f8f', '#c74343', '#ffffff', '#ffffff'],
  developer: ['#bae0bd', '#5e9c76', '#ffffff', '#5e9c76'],
  media: ['#ffeea3', '#ba9b48', '#ffffff', '#ba9b48'],
  photo: ['#869ce8', '#4e64b5', '#ffffff', '#ffffff'],
  document: ['#ffffff', '#c74343', '#ffd9d9', '#c74343'],
  archive: ['#ffc49c', '#a16a4a', '#ffffff', '#a16a4a'],
  configText: ['#dcd5f2', '#8b75a1', '#ffffff', '#8b75a1'],
}

// Trim to 4 chars then upper-case (matches the Rust `ext[..4]` clamp).
const label = (s: string) => (s.length > 4 ? s.slice(0, 4) : s).toUpperCase()

/** Map a file name to (type, label), or null if the extension isn't recognized. */
function classify(name: string): [FileType, string] | null {
  const lower = name.toLowerCase()
  if (lower.endsWith('.tar.gz') || lower.endsWith('.tar.bz2')) return ['archive', 'TAR']
  const ext = lower.includes('.') ? lower.slice(lower.lastIndexOf('.') + 1) : ''
  switch (ext) {
    case 'zip': case 'rar': case '7z': case 'tar': case 'gz': case 'tgz':
    case 'bz2': case 'tbz2': case 'tbz':
      return ['archive', ext === '7z' ? '7Z' : ext.toUpperCase()]
    case 'pdf': case 'doc': case 'docx': case 'xls': case 'xlsx': case 'ppt': case 'pptx':
      return ['document', label(ext)]
    case 'png': case 'jpg': case 'jpeg': case 'gif': case 'webp': case 'svg':
      return ['photo', ext === 'jpeg' ? 'JPG' : ext.toUpperCase()]
    case 'nef': case 'cr2': case 'cr3': case 'arw': case 'dng': case 'raf': case 'orf': case 'rw2': case 'pef':
      return ['photo', 'RAW']
    case 'rs': case 'js': case 'ts': case 'html': case 'css': case 'go': case 'py': case 'cpp': case 'c': case 'h': case 'cs': case 'java':
      return ['developer', label(ext)]
    case 'mp3': case 'wav': case 'ogg': case 'flac': case 'mp4': case 'mkv': case 'avi': case 'mov':
      return ['media', label(ext)]
    case 'txt': case 'ini': case 'conf': case 'toml': case 'yaml': case 'yml': case 'json':
      return ['configText', label(ext)]
    case 'apk': return ['executable', 'APK']
    case 'exe': return ['executable', 'EXE']
    case 'dll': return ['developer', 'DLL']
    default: return null
  }
}

function buildSvg(text: string, type: FileType): string {
  const [bg, border, fold, textColor] = COLORS[type]
  // Font size by label length, matching the Rust thresholds.
  const fontSize = text.length <= 3 ? '19px' : text.length === 4 ? '15px' : '12px'
  // Clean solid text (the GTK small-size branch); crisp at any rendered size.
  const textEl = `<text x="40" y="49" fill="${textColor}" font-family="sans-serif" font-weight="900" font-size="${fontSize}" text-anchor="middle" text-rendering="geometricPrecision">${text}</text>`
  return (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80" width="80" height="80">' +
    `<path fill="${bg}" d="M12.5 75.5L12.5 4.5 49.793 4.5 67.5 22.207 67.5 75.5z"/>` +
    `<path fill="${border}" d="M49.586,5L67,22.414V75H13V5H49.586 M50,4H12v72h56V22L50,4L50,4z"/>` +
    `<path fill="${fold}" d="M49.5 22.5L49.5 4.5 49.793 4.5 67.5 22.207 67.5 22.5z"/>` +
    `<path fill="${border}" d="M50,5.414L66.586,22H50V5.414 M50,4h-1v19h19v-1L50,4L50,4z"/>` +
    textEl +
    '</svg>'
  )
}

const cache = new Map<string, string>()

/**
 * Data-URI icon for a file name, or `null` when the extension isn't recognized
 * (the caller then falls back to the generic file icon). Directories are handled
 * by the caller, not here.
 */
export function generatedFileIcon(name: string): string | null {
  const c = classify(name)
  if (!c) return null
  const [type, text] = c
  const key = `${type}:${text}`
  let uri = cache.get(key)
  if (!uri) {
    uri = 'data:image/svg+xml,' + encodeURIComponent(buildSvg(text, type))
    cache.set(key, uri)
  }
  return uri
}
