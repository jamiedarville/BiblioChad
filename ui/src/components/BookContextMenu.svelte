<script lang="ts">
  import { tick } from 'svelte'
  import { api, errorMessage, type BookSummary, type Collection } from '../lib/api'
  import { toasts } from '../lib/toast.svelte'

  let { book, x, y, onClose, onDetails, onChanged }: {
    book: BookSummary
    x: number
    y: number
    onClose: () => void
    onDetails: (id: number) => void
    onChanged: () => void
  } = $props()

  let panel: HTMLDivElement
  let choosing = $state(false)
  let loading = $state(false)
  let saving = $state(false)
  let collections = $state<Collection[]>([])
  let memberships = $state<number[]>([])
  let name = $state('')
  let left = $state(0)
  let top = $state(0)

  $effect(() => {
    panel.showPopover()
    panel.querySelector<HTMLButtonElement>('button')?.focus()
  })

  $effect(() => {
    // Reposition after the picker or its collection list changes size.
    void [choosing, loading, collections.length]
    tick().then(reposition)
  })

  function reposition() {
    if (!panel?.isConnected) return
    const bounds = panel.getBoundingClientRect()
    left = Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8))
    const nativeHeight = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--android-visible-height'))
    const height = Math.min(window.innerHeight, nativeHeight || Infinity, window.visualViewport?.height ?? Infinity)
    const safeTop = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--android-safe-top')) || 0
    top = Math.max(safeTop + 8, Math.min(y, height - bounds.height - 8))
  }

  $effect(() => {
    const viewport = window.visualViewport
    viewport?.addEventListener('resize', reposition)
    window.addEventListener('android-viewport', reposition)
    return () => {
      viewport?.removeEventListener('resize', reposition)
      window.removeEventListener('android-viewport', reposition)
    }
  })

  async function chooseCollection() {
    choosing = true
    loading = true
    try {
      const [available, details] = await Promise.all([api.collections(), api.bookDetails(book.id)])
      collections = available
      memberships = details.collections
    } catch (e) {
      toasts.push('error', errorMessage(e))
      onClose()
    } finally {
      loading = false
      await tick()
      panel?.querySelector<HTMLElement>('button:not(:disabled), input')?.focus()
    }
  }

  async function add(collection: Collection) {
    if (saving) return
    saving = true
    try {
      await api.setInCollection(collection.id, book.id, true)
      toasts.push('success', `Added “${book.title}” to “${collection.name}”.`)
      onChanged()
      onClose()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    } finally {
      saving = false
    }
  }

  async function createCollection(e: SubmitEvent) {
    e.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || saving) return
    saving = true
    try {
      const id = await api.createCollection(trimmed)
      // Keep the new collection available if adding the book needs a retry.
      collections = [...collections, { id, name: trimmed, book_count: 0 }]
      name = ''
      saving = false
      await add({ id, name: trimmed, book_count: 0 })
    } catch (e) {
      toasts.push('error', errorMessage(e))
    } finally {
      saving = false
    }
  }

  function dismissOutside(e: MouseEvent) {
    // WebKit can open context menus before the secondary button is released.
    // Use mousedown: some WebKit input paths omit pointerdown after that gesture.
    // Only a new primary-button press outside should dismiss this popover.
    if (e.button === 0 && e.target instanceof Node && !panel.contains(e.target)) onClose()
  }

  function escape(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  function key(e: KeyboardEvent) {
    if (choosing || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return
    e.preventDefault()
    const buttons = [...panel.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1
      : (index + (e.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
    buttons[next]?.focus()
  }
</script>

<svelte:window onresize={reposition} onmousedown={dismissOutside} onkeydown={escape} />

<div
  bind:this={panel}
  popover="manual"
  class="book-menu card"
  class:picker={choosing}
  style:left="{left}px"
  style:top="{top}px"
  role={choosing ? 'dialog' : 'menu'}
  aria-label={choosing ? `Add ${book.title} to collection` : `Actions for ${book.title}`}
  tabindex="-1"
  onkeydown={key}
  oncontextmenu={(e) => e.preventDefault()}
>
  {#if choosing}
    <h3>Add to Collection</h3>
    <p class="muted title">{book.title}</p>
    {#if loading}
      <p class="muted" role="status">Loading collections…</p>
    {:else}
      <div class="choices" aria-busy={saving}>
        {#each collections as collection (collection.id)}
          <button disabled={saving || memberships.includes(collection.id)} onclick={() => add(collection)}>
            <span>{collection.name}</span>
            {#if memberships.includes(collection.id)}<span class="muted">Already added ✓</span>{/if}
          </button>
        {:else}
          <p class="muted">No collections yet. Create one below.</p>
        {/each}
      </div>
      <form onsubmit={createCollection}>
        <label for="new-book-collection">New collection</label>
        <input id="new-book-collection" bind:value={name} placeholder="Collection name" disabled={saving} />
        <button class="primary" type="submit" disabled={saving || !name.trim()}>Create and add</button>
      </form>
    {/if}
    <button class="cancel" onclick={onClose}>Cancel</button>
  {:else}
    <button role="menuitem" onclick={chooseCollection}>Add to Collection</button>
    <button role="menuitem" onclick={() => { onClose(); onDetails(book.id) }}>Book details</button>
  {/if}
</div>

<style>
  .book-menu {
    position: fixed;
    inset: auto;
    margin: 0;
    padding: 6px;
    min-width: 190px;
    max-width: calc(100vw - 16px);
    max-height: calc(var(--android-visible-height, 100vh) - var(--android-safe-top, 0px) - 16px);
    overflow: auto;
    color: var(--text);
  }
  .book-menu > button, .choices button {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    text-align: left;
    border-color: transparent;
    overflow-wrap: anywhere;
  }
  .picker { width: 320px; padding: 14px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  .title { margin: 0 0 12px; overflow-wrap: anywhere; }
  .choices { max-height: 220px; overflow: auto; }
  .choices .muted { font-size: 12px; flex-shrink: 0; }
  form { display: grid; gap: 8px; margin-top: 12px; }
  label { font-size: 12px; color: var(--text-2); }
  input { width: 100%; min-width: 0; }
  .cancel { margin-top: 8px; }
</style>
