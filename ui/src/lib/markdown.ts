// A small, safe Markdown subset for model chat replies (BC-4).
//
// Safety contract: this parser only ever produces the token types
// below. There is no HTML token and no renderer may use `{@html}` on
// model text — SafeMarkdown.svelte renders every token as Svelte
// elements/interpolated text, so markup in a reply (including
// `<script>…`) is shown as literal text. Links keep only their text:
// no `href` is ever emitted, so a reply cannot navigate the webview.
// The app CSP (script-src 'self') is untouched.

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'bold'; children: Inline[] }
  | { type: 'italic'; children: Inline[] }
  | { type: 'code'; text: string }
  | { type: 'link'; text: string }

export type Block =
  | { type: 'paragraph'; children: Inline[] }
  | { type: 'heading'; level: number; children: Inline[] }
  | { type: 'code'; text: string }
  | { type: 'list'; ordered: boolean; items: Inline[][] }
  | { type: 'quote'; children: Inline[] }
  | { type: 'rule' }

const INLINE_RE =
  /(\*\*[^*]+\*\*|__[^_]+__|\*[^*\n]+\*|_[^_\n]+_|`[^`\n]+`|\[[^\]\n]*\]\([^)\n]*\))/g

/** Parse inline emphasis/code/links. Everything else stays plain text. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = []
  for (const part of text.split(INLINE_RE)) {
    if (!part) continue
    if ((part.startsWith('**') && part.endsWith('**')) || (part.startsWith('__') && part.endsWith('__'))) {
      out.push({ type: 'bold', children: parseInline(part.slice(2, -2)) })
    } else if (
      (part.startsWith('*') && part.endsWith('*') && part.length > 2) ||
      (part.startsWith('_') && part.endsWith('_') && part.length > 2)
    ) {
      out.push({ type: 'italic', children: parseInline(part.slice(1, -1)) })
    } else if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
      out.push({ type: 'code', text: part.slice(1, -1) })
    } else {
      const link = /^\[([^\]\n]*)\]\([^)\n]*\)$/.exec(part)
      // Link destinations are dropped entirely — see the safety note.
      if (link) out.push({ type: 'link', text: link[1] || part })
      else out.push({ type: 'text', text: part })
    }
  }
  return out
}

/** Parse a Markdown document into safe blocks. */
export function parseMarkdown(src: string): Block[] {
  const lines = src.replace(/\r\n?/g, '\n').split('\n')
  const blocks: Block[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    if (!line.trim()) {
      i++
      continue
    }
    if (line.trimStart().startsWith('```')) {
      const buf: string[] = []
      i++
      while (i < lines.length && !lines[i].trimStart().startsWith('```')) buf.push(lines[i++])
      if (i < lines.length) i++ // closing fence (a missing one just ends the block)
      blocks.push({ type: 'code', text: buf.join('\n') })
      continue
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    if (heading) {
      blocks.push({ type: 'heading', level: heading[1].length, children: parseInline(heading[2]) })
      i++
      continue
    }
    if (/^(\s*)(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      blocks.push({ type: 'rule' })
      i++
      continue
    }
    if (/^\s*([-*+]|\d+[.)])\s+/.test(line)) {
      const ordered = /^\s*\d+[.)]\s+/.test(line)
      const items: Inline[][] = []
      while (i < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[i])) {
        items.push(parseInline(lines[i].replace(/^\s*([-*+]|\d+[.)])\s+/, '')))
        i++
      }
      blocks.push({ type: 'list', ordered, items })
      continue
    }
    if (/^\s*>/.test(line)) {
      const buf: string[] = []
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        buf.push(lines[i].replace(/^\s*>\s?/, ''))
        i++
      }
      blocks.push({ type: 'quote', children: parseInline(buf.join(' ')) })
      continue
    }
    const buf: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() &&
      !lines[i].trimStart().startsWith('```') &&
      !/^(#{1,6})\s+/.test(lines[i]) &&
      !/^\s*([-*+]|\d+[.)])\s+/.test(lines[i]) &&
      !/^\s*>/.test(lines[i])
    ) {
      buf.push(lines[i])
      i++
    }
    blocks.push({ type: 'paragraph', children: parseInline(buf.join('\n')) })
  }
  return blocks
}
