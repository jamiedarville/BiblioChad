import { describe, expect, it } from 'vitest'
import { buildSentCaption, handoffAlreadySeeded, toHistory, wholeBookGate } from './chat'
import type { BookChatMessage } from './api'
import { buildExplainHandoff } from './explain'

const base = {
  title: 'Dune',
  author: 'Frank Herbert',
  label: 'Chapter 3 · 41%',
  passageChars: 1234,
  outlineEntries: 12,
  profileSet: true,
  book: null,
  lastSent: null,
}

describe('buildSentCaption', () => {
  it('passage mode names page, metadata, outline and profile', () => {
    const lines = buildSentCaption({ ...base, mode: 'passage' })
    expect(lines[0]).toContain('page/section on screen (1234 characters)')
    expect(lines).toContain('Book: Dune by Frank Herbert — Chapter 3 · 41%')
    expect(lines.some((l) => l.includes('contents outline (12 entries)'))).toBe(true)
    expect(lines.some((l) => l.includes('saved background and goal'))).toBe(true)
    expect(lines.at(-1)).toContain('nothing is deleted')
  })

  it('PDF passage mode says page ±1 and omits unset profile/empty outline', () => {
    const lines = buildSentCaption({ ...base, mode: 'passage', passageChars: null, outlineEntries: 0, profileSet: false })
    expect(lines[0]).toContain('current page and the pages either side')
    expect(lines.some((l) => l.includes('outline'))).toBe(false)
    expect(lines.some((l) => l.includes('background'))).toBe(false)
  })

  it('book mode labels sampled books, before and after a send', () => {
    const pre = buildSentCaption({
      ...base,
      mode: 'book',
      book: { available: true, sampled: true, chars: 900_000, reason: null },
    })
    expect(pre[0]).toContain('sampled')
    const post = buildSentCaption({ ...base, mode: 'book', lastSent: { sampled: true, chars: 900_000 } })
    expect(post[0]).toContain('sampled, not sent in full')
    const whole = buildSentCaption({ ...base, mode: 'book', lastSent: { sampled: false, chars: 4000 } })
    expect(whole[0]).toBe("The whole book's text (4000 characters)")
  })
})

describe('wholeBookGate', () => {
  it('disables with the scanned-book explanation when unavailable', () => {
    const g = wholeBookGate({ available: false, sampled: false, chars: 0, reason: 'scan' }, false)
    expect(g).toEqual({ disabled: true, reason: 'scan', sampledNote: null })
  })

  it('notes sampling for long books and stays enabled', () => {
    const g = wholeBookGate({ available: true, sampled: true, chars: 10, reason: null }, false)
    expect(g.disabled).toBe(false)
    expect(g.sampledNote).toContain('sampled')
  })

  it('disables while checking', () => {
    expect(wholeBookGate(null, true).disabled).toBe(true)
  })
})

describe('history + hand-off seeding', () => {
  it('converts persisted rows to chatSend history', () => {
    const rows = [
      { role: 'user', content: 'q' },
      { role: 'assistant', content: 'a' },
    ] as Pick<BookChatMessage, 'role' | 'content'>[]
    expect(toHistory(rows)).toEqual(rows)
  })

  it('detects an already-seeded hand-off at the tail only', () => {
    const h = buildExplainHandoff({
      bookId: 7,
      locator: { kind: 'pdf', page: 1, offset: 0, fit: 'width', zoom: 1 },
      label: 'Page 2',
      selectionText: 'sel',
      explanation: 'because',
      model: 'm',
      truncated: false,
    })
    const asRows = h.history.map((m) => ({ role: m.role, content: m.content }))
    expect(handoffAlreadySeeded(asRows, h)).toBe(true)
    expect(handoffAlreadySeeded([{ role: 'user', content: 'other' }], h)).toBe(false)
    expect(handoffAlreadySeeded([], h)).toBe(false)
  })
})
