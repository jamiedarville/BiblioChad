<script lang="ts">
  // BC-4 book chat panel.
  //
  // REUSE CONTRACT FOR BC-5 (lab co-pilot): this component is the
  // generic chat shell — message list with safe Markdown, composer
  // (Enter sends, Shift+Enter newline), streaming with Stop,
  // error-keeps-draft + Retry, New chat/Clear, and a visible
  // "what is sent" caption. BC-5 should reuse it as-is by passing:
  //   - `context`: a getter for the current locator/label/passage/
  //     outline (Reader passes Reader.chatContext; a lab surface
  //     passes its lab/step scope the same way),
  //   - `loadHistory` / `appendHistory` / `clearHistory`: adapters
  //     over its own local table with the BookChatMessage shape
  //     (Reader passes api.chatHistory* over book_chat_messages),
  //   - `allowBookMode={false}` if whole-book context makes no
  //     sense there, and `heading` for its title,
  //   - optionally `seed` + `onSeedConsumed` (BC-3's Explain
  //     hand-off arrives this way; the seed is appended through
  //     `appendHistory` once, then the callback clears its source).
  // Sends go only through api.chatSend (BC-2) on an explicit Send;
  // mounting/opening this panel sends nothing to a provider.
  import { onMount, tick } from 'svelte'
  import {
    api,
    errorKind,
    errorMessage,
    newChatRunId,
    type BookChatMessage,
    type ChatBookStatus,
    type ChatContextMode,
    type ChatMessage,
    type Locator,
    type TutorStatus,
  } from '../lib/api'
  import { buildSentCaption, handoffAlreadySeeded, toHistory, wholeBookGate } from '../lib/chat'
  import { confirmAction } from '../lib/dialogs'
  import { toasts } from '../lib/toast.svelte'
  import SafeMarkdown from './SafeMarkdown.svelte'

  export interface ChatSeed {
    history: ChatMessage[]
    chip: string
    locator: Locator | null
    label: string
    /** Model that produced the seeded assistant turn, when known. */
    model?: string
  }

  let {
    bookId,
    bookTitle,
    bookAuthor,
    heading = 'Book chat',
    context,
    loadHistory,
    appendHistory,
    clearHistory,
    allowBookMode = true,
    seed = null,
    onSeedConsumed,
    onOpenLearningSettings,
    onClose,
  }: {
    bookId: number
    bookTitle: string
    bookAuthor: string | null
    heading?: string
    context: () => { locator: Locator; label: string; passage: string | null; outline: string | null } | null
    loadHistory: () => Promise<BookChatMessage[]>
    appendHistory: (m: {
      role: 'user' | 'assistant'
      content: string
      locator: Locator | null
      label: string
      model: string
      contextMode: ChatContextMode
    }) => Promise<BookChatMessage>
    clearHistory: () => Promise<void>
    allowBookMode?: boolean
    seed?: ChatSeed | null
    onSeedConsumed?: () => void
    onOpenLearningSettings: () => void
    onClose: () => void
  } = $props()

  let status = $state<TutorStatus | null>(null)
  let profileSet = $state<boolean | null>(null)
  let messages = $state<BookChatMessage[]>([])
  let draft = $state('')
  let mode = $state<ChatContextMode>('passage')
  let bookStatus = $state<ChatBookStatus | null>(null)
  let checkingBook = $state(false)
  let lastSent = $state<{ sampled: boolean; chars: number } | null>(null)
  let busy = $state(false)
  let streamText = $state('')
  let error = $state<{ kind: string | null; message: string } | null>(null)
  let saveError = $state<string | null>(null)
  let stoppedNote = $state(false)
  let seedChip = $state<string | null>(null)
  let loaded = $state(false)

  let runId: string | null = null
  let unlisten: (() => void) | null = null
  let disposed = false
  let listEl = $state<HTMLDivElement | null>(null)
  let composerEl = $state<HTMLTextAreaElement | null>(null)

  const gate = $derived(wholeBookGate(bookStatus, checkingBook))
  const caption = $derived.by(() => {
    const ctx = context()
    return buildSentCaption({
      mode,
      title: bookTitle,
      author: bookAuthor,
      label: ctx?.label ?? '',
      passageChars: ctx?.passage != null ? ctx.passage.length : null,
      outlineEntries: ctx?.outline ? ctx.outline.split('\n').filter((l) => l.trim()).length : 0,
      profileSet,
      book: bookStatus,
      lastSent: mode === 'book' ? lastSent : null,
    })
  })

  async function scrollDown() {
    await tick()
    if (listEl) listEl.scrollTop = listEl.scrollHeight
  }

  async function persist(role: 'user' | 'assistant', content: string, ctxLabel: string, loc: Locator | null, model: string, m: ChatContextMode) {
    try {
      const saved = await appendHistory({ role, content, locator: loc, label: ctxLabel, model, contextMode: m })
      if (!disposed) saveError = null
      return saved
    } catch (e) {
      // A failed local save must be visible, not silently lost.
      if (!disposed) saveError = `This chat couldn't be saved on this device: ${errorMessage(e)}`
      return null
    }
  }

  function localMsg(role: 'user' | 'assistant', content: string, m: ChatContextMode): BookChatMessage {
    return { id: `local-${Math.random().toString(36).slice(2)}`, book_id: bookId, role, content, locator: null, label: '', model: '', context_mode: m, created_at: Date.now() }
  }

  async function chooseMode(next: ChatContextMode) {
    mode = next
    if (next !== 'book' || bookStatus || checkingBook) return
    checkingBook = true
    try {
      const s = await api.chatBookStatus(bookId)
      if (disposed) return
      bookStatus = s
      if (!s.available) mode = 'passage'
    } catch (e) {
      if (!disposed) error = { kind: errorKind(e), message: errorMessage(e) }
    } finally {
      if (!disposed) checkingBook = false
    }
  }

  async function send() {
    const text = draft.trim()
    if (!text || busy || disposed) return
    if (status && !status.ready) return // notice below; draft is kept
    const ctx = context()
    if (!ctx) {
      error = { kind: null, message: 'The book is still loading — try again in a moment.' }
      return
    }
    if (mode === 'book' && gate.disabled) return
    busy = true
    error = null
    stoppedNote = false
    streamText = ''
    const sendMode = mode
    const history: ChatMessage[] = [...toHistory(messages), { role: 'user', content: text }]
    const optimistic = localMsg('user', text, sendMode)
    messages = [...messages, optimistic]
    draft = ''
    void scrollDown()
    const id = newChatRunId()
    runId = id
    let cancelled = false
    try {
      unlisten = await api.onChatDelta(id, (d) => {
        streamText += d.text
        void scrollDown()
      })
      const reply = await api.chatSend(bookId, ctx.locator, ctx.label, ctx.passage, sendMode, history, id, ctx.outline)
      if (disposed) return
      const savedUser = await persist('user', text, ctx.label, ctx.locator, '', sendMode)
      const savedAssistant = await persist('assistant', reply.text, ctx.label, ctx.locator, reply.model, sendMode)
      if (disposed) return
      messages = [
        ...messages.filter((x) => x.id !== optimistic.id),
        savedUser ?? optimistic,
        savedAssistant ?? { ...localMsg('assistant', reply.text, sendMode), model: reply.model },
      ]
      if (reply.context_chars != null) lastSent = { sampled: !!reply.context_sampled, chars: reply.context_chars }
      streamText = ''
    } catch (e) {
      if (disposed) return
      cancelled = errorKind(e) === 'cancelled'
      if (cancelled && streamText.trim()) {
        // Stop with partial text: keep what arrived as the reply.
        const partial = streamText
        const savedUser = await persist('user', text, ctx.label, ctx.locator, '', sendMode)
        const savedAssistant = await persist('assistant', partial, ctx.label, ctx.locator, '', sendMode)
        if (disposed) return
        messages = [
          ...messages.filter((x) => x.id !== optimistic.id),
          savedUser ?? optimistic,
          savedAssistant ?? localMsg('assistant', partial, sendMode),
        ]
        stoppedNote = true
      } else {
        // Error (or Stop before any text): give the draft back for Retry.
        messages = messages.filter((x) => x.id !== optimistic.id)
        draft = text
        if (!cancelled) error = { kind: errorKind(e), message: errorMessage(e) }
      }
      streamText = ''
    } finally {
      unlisten?.()
      unlisten = null
      if (runId === id) runId = null
      if (!disposed) {
        busy = false
        void scrollDown()
        composerEl?.focus()
      }
    }
  }

  function stop() {
    if (runId) void api.chatCancel(runId).catch(() => {})
  }

  function cancelRun() {
    stop()
    unlisten?.()
    unlisten = null
  }

  async function clearAll(resetAll: boolean) {
    if (busy) return
    if (messages.length && !(await confirmAction('Delete this book’s saved chat on this device? Other books are not affected.'))) return
    try {
      await clearHistory()
      messages = []
      lastSent = null
      error = null
      saveError = null
      stoppedNote = false
      seedChip = null
      if (resetAll) {
        draft = ''
        mode = 'passage'
      }
      toasts.push('info', 'Chat cleared.')
    } catch (e) {
      error = { kind: errorKind(e), message: errorMessage(e) }
    }
  }

  function onComposerKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault()
      void send()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  onMount(() => {
    void (async () => {
      try {
        const [s, o, rows] = await Promise.all([api.tutorStatus(), api.zoneOverview(bookId), loadHistory()])
        if (disposed) return
        status = s
        profileSet = Boolean(o.profile.background.trim() || o.profile.goal.trim())
        messages = rows.filter((m) => m.book_id === bookId)
        if (seed && seed.history.length && !handoffAlreadySeeded(messages, seed)) {
          // BC-3 "Ask follow-up": the explanation arrives pre-seeded.
          for (const m of seed.history) {
            const saved = await persist(m.role, m.content, seed.label, seed.locator, m.role === 'assistant' ? (seed.model ?? '') : '', 'passage')
            if (disposed) return
            messages = [...messages, saved ?? localMsg(m.role, m.content, 'passage')]
          }
          seedChip = seed.chip
          onSeedConsumed?.()
        }
      } catch (e) {
        if (!disposed) error = { kind: errorKind(e), message: errorMessage(e) }
      } finally {
        if (!disposed) {
          loaded = true
          void scrollDown()
          composerEl?.focus()
        }
      }
    })()
    return () => {
      disposed = true
      cancelRun()
    }
  })
