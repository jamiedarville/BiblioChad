// Typed wrappers around the Rust commands. Shapes mirror the serde output
// of the Rust structs in src-tauri/src/commands.rs and crates/bc-library.

import { invoke } from '@tauri-apps/api/core'

export type BookFormat = 'epub' | 'pdf'
export type ReadStatus = 'unread' | 'reading' | 'finished'
export type FitMode = 'width' | 'page' | 'custom'
export type SortKey = 'title' | 'author' | 'recently_read' | 'added' | 'progress'

export type EpubLocator = { kind: 'epub'; cfi: string; href?: string; percent: number }
export type PdfLocator = { kind: 'pdf'; page: number; offset: number; fit: FitMode; zoom: number }
export type Locator = EpubLocator | PdfLocator

export interface BookSummary {
  id: number
  key: string
  format: BookFormat
  title: string
  author: string | null
  series: string | null
  series_index: number | null
  has_cover: boolean
  percent: number
  status: ReadStatus
  last_read_at: number | null
  added_at: number
  size: number | null
  favorite: boolean
  pinned: boolean
  cached: boolean
  page_count: number | null
}

export interface BookDetails extends BookSummary {
  file_name: string
  publisher: string | null
  language: string | null
  isbn: string | null
  description: string | null
  title_override: string | null
  author_override: string | null
  series_override: string | null
  collections: number[]
  folder_path: string[]
}

export interface FolderSummary {
  id: string
  name: string
  book_count: number
  folder_count: number
  is_shortcut: boolean
}

export interface FolderListing {
  id: string
  breadcrumbs: { id: string; name: string }[]
  folders: FolderSummary[]
  books: BookSummary[]
}

export interface BookQuery {
  search?: string | null
  sort?: SortKey
  descending?: boolean
  format?: BookFormat | null
  status?: ReadStatus | null
  collection_id?: number | null
  favorites_only?: boolean
  cached_only?: boolean
  folder_id?: string | null
  limit?: number | null
  offset?: number | null
}

export interface Rank {
  name: string
  threshold: number
  next_threshold: number | null
}

export interface Stats {
  total_books: number
  finished: number
  reading: number
  streak_days: number
  rank: Rank
}

export interface DriveStatus {
  configured: boolean
  connected: boolean
  email: string | null
  root_id: string | null
  root_name: string | null
  last_synced_at: number | null
  syncing: boolean
  crawl_in_progress: boolean
}

export interface Overview {
  drive: DriveStatus
  stats: Stats
  continue_reading: BookSummary[]
  has_books: boolean
  pdf_available: boolean
}

export interface Collection {
  id: number
  name: string
  book_count: number
}

export interface Bookmark {
  id: string
  book_id: number
  locator: Locator
  label: string | null
  created_at: number
  updated_at: number
}

export interface Annotation {
  id: string
  book_id: number
  kind: 'highlight' | 'note'
  locator: Locator
  text: string | null
  color: string | null
  note: string | null
  created_at: number
  updated_at: number
}

export interface OutlineItem {
  title: string
  page: number | null
  children: OutlineItem[]
}

export interface ResumePrompt {
  device_id: string
  locator: Locator
  percent: number
  updated_at: number
  label: string
}

export interface OpenBook {
  id: number
  format: BookFormat
  title: string
  author: string | null
  locator: Locator | null
  percent: number
  prompt: ResumePrompt | null
  page_count: number | null
  page_sizes: [number, number][]
  outline: OutlineItem[]
  bookmarks: Bookmark[]
  annotations: Annotation[]
}

export interface TextRun {
  text: string
  x: number
  y: number
  w: number
  h: number
}

export interface SearchHit {
  page: number
  excerpt: string
}

export interface FolderEntry {
  id: string
  name: string
  shared_drive_id: string | null
}

export interface CacheStats {
  entries: number
  bytes: number
  pinned_bytes: number
}

export interface AppInfo {
  version: string
  data_dir: string
  device_id: string
  pdf_error: string | null
}

export interface CmdError {
  kind: string
  message: string
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) return String((e as CmdError).message)
  return String(e)
}

