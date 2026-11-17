<script lang="ts">
  // BC-3: streamed explanation for the selected text. Mounting this
  // panel is the reader's explicit Explain action (toolbar button,
  // right-click on a selection, or Ctrl/Cmd+E) — that, and only that,
  // sends anything to the provider, via the BC-2 chat_send command.
  import { onMount } from 'svelte'
  import { api, errorKind, errorMessage, newChatRunId, type TutorStatus } from '../lib/api'
  import {
    buildExplainDisclosure,
    buildExplainHandoff,
    buildExplainHistory,
    saveExplainHandoff,
    type ExplainSeed,
  } from '../lib/explain'
  import { toasts } from '../lib/toast.svelte'
  import SafeMarkdown from './SafeMarkdown.svelte'

  let {
    bookId,
    bookTitle,
    bookAuthor,
    seed,
    onClose,
    onOpenLearningSettings,
    onAskFollowUp,
  }: {
    bookId: number
    bookTitle: string
    bookAuthor: string | null
    seed: ExplainSeed
    onClose: () => void
    /** Deep-link to Settings → Learning assistant (closes the book). */
    onOpenLearningSettings: () => void
    /** Open book chat (BC-4), which consumes the saved hand-off. */
    onAskFollowUp?: () => void
  } = $props()

  let status = $state<TutorStatus | null>(null)
  let profileSet = $state<boolean | null>(null)
  let text = $state('')
  let model = $state('')
  let truncated = $state(false)
  let busy = $state(false)
  let error = $state<{ kind: string | null; message: string } | null>(null)
  let handoffSaved = $state(false)

  let runId: string | null = null
  let unlisten: (() => void) | null = null
  let disposed = false

  const disclosure = $derived(
    buildExplainDisclosure({
      title: bookTitle,
      author: bookAuthor,
      label: seed.label,
      selectionChars: seed.selection.text.length,
      contextChars: seed.passage != null ? seed.passage.length : null,
      profileSet,
    }),
  )

  async function explain() {
    if (busy || disposed) return
    busy = true
    error = null
    text = ''
    model = ''
    truncated = false
    handoffSaved = false
    const id = newChatRunId()
    runId = id
    try {
      unlisten = await api.onChatDelta(id, (d) => (text += d.text))
      const reply = await api.chatSend(
        bookId,
        seed.locator,
        seed.label,
        seed.passage,
        'passage',
        buildExplainHistory(seed.selection.text),
        id,
      )
      if (disposed) return
      // The return value is the authoritative full text.
      text = reply.text
      model = reply.model
      truncated = reply.truncated
    } catch (e) {
      if (!disposed && errorKind(e) !== 'cancelled') {
        error = { kind: errorKind(e), message: errorMessage(e) }
      }
    } finally {
      unlisten?.()
      unlisten = null
      if (runId === id) runId = null
      if (!disposed) busy = false
    }
  }

  function cancelRun() {
    if (runId) void api.chatCancel(runId).catch(() => {})
    unlisten?.()
    unlisten = null
  }

  onMount(() => {
    void (async () => {
      try {
        const [s, o] = await Promise.all([api.tutorStatus(), api.zoneOverview(bookId)])
        if (disposed) return
        status = s
        profileSet = Boolean(o.profile.background.trim() || o.profile.goal.trim())
        // Tutor not ready: show setup guidance, send nothing.
        if (s.ready) void explain()
      } catch (e) {
        if (!disposed) error = { kind: errorKind(e), message: errorMessage(e) }
      }
    })()
    return () => {
      disposed = true
      cancelRun()
    }
  })

  async function copy() {
    await navigator.clipboard.writeText(text)
    toasts.push('info', 'Copied.')
  }

  function askFollowUp() {
    const ok = saveExplainHandoff(
      buildExplainHandoff({
        bookId,
        locator: seed.locator,
        label: seed.label,
        selectionText: seed.selection.text,
        explanation: text,
        model,
        truncated,
      }),
    )
    handoffSaved = ok
    if (ok && onAskFollowUp) {
      // Book chat picks the hand-off up pre-seeded (BC-4).
      onAskFollowUp()
      return
    }
    toasts.push(
      ok ? 'info' : 'error',
      ok
        ? 'Saved on this device for book chat — your follow-ups will start from this explanation.'
        : "Couldn't save this for book chat on this device.",
    )
  }
</script>

<div class="explain">
  <div class="head">
    <h3>Explain</h3>
    <span class="spacer"></span>
    <button class="ghost icon" aria-label="Close explanation" onclick={onClose}>✕</button>
  </div>

  <blockquote>{seed.selection.text}</blockquote>
  {#if seed.selection.trimmed}
    <p class="muted small">
      That selection was long, so only the first part (of {seed.selection.originalLength} characters) was sent.
    </p>
  {/if}

  <details class="sent">
    <summary>What was sent</summary>
    <ul>
      {#each disclosure as line}<li>{line}</li>{/each}
    </ul>
    <p class="muted small">Sent to your learning-assistant provider with your key, only because you asked for this explanation.</p>
  </details>

  {#if status && !status.ready}
    <div class="notice">
      <strong>Add an API key to start.</strong>
      <p>Explanations use your learning assistant. Nothing has been sent.</p>
      <button class="primary" onclick={onOpenLearningSettings}>Open Settings → Learning assistant</button>
      <p class="muted small">This closes the book; your place is saved.</p>
    </div>
  {:else if error}
    <p class="error" role="alert">{error.message}</p>
    {#if error.kind === 'not_configured'}
      <button class="primary" onclick={onOpenLearningSettings}>Open Settings → Learning assistant</button>
    {:else}
      <div class="row"><button class="primary" disabled={busy} onclick={explain}>Retry</button></div>
    {/if}
  {:else if busy && !text}
    <p class="muted" aria-live="polite">Explaining…</p>
  {/if}

  {#if text}
    <div class="answer" aria-live="polite">
      <SafeMarkdown {text} />
      {#if busy}<p class="muted small">…</p>{/if}
    </div>
    {#if truncated}<p class="muted small">The answer was cut off there by the provider.</p>{/if}
    {#if model}<p class="muted small">{model}</p>{/if}
  {/if}

  {#if text || busy}
    <div class="row actions">
      {#if busy}
        <button onclick={cancelRun}>Stop</button>
      {:else if text}
        <button onclick={copy}>Copy</button>
        <button onclick={explain}>Regenerate</button>
        <button class="primary" disabled={handoffSaved} onclick={askFollowUp}>
          {handoffSaved ? 'Saved for book chat' : 'Ask follow-up'}
        </button>
      {/if}
    </div>
    {#if handoffSaved}
      <p class="muted small">Kept only on this device (never synced). Book chat will pick this explanation up.</p>
    {/if}
  {/if}
</div>

<style>
  .explain {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .head h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
  }
  .spacer {
    flex: 1;
  }
  blockquote {
    margin: 0;
    padding-left: 12px;
    border-left: 3px solid var(--accent);
    color: var(--text-2);
    max-height: 9em;
    overflow-y: auto;
    white-space: pre-wrap;
  }
  .sent summary {
    cursor: pointer;
    font-size: 12px;
    color: var(--text-2);
  }
  .sent ul {
    margin: 4px 0;
    padding-left: 18px;
    font-size: 12px;
  }
  .notice {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }
  .notice p {
    margin: 0;
  }
  .answer p {
    white-space: pre-wrap;
    line-height: 1.45;
  }
  .small {
    font-size: 11px;
  }
  .actions {
    flex-wrap: wrap;
  }
</style>
