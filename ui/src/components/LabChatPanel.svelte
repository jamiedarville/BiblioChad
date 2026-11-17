<script lang="ts">
  // Lab co-pilot (BC-5): lab-scoped chat over the saved guide.
  // Clarify/troubleshoot by default — chatting never changes the saved
  // lab. When the reader explicitly asks for an edit, the assistant may
  // reply with a structured proposal; it renders below as a
  // before/after preview and is persisted only on Apply (Rust
  // re-validates atomically). Reject writes nothing. History is local
  // per book, separate from Book chat, and never synced. Nothing is
  // sent until Send is pressed.
  import { onMount, tick } from 'svelte'
  import {
    api,
    errorKind,
    errorMessage,
    newChatRunId,
    type ChatMessage,
    type LabChatMessage,
    type LabGuide,
    type LabGuideRecord,
    type TutorStatus,
  } from '../lib/api'
  import { confirmAction } from '../lib/dialogs'
  import { attachmentLabel, buildProposalPreview, needsNoChangeNote, parseLabProposal, proposalDisplayText } from '../lib/labchat'
  import { toasts } from '../lib/toast.svelte'
  import SafeMarkdown from './SafeMarkdown.svelte'

  let {
    bookId,
    guide,
    attachment,
    ticked,
    onApplied,
    onOpenLearningSettings,
    onClose,
  }: {
    bookId: number
    guide: LabGuide
    /** Lab/step pre-attached to the next turn (per-lab/per-step Ask sets it). */
    attachment: { labIndex: number; stepIndex: number | null }
    /** Tick keys "lab:step" currently ticked, sent so answers know progress. */
    ticked: string[]
    onApplied: (record: LabGuideRecord) => void
    onOpenLearningSettings?: () => void
    onClose: () => void
  } = $props()

  let status = $state<TutorStatus | null>(null)
  let messages = $state<LabChatMessage[]>([])
  let draft = $state('')
  let busy = $state(false)
  let streamText = $state('')
  let error = $state<{ kind: string | null; message: string } | null>(null)
  let saveError = $state<string | null>(null)
  let stoppedNote = $state(false)
  let loaded = $state(false)
  /** message id -> proposal outcome, so preview buttons settle after use. */
  let proposalState = $state<Record<string, 'applied' | 'rejected'>>({})
  let applying = $state(false)
  let applyError = $state<string | null>(null)

  let runId: string | null = null
  let unlisten: (() => void) | null = null
  let disposed = false
  let listEl = $state<HTMLDivElement | null>(null)
  let composerEl = $state<HTMLTextAreaElement | null>(null)

  const attached = $derived(attachmentLabel(attachment.labIndex, attachment.stepIndex))

  async function scrollDown() {
    await tick()
    if (listEl) listEl.scrollTop = listEl.scrollHeight
  }

  function localMsg(role: 'user' | 'assistant', content: string): LabChatMessage {
    return {
      id: `local-${Math.random().toString(36).slice(2)}`,
      book_id: bookId,
      role,
      content,
      lab_index: attachment.labIndex,
      step_index: attachment.stepIndex,
      label: attached,
      model: '',
      created_at: Date.now(),
    }
  }

  async function persist(role: 'user' | 'assistant', content: string, model: string, labIndex: number, stepIndex: number | null) {
    try {
      const saved = await api.labChatHistoryAppend(bookId, role, content, labIndex, stepIndex, attachmentLabel(labIndex, stepIndex), model)
      if (!disposed) saveError = null
      return saved
    } catch (e) {
      if (!disposed) saveError = `This lab chat couldn't be saved on this device: ${errorMessage(e)}`
      return null
    }
  }

  async function send() {
    const text = draft.trim()
    if (!text || busy || disposed) return
    if (status && !status.ready) return // notice below; draft is kept
    busy = true
    error = null
    stoppedNote = false
    streamText = ''
    const labIndex = attachment.labIndex
    const stepIndex = attachment.stepIndex
    const history: ChatMessage[] = [
      ...messages.map((m) => ({ role: m.role, content: m.content })),
      { role: 'user', content: text },
    ]
    const optimistic = localMsg('user', text)
    messages = [...messages, optimistic]
    draft = ''
    void scrollDown()
    const id = newChatRunId()
    runId = id
    try {
      unlisten = await api.onChatDelta(id, (d) => {
        streamText += d.text
        void scrollDown()
      })
      const reply = await api.labChatSend(bookId, labIndex, stepIndex, history, id, ticked)
      if (disposed) return
      const savedUser = await persist('user', text, '', labIndex, stepIndex)
      const savedAssistant = await persist('assistant', reply.text, reply.model, labIndex, stepIndex)
      if (disposed) return
      messages = [
        ...messages.filter((x) => x.id !== optimistic.id),
        savedUser ?? optimistic,
        savedAssistant ?? { ...localMsg('assistant', reply.text), model: reply.model },
      ]
      streamText = ''
    } catch (e) {
      if (disposed) return
      const cancelled = errorKind(e) === 'cancelled'
      if (cancelled && streamText.trim()) {
        const partial = streamText
        const savedUser = await persist('user', text, '', labIndex, stepIndex)
        const savedAssistant = await persist('assistant', partial, '', labIndex, stepIndex)
        if (disposed) return
        messages = [
          ...messages.filter((x) => x.id !== optimistic.id),
          savedUser ?? optimistic,
          savedAssistant ?? localMsg('assistant', partial),
        ]
        stoppedNote = true
      } else {
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

  async function applyProposal(msg: LabChatMessage) {
    const parsed = parseLabProposal(msg.content)
    if (parsed.kind !== 'proposal' || applying) return
    const preview = buildProposalPreview(guide, parsed.proposal)
    if (preview.errors.length) {
      applyError = `This lab edit was refused and nothing was changed: ${preview.errors.join('; ')}`
      return
    }
    applying = true
    applyError = null
    try {
      const record = await api.labProposalApply(bookId, parsed.proposal)
      proposalState[msg.id] = 'applied'
      onApplied(record)
      toasts.push('success', 'Lab updated. Undo is available in the lab toolbar.')
    } catch (e) {
      // Rust refused (or saving failed): the saved lab is unchanged.
      applyError = errorMessage(e)
    } finally {
      applying = false
    }
  }

  function rejectProposal(msg: LabChatMessage) {
    // Reject writes nothing: the saved lab stays byte-identical.
    proposalState[msg.id] = 'rejected'
    applyError = null
  }

  async function clearAll() {
    if (busy) return
    if (messages.length && !(await confirmAction('Delete this book’s saved lab chat on this device? Its book chat is not affected.'))) return
    try {
      await api.labChatHistoryClear(bookId)
      messages = []
      proposalState = {}
      error = null
      saveError = null
      toasts.push('info', 'Lab chat cleared.')
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
        const [s, rows] = await Promise.all([api.tutorStatus(), api.labChatHistory(bookId)])
        if (disposed) return
        status = s
        messages = rows.filter((m) => m.book_id === bookId)
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
      stop()
      unlisten?.()
      unlisten = null
    }
  })
</script>

<div class="chat">
  <div class="head">
    <h3>Lab co-pilot</h3>
    <span class="spacer"></span>
    <button onclick={clearAll} disabled={busy} title="Delete the saved lab chat for this book">Clear</button>
    <button class="ghost icon" aria-label="Close lab chat" onclick={onClose}>✕</button>
  </div>

  <p class="caption small" aria-label="What is sent">
    <strong>Sends:</strong> {attached} (its steps, commands, diagrams and checks, plus which steps you ticked) · book details and outline · sampled book excerpts · your background/goal if set. Chatting never changes the saved lab — edits appear as a preview you Apply or Reject.
  </p>
  <p class="muted small">Lab chat history is kept only on this device for this book, separately from Book chat — never synced. Nothing is sent until you press Send.</p>

  <div class="list" bind:this={listEl} aria-live="polite" aria-label="Lab conversation">
    {#if !loaded}
      <p class="muted">Loading this book's lab chat…</p>
    {:else if messages.length === 0 && !busy}
      <p class="muted">Ask about {attached}, or paste a terminal error you're stuck on. To change the lab itself, say so explicitly — you'll get a before/after preview to confirm first.</p>
    {/if}
    {#each messages as m, i (m.id)}
      <div class="msg {m.role}">
        <span class="who">{m.role === 'user' ? 'You' : 'Co-pilot'}{#if m.label} <span class="muted">· {m.label}</span>{/if}</span>
        {#if m.role === 'assistant'}{@const shown = proposalDisplayText(m.content)}{#if shown}<SafeMarkdown text={shown} />{/if}
        {:else}<p class="plain">{m.content}</p>{/if}
        {#if m.model}<span class="muted small">{m.model}</span>{/if}
        {#if m.role === 'assistant'}
          {@const parsed = parseLabProposal(m.content)}
          {#if parsed.kind === 'none' && needsNoChangeNote(messages[i - 1]?.role === 'user' ? messages[i - 1].content : null, m.content)}
            <p class="notice-small small">No edit was proposed, so the saved lab is unchanged. To change it, ask again (for example “Propose an edit to step 1 that …”) and you'll get a before/after preview to Apply or Reject.</p>
          {:else if parsed.kind === 'invalid'}
            <p class="error small" role="alert">{parsed.error} The saved lab was not changed.</p>
          {:else if parsed.kind === 'proposal'}
            {@const preview = buildProposalPreview(guide, parsed.proposal)}
            <div class="proposal">
              <strong>Proposed lab edit</strong>
              {#if parsed.proposal.summary}<p class="small">{parsed.proposal.summary}</p>{/if}
              {#each preview.entries as entry}
                <div class="diff">
                  <p class="small"><strong>{entry.target}</strong></p>
                  <p class="small muted">Before</p>
                  <pre>{entry.before}</pre>
                  <p class="small muted">After</p>
                  <pre>{entry.after}</pre>
                </div>
              {/each}
              {#if preview.errors.length}
                <p class="error small" role="alert">This edit can't be applied, so nothing will change: {preview.errors.join('; ')}</p>
              {:else if proposalState[m.id] === 'applied'}
                <p class="small">Applied — the saved lab now matches “After”. Undo is in the lab toolbar.</p>
              {:else if proposalState[m.id] === 'rejected'}
                <p class="small muted">Rejected — the saved lab was not changed.</p>
              {:else}
                <div class="row">
                  <button class="primary" disabled={applying} onclick={() => applyProposal(m)}>{applying ? 'Applying…' : 'Apply'}</button>
                  <button disabled={applying} onclick={() => rejectProposal(m)}>Reject</button>
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    {/each}
    {#if busy}
      <div class="msg assistant">
        <span class="who">Co-pilot</span>
        {#if streamText}<SafeMarkdown text={streamText} />{:else}<p class="muted">Thinking…</p>{/if}
      </div>
    {/if}
    {#if stoppedNote}<p class="muted small">Stopped — the partial answer above was kept.</p>{/if}
  </div>

  {#if applyError}<p class="error" role="alert">{applyError}</p>{/if}
  {#if status && !status.ready}
    <div class="notice">
      <strong>Add an API key to start.</strong>
      <p>Open <em>Settings → Learning assistant</em> and paste a key (or point it at a local model). Nothing has been sent; your draft stays here.</p>
      {#if onOpenLearningSettings}<button class="primary" onclick={onOpenLearningSettings}>Open Settings → Learning assistant</button>{/if}
    </div>
  {:else if error}
    <p class="error" role="alert">{error.message}</p>
    {#if draft.trim() && error.kind !== 'not_configured'}
      <div class="row"><button class="primary" disabled={busy} onclick={send}>Retry</button><span class="muted small">Your message is back in the box below.</span></div>
    {/if}
  {/if}
  {#if saveError}<p class="error" role="alert">{saveError}</p>{/if}

  <form class="composer" onsubmit={(e) => { e.preventDefault(); void send() }}>
    <label class="sr" for="lab-chat-composer">Message</label>
    <textarea
      id="lab-chat-composer"
      bind:this={composerEl}
      bind:value={draft}
      rows="3"
      placeholder="Ask about {attached}… (Enter to send, Shift+Enter for a new line)"
      onkeydown={onComposerKey}
    ></textarea>
    <div class="row">
      {#if busy}<button type="button" onclick={stop}>Stop</button>{/if}
      <span class="spacer"></span>
      <button class="primary" type="submit" disabled={busy || !draft.trim() || (status != null && !status.ready)}>Send</button>
    </div>
  </form>
</div>

<style>
  .chat { position: relative; display: flex; flex-direction: column; gap: 8px; min-height: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: 12px; margin-top: 16px; }
  .head { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .head h3 { margin: 0; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-2); }
  .spacer { flex: 1; }
  .caption { margin: 0; color: var(--text-2); }
  .small { font-size: 11px; }
  .list { max-height: 52vh; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; padding-right: 2px; }
  .msg { display: flex; flex-direction: column; gap: 2px; }
  .msg .who { font-size: 11px; font-weight: 600; color: var(--text-2); }
  .msg.user { align-items: flex-end; }
  .msg .plain { margin: 0; white-space: pre-wrap; line-height: 1.45; background: var(--surface-2); border-radius: 8px; padding: 6px 8px; }
  .proposal { border: 1px solid var(--accent); border-radius: var(--radius); padding: 10px; display: flex; flex-direction: column; gap: 6px; }
  .notice-small { margin: 0; padding: 6px 8px; border-left: 3px solid var(--accent); background: var(--surface-2); border-radius: 4px; }
  .proposal p { margin: 0; }
  .diff pre { white-space: pre-wrap; font-size: 12px; background: var(--surface-2); border-radius: var(--radius); padding: 8px; margin: 2px 0 6px; overflow-x: auto; }
  .row { display: flex; gap: 6px; align-items: center; }
  .notice { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
  .notice p { margin: 0; }
  .error { color: var(--danger); }
  .composer { display: flex; flex-direction: column; gap: 6px; }
  .composer textarea { width: 100%; resize: vertical; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
