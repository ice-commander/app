// Archive extensions the backend router treats as enterable folders
// (mirrors panel-router check_and_push_archives). Entering one pushes an
// ArchiveFileSystemRpc with virtual paths, same as the GTK app.
export function isArchive(name: string): boolean {
  const n = name.toLowerCase()
  return (
    n.endsWith('.zip') ||
    n.endsWith('.tar') ||
    n.endsWith('.tar.gz') ||
    n.endsWith('.tgz') ||
    n.endsWith('.tar.bz2') ||
    n.endsWith('.tbz2') ||
    n.endsWith('.tbz')
  )
}
