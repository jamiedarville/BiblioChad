<script lang="ts">
  import { onMount } from 'svelte'
  import { open as openDialog } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import {
    api,
    errorKind,
    errorMessage,
    type AppInfo,
    type CacheStats,
    type DriveStatus,
    type FolderEntry,
    type Stats,
  } from '../lib/api'
  import { prefs } from '../lib/prefs.svelte'
  import { toasts } from '../lib/toast.svelte'
  import { formatBytes, timeAgo } from '../lib/util'
  import { achievements } from '../lib/chad'
  import Icon from './Icon.svelte'

  let { onBack }: { onBack: () => void } = $props()

  let status = $state<DriveStatus | null>(null)
  let cache = $state<CacheStats | null>(null)
  let info = $state<AppInfo | null>(null)
  let stats = $state<Stats | null>(null)
  let connecting = $state(false)
  let picking = $state(false)
  let stack = $state<FolderEntry[]>([])
  let folders = $state<FolderEntry[]>([])
  let loadingFolders = $state(false)

  async function refresh() {
    try {
      ;[status, cache, info] = await Promise.all([api.driveStatus(), api.cacheStats(), api.appInfo()])
      stats = (await api.overview()).stats
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }
  onMount(refresh)

  async function importClientJson() {
    const path = await openDialog({ filters: [{ name: 'OAuth client JSON', extensions: ['json'] }] })
    if (!path || Array.isArray(path)) return
    try {
      await api.driveImportClientFile(path)
      toasts.push('success', 'OAuth client saved.')
      refresh()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function connect() {
    connecting = true
    try {
      status = await api.driveConnect()
      toasts.push('success', prefs.value.chadMode ? 'Drive connected. Chad has the keys.' : 'Google Drive connected.')
      if (!status.root_id) startPicking()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    } finally {
      connecting = false
    }
  }

  async function startPicking() {
    picking = true
    stack = []
    await loadFolders(null)
  }

  async function loadFolders(parent: FolderEntry | null) {
    loadingFolders = true
    try {
      folders = await api.driveListFolders(parent?.id ?? null)
    } catch (e) {
      toasts.push('error', errorMessage(e))
      if (errorKind(e) === 'reauth') picking = false
    } finally {
      loadingFolders = false
    }
  }

  async function enter(f: FolderEntry) {
    stack = [...stack, f]
    await loadFolders(f)
  }

  async function up(i: number) {
    stack = stack.slice(0, i)
    await loadFolders(stack.at(-1) ?? null)
  }

  async function choose() {
    const f = stack.at(-1)
    if (!f) return
    // Items inside a shared drive inherit the drive id from the first crumb.
    const shared = stack.find((s) => s.shared_drive_id)?.shared_drive_id ?? f.shared_drive_id
    try {
      await api.driveChooseRoot({ ...f, shared_drive_id: shared ?? null })
      picking = false
      toasts.push('info', prefs.value.chadMode ? 'Scanning your shelves. Spot me.' : 'Scanning the folder…')
      refresh()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function syncNow() {
    try {
      await api.driveSyncNow()
      toasts.push('success', 'Library is up to date.')
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
    refresh()
  }

  async function disconnect(wipe: boolean) {
    const msg = wipe
      ? 'Disconnect Google Drive and delete all local data (cache, covers, progress, highlights, collections)? Progress already synced to your Drive stays there.'
      : 'Disconnect Google Drive? Your local progress and notes are kept.'
    if (!confirm(msg)) return
    try {
      await api.driveDisconnect(wipe)
      toasts.push('info', wipe ? 'Disconnected and wiped.' : 'Disconnected.')
      refresh()
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function clearCache() {
    await api.clearCache()
    cache = await api.cacheStats()
    toasts.push('info', 'Cache cleared (pinned books kept).')
  }
</script>

<div class="settings">
  <header class="row">
    <button class="ghost" onclick={onBack}><Icon name="back" /> Library</button>
    <h1>Settings</h1>
  </header>

  <main>
    <section class="card">
      <h2>Google Drive</h2>
      {#if status}
        {#if !status.configured}
          <p>
            BiblioChad needs a Google OAuth client of type <strong>Desktop app</strong>. Create one in Google Cloud console
            (enable the Drive API, set the consent screen to <em>In production</em> so sign-ins don't expire after 7 days),
            download its JSON and import it here.
          </p>
          <div class="row">
            <button class="primary" onclick={importClientJson}>Import client JSON…</button>
            <button class="ghost" onclick={() => openUrl('https://console.cloud.google.com/apis/credentials').catch((e) => toasts.push('error', `Could not open the browser: ${errorMessage(e)}`))}>Open Google Cloud console</button>
          </div>
        {:else if !status.connected}
          <p>Connect to choose the Drive folder that becomes your library. Access is read-only; BiblioChad never changes your books.</p>
          <div class="row">
            <button class="primary" disabled={connecting} onclick={connect}>{connecting ? 'Waiting for the browser…' : 'Connect Google Drive'}</button>
            <button class="ghost" onclick={importClientJson}>Replace client JSON…</button>
          </div>
          {#if connecting}<p class="muted small">Finish signing in in your browser, then come back here.</p>{/if}
        {:else}
          <dl>
            <dt>Account</dt><dd>{status.email ?? 'Connected'}</dd>
            <dt>Library folder</dt><dd>{status.root_name ?? 'Not chosen yet'}</dd>
            <dt>Last sync</dt><dd>{status.syncing ? 'Syncing now…' : timeAgo(status.last_synced_at)}{status.crawl_in_progress ? ' (first scan in progress)' : ''}</dd>
          </dl>
          <div class="row wrap">
            <button class="primary" onclick={startPicking}>{status.root_id ? 'Change folder…' : 'Choose folder…'}</button>
            <button onclick={syncNow} disabled={!status.root_id}>Sync now</button>
            <button onclick={() => disconnect(false)}>Disconnect</button>
            <button class="danger" onclick={() => disconnect(true)}>Disconnect and wipe</button>
          </div>
        {/if}
      {/if}
    </section>

    <section class="card">
      <h2>Appearance</h2>
      <div class="row wrap">
        {#each [['system', 'Match Windows'], ['light', 'Light'], ['sepia', 'Sepia'], ['dark', 'Dark']] as [t, l]}
          <button class:primary={prefs.value.theme === t} onclick={() => prefs.update((p) => (p.theme = t as typeof p.theme))}>{l}</button>
        {/each}
      </div>
      <label class="check">
        <input type="checkbox" checked={prefs.value.chadMode} onchange={(e) => prefs.update((p) => (p.chadMode = (e.target as HTMLInputElement).checked))} />
        <span><strong>Chad Mode</strong> <span class="muted">— jokes, ranks and achievements. Turn off for a quiet reader.</span></span>
      </label>
    </section>

    {#if prefs.value.chadMode && stats}
      <section class="card">
        <h2>Trophy shelf</h2>
        <p>Rank: <strong>{stats.rank.name}</strong> · {stats.finished} finished · {stats.streak_days}-day streak</p>
        <ul class="ach">
          {#each achievements(stats) as a (a.id)}
            <li class:earned={a.earned}><span>{a.earned ? '🏆' : '🔒'}</span><strong>{a.name}</strong><span class="muted">{a.description}</span></li>
          {/each}
        </ul>
      </section>
    {/if}

    <section class="card">
      <h2>Offline cache</h2>
      {#if cache}
        <p>{cache.entries} book{cache.entries === 1 ? '' : 's'} downloaded · {formatBytes(cache.bytes)} ({formatBytes(cache.pinned_bytes)} pinned for offline)</p>
      {/if}
      <label>
        Maximum cache size: <strong>{prefs.value.cache_cap_mb >= 1024 ? `${(prefs.value.cache_cap_mb / 1024).toFixed(1)} GB` : `${prefs.value.cache_cap_mb} MB`}</strong>
        <input type="range" min="256" max="20480" step="256" value={prefs.value.cache_cap_mb} onchange={(e) => prefs.update((p) => (p.cache_cap_mb = +(e.target as HTMLInputElement).value))} />
      </label>
      <p class="muted small">Least recently opened books are removed first. Books marked “Keep offline” are never removed.</p>
      <button onclick={clearCache}>Clear cache</button>
    </section>

    <section class="card">
      <h2>About</h2>
      {#if info}
        <dl>
          <dt>Version</dt><dd>{info.version}</dd>
          <dt>Data folder</dt><dd class="mono">{info.data_dir}</dd>
          <dt>PDF engine</dt><dd>{info.pdf_error ? `Unavailable: ${info.pdf_error}` : 'PDFium'}</dd>
        </dl>
      {/if}
      <p class="muted small">Made with Rust, Tauri, PDFium and foliate-js. No telemetry. Your books stay in your Drive.</p>
    </section>
  </main>
</div>

{#if picking}
  <div class="modal-backdrop" role="presentation">
    <div class="modal card" role="dialog" aria-modal="true" aria-label="Choose library folder">
      <h2>Choose your library folder</h2>
      <div class="crumbs">
        <button class="ghost" onclick={() => up(0)}>Drives</button>
        {#each stack as s, i (s.id + i)}
          <span>›</span><button class="ghost" onclick={() => up(i + 1)}>{s.name}</button>
        {/each}
      </div>
      <ul class="folders">
        {#if loadingFolders}
          <li class="muted">Loading…</li>
        {:else}
          {#each folders as f (f.id)}
            <li><button class="ghost" onclick={() => enter(f)}>📁 {f.name}</button></li>
          {:else}
            <li class="muted">No sub-folders here.</li>
          {/each}
        {/if}
      </ul>
      <div class="row">
        <span class="muted small">Sub-folders become shelves.</span>
        <span class="spacer"></span>
        <button onclick={() => (picking = false)}>Cancel</button>
        <button class="primary" disabled={stack.length === 0} onclick={choose}>Use “{stack.at(-1)?.name ?? '…'}”</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .settings {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  header {
    padding: 12px 24px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    font-family: var(--font-display);
    font-size: 20px;
    margin: 0;
  }
  main {
    flex: 1;
    overflow-y: auto;
    padding: 20px 24px 48px;
    display: grid;
    gap: 16px;
    max-width: 820px;
    width: 100%;
    margin: 0 auto;
    align-content: start;
  }
  section {
    padding: 16px 18px;
    box-shadow: none;
  }
  h2 {
    font-size: 16px;
    margin: 0 0 10px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 0 0 12px;
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: Consolas, monospace;
    font-size: 12px;
  }
  .wrap {
    flex-wrap: wrap;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 12px 0 4px;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 10px;
  }
  .small {
    font-size: 12px;
  }
  .ach {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 6px;
  }
  .ach li {
    display: grid;
    grid-template-columns: 28px 200px 1fr;
    align-items: center;
    opacity: 0.55;
  }
  .ach li.earned {
    opacity: 1;
  }
  .crumbs {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px;
    margin-bottom: 8px;
  }
  .folders {
    list-style: none;
    padding: 0;
    margin: 0 0 12px;
    max-height: 50vh;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .folders li {
    border-bottom: 1px solid var(--border);
  }
  .folders li:last-child {
    border-bottom: none;
  }
  .folders li.muted {
    padding: 10px 12px;
  }
  .folders button {
    width: 100%;
    text-align: left;
    border-radius: 0;
  }
</style>
