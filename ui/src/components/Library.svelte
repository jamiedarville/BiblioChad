<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog'
  import {
    api,
    errorMessage,
    type BookFormat,
    type BookQuery,
    type BookSummary,
    type Collection,
    type FolderListing,
    type Overview,
    type ReadStatus,
    type SortKey,
  } from '../lib/api'
  import { prefs } from '../lib/prefs.svelte'
  import { toasts } from '../lib/toast.svelte'
  import { emptyLibrary, greeting, rankProgress } from '../lib/chad'
  import BookGrid from './BookGrid.svelte'
  import BookDetails from './BookDetails.svelte'
  import Cover from './Cover.svelte'
  import Icon from './Icon.svelte'

  let {
    version,
    syncMessage,
    onOpen,
    onSettings,
  }: { version: number; syncMessage: string | null; onOpen: (id: number) => void; onSettings: () => void } = $props()

  type Tab = 'shelves' | 'all' | 'recent' | 'favorites' | 'collections'
  let tab = $state<Tab>('shelves')
  let folderId = $state<string | null>(null)
  let search = $state('')
  let format = $state<BookFormat | ''>('')
  let status = $state<ReadStatus | ''>('')
  let collectionId = $state<number | null>(null)

  let overview = $state<Overview | null>(null)
  let listing = $state<FolderListing | null>(null)
  let books = $state<BookSummary[]>([])
  let collections = $state<Collection[]>([])
  let detailsId = $state<number | null>(null)
  let loading = $state(false)

  const chad = $derived(prefs.value.chadMode)
  const filtering = $derived(search.trim() !== '' || format !== '' || status !== '')
  const showFolders = $derived(tab === 'shelves' && !filtering)

  let searchTimer: ReturnType<typeof setTimeout> | null = null
  let debouncedSearch = $state('')
  $effect(() => {
    const s = search
    if (searchTimer) clearTimeout(searchTimer)
    searchTimer = setTimeout(() => (debouncedSearch = s), 180)
  })

  async function loadOverview() {
    try {
      overview = await api.overview()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function load() {
    loading = true
    try {
      const base: BookQuery = {
        search: debouncedSearch || null,
        sort: prefs.value.sort,
        descending: prefs.value.sortDesc,
        format: format || null,
        status: status || null,
      }
      if (tab === 'shelves') {
        if (filtering) {
          // Search within the current shelf, recursively.
          listing = listing ?? (await api.listFolder(folderId))
          books = await api.queryBooks({ ...base, folder_id: listing?.id ?? folderId })
        } else {
          listing = await api.listFolder(folderId)
          books = sortLocal(listing.books, prefs.value.sort, prefs.value.sortDesc)
        }
      } else if (tab === 'all') {
        books = await api.queryBooks(base)
      } else if (tab === 'recent') {
        books = await api.queryBooks({ ...base, sort: 'recently_read', descending: false })
        books = books.filter((b) => b.last_read_at)
      } else if (tab === 'favorites') {
        books = await api.queryBooks({ ...base, favorites_only: true })
      } else if (tab === 'collections') {
        collections = await api.collections()
        books = collectionId != null ? await api.queryBooks({ ...base, collection_id: collectionId }) : []
      }
    } catch (e) {
      toasts.push('error', errorMessage(e))
    } finally {
      loading = false
    }
  }

  function sortLocal(list: BookSummary[], key: SortKey, desc: boolean): BookSummary[] {
    const dir = desc ? -1 : 1
    const by: Record<SortKey, (a: BookSummary, b: BookSummary) => number> = {
      title: (a, b) => a.title.localeCompare(b.title, undefined, { numeric: true }) * dir,
      author: (a, b) => (a.author ?? '￿').localeCompare(b.author ?? '￿') * dir,
      recently_read: (a, b) => ((b.last_read_at ?? 0) - (a.last_read_at ?? 0)) * dir,
      added: (a, b) => (b.added_at - a.added_at) * dir,
      progress: (a, b) => (b.percent - a.percent) * dir,
    }
    return [...list].sort(by[key])
  }

  $effect(() => {
    void [version, tab, folderId, debouncedSearch, format, status, collectionId, prefs.value.sort, prefs.value.sortDesc]
    load()
  })
  $effect(() => {
    void version
    loadOverview()
  })

  function go(id: string | null) {
    folderId = id
    search = ''
    debouncedSearch = ''
    listing = null
  }

  async function importFiles() {
    const picked = await openDialog({
      multiple: true,
      filters: [{ name: 'Books', extensions: ['epub', 'pdf'] }],
    })
    if (!picked) return
    const paths = Array.isArray(picked) ? picked : [picked]
    try {
      const r = await api.importLocalFiles(paths)
      if (r.imported.length) toasts.push('success', `Imported ${r.imported.length} book${r.imported.length > 1 ? 's' : ''}.`)
      for (const f of r.failed) toasts.push('error', `Could not import ${f}`)
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function newCollection() {
    const name = prompt('Collection name')
    if (!name?.trim()) return
    try {
      collectionId = await api.createCollection(name)
      collections = await api.collections()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function removeCollection(c: Collection) {
    if (!confirm(`Delete the collection “${c.name}”? The books stay in your library.`)) return
    await api.deleteCollection(c.id)
    if (collectionId === c.id) collectionId = null
    load()
  }

  const empty = $derived(emptyLibrary(chad, overview?.drive.connected ?? false))
</script>

<div class="library">
  <header>
    <div class="brand">
      <img src="/mascot.svg" alt="" width="36" height="36" />
      <div>
        <h1>{greeting(chad, overview?.stats.streak_days ?? 0)}</h1>
        {#if overview && chad && overview.has_books}
          {@const s = overview.stats}
          <div class="rank" title="{s.finished} finished · {s.reading} in progress">
            <span>{s.rank.name}</span>
            <span class="meter"><span style:width="{rankProgress(s.rank, s.finished) * 100}%"></span></span>
            {#if s.rank.next_threshold}<span class="muted">{s.finished}/{s.rank.next_threshold}</span>{/if}
          </div>
        {/if}
      </div>
    </div>
    <div class="search">
      <input type="search" placeholder={showFolders || tab === 'shelves' ? 'Search this shelf' : 'Search title, author, series'} bind:value={search} aria-label="Search books" />
    </div>
    <div class="row tools">
      <select aria-label="Sort" value={prefs.value.sort} onchange={(e) => prefs.update((p) => (p.sort = (e.target as HTMLSelectElement).value as SortKey))}>
        <option value="title">Title</option>
        <option value="author">Author</option>
        <option value="recently_read">Recently read</option>
        <option value="added">Recently added</option>
        <option value="progress">Progress</option>
      </select>
      <button class="icon" title="Reverse order" aria-label="Reverse sort order" onclick={() => prefs.update((p) => (p.sortDesc = !p.sortDesc))}><Icon name={prefs.value.sortDesc ? 'sort-asc' : 'sort-desc'} /></button>
      <select aria-label="Format" bind:value={format}>
        <option value="">All formats</option>
        <option value="epub">EPUB</option>
        <option value="pdf">PDF</option>
      </select>
      <select aria-label="Status" bind:value={status}>
        <option value="">Any status</option>
        <option value="unread">Unread</option>
        <option value="reading">Reading</option>
        <option value="finished">Finished</option>
      </select>
      <button class="icon" title={prefs.value.view === 'grid' ? 'List view' : 'Grid view'} aria-label="Toggle grid or list" onclick={() => prefs.update((p) => (p.view = p.view === 'grid' ? 'list' : 'grid'))}><Icon name={prefs.value.view === 'grid' ? 'list' : 'grid'} /></button>
      <button onclick={importFiles} title="Import EPUB or PDF files from this computer">Import</button>
      <button class="icon" title="Settings" aria-label="Settings" onclick={onSettings}><Icon name="settings" /></button>
    </div>
  </header>

  <nav class="tabs" aria-label="Library views">
    {#each [['shelves', 'Shelves'], ['all', 'All books'], ['recent', 'Recently read'], ['favorites', 'Favorites'], ['collections', 'Collections']] as [id, label]}
      <button class="tab" class:active={tab === id} aria-pressed={tab === id} onclick={() => (tab = id as Tab)}>{label}</button>
    {/each}
    <span class="spacer"></span>
    {#if syncMessage}<span class="sync muted" role="status"><span class="spin"></span>{syncMessage}</span>{/if}
  </nav>

  <main>
    {#if overview && overview.continue_reading.length > 0 && !filtering && ((tab === 'shelves' && (listing?.breadcrumbs.length ?? 0) <= 1) || tab === 'all')}
      <section class="continue">
        <h2>{chad ? 'Unfinished business' : 'Continue reading'}</h2>
        <div class="strip">
          {#each overview.continue_reading as b (b.id)}
            <button class="cr" onclick={() => onOpen(b.id)} title="Continue {b.title}">
              <Cover book={b} />
              <span class="t">{b.title}</span>
              <span class="muted">{Math.round(b.percent * 100)}%</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if tab === 'shelves' && listing}
      <div class="crumbs" aria-label="Folder path">
        {#each listing.breadcrumbs as c, i (c.id)}
          {#if i > 0}<span class="sep">›</span>{/if}
          <button class="ghost crumb" disabled={i === listing.breadcrumbs.length - 1} onclick={() => go(c.id)}>{c.name}</button>
        {/each}
      </div>
    {/if}

    {#if tab === 'collections'}
      <div class="collections row">
        {#each collections as c (c.id)}
          <span class="chip" class:active={collectionId === c.id}>
            <button class="ghost" onclick={() => (collectionId = c.id)}>{c.name} <span class="muted">{c.book_count}</span></button>
            <button class="ghost icon" aria-label="Delete collection {c.name}" onclick={() => removeCollection(c)}>✕</button>
          </span>
        {/each}
        <button onclick={newCollection}>+ New collection</button>
      </div>
    {/if}

    {#if showFolders && listing && listing.folders.length > 0}
      <section class="folders">
        {#each listing.folders as f (f.id)}
          <button class="folder card" onclick={() => go(f.id)}>
            <span class="ficon" aria-hidden="true">{f.id === 'local:' ? '💻' : '📚'}</span>
            <span class="fname">{f.name}{f.is_shortcut ? ' ↗' : ''}</span>
            <span class="muted fcount">
              {f.book_count} book{f.book_count === 1 ? '' : 's'}{f.folder_count ? ` · ${f.folder_count} shelf${f.folder_count === 1 ? '' : 'es'}` : ''}
            </span>
          </button>
        {/each}
      </section>
    {/if}

    {#if books.length > 0}
      <BookGrid {books} mode={prefs.value.view} {onOpen} onDetails={(id) => (detailsId = id)} />
    {:else if !loading}
      {#if overview && !overview.has_books}
        <div class="empty">
          <img src="/mascot.svg" alt="" width="96" height="96" />
          <h2>{empty.title}</h2>
          <p class="muted">{empty.body}</p>
          <div class="row">
            {#if !overview.drive.connected || !overview.drive.root_id}
              <button class="primary" onclick={onSettings}>Connect Google Drive</button>
            {/if}
            <button onclick={importFiles}>Import files</button>
          </div>
        </div>
      {:else if filtering}
        <p class="muted pad">{chad ? 'Nothing. Not even Chad could find it.' : 'No books match.'}</p>
      {:else if tab === 'collections' && collectionId == null}
        <p class="muted pad">Pick a collection, or make one. Collections are separate from your Drive folders, so a book can be in several.</p>
      {:else if tab === 'recent'}
        <p class="muted pad">Nothing read yet{chad ? '. The gym is open.' : '.'}</p>
      {:else if tab === 'favorites'}
        <p class="muted pad">No favorites yet. Use ⋯ on a book to add one.</p>
      {:else if showFolders && listing && listing.folders.length === 0}
        <p class="muted pad">{empty.body}</p>
      {/if}
    {/if}
  </main>
</div>

{#if detailsId != null}
  <BookDetails
    id={detailsId}
    onClose={() => {
      detailsId = null
      load()
    }}
    onOpen={(id) => {
      detailsId = null
      onOpen(id)
    }}
  />
{/if}

<style>
  .library {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 24px 10px;
    flex-wrap: wrap;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 220px;
  }
  h1 {
    font-family: var(--font-display);
    font-size: 20px;
    margin: 0;
  }
  .rank {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
    color: var(--accent);
    font-weight: 600;
  }
  .meter {
    width: 80px;
    height: 5px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .search {
    flex: 1;
    min-width: 200px;
  }
  .search input {
    width: 100%;
  }
  .tools {
    flex-wrap: wrap;
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 0 24px;
    border-bottom: 1px solid var(--border);
    align-items: center;
  }
  .tab {
    border: none;
    background: none;
    border-radius: 0;
    border-bottom: 2px solid transparent;
    padding: 8px 12px;
    color: var(--text-2);
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
    font-weight: 600;
  }
  .sync {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
  }
  .spin {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  main {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px 48px;
  }
  h2 {
    font-family: var(--font-display);
    font-size: 15px;
    margin: 0 0 10px;
  }
  .continue {
    margin-bottom: 20px;
  }
  .strip {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    padding-bottom: 8px;
  }
  .cr {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 104px;
    flex: none;
    padding: 0;
    border: none;
    background: none;
    text-align: left;
  }
  .cr .t {
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .crumbs {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px;
    margin-bottom: 12px;
  }
  .crumb {
    padding: 4px 8px;
    font-weight: 600;
  }
  .crumb:disabled {
    opacity: 1;
    color: var(--text);
  }
  .sep {
    color: var(--text-2);
  }
  .folders {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }
  .folder {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    column-gap: 10px;
    text-align: left;
    padding: 12px 14px;
    box-shadow: none;
  }
  .ficon {
    grid-row: span 2;
    font-size: 24px;
    align-self: center;
  }
  .fname {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fcount {
    font-size: 12px;
  }
  .collections {
    flex-wrap: wrap;
    margin-bottom: 16px;
  }
  .chip {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 999px;
    overflow: hidden;
  }
  .chip button {
    border-radius: 0;
  }
  .chip.active {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 64px 16px;
    gap: 6px;
  }
  .empty h2 {
    font-size: 22px;
    margin: 12px 0 0;
  }
  .pad {
    padding: 24px 0;
  }
</style>
