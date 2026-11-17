<script lang="ts">
  // Renders model text through the safe Markdown subset in
  // lib/markdown.ts. Every token becomes a Svelte element or
  // interpolated (escaped) text — never {@html} — so raw HTML or
  // scripts in a reply are shown literally, and links render as
  // text only (no href, no navigation).
  import { parseMarkdown, type Inline } from '../lib/markdown'

  let { text }: { text: string } = $props()
  const blocks = $derived(parseMarkdown(text))
</script>

{#snippet inlines(nodes: Inline[])}
  {#each nodes as n}
    {#if n.type === 'bold'}<strong>{@render inlines(n.children)}</strong>
    {:else if n.type === 'italic'}<em>{@render inlines(n.children)}</em>
    {:else if n.type === 'code'}<code>{n.text}</code>
    {:else if n.type === 'link'}<span class="linkish">{n.text}</span>
    {:else}{n.text}{/if}
  {/each}
{/snippet}

<div class="md">
  {#each blocks as b}
    {#if b.type === 'heading'}
      {#if b.level <= 2}<h4>{@render inlines(b.children)}</h4>{:else}<h5>{@render inlines(b.children)}</h5>{/if}
    {:else if b.type === 'code'}<pre class="code">{b.text}</pre>
    {:else if b.type === 'list'}
      {#if b.ordered}<ol>{#each b.items as item}<li>{@render inlines(item)}</li>{/each}</ol>
      {:else}<ul>{#each b.items as item}<li>{@render inlines(item)}</li>{/each}</ul>{/if}
    {:else if b.type === 'quote'}<blockquote>{@render inlines(b.children)}</blockquote>
    {:else if b.type === 'rule'}<hr />
    {:else}<p>{@render inlines(b.children)}</p>{/if}
  {/each}
</div>

<style>
  .md p {
    margin: 4px 0;
    white-space: pre-wrap;
    line-height: 1.45;
  }
  .md h4,
  .md h5 {
    margin: 8px 0 4px;
  }
  .md ul,
  .md ol {
    margin: 4px 0;
    padding-left: 20px;
  }
  .md blockquote {
    margin: 4px 0;
    padding-left: 10px;
    border-left: 3px solid var(--border);
    color: var(--text-2);
  }
  .md .code {
    overflow-x: auto;
    padding: 8px;
    background: var(--surface-2);
    border-radius: 6px;
    font-size: 12px;
  }
  .md code {
    font-size: 0.92em;
  }
  .md .linkish {
    text-decoration: underline dotted;
  }
</style>
