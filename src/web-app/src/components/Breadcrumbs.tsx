import type { Level } from '../api/types'
import { tr } from "../lib/i18n";
import homeIcon from '../assets/home.svg'
import folderIcon from '../assets/folder.svg'
import startIcon from '../assets/start.svg'
import { driveIcon } from '../lib/driveIcons'

const SEG_ICON = { width: '14px', height: '14px', objectFit: 'contain' as const }

interface Props {
  levels: Level[]
  // flat level index (0 = provider root)
  onBreadcrumb: (level: number) => void
  // flag at the head of the breadcrumbs → panel start state (sources selector)
  onStart?: () => void
}

// Flag button that opens the panel start state — first element of the breadcrumbs,
// to the left of the home/root icon. Home goes to the drive root, the flag to the start.
function StartFlag({ onStart }: { onStart?: () => void }) {
  if (!onStart) return null
  return (
    <div
      className="breadcrumb-home"
      onClick={onStart}
      title={tr("webpult.panel_start")}
      style={{ display: 'inline-flex', alignItems: 'center' }}
    >
      <img src={startIcon} alt="start" style={{ width: '14px', height: '14px', marginRight: '5px' }} />
    </div>
  )
}

export function Breadcrumbs({ levels, onBreadcrumb, onStart }: Props) {
  if (levels.length === 0) {
    return (
      <div className="breadcrumbs-inner">
        <StartFlag onStart={onStart} />
        <div className="breadcrumb-home" style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
          <img src={homeIcon} alt="/" style={{ width: '14px', height: '14px' }} />
          <span style={{ color: 'var(--text-dim)' }}>{tr("fm.root_tooltip")}</span>
        </div>
      </div>
    )
  }

  const root = levels[0]
  const last = levels.length - 1

  return (
    <div className="breadcrumbs-inner">
      <StartFlag onStart={onStart} />
      {/* filesystem root / remote mount chip */}
      <div
        className="breadcrumb-home"
        onClick={() => onBreadcrumb(0)}
        style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}
      >
        <img
          src={root.label ? driveIcon(root.icon ?? '') : homeIcon}
          alt="/"
          style={{ width: '14px', height: '14px' }}
        />
        {root.label && <span style={{ color: 'var(--color-star)' }}>{root.label}</span>}
      </div>

      {levels.slice(1).map((lvl, i) => {
        const idx = i + 1
        const isLast = idx === last
        // archive boundary: a starred ⊞ chip whose click enters the archive root
        if (lvl.is_archive) {
          return (
            <span key={idx} style={{ display: 'inline-flex', alignItems: 'center' }}>
              <span className="breadcrumb-separator" style={{ color: 'var(--color-star)' }}>⊞</span>
              <span
                className="breadcrumb-item"
                onClick={() => !isLast && onBreadcrumb(idx)}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '4px',
                  color: 'var(--color-star)',
                  ...(isLast ? { fontWeight: 500 } : {}),
                }}
              >
                {lvl.name}
              </span>
            </span>
          )
        }
        return (
          <span key={idx} style={{ display: 'inline-flex', alignItems: 'center', gap: '4px' }}>
            <span className="breadcrumb-separator">/</span>
            <span
              className="breadcrumb-item"
              onClick={() => !isLast && onBreadcrumb(idx)}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: '6px',
                ...(isLast ? { color: 'var(--text-main)', fontWeight: 500 } : {}),
              }}
            >
              <img src={folderIcon} alt="" style={SEG_ICON} />
              <span>{lvl.name}</span>
            </span>
          </span>
        )
      })}
    </div>
  )
}
