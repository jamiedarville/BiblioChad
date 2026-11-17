import { describe, expect, it } from 'vitest'
import { parseInline, parseMarkdown, type Block, type Inline } from './markdown'

function inlineText(nodes: Inline[]): string {
  return nodes
    .map((n) => (n.type === 'bold' || n.type === 'italic' ? inlineText(n.children) : n.text))
    .join('')
}

describe('parseMarkdown blocks', () => {
  it('parses headings, paragraphs, lists, quotes and fenced code', () => {
    const blocks = parseMarkdown('# Title\n\nHello **world**.\n\n- one\n- two\n\n> quoted\n\n```\nlet x = 1\n```')
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'paragraph', 'list', 'quote', 'code'])
    expect(blocks[0]).toMatchObject({ level: 1 })
    const list = blocks[2] as Extract<Block, { type: 'list' }>
    expect(list.ordered).toBe(false)
    expect(list.items.map(inlineText)).toEqual(['one', 'two'])
    expect(blocks[4]).toEqual({ type: 'code', text: 'let x = 1' })
  })

  it('treats an unclosed fence as code to end of input', () => {
    expect(parseMarkdown('before\n\n```js\ncode')).toEqual([
      { type: 'paragraph', children: [{ type: 'text', text: 'before' }] },
      { type: 'code', text: 'code' },
    ])
  })
})

describe('safety', () => {
  it('keeps raw HTML and scripts as literal text tokens', () => {
    const src = 'Look: <script>alert(1)</script> and <img src=x onerror=alert(2)>'
    const blocks = parseMarkdown(src)
    expect(blocks).toHaveLength(1)
    expect(JSON.stringify(blocks)).not.toContain('"html"')
    const p = blocks[0] as Extract<Block, { type: 'paragraph' }>
    expect(inlineText(p.children)).toBe(src)
  })

  it('drops link destinations, including javascript: URLs', () => {
    const nodes = parseInline('[click](javascript:alert(1)) and [ok](https://example.com)')
    expect(nodes.filter((n) => n.type === 'link')).toEqual([
      { type: 'link', text: 'click' },
      { type: 'link', text: 'ok' },
    ])
    expect(JSON.stringify(nodes)).not.toContain('javascript:')
    expect(JSON.stringify(nodes)).not.toContain('example.com')
  })
})

describe('parseInline', () => {
  it('parses bold, italic and code', () => {
    expect(parseInline('a **b** *c* `d`')).toEqual([
      { type: 'text', text: 'a ' },
      { type: 'bold', children: [{ type: 'text', text: 'b' }] },
      { type: 'text', text: ' ' },
      { type: 'italic', children: [{ type: 'text', text: 'c' }] },
      { type: 'text', text: ' ' },
      { type: 'code', text: 'd' },
    ])
  })
})
