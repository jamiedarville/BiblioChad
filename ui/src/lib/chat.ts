// Pure book-chat helpers (BC-4): what-was-sent captions, the whole-book
// gate, history conversion and the BC-3 explain hand-off seeding rule.
// Nothing here talks to a provider; api.chatSend (BC-2) stays the only
// network path, called by ChatPanel on an explicit Send. Send-time
// trimming of long conversations happens in bc-tutor (prepare_chat
// drops the oldest pairs); the visible transcript is never trimmed.

import type { BookChatMessage, ChatBookStatus, ChatContextMode, ChatMessage } from './api'
import type { ExplainHandoff } from './explain'

/** Persisted rows -> the history `chatSend` expects (oldest first). */
export function toHistory(messages: Pick<BookChatMessage, 'role' | 'content'>[]): ChatMessage[] {
  return messages.map((m) => ({ role: m.role, content: m.content }))
}

export interface SentCaptionInput {
  mode: ChatContextMode
  title: string
  author: string | null
  /** Current location label, e.g. "Chapter 3 · 41%" or "". */
  label: string
  /** EPUB on-screen passage chars; null for PDF (page ±1 read Rust-side). */
  passageChars: number | null
  outlineEntries: number
  profileSet: boolean | null
  /** Whole-book availability, once checked (book mode only). */
  book: ChatBookStatus | null
  /** What the last reply reported, once one exists. */
  lastSent: { sampled: boolean; chars: number } | null
}

/** Visible caption lines describing exactly what a send includes. */
export function buildSentCaption(i: SentCaptionInput): string[] {
  const lines: string[] = []
  if (i.mode === 'book') {
    if (i.lastSent) {
      lines.push(
        i.lastSent.sampled
          ? `Excerpts sampled across the whole book (${i.lastSent.chars} characters of text — the book is long, so it is sampled, not sent in full)`
          : `The whole book's text (${i.lastSent.chars} characters)`,
      )
    } else if (i.book) {
      lines.push(
        i.book.sampled
          ? `Excerpts sampled across the whole book (sampled — the book's ${i.book.chars} characters are too long to send in full)`
          : `The whole book's text (${i.book.chars} characters)`,
      )
    } else {
      lines.push("The whole book's text (sampled if the book is long)")
    }
  } else {
    lines.push(
      i.passageChars != null
        ? `The page/section on screen (${i.passageChars} characters)`
        : 'The current page and the pages either side (read on this device)',
    )
  }
  lines.push(`Book: ${i.title}${i.author ? ` by ${i.author}` : ''}${i.label ? ` — ${i.label}` : ''}`)
  if (i.outlineEntries > 0) lines.push(`The book's contents outline (${i.outlineEntries} entries)`)
  if (i.profileSet) lines.push('Your saved background and goal for this book')
  lines.push('This conversation so far (older turns may be trimmed to fit; nothing is deleted here)')
  return lines
}

export interface WholeBookGate {
  disabled: boolean
  /** Explanation to show when disabled (or while checking). */
  reason: string | null
  /** "Sampled" note for long books, once known. */
  sampledNote: string | null
}

/** Whole-book mode availability from the device-local status check. */
export function wholeBookGate(book: ChatBookStatus | null, checking: boolean): WholeBookGate {
  if (checking) return { disabled: true, reason: 'Checking this book’s text on this device…', sampledNote: null }
  if (!book) return { disabled: false, reason: null, sampledNote: null }
  if (!book.available) {
    return {
      disabled: true,
      reason: book.reason ?? "This book has no readable text, so whole-book chat isn't available.",
      sampledNote: null,
    }
  }
  return {
    disabled: false,
    reason: null,
    sampledNote: book.sampled ? 'Long book — whole-book chat uses excerpts sampled across it.' : null,
  }
}

/**
 * True when a BC-3 hand-off is already the tail of the persisted chat
 * (so reopening the panel must not seed it twice).
 */
export function handoffAlreadySeeded(
  persisted: Pick<BookChatMessage, 'role' | 'content'>[],
  handoff: Pick<ExplainHandoff, 'history'>,
): boolean {
  const h = handoff.history
  if (h.length === 0 || persisted.length < h.length) return false
  const tail = persisted.slice(-h.length)
  return h.every((m, idx) => tail[idx].role === m.role && tail[idx].content === m.content)
}
