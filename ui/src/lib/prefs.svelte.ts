// User preferences, persisted through the Rust `prefs` setting.

import { api, type SortKey } from './api'

export type Theme = 'system' | 'light' | 'dark' | 'sepia'

export interface ReaderPrefs {
  fontFamily: 'publisher' | 'serif' | 'sans' | 'mono'
  fontSize: number // percent
  lineHeight: number
  margin: number // px
  maxWidth: number // px, line length cap
  justify: boolean
  hyphenate: boolean
  flow: 'paginated' | 'scrolled'
}

export interface PdfPrefs {
  fit: 'width' | 'page'
  zoom: number
  mode: 'continuous' | 'single'
  textLayer: boolean
}

export interface Prefs {
  theme: Theme
  chadMode: boolean
  view: 'grid' | 'list'
  sort: SortKey
  sortDesc: boolean
  reader: ReaderPrefs
  pdf: PdfPrefs
  cache_cap_mb: number
}

export const defaultPrefs: Prefs = {
  theme: 'system',
  chadMode: true,
  view: 'grid',
  sort: 'title',
  sortDesc: false,
  reader: {
    fontFamily: 'publisher',
    fontSize: 100,
    lineHeight: 1.5,
    margin: 48,
    maxWidth: 720,
    justify: true,
    hyphenate: true,
    flow: 'paginated',
  },
  pdf: { fit: 'width', zoom: 1, mode: 'continuous', textLayer: true },
  cache_cap_mb: 2048,
}

function merge(stored: Record<string, unknown>): Prefs {
  const s = stored as Partial<Prefs>
  return {
    ...defaultPrefs,
    ...s,
    reader: { ...defaultPrefs.reader, ...(s.reader ?? {}) },
    pdf: { ...defaultPrefs.pdf, ...(s.pdf ?? {}) },
  }
}

class PrefsStore {
  value = $state<Prefs>(structuredClone(defaultPrefs))
  loaded = $state(false)
  #timer: ReturnType<typeof setTimeout> | null = null

  async load() {
    try {
      this.value = merge(await api.getPrefs())
    } catch {
      this.value = structuredClone(defaultPrefs)
    }
    this.loaded = true
  }

  /** Apply a change and persist it shortly after. */
  update(fn: (p: Prefs) => void) {
    fn(this.value)
    if (this.#timer) clearTimeout(this.#timer)
    this.#timer = setTimeout(() => {
      api.setPrefs($state.snapshot(this.value) as unknown as Record<string, unknown>).catch(() => {})
    }, 300)
  }
}

export const prefs = new PrefsStore()

/** Resolve the effective theme for CSS. */
export function effectiveTheme(t: Theme): 'light' | 'dark' | 'sepia' {
  if (t !== 'system') return t
  return typeof matchMedia !== 'undefined' && matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}
