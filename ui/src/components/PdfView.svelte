<script lang="ts">
  // PDF pages are rendered by PDFium in Rust and served as PNGs through the
  // bibliochad:// protocol. Only pages near the viewport are mounted.
  import { onMount } from 'svelte'
  import { api, type PdfLocator, type TextRun } from '../lib/api'
  import type { PdfPrefs } from '../lib/prefs.svelte'
  import { pageAt, protoUrl, renderWidth } from '../lib/util'

  let {
    bookId,
    pageSizes,
    start,
    pdf,
    onRelocate,
    onKey,
    onZoom,
  }: {
    bookId: number
    pageSizes: [number, number][]
    start: PdfLocator | null
    pdf: PdfPrefs
    onRelocate: (loc: PdfLocator, label: string) => void
    onKey: (e: KeyboardEvent) => void
    onZoom: (zoom: number) => void
  } = $props()

  const GAP = 14
  const PAD = 16
  let scroller: HTMLDivElement
  let cw = $state(800)
  let ch = $state(600)
  let scrollTop = $state(0)
  let current = $state(0)
  let textRuns = $state<Record<number, TextRun[]>>({})
  let restored = false

  const count = $derived(pageSizes.length)
  const dpr = typeof devicePixelRatio === 'number' ? devicePixelRatio : 1

  // Page widths in CSS px for the fit mode and zoom.
  const widths = $derived(
    pageSizes.map(([w, h]) => {
      const avail = Math.max(200, cw - PAD * 2)
      const base = pdf.fit === 'page' ? Math.min(avail, ((ch - PAD * 2) * w) / h) : avail
      return Math.round(base * pdf.zoom)
    }),
  )
  const heights = $derived(pageSizes.map(([w, h], i) => Math.round((widths[i] * h) / w)))
  const offsets = $derived.by(() => {
    const out: number[] = []
    let y = PAD
    for (const h of heights) {
      out.push(y)
      y += h + GAP
    }
    return out
  })
  const total = $derived(count ? offsets[count - 1] + heights[count - 1] + PAD : 0)

  const visible = $derived.by(() => {
    if (pdf.mode === 'single') return count ? [current] : []
    const top = scrollTop - ch
    const bottom = scrollTop + ch * 2
    const out: number[] = []
    for (let i = 0; i < count; i++) {
      if (offsets[i] + heights[i] >= top && offsets[i] <= bottom) out.push(i)
    }
    return out
  })

  function src(i: number) {
    return protoUrl(`pdf/${bookId}/page/${i}?w=${renderWidth(widths[i], dpr)}`)
  }

  function emit() {
    let loc: { page: number; offset: number }
    if (pdf.mode === 'single') {
      const h = Math.max(1, scroller.scrollHeight - scroller.clientHeight)
      loc = { page: current, offset: scroller.scrollTop / h }
    } else {
      loc = pageAt(offsets, heights.map((h) => h + GAP), scroller.scrollTop + 1)
      current = loc.page
    }
    onRelocate(
      { kind: 'pdf', page: loc.page, offset: loc.offset, fit: pdf.fit, zoom: pdf.zoom },
      `Page ${loc.page + 1} of ${count}`,
    )
  }

  function onScroll() {
    scrollTop = scroller.scrollTop
    if (restored) emit()
  }

  export function goToPage(page: number, offset = 0) {
    const p = Math.max(0, Math.min(count - 1, page))
    current = p
    if (pdf.mode === 'single') {
      requestAnimationFrame(() => {
        scroller.scrollTop = offset * Math.max(0, scroller.scrollHeight - scroller.clientHeight)
        emit()
      })
    } else {
      scroller.scrollTop = offsets[p] + offset * (heights[p] + GAP) - 1
      scrollTop = scroller.scrollTop
      emit()
    }
  }

  export function next() {
    if (pdf.mode === 'single') return goToPage(current + 1)
    // Continuous: scroll by most of a screen.
    scroller.scrollBy({ top: scroller.clientHeight * 0.9, behavior: 'smooth' })
  }
  export function prev() {
    if (pdf.mode === 'single') return goToPage(current - 1)
    scroller.scrollBy({ top: -scroller.clientHeight * 0.9, behavior: 'smooth' })
  }
  export function goToFraction(f: number) {
    goToPage(Math.round(f * (count - 1)))
  }

  // Keep the reading position stable when fit/zoom/mode/size change.
  let lastLayoutKey = ''
  $effect(() => {
    const key = `${pdf.fit}|${pdf.zoom}|${pdf.mode}|${cw}|${ch}`
    if (!restored || key === lastLayoutKey) {
      lastLayoutKey = key
      return
    }
    lastLayoutKey = key
    const page = current
    queueMicrotask(() => goToPage(page))
  })

  // Text layer for visible pages.
  $effect(() => {
    if (!pdf.textLayer) return
    for (const i of visible) {
      if (textRuns[i] === undefined) {
        textRuns[i] = []
        api.pdfText(bookId, i).then((runs) => (textRuns[i] = runs)).catch(() => {})
      }
    }
  })

  const measure = (() => {
    let ctx: CanvasRenderingContext2D | null = null
    return (text: string, px: number) => {
      ctx ??= document.createElement('canvas').getContext('2d')
      if (!ctx) return text.length * px * 0.5
      ctx.font = `${px}px sans-serif`
      return ctx.measureText(text).width || 1
    }
  })()

  function wheel(e: WheelEvent) {
    if (e.ctrlKey) {
      e.preventDefault()
      const z = Math.min(4, Math.max(0.25, pdf.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1)))
      onZoom(Math.round(z * 100) / 100)
    } else if (pdf.mode === 'single') {
      const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2
      const atStart = scroller.scrollTop <= 0
      if (e.deltaY > 30 && atEnd) next()
      else if (e.deltaY < -30 && atStart) prev()
    }
  }

  onMount(() => {
    const ro = new ResizeObserver(() => {
      cw = scroller.clientWidth
      ch = scroller.clientHeight
    })
    ro.observe(scroller)
    cw = scroller.clientWidth
    ch = scroller.clientHeight
    requestAnimationFrame(() => {
      goToPage(start?.page ?? 0, start?.offset ?? 0)
      restored = true
    })
    return () => ro.disconnect()
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="scroller"
  bind:this={scroller}
  onscroll={onScroll}
  onwheel={wheel}
  onkeydown={onKey}
  tabindex="-1"
  role="document"
  aria-label="PDF pages"
>
  {#if pdf.mode === 'single'}
    {#if count}
      {@const i = current}
      <div class="single" style:padding="{PAD}px">
        <div class="page" style:width="{widths[i]}px" style:height="{heights[i]}px">
          <img src={src(i)} alt="Page {i + 1}" draggable="false" />
          {#if pdf.textLayer && textRuns[i]}
            <div class="text-layer">
              {#each textRuns[i] as r}
                {@const fs = r.h * heights[i] * 0.88}
                <span
                  style:left="{r.x * 100}%"
                  style:top="{r.y * 100}%"
                  style:font-size="{fs}px"
                  style:transform="scaleX({(r.w * widths[i]) / measure(r.text, fs)})">{r.text}</span
                >
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {:else}
    <div class="stack" style:height="{total}px">
      {#each visible as i (i)}
        <div
          class="page abs"
          style:top="{offsets[i]}px"
          style:width="{widths[i]}px"
          style:height="{heights[i]}px"
          style:left="max({PAD}px, calc(50% - {widths[i] / 2}px))"
        >
          <img src={src(i)} alt="Page {i + 1}" draggable="false" loading="lazy" />
          {#if pdf.textLayer && textRuns[i]}
            <div class="text-layer">
              {#each textRuns[i] as r}
                {@const fs = r.h * heights[i] * 0.88}
                <span
                  style:left="{r.x * 100}%"
                  style:top="{r.y * 100}%"
                  style:font-size="{fs}px"
                  style:transform="scaleX({(r.w * widths[i]) / measure(r.text, fs)})">{r.text}</span
                >
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .scroller {
    width: 100%;
    height: 100%;
    overflow: auto;
    background: var(--surface-2);
    outline: none;
  }
  .stack {
    position: relative;
    min-width: 100%;
  }
  .single {
    display: flex;
    justify-content: center;
    min-width: fit-content;
  }
  .page {
    background: #fff;
    box-shadow: var(--shadow);
    position: relative;
  }
  .page.abs {
    position: absolute;
  }
  .page img {
    width: 100%;
    height: 100%;
    display: block;
    user-select: none;
  }
  :global([data-theme='dark']) .page img {
    filter: invert(0.88) hue-rotate(180deg);
  }
  .text-layer {
    position: absolute;
    inset: 0;
    overflow: hidden;
    line-height: 1;
  }
  .text-layer span {
    position: absolute;
    white-space: pre;
    color: transparent;
    transform-origin: 0 0;
    font-family: sans-serif;
    cursor: text;
  }
  .text-layer span::selection {
    background: rgb(80 140 255 / 0.35);
  }
</style>
