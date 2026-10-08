// Small pure helpers (unit tested in util.test.ts).

/** URL for the bibliochad:// custom protocol. WebView2 on Windows serves
 * custom schemes as http://<scheme>.localhost/. */
export function protoUrl(path: string, isWindows = detectWindows()): string {
  const clean = path.replace(/^\/+/, '')
  return isWindows ? `http://bibliochad.localhost/${clean}` : `bibliochad://localhost/${clean}`
}

function detectWindows(): boolean {
  return typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
}

/** Encode an archive path for the URL, segment by segment. */
export function encodeEntryPath(name: string): string {
  return name.split('/').map(encodeURIComponent).join('/')
}

export function formatBytes(n: number | null | undefined): string {
  if (n == null) return ''
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`
}

export function formatPercent(p: number): string {
  if (p > 0 && p < 0.01) return '<1%'
  return `${Math.round(p * 100)}%`
}

export function timeAgo(ms: number | null | undefined, now = Date.now()): string {
  if (!ms) return 'never'
  const s = Math.max(0, Math.round((now - ms) / 1000))
  if (s < 60) return 'just now'
  const m = Math.round(s / 60)
  if (m < 60) return `${m} min ago`
  const h = Math.round(m / 60)
  if (h < 24) return `${h} h ago`
  const d = Math.round(h / 24)
  if (d < 30) return `${d} d ago`
  return new Date(ms).toLocaleDateString()
}

/** Deterministic pleasant hue for placeholder covers. */
export function hueFor(text: string): number {
  let h = 0
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0
  return h % 360
}

/** Trailing-edge debounce with an explicit flush (used for saving the
 * reading position about 2 s after the last page turn). */
export function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending: A | null = null
  const call = (...args: A) => {
    pending = args
    if (timer) clearTimeout(timer)
    timer = setTimeout(flush, ms)
  }
  function flush() {
    if (timer) clearTimeout(timer)
    timer = null
    if (pending) {
      const args = pending
      pending = null
      fn(...args)
    }
  }
  call.flush = flush
  call.cancel = () => {
    if (timer) clearTimeout(timer)
    timer = null
    pending = null
  }
  return call
}

/** Pick a render width bucket so page images can be cached by the webview. */
export function renderWidth(cssWidth: number, dpr: number): number {
  const px = Math.max(200, Math.ceil(cssWidth * dpr))
  return Math.min(4000, Math.ceil(px / 200) * 200)
}

/** Given the top offsets of pages (and total height), find the page at a
 * scroll position, plus how far into that page it is (0..1). */
export function pageAt(offsets: number[], heights: number[], scrollTop: number): { page: number; offset: number } {
  if (offsets.length === 0) return { page: 0, offset: 0 }
  let lo = 0
  let hi = offsets.length - 1
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (offsets[mid] <= scrollTop) lo = mid
    else hi = mid - 1
  }
  const h = heights[lo] || 1
  return { page: lo, offset: Math.min(1, Math.max(0, (scrollTop - offsets[lo]) / h)) }
}
