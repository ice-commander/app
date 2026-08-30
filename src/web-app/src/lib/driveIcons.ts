import ssdIcon from '../assets/ssd.svg'
import homeIcon from '../assets/home.svg'
import atHomeIcon from '../assets/at-home.svg'
import ftpIcon from '../assets/ftp.svg'
import netdriveIcon from '../assets/netdrive.svg'
import connectIcon from '../assets/connect.svg'
import folderIcon from '../assets/folder.svg'

// Map the backend icon basename (from the unified get_all_app_drives list) to a bundled asset.
const ICONS: Record<string, string> = {
  'ssd.svg': ssdIcon,
  'home.svg': homeIcon,
  'at-home.svg': atHomeIcon,
  'ftp.svg': ftpIcon,
  'netdrive.svg': netdriveIcon,
  'connect.svg': connectIcon,
  'folder.svg': folderIcon,
}

export function driveIcon(iconBasename: string): string {
  return ICONS[iconBasename] ?? folderIcon
}

// Section grouping for the unified source list, mirroring the GTK selector.
export type DriveSection = 'local' | 'net'

export function driveSection(kind: string): DriveSection {
  return kind === 'net' ? 'net' : 'local'
}

export const SECTION_TITLES: Record<DriveSection, string> = {
  local: 'Drives',
  net: 'Connections',
}

export const SECTION_ORDER: DriveSection[] = ['local', 'net']