</script>

<div class="chat">
  <div class="head">
    <h3>{heading}</h3>
    <span class="spacer"></span>
    <button onclick={() => clearAll(true)} disabled={busy} title="Start a new conversation for this book">New chat</button>
    <button onclick={() => clearAll(false)} disabled={busy} title="Delete the saved chat for this book">Clear</button>
    <button class="ghost icon" aria-label="Close chat" onclick={onClose}>✕</button>
  </div>

  {#if allowBookMode}
    <div class="seg" role="group" aria-label="Chat context">
      <button class:primary={mode === 'passage'} aria-pressed={mode === 'passage'} onclick={() => chooseMode('passage')}>This page</button>
      <button
        class:primary={mode === 'book'}
        aria-pressed={mode === 'book'}
        disabled={checkingBook || (bookStatus != null && !bookStatus.available)}
        title={gate.reason ?? 'Chat about the whole book'}
        onclick={() => chooseMode('book')}
      >Whole book</button>
    </div>
    {#if gate.reason}<p class="muted small">{gate.reason}</p>{/if}
    {#if gate.sampledNote && mode === 'book'}<p class="muted small">{gate.sampledNote}</p>{/if}
  {/if}

  <p class="caption small" aria-label="What is sent">
    <strong>Sends:</strong>
    {#each caption as line, idx}{#if idx}{' · '}{/if}{line}{/each}
  </p>
  <p class="muted small">History is kept only on this device for this book — never synced. Nothing is sent until you press Send.</p>

  {#if seedChip}
    <p class="chip" title={seedChip}>Continuing from your explanation: “{seedChip}”</p>
  {/if}

  <div class="list" bind:this={listEl} aria-live="polite" aria-label="Conversation">
    {#if !loaded}
      <p class="muted">Loading this book’s chat…</p>
    {:else if messages.length === 0 && !busy}
      <p class="muted">Ask about what you’re reading. Answers are grounded in the context named above — and the assistant should say so when the book doesn’t cover your question.</p>
    {/if}
    {#each messages as m (m.id)}
      <div class="msg {m.role}">
        <span class="who">{m.role === 'user' ? 'You' : 'Assistant'}</span>
        {#if m.role === 'assistant'}<SafeMarkdown text={m.content} />
        {:else}<p class="plain">{m.content}</p>{/if}
        {#if m.model}<span class="muted small">{m.model}</span>{/if}
      </div>
    {/each}
    {#if busy}
      <div class="msg assistant">
        <span class="who">Assistant</span>
        {#if streamText}<SafeMarkdown text={streamText} />{:else}<p class="muted">Thinking…</p>{/if}
      </div>
    {/if}
    {#if stoppedNote}<p class="muted small">Stopped — the partial answer above was kept.</p>{/if}
  </div>

  {#if status && !status.ready}
    <div class="notice">
      <strong>Add an API key to start.</strong>
      <p>Book chat uses your learning assistant. Nothing has been sent; your draft stays here.</p>
      <button class="primary" onclick={onOpenLearningSettings}>Open Settings → Learning assistant</button>
      <p class="muted small">This closes the book; your place is saved.</p>
    </div>
  {:else if error}
    <p class="error" role="alert">{error.message}</p>
    {#if error.kind === 'not_configured'}
      <button class="primary" onclick={onOpenLearningSettings}>Open Settings → Learning assistant</button>
    {:else if draft.trim()}
      <div class="row"><button class="primary" disabled={busy} onclick={send}>Retry</button><span class="muted small">Your message is back in the box below.</span></div>
    {/if}
  {/if}
  {#if saveError}<p class="error" role="alert">{saveError}</p>{/if}

  <form class="composer" onsubmit={(e) => { e.preventDefault(); void send() }}>
    <label class="sr" for="chat-composer">Message</label>
    <textarea
      id="chat-composer"
      bind:this={composerEl}
      bind:value={draft}
      rows="3"
      placeholder="Ask about this book… (Enter to send, Shift+Enter for a new line)"
      onkeydown={onComposerKey}
    ></textarea>
    <div class="row">
      {#if busy}<button type="button" onclick={stop}>Stop</button>{/if}
      <span class="spacer"></span>
      <button class="primary" type="submit" disabled={busy || !draft.trim() || (status != null && !status.ready) || (mode === 'book' && gate.disabled)}>Send</button>
    </div>
  </form>
</div>

<style>
  .chat { position: relative; display: flex; flex-direction: column; gap: 8px; min-height: 0; flex: 1; }
  .head { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .head h3 { margin: 0; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-2); }
  .spacer { flex: 1; }
  .seg { display: flex; gap: 4px; }
  .seg button { flex: 1; }
  .caption { margin: 0; color: var(--text-2); }
  .small { font-size: 11px; }
  .chip { margin: 0; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .list { flex: 1 1 auto; min-height: 120px; max-height: 52vh; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; padding-right: 2px; }
  .msg { display: flex; flex-direction: column; gap: 2px; }
  .msg .who { font-size: 11px; font-weight: 600; color: var(--text-2); }
  .msg.user { align-items: flex-end; }
  .msg .plain { margin: 0; white-space: pre-wrap; line-height: 1.45; background: var(--surface-2); border-radius: 8px; padding: 6px 8px; }
  .notice { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
  .notice p { margin: 0; }
  .composer { display: flex; flex-direction: column; gap: 6px; }
  .composer textarea { width: 100%; resize: vertical; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>

