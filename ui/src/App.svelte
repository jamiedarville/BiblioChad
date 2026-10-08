<script lang="ts">
  import { onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import Library from './components/Library.svelte'
  import Reader from './components/Reader.svelte'
  import Settings from './components/Settings.svelte'
  import Toasts from './components/Toasts.svelte'
  import { prefs, effectiveTheme } from './lib/prefs.svelte'
  import { toasts } from './lib/toast.svelte'
  import { runBeforeClose } from './lib/session'
  import { api } from './lib/api'

  type View = { name: 'library' } | { name: 'reader'; bookId: number } | { name: 'settings' }

  let view = $state<View>({ name: 'library' })
  let libraryVersion = $state(0)
  let syncMessage = $state<string | null>(null)
  let downloads = $state<Record<number, { done: number; total: number | null }>>({})

  $effect(() => {
    const apply = () => {
      document.documentElement.dataset.theme = effectiveTheme(prefs.value.theme)
    }
    apply()
    const mq = matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  })

  onMount(() => {
    prefs.load().then(async () => {
      const id = await api.takePendingOpen().catch(() => null)
      if (id != null) view = { name: 'reader', bookId: id }
    })
    const unlisten: Promise<() => void>[] = [
      listen('library-changed', () => libraryVersion++),
      listen<{ phase: string; message: string }>('sync-status', (e) => {
        syncMessage = e.payload.phase === 'idle' ? null : e.payload.message
        if (e.payload.phase === 'error') {
          toasts.push('error', `Sync failed: ${e.payload.message}`)
          syncMessage = null
        }
      }),
      listen<{ book_id: number; done: number; total: number | null }>('download-progress', (e) => {
        downloads = { ...downloads, [e.payload.book_id]: { done: e.payload.done, total: e.payload.total } }
        if (e.payload.total != null && e.payload.done >= e.payload.total) {
          const { [e.payload.book_id]: _, ...rest } = downloads
          downloads = rest
        }
      }),
      listen<string>('drive-reauth', (e) => {
        toasts.push('error', e.payload, { label: 'Reconnect', run: () => (view = { name: 'settings' }) }, 0)
      }),
      listen('app-closing', async () => {
        await runBeforeClose()
        await getCurrentWindow().destroy()
      }),
    ]
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'F11') {
        e.preventDefault()
        const w = getCurrentWindow()
        w.isFullscreen().then((f) => w.setFullscreen(!f))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => {
      unlisten.forEach((p) => p.then((u) => u()))
      window.removeEventListener('keydown', onKey)
    }
  })
</script>

{#if prefs.loaded}
  {#if view.name === 'reader'}
    <Reader
      bookId={view.bookId}
      download={downloads[view.bookId] ?? null}
      onClose={() => {
        view = { name: 'library' }
        libraryVersion++
      }}
    />
  {:else if view.name === 'settings'}
    <Settings onBack={() => (view = { name: 'library' })} />
  {:else}
    <Library
      version={libraryVersion}
      {syncMessage}
      onOpen={(id) => (view = { name: 'reader', bookId: id })}
      onSettings={() => (view = { name: 'settings' })}
    />
  {/if}
{/if}

<Toasts />
