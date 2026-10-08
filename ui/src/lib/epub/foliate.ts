// The only module that knows about foliate-js (risk R6: keep it swappable).
// Book entries are fetched one at a time from the Rust custom protocol, which
// validates paths and strips scripts; foliate never sees the raw ZIP.

import '../../vendor/foliate-js/view.js'
import { EPUB } from '../../vendor/foliate-js/epub.js'
import { Overlayer } from '../../vendor/foliate-js/overlayer.js'
import { encodeEntryPath, protoUrl } from '../util'
import type { ReaderPrefs } from '../prefs.svelte'

export interface TocItem {
  label: string
  href: string
  subitems?: TocItem[]
}

export interface RelocateDetail {
  fraction: number
  cfi: string
  tocItem?: { label?: string; href?: string }
  location?: { current: number; total: number }
  section?: { current: number; total: number }
}

/** Minimal typing for the <foliate-view> element. */
export interface FoliateView extends HTMLElement {
  book: { toc?: TocItem[]; metadata?: Record<string, unknown>; dir?: string; sections: unknown[] }
  renderer: HTMLElement & {
    setStyles?: (css: string) => void
    next: () => Promise<void>
    prev: () => Promise<void>
  }
  lastLocation: RelocateDetail | null
  open(book: unknown): Promise<void>
  init(opts: { lastLocation?: string | null; showTextStart?: boolean }): Promise<void>
  goTo(target: string | number): Promise<unknown>
  goToFraction(f: number): Promise<void>
  goLeft(): Promise<void>
  goRight(): Promise<void>
  prev(): Promise<void>
  next(): Promise<void>
  getCFI(index: number, range: Range): string
  addAnnotation(a: { value: string; color?: string }, remove?: boolean): Promise<unknown>
  deleteAnnotation(a: { value: string }): Promise<unknown>
  search(opts: { query: string; matchCase?: boolean; matchDiacritics?: boolean; matchWholeWords?: boolean }): AsyncGenerator<
    | 'done'
    | { progress: number }
    | { label: string; subitems: { cfi: string; excerpt: { pre: string; match: string; post: string } }[] }
  >
  clearSearch(): void
  close(): void
  history: EventTarget & { back(): void; forward(): void; canGoBack: boolean; canGoForward: boolean }
}

export const HIGHLIGHT_COLORS: Record<string, string> = {
  yellow: 'rgb(255 214 0 / 0.45)',
  green: 'rgb(90 200 90 / 0.4)',
  blue: 'rgb(80 160 255 / 0.4)',
  pink: 'rgb(255 110 170 / 0.4)',
}

export async function loadEpub(bookId: number): Promise<unknown> {
  const base = protoUrl(`epub/${bookId}/`)
  const res = await fetch(base + 'manifest')
  if (!res.ok) throw new Error(`Could not read the book (${res.status})`)
  const entries: { name: string; size: number }[] = await res.json()
  const sizes = new Map(entries.map((e) => [e.name, e.size]))
  const url = (name: string) => base + 'file/' + encodeEntryPath(name)
  const fetchOk = async (name: string) => {
    const r = await fetch(url(name))
    if (!r.ok) throw new Error(`${name}: ${r.status}`)
    return r
  }
  const loader = {
    entries: entries.map((e) => ({ filename: e.name, uncompressedSize: e.size })),
    loadText: async (name: string) => (sizes.has(name) ? (await fetchOk(name)).text() : null),
    loadBlob: async (name: string, type?: string) => {
      if (!sizes.has(name)) return null
      const buf = await (await fetchOk(name)).arrayBuffer()
      return new Blob([buf], type ? { type } : undefined)
    },
    getSize: (name: string) => sizes.get(name) ?? 0,
    // Let foliate use its default (WebCrypto) for font deobfuscation.
    sha1: undefined,
  }
  return new EPUB(loader).init()
}

const FONTS: Record<ReaderPrefs['fontFamily'], string | null> = {
  publisher: null,
  serif: "Georgia, 'Iowan Old Style', 'Palatino Linotype', 'Times New Roman', serif",
  sans: "'Segoe UI', system-ui, -apple-system, sans-serif",
  mono: "'Cascadia Mono', Consolas, monospace",
}

/** CSS injected into every section. `colors` come from the app theme. */
export function readerCss(p: ReaderPrefs, colors: { bg: string; text: string; link: string }): string {
  const font = FONTS[p.fontFamily]
  return `
    @namespace epub "http://www.idpf.org/2007/ops";
    html {
      color: ${colors.text} !important;
      background: transparent !important;
      font-size: ${p.fontSize}% !important;
    }
    body { background: transparent !important; color: inherit !important; }
    ${font ? `body, p, div, span, li, blockquote, td { font-family: ${font} !important; }` : ''}
    a:link, a:visited { color: ${colors.link} !important; }
    p, li, blockquote, dd {
      line-height: ${p.lineHeight} !important;
      text-align: ${p.justify ? 'justify' : 'start'};
      -webkit-hyphens: ${p.hyphenate ? 'auto' : 'manual'};
      hyphens: ${p.hyphenate ? 'auto' : 'manual'};
      widows: 2;
      orphans: 2;
    }
    [align="left"] { text-align: left; }
    [align="right"] { text-align: right; }
    [align="center"] { text-align: center; }
    pre { white-space: pre-wrap !important; }
    img, svg { max-width: 100%; }
    aside[epub|type~="footnote"], aside[epub|type~="endnote"], aside[epub|type~="rearnote"] { display: none; }
  `
}

export function applyLayout(view: FoliateView, p: ReaderPrefs) {
  const r = view.renderer
  r.setAttribute('flow', p.flow)
  r.setAttribute('gap', '6%')
  r.setAttribute('margin', `${p.margin}px`)
  r.setAttribute('max-inline-size', `${p.maxWidth}px`)
  r.setAttribute('max-column-count', '2')
}

export function drawHighlight(e: Event) {
  const { draw, annotation } = (e as CustomEvent).detail
  draw(Overlayer.highlight, { color: HIGHLIGHT_COLORS[annotation.color] ?? annotation.color ?? HIGHLIGHT_COLORS.yellow })
}

/** Flatten a TOC tree for display with depth. */
export function flattenToc(items: TocItem[] | undefined, depth = 0): { label: string; href: string; depth: number }[] {
  if (!items) return []
  return items.flatMap((i) => [
    { label: (i.label ?? '').trim(), href: i.href, depth },
    ...flattenToc(i.subitems, depth + 1),
  ])
}
