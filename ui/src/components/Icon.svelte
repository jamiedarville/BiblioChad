<script lang="ts">
  // Small stroke icons (original, 24x24 grid).
  const paths: Record<string, string> = {
    back: 'M15 18l-6-6 6-6',
    toc: 'M4 6h16M4 12h16M4 18h10',
    bookmark: 'M6 3h12v18l-6-4-6 4z',
    'bookmark-add': 'M6 3h12v18l-6-4-6 4zM12 7v6M9 10h6',
    search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
    type: 'M4 19l5-14 5 14M6 13h6M15 19l3-8 3 8M16 16h4',
    fullscreen: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
    settings: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1',
    grid: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
    list: 'M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01',
    'sort-asc': 'M12 19V5M6 11l6-6 6 6',
    'sort-desc': 'M12 5v14M6 13l6 6 6-6',
    close: 'M6 6l12 12M18 6L6 18',
    more: 'M5 12h.01M12 12h.01M19 12h.01',
  }
  let { name, size = 18, label }: { name: string; size?: number; label?: string } = $props()
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden={label ? undefined : 'true'}
  role={label ? 'img' : undefined}
  aria-label={label}
  style="vertical-align: middle"
>
  <path d={paths[name] ?? ''} />
</svg>
