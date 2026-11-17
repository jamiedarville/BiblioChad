// BC-3 "Explain selected text with AI": pure payload/selection helpers
// and the local-only hand-off record BC-4 book chat will consume.
// Nothing here talks to a provider; the only network path is
// api.chatSend (BC-2), invoked by ExplainPanel after an explicit
// Explain click/right-click/keyboard shortcut.

import type { ChatMessage, Locator } from './api'

/** Longest selection sent for an explanation, in characters. Longer
 * selections are trimmed (with a notice in the UI) before anything is
 * sent; bc-tutor additionally clips every chat message to 8000 chars. */
export const MAX_EXPLAIN_SELECTION_CHARS = 3000

export interface TrimmedSelection {
  text: string
  /** True when the selection was longer than the cap and was cut. */
  trimmed: boolean
  originalLength: number
}

/** Trim a selection to `max` chars, preferring a word boundary. */
export function trimSelection(raw: string, max = MAX_EXPLAIN_SELECTION_CHARS): TrimmedSelection {
  const text = raw.trim()
  const originalLength = text.length
  if (originalLength <= max) return { text, trimmed: false, originalLength }
  const cut = text.lastIndexOf(' ', max)
  return { text: text.slice(0, cut > max * 0.8 ? cut : max).trimEnd(), trimmed: true, originalLength }
}

/** PDF text-layer selections come from many absolutely positioned spans,
 * so `Selection.toString()` spacing is ragged. Collapse it to readable
 * text without inventing words: normalise spaces and blank lines. */
export function normalizePdfSelectionText(raw: string): string {
  return raw
    .replace(/\u00a0/g, ' ')
    .replace(/[ \t]+/g, ' ')
    .replace(/ *\n */g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

/** The user turn for one explanation. The surrounding passage, book
 * metadata and learner profile are added Rust-side by chat_send. */
export function buildExplainHistory(selectionText: string): ChatMessage[] {
  return [
    {
      role: 'user',
      content: `Explain this selected passage from the book I'm reading. Explain what it means, any unfamiliar terms, and why it matters here.\n\nSelected passage:\n"""\n${selectionText}\n"""`,
    },
  ]
}

export interface ExplainDisclosureInput {
  title: string
  author: string | null
  label: string
  selectionChars: number
  /** EPUB: chars of on-screen context about to be sent. PDF: null —
   * the selected page and its neighbours are read Rust-side. */
  contextChars: number | null
  /** Whether a learner profile (background/goal) is saved for this
   * book; null while it is still being checked. */
  profileSet: boolean | null
}

/** Human-readable list of exactly what an Explain request sends. */
export function buildExplainDisclosure(i: ExplainDisclosureInput): string[] {
  const lines = [
    `Your selected text (${i.selectionChars} characters)`,
    i.contextChars != null
      ? `The text on screen around it (${i.contextChars} characters)`
      : 'The selected page and the pages either side (read on this device)',
    `Book: ${i.title}${i.author ? ` by ${i.author}` : ''}${i.label ? ` — ${i.label}` : ''}`,
  ]
  if (i.profileSet) lines.push('Your saved background and goal for this book')
  return lines
}

/** What Reader hands to ExplainPanel for one explanation. */
export interface ExplainSeed {
  source: 'epub' | 'pdf'
  locator: Locator
  label: string
  /** EPUB on-screen context; null for PDF (read Rust-side by page). */
  passage: string | null
  selection: TrimmedSelection
}

/**
 * BC-4 hand-off contract (local-only, never synced to Drive):
 * "Ask follow-up" in ExplainPanel stores one ExplainHandoff per book
 * under `bibliochad-explain-handoff-{bookId}` in localStorage.
 * BC-4's chat panel should, on open, load it with loadExplainHandoff,
 * seed its message list with `history` ([user explain prompt,
 * assistant explanation]), show `selectionText`/`label` as the quoted
 * context chip, send follow-ups via api.chatSend(bookId, locator,
 * label, passage-like context, 'passage', [...history, newUserTurn],
 * runId), and clearExplainHandoff once consumed or superseded.
 */
export interface ExplainHandoff {
  version: 1
  bookId: number
  locator: Locator
  label: string
  selectionText: string
  explanation: string
  model: string
  truncated: boolean
  history: ChatMessage[]
  createdAt: number
}

export function explainHandoffKey(bookId: number): string {
  return `bibliochad-explain-handoff-${bookId}`
}

export function buildExplainHandoff(args: {
  bookId: number
  locator: Locator
  label: string
  selectionText: string
  explanation: string
  model: string
  truncated: boolean
  now?: number
}): ExplainHandoff {
  return {
    version: 1,
    bookId: args.bookId,
    locator: args.locator,
    label: args.label,
    selectionText: args.selectionText,
    explanation: args.explanation,
    model: args.model,
    truncated: args.truncated,
    history: [
      ...buildExplainHistory(args.selectionText),
      { role: 'assistant', content: args.explanation },
    ],
    createdAt: args.now ?? Date.now(),
  }
}

type KeyValueStore = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

function defaultStore(): KeyValueStore | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null
  } catch {
    return null
  }
}

export function saveExplainHandoff(h: ExplainHandoff, store: KeyValueStore | null = defaultStore()): boolean {
  if (!store) return false
  try {
    store.setItem(explainHandoffKey(h.bookId), JSON.stringify(h))
    return true
  } catch {
    return false
  }
}

export function loadExplainHandoff(
  bookId: number,
  store: KeyValueStore | null = defaultStore(),
): ExplainHandoff | null {
  if (!store) return null
  try {
    const raw = store.getItem(explainHandoffKey(bookId))
    if (!raw) return null
    const h = JSON.parse(raw) as ExplainHandoff
    return h && h.version === 1 && h.bookId === bookId && Array.isArray(h.history) ? h : null
  } catch {
    return null
  }
}

export function clearExplainHandoff(bookId: number, store: KeyValueStore | null = defaultStore()): void {
  try {
    store?.removeItem(explainHandoffKey(bookId))
  } catch {
    /* nothing stored */
  }
}