export function errorKind(e: unknown): string | null {
  if (e && typeof e === 'object' && 'kind' in e) return String((e as CmdError).kind)
  return null
}

export const api = {
  overview: () => invoke<Overview>('get_overview'),
  listFolder: (folderId: string | null) => invoke<FolderListing>('list_folder', { folderId }),
  queryBooks: (query: BookQuery) => invoke<BookSummary[]>('query_books', { query }),
  recentlyRead: (limit?: number) => invoke<BookSummary[]>('recently_read', { limit }),
  bookDetails: (id: number) => invoke<BookDetails>('book_details', { id }),
  setOverrides: (id: number, title: string | null, author: string | null, series: string | null) =>
    invoke<void>('set_book_overrides', { id, title, author, series }),
  setFavorite: (id: number, favorite: boolean) => invoke<void>('set_favorite', { id, favorite }),
  setStatus: (id: number, status: ReadStatus | null) => invoke<void>('set_status', { id, status }),
  setPinned: (id: number, pinned: boolean) => invoke<void>('set_pinned', { id, pinned }),
  collections: () => invoke<Collection[]>('collections'),
  createCollection: (name: string) => invoke<number>('create_collection', { name }),
  renameCollection: (id: number, name: string) => invoke<void>('rename_collection', { id, name }),
  deleteCollection: (id: number) => invoke<void>('delete_collection', { id }),
  setInCollection: (collectionId: number, bookId: number, member: boolean) =>
    invoke<void>('set_in_collection', { collectionId, bookId, member }),
  importLocalFiles: (paths: string[]) =>
    invoke<{ imported: number[]; failed: string[] }>('import_local_files', { paths }),
  removeLocalBook: (id: number) => invoke<void>('remove_local_book', { id }),
  exportAnnotations: (id: number, dest: string) => invoke<void>('export_annotations', { id, dest }),

  openBook: (id: number) => invoke<OpenBook>('open_book', { id }),
  saveProgress: (id: number, locator: Locator, percent: number) =>
    invoke<void>('save_progress', { id, locator, percent }),
  closeBook: () => invoke<void>('close_book'),
  addBookmark: (id: number, locator: Locator, label: string | null) =>
    invoke<Bookmark>('add_bookmark', { id, locator, label }),
  deleteBookmark: (bookmarkId: string) => invoke<void>('delete_bookmark', { bookmarkId }),
  addAnnotation: (
    id: number,
    annotation: { locator: Locator; text: string | null; color: string | null; note: string | null },
  ) => invoke<Annotation>('add_annotation', { id, annotation }),
  updateAnnotation: (annotationId: string, color: string | null, note: string | null) =>
    invoke<void>('update_annotation', { annotationId, color, note }),
  deleteAnnotation: (annotationId: string) => invoke<void>('delete_annotation', { annotationId }),
  pdfText: (id: number, page: number) => invoke<TextRun[]>('pdf_text', { id, page }),
  pdfSearch: (id: number, query: string) => invoke<SearchHit[]>('pdf_search', { id, query }),

  driveStatus: () => invoke<DriveStatus>('drive_status'),
  driveImportClientFile: (path: string) => invoke<void>('drive_import_client_file', { path }),
  driveConnect: () => invoke<DriveStatus>('drive_connect'),
  driveListFolders: (parent: string | null) => invoke<FolderEntry[]>('drive_list_folders', { parent }),
  driveChooseRoot: (folder: FolderEntry) => invoke<void>('drive_choose_root', { folder }),
  driveSyncNow: () => invoke<void>('drive_sync_now'),
  driveDisconnect: (wipe: boolean) => invoke<void>('drive_disconnect', { wipe }),

  getPrefs: () => invoke<Record<string, unknown>>('get_prefs'),
  setPrefs: (prefs: Record<string, unknown>) => invoke<void>('set_prefs', { prefs }),
  cacheStats: () => invoke<CacheStats>('cache_stats'),
  clearCache: () => invoke<void>('clear_cache'),
  appInfo: () => invoke<AppInfo>('app_info'),
  takePendingOpen: () => invoke<number | null>('take_pending_open'),
}
