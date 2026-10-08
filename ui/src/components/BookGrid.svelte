<script lang="ts">
  // Virtualized grid/list: only rows near the viewport are rendered, so a
  // 5,000-book library scrolls smoothly.
  import type { BookSummary } from '../lib/api'
  import Cover from './Cover.svelte'
  import { formatPercent } from '../lib/util'

  let {
    books,
    mode,
    onOpen,
    onDetails,
  }: {
    books: BookSummary[]
    mode: 'grid' | 'list'
    onOpen: (id: number) => void
    onDetails: (id: number) => void
  } = $props()

  let host: HTMLDivElement
  let width = $state(800)
  let scrollTop = $state(0)
  let viewport = $state(800)

  const CARD_W = 150
  const GAP = 20
  const cols = $derived(mode === 'list' ? 1 : Math.max(1, Math.floor((width + GAP) / (CARD_W + GAP))))
  const rowH = $derived(mode === 'list' ? 64 : Math.round(((width - GAP * (cols - 1)) / cols) * 1.5) + 64)
  const rows = $derived(Math.ceil(books.length / cols))
  const first = $derived(Math.max(0, Math.floor(scrollTop / rowH) - 3))
  const last = $derived(Math.min(rows, Math.ceil((scrollTop + viewport) / rowH) + 3))
  const visible = $derived(
    Array.from({ length: Math.max(0, last - first) }, (_, i) => first + i).map((r) => ({
      row: r,
      items: books.slice(r * cols, r * cols + cols),
    })),
  )

  function scrollParent(el: HTMLElement): HTMLElement {
    let p = el.parentElement
    while (p) {
      const o = getComputedStyle(p).overflowY
      if (o === 'auto' || o === 'scroll') return p
      p = p.parentElement
    }
    return document.documentElement
  }

  $effect(() => {
    const sp = scrollParent(host)
    const update = () => {
      const hostTop = host.getBoundingClientRect().top - sp.getBoundingClientRect().top + sp.scrollTop
      scrollTop = Math.max(0, sp.scrollTop - hostTop)
      viewport = sp.clientHeight
      width = host.clientWidth
    }
    update()
    const ro = new ResizeObserver(update)
    ro.observe(host)
    ro.observe(sp)
    sp.addEventListener('scroll', update, { passive: true })
    return () => {
      ro.disconnect()
      sp.removeEventListener('scroll', update)
    }
  })

  function key(e: KeyboardEvent, id: number) {
    if (e.key === 'Enter') onOpen(id)
    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault()
      onDetails(id)
    }
  }
</script>

<div class="grid-host" bind:this={host} style:height="{rows * rowH}px">
  {#each visible as { row, items } (row)}
    <div
      class="row {mode}"
      style:top="{row * rowH}px"
      style:height="{rowH}px"
      style:grid-template-columns="repeat({cols}, minmax(0, 1fr))"
    >
      {#each items as b (b.id)}
        <div
          class="item"
          role="button"
          tabindex="0"
          aria-label="Open {b.title}"
          title={b.title}
          onclick={() => onOpen(b.id)}
          onkeydown={(e) => key(e, b.id)}
          oncontextmenu={(e) => {
            e.preventDefault()
            onDetails(b.id)
          }}
        >
          {#if mode === 'grid'}
            <Cover book={b} />
            <div class="meta">
              <div class="title">{b.title}</div>
              <div class="sub">
                <span class="author">{b.author ?? ''}</span>
                <button
                  class="ghost icon more"
                  aria-label="Details for {b.title}"
                  onclick={(e) => {
                    e.stopPropagation()
                    onDetails(b.id)
                  }}>⋯</button
                >
              </div>
            </div>
          {:else}
            <Cover book={b} size="sm" />
            <div class="lmeta">
              <div class="title">{b.title}</div>
              <div class="sub">{b.author ?? 'Unknown author'}{b.series ? ` · ${b.series}${b.series_index ? ` #${b.series_index}` : ''}` : ''}</div>
            </div>
            <span class="pill">{b.format.toUpperCase()}</span>
            <span class="pct">{b.status === 'finished' ? 'Finished' : b.percent > 0 ? formatPercent(b.percent) : ''}</span>
            {#if b.favorite}<span title="Favorite">★</span>{/if}
            <button
              class="ghost icon"
              aria-label="Details for {b.title}"
              onclick={(e) => {
                e.stopPropagation()
                onDetails(b.id)
              }}>⋯</button
            >
          {/if}
        </div>
      {/each}
    </div>
  {/each}
</div>

<style>
  .grid-host {
    position: relative;
    width: 100%;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: grid;
    gap: 20px;
  }
  .row.list {
    gap: 0;
  }
  .item {
    cursor: pointer;
    border-radius: 8px;
    outline-offset: 4px;
    min-width: 0;
  }
  .row.grid .item:hover :global(.cover) {
    transform: translateY(-2px);
  }
  .row.grid .item :global(.cover) {
    transition: transform 0.15s;
  }
  .meta {
    padding-top: 8px;
  }
  .title {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    display: flex;
    align-items: center;
    color: var(--text-2);
    font-size: 12px;
    min-height: 24px;
  }
  .author {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    min-height: 22px;
    padding: 0 6px;
    opacity: 0;
  }
  .item:hover .more,
  .item:focus-within .more {
    opacity: 1;
  }
  .row.list .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    height: 64px;
  }
  .row.list .item:hover {
    background: var(--surface-2);
  }
  .lmeta {
    flex: 1;
    min-width: 0;
  }
  .lmeta .sub {
    display: block;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-height: 0;
  }
  .pill {
    font-size: 10px;
    letter-spacing: 0.08em;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text-2);
  }
  .pct {
    width: 64px;
    text-align: right;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
</style>
