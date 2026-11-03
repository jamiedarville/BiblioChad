<script lang="ts">
  import { onMount } from 'svelte'
  import type { Annotation, EpubLocator } from '../lib/api'
  import type { ReaderPrefs } from '../lib/prefs.svelte'
  import {
    applyLayout,
    drawHighlight,
    loadEpub,
    readerCss,
    type FoliateView,
    type RelocateDetail,
    type TocItem,
  } from '../lib/epub/foliate'

  let {
    bookId,
    start,
    reader,
    themeKey,
    annotations,
    onRelocate,
    onSelection,
    onShowAnnotation,
    onReady,
    onError,
    onKey,
  }: {
    bookId: number
    start: EpubLocator | null
    reader: ReaderPrefs
    themeKey: string
    annotations: Annotation[]
    onRelocate: (loc: EpubLocator, label: string) => void
    onSelection: (sel: { cfi: string; text: string; percent: number } | null) => void
    onShowAnnotation: (cfi: string) => void
    onReady: (toc: TocItem[]) => void
    onError: (msg: string) => void
    onKey: (e: KeyboardEvent) => void
  } = $props()

  const BLOCK = /^(p|div|h[1-6]|li|blockquote|section|article|aside|tr|dt|dd|pre|figcaption|br|hr)$/

  /** Range text with line breaks at block boundaries. */
  function rangeText(r: Range): string {
    let out = ''
    const walk = (n: Node) => {
      if (n.nodeType === Node.TEXT_NODE) {
        out += n.nodeValue ?? ''
      } else if (n.nodeType === Node.ELEMENT_NODE || n.nodeType === Node.DOCUMENT_FRAGMENT_NODE) {
        const name = n.nodeType === Node.ELEMENT_NODE ? (n as Element).localName.toLowerCase() : ''
        if (name === 'script' || name === 'style') return
        const block = BLOCK.test(name)
        if (block) out += '\n'
        n.childNodes.forEach(walk)
        if (block) out += '\n'
      }
    }
    walk(r.cloneContents())
    return out
      .replace(/[ \t\u00a0]+/g, ' ')
      .replace(/ *\n[\s]*/g, '\n')
      .trim()
  }

  function clip(s: string, max: number): string {
    if (s.length <= max) return s
    const cut = s.lastIndexOf(' ', max)
    return s.slice(0, cut > max * 0.8 ? cut : max) + '…'
  }

  let host: HTMLDivElement
  let view: FoliateView | null = null
  let percent = 0
  let wheelLock = 0

  function colors() {
    const cs = getComputedStyle(document.documentElement)
    return {
      bg: cs.getPropertyValue('--reader-bg').trim(),
      text: cs.getPropertyValue('--reader-text').trim(),
      link: cs.getPropertyValue('--accent').trim(),
    }
  }

  function applyStyles() {
    if (!view) return
    applyLayout(view, reader)
    view.renderer.setStyles?.(readerCss(reader, colors()))
  }

  // Re-style when typography or theme changes.
  $effect(() => {
    void [themeKey, JSON.stringify(reader)]
    applyStyles()
  })

  onMount(() => {
    let disposed = false
    ;(async () => {
      try {
        const book = await loadEpub(bookId)
        if (disposed) return
        view = document.createElement('foliate-view') as FoliateView
        view.style.width = '100%'
        view.style.height = '100%'
        host.append(view)
        await view.open(book)
        view.addEventListener('relocate', (e) => {
          const d = (e as CustomEvent<RelocateDetail>).detail
          percent = d.fraction ?? 0
          onRelocate(
            { kind: 'epub', cfi: d.cfi, href: d.tocItem?.href, percent },
            d.tocItem?.label?.trim() ?? '',
          )
        })
        view.addEventListener('draw-annotation', drawHighlight)
        view.addEventListener('show-annotation', (e) => onShowAnnotation((e as CustomEvent).detail.value))
        view.addEventListener('create-overlay', (e) => {
          // addAnnotation resolves each CFI and only draws it if its
          // section is the one that just loaded.
          void (e as CustomEvent).detail.index
          for (const a of annotations) {
            if (a.locator.kind !== 'epub') continue
            view?.addAnnotation({ value: a.locator.cfi, color: a.color ?? 'yellow' })
          }
        })
        view.addEventListener('load', (e) => {
          const { doc, index } = (e as CustomEvent).detail as { doc: Document; index: number }
          doc.addEventListener('keydown', onKey)
          doc.addEventListener('wheel', (ev: WheelEvent) => {
            if (reader.flow !== 'paginated' || Math.abs(ev.deltaY) < 4) return
            const now = Date.now()
            if (now - wheelLock < 350) return
            wheelLock = now
            if (ev.deltaY > 0) view?.next()
            else view?.prev()
          }, { passive: true })
          const report = () => {
            const sel = doc.getSelection()
            if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
              onSelection(null)
              return
            }
            const range = sel.getRangeAt(0)
            const text = sel.toString().trim()
            if (!text) return onSelection(null)
            onSelection({ cfi: view!.getCFI(index, range), text, percent })
          }
          doc.addEventListener('pointerup', () => setTimeout(report, 10))
          doc.addEventListener('keyup', (ev) => ev.shiftKey && report())
        })
        applyStyles()
        await view.init({ lastLocation: start?.cfi ?? null, showTextStart: !start })
        onReady(view.book.toc ?? [])
      } catch (e) {
        onError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      disposed = true
      try {
        view?.close()
      } catch {
        /* already closed */
      }
      view?.remove()
      view = null
    }
  })

  export function next() {
    return view?.next()
  }
  export function prev() {
    return view?.prev()
  }
  export function goLeft() {
    return view?.goLeft()
  }
  export function goRight() {
    return view?.goRight()
  }
  export function goTo(target: string) {
    return view?.goTo(target)
  }
  export function goToFraction(f: number) {
    return view?.goToFraction(f)
  }
  export function back() {
    view?.history.back()
  }
  export function addHighlight(cfi: string, color: string) {
    return view?.addAnnotation({ value: cfi, color })
  }
  export function removeHighlight(cfi: string) {
    return view?.deleteAnnotation({ value: cfi })
  }
  export function clearSelection() {
    onSelection(null)
  }
  export async function* search(query: string) {
    if (!view) return
    for await (const r of view.search({ query })) {
      if (r === 'done') return
      if ('subitems' in r) yield r
    }
  }
  export function clearSearch() {
    view?.clearSearch()
  }

  /**
   * The text on screen plus some context either side, for the learning
   * assistant. Markers tell the model where the reader's screen is.
   */
  export function passage(): string {
    const range = (view?.lastLocation as { range?: Range } | null)?.range
    const doc = range?.startContainer.ownerDocument
    if (!range || !doc?.body) return ''
    const visible = clip(rangeText(range), 8000)
    const before = doc.createRange()
    before.selectNodeContents(doc.body)
    before.setEnd(range.startContainer, range.startOffset)
    const after = doc.createRange()
    after.selectNodeContents(doc.body)
    after.setStart(range.endContainer, range.endOffset)
    const prev = rangeText(before).slice(-1500)
    const next = clip(rangeText(after), Math.max(1500, 9000 - visible.length))
    return [
      prev && `${prev}\n`,
      "[The reader's screen starts here]",
      visible,
      "[The reader's screen ends here]",
      next && `\n${next}`,
    ]
      .filter(Boolean)
      .join('\n')
      .trim()
  }
</script>

<div class="epub" bind:this={host}></div>

<style>
  .epub {
    width: 100%;
    height: 100%;
    background: var(--reader-bg);
  }
</style>
