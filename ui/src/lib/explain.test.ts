import { describe, expect, it } from 'vitest'
import {
  buildExplainDisclosure,
  buildExplainHandoff,
  buildExplainHistory,
  clearExplainHandoff,
  explainHandoffKey,
  loadExplainHandoff,
  normalizePdfSelectionText,
  saveExplainHandoff,
  trimSelection,
  MAX_EXPLAIN_SELECTION_CHARS,
} from './explain'

describe('trimSelection', () => {
  it('keeps short selections, trimmed of edge whitespace only', () => {
    expect(trimSelection('  hello world  ')).toEqual({ text: 'hello world', trimmed: false, originalLength: 11 })
    expect(trimSelection('   ')).toEqual({ text: '', trimmed: false, originalLength: 0 })
  })
  it('cuts over-long selections at a word boundary and flags it', () => {
    const t = trimSelection('word '.repeat(100), 50)
    expect(t.trimmed).toBe(true)
    expect(t.originalLength).toBe(499)
    expect(t.text.length).toBeLessThanOrEqual(50)
    expect(t.text.endsWith('word')).toBe(true)
  })
  it('hard-cuts a single huge word at the cap', () => {
    const t = trimSelection('x'.repeat(MAX_EXPLAIN_SELECTION_CHARS + 10))
    expect(t).toEqual({ text: 'x'.repeat(MAX_EXPLAIN_SELECTION_CHARS), trimmed: true, originalLength: MAX_EXPLAIN_SELECTION_CHARS + 10 })
  })
})

describe('normalizePdfSelectionText', () => {
  it('collapses ragged span spacing', () => {
    expect(normalizePdfSelectionText('  alpha\u00a0 beta\t\tgamma \n \n\n\n delta ')).toBe('alpha beta gamma\n\ndelta')
  })
  it('returns empty for whitespace-only captures', () => {
    expect(normalizePdfSelectionText(' \n\t ')).toBe('')
  })
})

describe('explain payload building', () => {
  it('builds one user turn quoting the selection', () => {
    const h = buildExplainHistory('the mitochondria')
    expect(h).toHaveLength(1)
    expect(h[0].role).toBe('user')
    expect(h[0].content).toContain('"""\nthe mitochondria\n"""')
  })
  it('discloses selection, context, book and profile only when set', () => {
    const base = { title: 'T', author: 'A', label: 'Ch. 1 · 4%', selectionChars: 12, contextChars: 900, profileSet: false }
    expect(buildExplainDisclosure(base)).toEqual([
      'Your selected text (12 characters)',
      'The text on screen around it (900 characters)',
      'Book: T by A — Ch. 1 · 4%',
    ])
    expect(buildExplainDisclosure({ ...base, contextChars: null, profileSet: true })).toEqual([
      'Your selected text (12 characters)',
      'The selected page and the pages either side (read on this device)',
      'Book: T by A — Ch. 1 · 4%',
      'Your saved background and goal for this book',
    ])
  })
})

describe('BC-4 explain hand-off', () => {
  function fakeStore() {
    const m = new Map<string, string>()
    return {
      getItem: (k: string) => m.get(k) ?? null,
      setItem: (k: string, v: string) => void m.set(k, v),
      removeItem: (k: string) => void m.delete(k),
    }
  }
  const args = {
    bookId: 7,
    locator: { kind: 'epub', cfi: 'epubcfi(/6/4)', percent: 0.5 } as const,
    label: 'Ch. 2 · 50%',
    selectionText: 'selected words',
    explanation: 'It means this.',
    model: 'm1',
    truncated: false,
    now: 1234,
  }
  it('seeds history as explain prompt + assistant explanation', () => {
    const h = buildExplainHandoff(args)
    expect(h.history[0]).toEqual(buildExplainHistory('selected words')[0])
    expect(h.history[1]).toEqual({ role: 'assistant', content: 'It means this.' })
    expect(h.createdAt).toBe(1234)
  })
  it('round-trips through storage and clears', () => {
    const store = fakeStore()
    const h = buildExplainHandoff(args)
    expect(saveExplainHandoff(h, store)).toBe(true)
    expect(loadExplainHandoff(7, store)).toEqual(h)
    expect(loadExplainHandoff(8, store)).toBeNull()
    clearExplainHandoff(7, store)
    expect(loadExplainHandoff(7, store)).toBeNull()
    expect(explainHandoffKey(7)).toBe('bibliochad-explain-handoff-7')
  })
  it('rejects corrupt or foreign records', () => {
    const store = fakeStore()
    store.setItem(explainHandoffKey(7), '{not json')
    expect(loadExplainHandoff(7, store)).toBeNull()
    store.setItem(explainHandoffKey(7), JSON.stringify({ version: 2, bookId: 7, history: [] }))
    expect(loadExplainHandoff(7, store)).toBeNull()
  })
})
