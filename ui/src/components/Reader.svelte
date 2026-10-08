<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import {
    api,
    errorKind,
    errorMessage,
    type Annotation,
    type EpubLocator,
    type Locator,
    type OpenBook,
    type OutlineItem,
    type PdfLocator,
    type SearchHit,
  } from '../lib/api'
  import { prefs, effectiveTheme } from '../lib/prefs.svelte'
  import { toasts } from '../lib/toast.svelte'
  import { debounce, formatBytes, formatPercent } from '../lib/util'
  import { finishedLine, loadingLine, resumeLine, searchEmpty } from '../lib/chad'
  import { onBeforeClose } from '../lib/session'
  import { flattenToc, HIGHLIGHT_COLORS, type TocItem } from '../lib/epub/foliate'
  import EpubView from './EpubView.svelte'
  import PdfView from './PdfView.svelte'
  import Icon from './Icon.svelte'

  let {
    bookId,
    download,
    onClose,
  }: { bookId: number; download: { done: number; total: number | null } | null; onClose: () => void } = $props()

  let book = $state<OpenBook | null>(null)
  let error = $state<{ kind: string | null; message: string } | null>(null)
  let percent = $state(0)
  let label = $state('')
  let locator = $state<Locator | null>(null)
  let toc = $state<{ label: string; href: string; depth: number }[]>([])
  let panel = $state<null | 'toc' | 'marks' | 'search' | 'style'>(null)
  let prompt = $state<OpenBook['prompt']>(null)
  let selection = $state<{ cfi: string; text: string; percent: number } | null>(null)
  let editing = $state<Annotation | null>(null)
  let chromeVisible = $state(true)
  let query = $state('')
  let searching = $state(false)
  let epubHits = $state<{ label: string; cfi: string; pre: string; match: string; post: string }[]>([])
  let pdfHits = $state<SearchHit[]>([])
  let searched = $state(false)
  let finishedShown = false

  let epubView = $state<EpubView | null>(null)
  let pdfView = $state<PdfView | null>(null)

  const chad = $derived(prefs.value.chadMode)
  const themeKey = $derived(effectiveTheme(prefs.value.theme))
  const loading = loadingLine(prefs.value.chadMode)

  const save = debounce((loc: Locator, pct: number) => {
    api.saveProgress(bookId, loc, pct).catch((e) => console.warn('save failed', e))
  }, 2000)

  function relocated(loc: Locator, where: string) {
    locator = loc
    percent = loc.kind === 'epub' ? loc.percent : book?.page_count ? (loc.page + 1) / book.page_count : 0
    label = where
    save(loc, percent)
    if (percent >= 0.995 && !finishedShown && book) {
      finishedShown = true
      if ((book.percent ?? 0) < 0.995) toasts.push('success', finishedLine(chad, book.title))
    }
  }

  async function load() {
    try {
      book = await api.openBook(bookId)
      prompt = book.prompt
      percent = book.percent
      finishedShown = book.percent >= 0.995
      if (book.format === 'pdf') {
        toc = flattenOutline(book.outline)
      }
    } catch (e) {
      error = { kind: errorKind(e), message: errorMessage(e) }
    }
  }

  function flattenOutline(items: OutlineItem[], depth = 0): { label: string; href: string; depth: number }[] {
    return items.flatMap((i) => [
      { label: i.title || '(untitled)', href: i.page != null ? String(i.page) : '', depth },
      ...flattenOutline(i.children, depth + 1),
    ])
  }

  async function close() {
    save.flush()
    try {
      await api.closeBook()
    } catch {
      /* progress is already saved locally */
    }
    onClose()
  }

  function goToLocator(loc: Locator) {
    if (loc.kind === 'epub') epubView?.goTo(loc.cfi)
    else pdfView?.goToPage(loc.page, loc.offset)
  }

  function goToc(href: string) {
    if (!href) return
    if (book?.format === 'pdf') pdfView?.goToPage(Number(href))
    else epubView?.goTo(href)
    if (window.innerWidth < 900) panel = null
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')) return
    const k = e.key
    if (k === 'ArrowRight' || k === 'PageDown' || (k === ' ' && !e.shiftKey && book?.format === 'epub')) {
      e.preventDefault()
      book?.format === 'epub' ? epubView?.goRight() : pdfView?.next()
    } else if (k === 'ArrowLeft' || k === 'PageUp' || (k === ' ' && e.shiftKey && book?.format === 'epub')) {
      e.preventDefault()
      book?.format === 'epub' ? epubView?.goLeft() : pdfView?.prev()
    } else if (k === 'Escape') {
      if (panel) panel = null
      else if (selection) selection = null
      else close()
    } else if ((e.ctrlKey || e.metaKey) && k === 'f') {
      e.preventDefault()
      panel = 'search'
    } else if ((e.ctrlKey || e.metaKey) && k === 'd') {
      e.preventDefault()
      addBookmark()
    } else if (k === 'Home' && book?.format === 'pdf') {
      pdfView?.goToPage(0)
    } else if (k === 'End' && book?.format === 'pdf') {
      pdfView?.goToPage((book.page_count ?? 1) - 1)
    } else if (k === 'f' && !e.ctrlKey) {
      fullscreen()
    }
  }

  async function fullscreen() {
    const w = getCurrentWindow()
    await w.setFullscreen(!(await w.isFullscreen()))
  }

  async function addBookmark() {
    if (!locator || !book) return
    try {
      const b = await api.addBookmark(bookId, locator, label || formatPercent(percent))
      book.bookmarks = [...book.bookmarks, b]
      toasts.push('info', chad ? 'Bookmarked. Chad will remember this spot.' : 'Bookmark added.')
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function removeBookmark(id: string) {
    if (!book) return
    await api.deleteBookmark(id)
    book.bookmarks = book.bookmarks.filter((b) => b.id !== id)
  }

  async function highlight(color: string, withNote = false) {
    if (!selection || !book) return
    const sel = selection
    selection = null
    const note = withNote ? prompt_('Note') : null
    if (withNote && note === null) return
    try {
      const loc: EpubLocator = { kind: 'epub', cfi: sel.cfi, percent: sel.percent }
      const a = await api.addAnnotation(bookId, { locator: loc, text: sel.text, color, note })
      book.annotations = [...book.annotations, a]
      await epubView?.addHighlight(sel.cfi, color)
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  function prompt_(title: string, value = ''): string | null {
    const v = window.prompt(title, value)
    return v === null ? null : v.trim()
  }

  async function copySelection() {
    if (!selection) return
    await navigator.clipboard.writeText(selection.text)
    selection = null
    toasts.push('info', 'Copied.')
  }

  function showAnnotation(cfi: string) {
    const a = book?.annotations.find((x) => x.locator.kind === 'epub' && x.locator.cfi === cfi)
    if (a) editing = { ...a }
  }

  async function saveAnnotation() {
    if (!editing || !book) return
    const ed = editing
    await api.updateAnnotation(ed.id, ed.color, ed.note || null)
    book.annotations = book.annotations.map((a) => (a.id === ed.id ? { ...ed } : a))
    if (ed.locator.kind === 'epub') await epubView?.addHighlight(ed.locator.cfi, ed.color ?? 'yellow')
    editing = null
  }

  async function deleteAnnotation(a: Annotation) {
    if (!book) return
    await api.deleteAnnotation(a.id)
    book.annotations = book.annotations.filter((x) => x.id !== a.id)
    if (a.locator.kind === 'epub') await epubView?.removeHighlight(a.locator.cfi)
    editing = null
  }

  async function runSearch() {
    const q = query.trim()
    if (!q || !book) return
    searching = true
    searched = true
    epubHits = []
    pdfHits = []
    try {
      if (book.format === 'pdf') {
        pdfHits = await api.pdfSearch(bookId, q)
      } else if (epubView) {
        for await (const r of epubView.search(q)) {
          epubHits = [
            ...epubHits,
            ...r.subitems.map((s) => ({ label: r.label, cfi: s.cfi, ...s.excerpt })),
          ]
          if (epubHits.length > 1000) break
        }
      }
    } catch (e) {
      toasts.push('error', errorMessage(e))
    } finally {
      searching = false
    }
  }

  function seek(e: Event) {
    const f = Number((e.target as HTMLInputElement).value)
    if (book?.format === 'epub') epubView?.goToFraction(f)
    else pdfView?.goToFraction(f)
  }

  onMount(() => {
    load()
    const off = onBeforeClose(() => save.flush())
    const vis = () => document.visibilityState === 'hidden' && save.flush()
    document.addEventListener('visibilitychange', vis)
    let idle: ReturnType<typeof setTimeout>
    const poke = () => {
      chromeVisible = true
      clearTimeout(idle)
      idle = setTimeout(() => (chromeVisible = panel !== null || selection !== null), 3500)
    }
    window.addEventListener('pointermove', poke)
    poke()
    return () => {
      off()
      save.flush()
      document.removeEventListener('visibilitychange', vis)
      window.removeEventListener('pointermove', poke)
      clearTimeout(idle)
    }
  })

  const r = $derived(prefs.value.reader)
  const p = $derived(prefs.value.pdf)
</script>

<svelte:window onkeydown={onKey} />

<div class="reader" class:hide-chrome={!chromeVisible && !panel}>
  <header class="bar">
    <button class="ghost" onclick={close} aria-label="Back to library"><Icon name="back" /> Library</button>
    <div class="title">
      <strong>{book?.title ?? ''}</strong>
      {#if book?.author}<span class="muted"> · {book.author}</span>{/if}
    </div>
    <div class="row">
      <button class="ghost icon" class:on={panel === 'toc'} title="Contents" aria-label="Table of contents" onclick={() => (panel = panel === 'toc' ? null : 'toc')}><Icon name="toc" /></button>
      <button class="ghost icon" class:on={panel === 'marks'} title="Bookmarks and notes" aria-label="Bookmarks and notes" onclick={() => (panel = panel === 'marks' ? null : 'marks')}><Icon name="bookmark" /></button>
      <button class="ghost icon" class:on={panel === 'search'} title="Search (Ctrl+F)" aria-label="Search in book" onclick={() => (panel = panel === 'search' ? null : 'search')}><Icon name="search" /></button>
      <button class="ghost icon" class:on={panel === 'style'} title="Display settings" aria-label="Display settings" onclick={() => (panel = panel === 'style' ? null : 'style')}><Icon name="type" /></button>
      <button class="ghost icon" title="Add bookmark (Ctrl+D)" aria-label="Add bookmark" onclick={addBookmark}><Icon name="bookmark-add" /></button>
      <button class="ghost icon" title="Fullscreen (F11)" aria-label="Toggle fullscreen" onclick={fullscreen}><Icon name="fullscreen" /></button>
    </div>
  </header>

  {#if prompt}
    <div class="prompt" role="alert">
      <span>{resumeLine(chad, prompt.label)}</span>
      <button class="primary" onclick={() => { goToLocator(prompt!.locator); prompt = null }}>Jump to {prompt.label}</button>
      <button onclick={() => (prompt = null)}>Stay here</button>
    </div>
  {/if}

  <div class="body">
    {#if panel && panel !== 'style'}
      <aside class="panel">
        {#if panel === 'toc'}
          <h3>Contents</h3>
          {#if toc.length === 0}<p class="muted">This book has no table of contents.</p>{/if}
          <ul class="toc">
            {#each toc as t}
              <li style:padding-left="{t.depth * 14}px"><button class="ghost" onclick={() => goToc(t.href)}>{t.label}</button></li>
            {/each}
          </ul>
        {:else if panel === 'marks'}
          <h3>Bookmarks</h3>
          {#if !book?.bookmarks.length}<p class="muted">None yet. Ctrl+D adds one.</p>{/if}
          <ul class="list">
            {#each book?.bookmarks ?? [] as b (b.id)}
              <li>
                <button class="ghost grow" onclick={() => goToLocator(b.locator)}>{b.label ?? 'Bookmark'}</button>
                <button class="ghost icon" aria-label="Delete bookmark" onclick={() => removeBookmark(b.id)}>✕</button>
              </li>
            {/each}
          </ul>
          <h3>Highlights and notes</h3>
          {#if !book?.annotations.length}<p class="muted">{book?.format === 'pdf' ? 'Highlights are available for EPUB books.' : 'Select text to highlight it.'}</p>{/if}
          <ul class="list">
            {#each book?.annotations ?? [] as a (a.id)}
              <li class="ann">
                <button class="ghost grow left" onclick={() => goToLocator(a.locator)}>
                  <span class="swatch" style:background={HIGHLIGHT_COLORS[a.color ?? 'yellow']}></span>
                  <span class="quote">{a.text}</span>
                  {#if a.note}<span class="note">{a.note}</span>{/if}
                </button>
                <button class="ghost icon" aria-label="Edit" onclick={() => (editing = { ...a })}>✎</button>
              </li>
            {/each}
          </ul>
        {:else if panel === 'search'}
          <h3>Search</h3>
          <form class="row" onsubmit={(e) => { e.preventDefault(); runSearch() }}>
            <input type="search" bind:value={query} placeholder="Find in book" aria-label="Find in book" />
            <button class="primary" disabled={searching}>{searching ? '…' : 'Go'}</button>
          </form>
          <ul class="list hits">
            {#each epubHits as h}
              <li><button class="ghost left" onclick={() => epubView?.goTo(h.cfi)}><span class="muted small">{h.label}</span><br />{h.pre}<mark>{h.match}</mark>{h.post}</button></li>
            {/each}
            {#each pdfHits as h}
              <li><button class="ghost left" onclick={() => pdfView?.goToPage(h.page)}><span class="muted small">Page {h.page + 1}</span><br />{h.excerpt}</button></li>
            {/each}
          </ul>
          {#if searched && !searching && epubHits.length === 0 && pdfHits.length === 0}<p class="muted">{searchEmpty(chad)}</p>{/if}
        {/if}
      </aside>
    {/if}

    <div class="stage">
      {#if error}
        <div class="center">
          <h2>Can't open this book</h2>
          <p>{error.message}</p>
          <div class="row"><button class="primary" onclick={() => { error = null; load() }}>Try again</button><button onclick={onClose}>Back to library</button></div>
        </div>
      {:else if !book}
        <div class="center">
          <img src="/chad.png" alt="" width="72" height="72" class="bob" />
          <p>{loading}</p>
          {#if download}
            <div class="dl"><div style:width="{download.total ? (download.done / download.total) * 100 : 30}%"></div></div>
            <p class="muted small">{formatBytes(download.done)}{download.total ? ` of ${formatBytes(download.total)}` : ''}</p>
          {/if}
        </div>
      {:else if book.format === 'epub'}
        <EpubView
          bind:this={epubView}
          {bookId}
          start={book.locator?.kind === 'epub' ? book.locator : null}
          reader={r}
          {themeKey}
          annotations={book.annotations}
          onRelocate={relocated}
          onSelection={(s) => (selection = s)}
          onShowAnnotation={showAnnotation}
          onReady={(t: TocItem[]) => (toc = flattenToc(t))}
          onError={(m) => (error = { kind: 'book', message: m })}
          {onKey}
        />
      {:else}
        <PdfView
          bind:this={pdfView}
          {bookId}
          pageSizes={book.page_sizes}
          start={book.locator?.kind === 'pdf' ? (book.locator as PdfLocator) : null}
          pdf={p}
          onRelocate={relocated}
          {onKey}
          onZoom={(z) => prefs.update((x) => (x.pdf.zoom = z))}
        />
      {/if}
    </div>

    {#if panel === 'style'}
      <aside class="panel right">
        <h3>Theme</h3>
        <div class="seg">
          {#each [['system', 'Auto'], ['light', 'Light'], ['sepia', 'Sepia'], ['dark', 'Dark']] as [t, l]}
            <button class:primary={prefs.value.theme === t} onclick={() => prefs.update((x) => (x.theme = t as typeof x.theme))}>{l}</button>
          {/each}
        </div>
        {#if book?.format === 'epub'}
          <h3>Layout</h3>
          <div class="seg">
            <button class:primary={r.flow === 'paginated'} onclick={() => prefs.update((x) => (x.reader.flow = 'paginated'))}>Pages</button>
            <button class:primary={r.flow === 'scrolled'} onclick={() => prefs.update((x) => (x.reader.flow = 'scrolled'))}>Scroll</button>
          </div>
          <h3>Font</h3>
          <select value={r.fontFamily} onchange={(e) => prefs.update((x) => (x.reader.fontFamily = (e.target as HTMLSelectElement).value as typeof x.reader.fontFamily))}>
            <option value="publisher">Publisher default</option>
            <option value="serif">Serif</option>
            <option value="sans">Sans-serif</option>
            <option value="mono">Monospace</option>
          </select>
          <label>Size <span class="muted">{r.fontSize}%</span>
            <input type="range" min="70" max="200" step="5" value={r.fontSize} oninput={(e) => prefs.update((x) => (x.reader.fontSize = +(e.target as HTMLInputElement).value))} /></label>
          <label>Line spacing <span class="muted">{r.lineHeight.toFixed(1)}</span>
            <input type="range" min="1" max="2.4" step="0.1" value={r.lineHeight} oninput={(e) => prefs.update((x) => (x.reader.lineHeight = +(e.target as HTMLInputElement).value))} /></label>
          <label>Margins <span class="muted">{r.margin}px</span>
            <input type="range" min="0" max="120" step="4" value={r.margin} oninput={(e) => prefs.update((x) => (x.reader.margin = +(e.target as HTMLInputElement).value))} /></label>
          <label>Line length <span class="muted">{r.maxWidth}px</span>
            <input type="range" min="400" max="1400" step="20" value={r.maxWidth} oninput={(e) => prefs.update((x) => (x.reader.maxWidth = +(e.target as HTMLInputElement).value))} /></label>
          <label class="check"><input type="checkbox" checked={r.justify} onchange={(e) => prefs.update((x) => (x.reader.justify = (e.target as HTMLInputElement).checked))} /> Justify text</label>
          <label class="check"><input type="checkbox" checked={r.hyphenate} onchange={(e) => prefs.update((x) => (x.reader.hyphenate = (e.target as HTMLInputElement).checked))} /> Hyphenate</label>
        {:else if book?.format === 'pdf'}
          <h3>Fit</h3>
          <div class="seg">
            <button class:primary={p.fit === 'width' && p.zoom === 1} onclick={() => prefs.update((x) => { x.pdf.fit = 'width'; x.pdf.zoom = 1 })}>Width</button>
            <button class:primary={p.fit === 'page' && p.zoom === 1} onclick={() => prefs.update((x) => { x.pdf.fit = 'page'; x.pdf.zoom = 1 })}>Page</button>
          </div>
          <label>Zoom <span class="muted">{Math.round(p.zoom * 100)}%</span>
            <input type="range" min="0.25" max="4" step="0.05" value={p.zoom} oninput={(e) => prefs.update((x) => (x.pdf.zoom = +(e.target as HTMLInputElement).value))} /></label>
          <h3>Pages</h3>
          <div class="seg">
            <button class:primary={p.mode === 'continuous'} onclick={() => prefs.update((x) => (x.pdf.mode = 'continuous'))}>Continuous</button>
            <button class:primary={p.mode === 'single'} onclick={() => prefs.update((x) => (x.pdf.mode = 'single'))}>Single</button>
          </div>
          <label class="check"><input type="checkbox" checked={p.textLayer} onchange={(e) => prefs.update((x) => (x.pdf.textLayer = (e.target as HTMLInputElement).checked))} /> Selectable text</label>
        {/if}
      </aside>
    {/if}
  </div>

  {#if selection}
    <div class="selbar card" role="toolbar" aria-label="Selection">
      {#each Object.entries(HIGHLIGHT_COLORS) as [name, css]}
        <button class="swatch-btn" style:background={css} aria-label="Highlight {name}" onclick={() => highlight(name)}></button>
      {/each}
      <button onclick={() => highlight('yellow', true)}>Note</button>
      <button onclick={copySelection}>Copy</button>
      <button class="ghost icon" aria-label="Cancel" onclick={() => (selection = null)}>✕</button>
    </div>
  {/if}

  <footer class="bar foot">
    <span class="loc">{label}</span>
    <input type="range" min="0" max="1" step="0.001" value={percent} onchange={seek} aria-label="Position in book" />
    <span class="pct">{formatPercent(percent)}</span>
  </footer>
</div>

{#if editing}
  <div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (editing = null)}>
    <div class="modal card" role="dialog" aria-modal="true" aria-label="Edit highlight">
      <h2>Highlight</h2>
      <blockquote>{editing.text}</blockquote>
      <div class="row">
        {#each Object.entries(HIGHLIGHT_COLORS) as [name, css]}
          <button class="swatch-btn" class:sel={editing.color === name} style:background={css} aria-label={name} onclick={() => (editing!.color = name)}></button>
        {/each}
      </div>
      <textarea rows="4" placeholder="Add a note" bind:value={editing.note} style="width:100%;margin-top:12px"></textarea>
      <div class="row" style="margin-top:12px">
        <button class="danger" onclick={() => deleteAnnotation(editing!)}>Delete</button>
        <span class="spacer"></span>
        <button onclick={() => (editing = null)}>Cancel</button>
        <button class="primary" onclick={saveAnnotation}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .reader {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--reader-bg);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    transition: opacity 0.2s;
  }
  .title {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
  }
  .hide-chrome .bar {
    opacity: 0;
  }
  .hide-chrome .bar:hover,
  .hide-chrome .bar:focus-within {
    opacity: 1;
  }
  button.on {
    background: var(--surface-2);
  }
  .prompt {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 14px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }
  .prompt span {
    flex: 1;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .stage {
    flex: 1;
    min-width: 0;
    position: relative;
  }
  .panel {
    width: 320px;
    flex: none;
    overflow-y: auto;
    padding: 12px 14px;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }
  .panel.right {
    border-right: none;
    border-left: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .panel h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
    margin: 14px 0 6px;
  }
  .panel label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }
  .panel label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  .seg {
    display: flex;
    gap: 4px;
  }
  .seg button {
    flex: 1;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .toc button,
  .list button.left {
    width: 100%;
    text-align: left;
    white-space: normal;
  }
  .list li {
    display: flex;
    align-items: flex-start;
    gap: 4px;
  }
  .grow {
    flex: 1;
    text-align: left;
  }
  .ann .left {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .swatch {
    width: 28px;
    height: 6px;
    border-radius: 3px;
  }
  .quote {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .note {
    font-style: italic;
    color: var(--text-2);
  }
  .hits mark {
    background: var(--accent);
    color: var(--accent-ink);
    border-radius: 2px;
  }
  .small {
    font-size: 11px;
  }
  .center {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
    padding: 24px;
  }
  .bob {
    animation: bob 1.2s ease-in-out infinite;
  }
  @keyframes bob {
    50% {
      transform: translateY(-6px);
    }
  }
  .dl {
    width: 240px;
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
  }
  .dl div {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s;
  }
  .selbar {
    position: absolute;
    left: 50%;
    bottom: 56px;
    transform: translateX(-50%);
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 6px 8px;
    z-index: 20;
  }
  .swatch-btn {
    width: 28px;
    height: 28px;
    min-height: 28px;
    border-radius: 50%;
    padding: 0;
  }
  .swatch-btn.sel {
    outline: 2px solid var(--text);
  }
  .foot {
    border-top: 1px solid var(--border);
    border-bottom: none;
    font-size: 12px;
  }
  .foot input {
    flex: 1;
  }
  .loc {
    min-width: 180px;
    max-width: 30%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-2);
  }
  .pct {
    width: 44px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  blockquote {
    margin: 0 0 12px;
    padding-left: 12px;
    border-left: 3px solid var(--accent);
    color: var(--text-2);
  }
</style>
