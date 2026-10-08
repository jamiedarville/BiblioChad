<script lang="ts">
  import { save as saveDialog } from '@tauri-apps/plugin-dialog'
  import { api, errorMessage, type BookDetails, type Collection, type ReadStatus } from '../lib/api'
  import { toasts } from '../lib/toast.svelte'
  import { formatBytes, formatPercent, timeAgo } from '../lib/util'
  import Cover from './Cover.svelte'

  let { id, onClose, onOpen }: { id: number; onClose: () => void; onOpen: (id: number) => void } = $props()

  let book = $state<BookDetails | null>(null)
  let collections = $state<Collection[]>([])
  let editing = $state(false)
  let title = $state('')
  let author = $state('')
  let series = $state('')

  async function load() {
    try {
      book = await api.bookDetails(id)
      collections = await api.collections()
      title = book.title_override ?? ''
      author = book.author_override ?? ''
      series = book.series_override ?? ''
    } catch (e) {
      toasts.push('error', errorMessage(e))
      onClose()
    }
  }
  $effect(() => {
    void id
    load()
  })

  async function run(fn: () => Promise<unknown>) {
    try {
      await fn()
      await load()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function saveOverrides() {
    await run(() => api.setOverrides(id, title || null, author || null, series || null))
    editing = false
  }

  async function exportNotes() {
    if (!book) return
    const dest = await saveDialog({
      defaultPath: `${book.title.replace(/[\\/:*?"<>|]/g, '_')} - highlights.md`,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    })
    if (!dest) return
    await run(() => api.exportAnnotations(id, dest))
    toasts.push('success', 'Highlights exported.')
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose()
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div class="modal card" role="dialog" aria-modal="true" aria-label="Book details">
    {#if book}
      <div class="top">
        <div class="cov"><Cover {book} /></div>
        <div class="info">
          <h2>{book.title}</h2>
          <div class="muted">{book.author ?? 'Unknown author'}</div>
          {#if book.series}<div class="muted">{book.series}{book.series_index ? ` #${book.series_index}` : ''}</div>{/if}
          <dl>
            <dt>Format</dt><dd>{book.format.toUpperCase()}{book.page_count ? ` · ${book.page_count} pages` : ''}</dd>
            <dt>Progress</dt><dd>{book.status === 'finished' ? 'Finished' : formatPercent(book.percent)}{book.last_read_at ? ` · read ${timeAgo(book.last_read_at)}` : ''}</dd>
            {#if book.folder_path.length}<dt>Shelf</dt><dd>{book.folder_path.join(' › ')}</dd>{/if}
            <dt>File</dt><dd class="file">{book.file_name}{book.size ? ` · ${formatBytes(book.size)}` : ''}{book.cached ? ' · downloaded' : ''}</dd>
            {#if book.publisher}<dt>Publisher</dt><dd>{book.publisher}</dd>{/if}
            {#if book.language}<dt>Language</dt><dd>{book.language}</dd>{/if}
            {#if book.isbn}<dt>ISBN</dt><dd>{book.isbn}</dd>{/if}
          </dl>
          <div class="row wrap">
            <button class="primary" onclick={() => onOpen(id)}>{book.percent > 0 ? 'Continue' : 'Read'}</button>
            <button onclick={() => run(() => api.setFavorite(id, !book!.favorite))}>{book.favorite ? '★ Favorite' : '☆ Favorite'}</button>
            <button onclick={() => run(() => api.setPinned(id, !book!.pinned))} title="Keep this book downloaded for offline reading">
              {book.pinned ? '⬇ Offline: on' : '⬇ Keep offline'}
            </button>
          </div>
        </div>
      </div>

      {#if book.description}
        <!-- Descriptions come from untrusted book files: shown as plain text. -->
        <p class="desc">{book.description.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim()}</p>
      {/if}

      <h3>Status</h3>
      <div class="row wrap">
        {#each [['unread', 'Unread'], ['reading', 'Reading'], ['finished', 'Finished']] as [s, label]}
          <button class:primary={book.status === s} onclick={() => run(() => api.setStatus(id, s as ReadStatus))}>{label}</button>
        {/each}
        <button class="ghost" onclick={() => run(() => api.setStatus(id, null))} title="Let progress decide">Auto</button>
      </div>

      <h3>Collections</h3>
      <div class="row wrap">
        {#each collections as c (c.id)}
          <label class="check">
            <input
              type="checkbox"
              checked={book.collections.includes(c.id)}
              onchange={(e) => run(() => api.setInCollection(c.id, id, (e.target as HTMLInputElement).checked))}
            />
            {c.name}
          </label>
        {:else}
          <span class="muted">No collections yet. Create one from the Collections tab.</span>
        {/each}
      </div>

      <h3>Local metadata</h3>
      {#if editing}
        <div class="form">
          <label>Title <input bind:value={title} placeholder={book.title} /></label>
          <label>Author <input bind:value={author} placeholder={book.author ?? ''} /></label>
          <label>Series <input bind:value={series} placeholder={book.series ?? ''} /></label>
          <p class="muted small">Edits stay on this computer. BiblioChad never changes your files in Drive. Leave a field empty to use the book's own value.</p>
          <div class="row"><button class="primary" onclick={saveOverrides}>Save</button><button onclick={() => (editing = false)}>Cancel</button></div>
        </div>
      {:else}
        <div class="row wrap">
          <button onclick={() => (editing = true)}>Edit title, author, series</button>
          <button onclick={exportNotes}>Export highlights (Markdown)</button>
          {#if book.key.startsWith('local:')}
            <button class="danger" onclick={() => run(() => api.removeLocalBook(id)).then(onClose)}>Remove from library</button>
          {/if}
        </div>
      {/if}
      <div class="row end"><button onclick={onClose}>Close</button></div>
    {/if}
  </div>
</div>

<style>
  .top {
    display: flex;
    gap: 18px;
  }
  .cov {
    width: 140px;
    flex: none;
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .info h2 {
    margin-bottom: 2px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
    margin: 12px 0;
    font-size: 13px;
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  .file {
    word-break: break-all;
  }
  .desc {
    font-size: 13px;
    line-height: 1.5;
    max-height: 140px;
    overflow: auto;
    color: var(--text-2);
  }
  h3 {
    font-size: 13px;
    margin: 16px 0 8px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
  }
  .wrap {
    flex-wrap: wrap;
  }
  .check {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
  }
  .form {
    display: grid;
    gap: 8px;
  }
  .form label {
    display: grid;
    gap: 4px;
    font-size: 12px;
    color: var(--text-2);
  }
  .small {
    font-size: 12px;
    margin: 0;
  }
  .end {
    justify-content: flex-end;
    margin-top: 16px;
  }
</style>
