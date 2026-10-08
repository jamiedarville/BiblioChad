<script lang="ts">
  import type { BookSummary } from '../lib/api'
  import { hueFor, protoUrl } from '../lib/util'

  let { book, size = 'md' }: { book: BookSummary; size?: 'sm' | 'md' } = $props()
  let failed = $state(false)
  const hue = $derived(hueFor(book.title + (book.author ?? '')))
</script>

<div class="cover {size}" style:--h={hue}>
  {#if book.has_cover && !failed}
    <img src={protoUrl(`cover/${book.id}`)} alt="" loading="lazy" decoding="async" onerror={() => (failed = true)} />
  {:else}
    <div class="placeholder" aria-hidden="true">
      <span class="t">{book.title}</span>
      {#if book.author}<span class="a">{book.author}</span>{/if}
      <span class="fmt">{book.format.toUpperCase()}</span>
    </div>
  {/if}
  {#if book.percent > 0 && book.status !== 'finished'}
    <div class="bar" aria-hidden="true"><div style:width="{Math.round(book.percent * 100)}%"></div></div>
  {/if}
  {#if book.status === 'finished'}
    <span class="badge done" title="Finished">✓</span>
  {/if}
  {#if book.pinned}
    <span class="badge pin" title="Available offline">⬇</span>
  {/if}
</div>

<style>
  .cover {
    position: relative;
    aspect-ratio: 2 / 3;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface-2);
    box-shadow: var(--shadow);
  }
  .cover.sm {
    width: 44px;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 8px;
    padding: 12px;
    background: linear-gradient(160deg, hsl(var(--h) 45% 38%), hsl(calc(var(--h) + 40) 50% 22%));
    color: #fff8ec;
    text-align: center;
    overflow: hidden;
  }
  .placeholder .t {
    font-family: Georgia, 'Times New Roman', serif;
    font-weight: 700;
    font-size: 15px;
    line-height: 1.2;
    display: -webkit-box;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .placeholder .a {
    font-size: 11px;
    opacity: 0.85;
  }
  .placeholder .fmt {
    position: absolute;
    bottom: 8px;
    left: 0;
    right: 0;
    font-size: 9px;
    letter-spacing: 0.15em;
    opacity: 0.6;
  }
  .sm .placeholder {
    padding: 2px;
  }
  .sm .placeholder .t,
  .sm .placeholder .a {
    display: none;
  }
  .bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 4px;
    background: rgb(0 0 0 / 0.35);
  }
  .bar div {
    height: 100%;
    background: var(--accent);
  }
  .badge {
    position: absolute;
    top: 6px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 12px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
  }
  .badge.done {
    right: 6px;
    background: var(--ok);
  }
  .badge.pin {
    left: 6px;
  }
  .sm .badge,
  .sm .bar {
    display: none;
  }
</style>
