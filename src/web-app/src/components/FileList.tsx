import { useEffect, useRef } from 'react'
import { tr } from "../lib/i18n";
import type { FileEntry } from '../api/types'
import { formatSize } from '../lib/format'
import { isArchive } from '../lib/archive'
import { generatedFileIcon } from '../lib/fileIcon'
import folderIcon from '../assets/folder.svg'
import fileIcon from '../assets/file.svg'

export type ViewMode = 'list' | 'medium' | 'tiles'

export interface SelectMods {
  ctrl: boolean
  shift: boolean
}

interface Props {
  entries: FileEntry[]
  viewMode: ViewMode
  selectedNames: string[]
  showUp: boolean
  onSelect: (name: string, mods: SelectMods) => void
  onOpenDir: (name: string) => void
  onOpenFile: (entry: FileEntry) => void
  onUp: () => void
}

interface Row {
  name: string
  is_dir: boolean
  isUp: boolean
  modified: string | null
  size: number | null
}

export function FileList({ entries, viewMode, selectedNames, showUp, onSelect, onOpenDir, onOpenFile, onUp }: Props) {
  // Keyboard-driven selection (type-ahead) must stay visible: when the
  // selection collapses to one row, scroll it into view.
  const rootRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    if (selectedNames.length !== 1) return
    rootRef.current?.querySelector('.selected')?.scrollIntoView({ block: 'nearest' })
  }, [selectedNames])

  const rows: Row[] = []
  if (showUp) rows.push({ name: '..', is_dir: true, isUp: true, modified: null, size: null })
  for (const e of entries) {
    rows.push({ name: e.name, is_dir: e.is_dir, isUp: false, modified: e.modified, size: e.size })
  }

  const open = (row: Row) => {
    if (row.isUp) onUp()
    // archives are files on disk but the router enters them as folders (virtual paths)
    else if (row.is_dir || isArchive(row.name)) onOpenDir(row.name)
    else onOpenFile(entries.find((e) => e.name === row.name)!)
  }

  // Dirs → folder asset; files → the GTK-matching generated icon by extension,
  // falling back to the generic file asset for unrecognized types.
  const iconFor = (row: Row) => (row.is_dir ? folderIcon : (generatedFileIcon(row.name) ?? fileIcon))

  if (viewMode === 'list') {
    return (
      <div ref={rootRef}>
      <table className="files-table">
        <thead>
          <tr>
            <th>{tr("find.hdr_name")}</th>
            <th>{tr("webpult.date_modified")}</th>
            <th>{tr("find.hdr_size")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, idx) => (
            <tr
              key={`${row.name}-${idx}`}
              className={selectedNames.includes(row.name) ? 'selected' : ''}
              onClick={(e) => !row.isUp && onSelect(row.name, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })}
              onDoubleClick={() => open(row)}
            >
              <td>
                <div className="file-name-cell">
                  <img src={iconFor(row)} alt="icon" className="file-icon" />
                  <span className="file-name-txt">{row.name}</span>
                </div>
              </td>
              <td>{row.modified || '—'}</td>
              <td>{row.is_dir ? '' : formatSize(row.size)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      </div>
    )
  }

  return (
    <div ref={rootRef} className={`files-grid ${viewMode}`}>
      {rows.map((row, idx) => (
        <div
          key={`${row.name}-${idx}`}
          className={`grid-item ${selectedNames.includes(row.name) ? 'selected' : ''}`}
          onClick={(e) => !row.isUp && onSelect(row.name, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })}
          onDoubleClick={() => open(row)}
        >
          <img src={iconFor(row)} alt="icon" className="file-icon" />
          <div className="grid-item-name">{row.name}</div>
          {viewMode === 'tiles' && (
            <div className="grid-item-meta">{row.is_dir ? 'Folder' : formatSize(row.size) || '0 KB'}</div>
          )}
        </div>
      ))}
    </div>
  )
}
