import { describe, expect, it, vi } from 'vitest'
import { debounce, encodeEntryPath, formatBytes, formatPercent, pageAt, protoUrl, renderWidth, timeAgo } from './util'
import { rankProgress, greeting } from './chad'

describe('protoUrl', () => {
  it('uses http://bibliochad.localhost on Windows', () => {
    expect(protoUrl('/cover/1', true)).toBe('http://bibliochad.localhost/cover/1')
    expect(protoUrl('cover/1', false)).toBe('bibliochad://localhost/cover/1')
  })
  it('encodes archive paths per segment', () => {
    expect(encodeEntryPath('OEBPS/images/cover art#1.jpg')).toBe('OEBPS/images/cover%20art%231.jpg')
  })
})

describe('formatting', () => {
  it('formats bytes and percents', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1536)).toBe('1.5 KB')
    expect(formatBytes(5 * 1024 * 1024 * 1024)).toBe('5.0 GB')
    expect(formatPercent(0.004)).toBe('<1%')
    expect(formatPercent(0.426)).toBe('43%')
  })
  it('formats relative times', () => {
    const now = 1_000_000_000
    expect(timeAgo(null)).toBe('never')
    expect(timeAgo(now - 30_000, now)).toBe('just now')
    expect(timeAgo(now - 5 * 60_000, now)).toBe('5 min ago')
    expect(timeAgo(now - 3 * 3600_000, now)).toBe('3 h ago')
  })
})

describe('pdf helpers', () => {
  it('buckets render widths', () => {
    expect(renderWidth(700, 1.5)).toBe(1200)
    expect(renderWidth(10, 1)).toBe(200)
    expect(renderWidth(5000, 2)).toBe(4000)
  })
  it('finds the page at a scroll offset', () => {
    const offsets = [0, 1000, 2000]
    const heights = [990, 990, 990]
    expect(pageAt(offsets, heights, 0)).toEqual({ page: 0, offset: 0 })
    expect(pageAt(offsets, heights, 1495)).toEqual({ page: 1, offset: 0.5 })
    expect(pageAt(offsets, heights, 99999).page).toBe(2)
    expect(pageAt([], [], 10)).toEqual({ page: 0, offset: 0 })
  })
})

describe('debounce', () => {
  it('runs once after the delay and supports flush', () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const d = debounce(fn, 2000)
    d(1)
    d(2)
    vi.advanceTimersByTime(1999)
    expect(fn).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(fn).toHaveBeenCalledWith(2)
    d(3)
    d.flush()
    expect(fn).toHaveBeenLastCalledWith(3)
    expect(fn).toHaveBeenCalledTimes(2)
    d.flush()
    expect(fn).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })
})

describe('chad', () => {
  it('computes rank progress', () => {
    expect(rankProgress({ name: 'Page Turner', threshold: 3, next_threshold: 10 }, 6)).toBeCloseTo(3 / 7)
    expect(rankProgress({ name: 'Gigachad of Letters', threshold: 50, next_threshold: null }, 80)).toBe(1)
  })
  it('stays quiet without Chad Mode', () => {
    expect(greeting(false, 3)).toBe('Library')
    expect(greeting(true, 3)).not.toBe('Library')
  })
})
