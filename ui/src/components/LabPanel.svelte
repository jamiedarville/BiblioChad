<script lang="ts">
  // "Build a lab": asks the configured LLM to analyse the whole book and
  // design a practical lab series. The guide is saved with the book in
  // Rust (lab_guides table), so it is simply here again on the next open.
  import { onMount } from 'svelte'
  import { save as saveDialog } from '@tauri-apps/plugin-dialog'
  import { api, errorMessage, type LabGuideRecord, type TutorStatus } from '../lib/api'
  import { toasts } from '../lib/toast.svelte'
  import { timeAgo } from '../lib/util'

  let { bookId, bookTitle }: { bookId: number; bookTitle: string } = $props()

  let status = $state<TutorStatus | null>(null)
  let record = $state<LabGuideRecord | null>(null)
  let stale = $state(false)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let labCount = $state(5)
  let focus = $state('')
  let openLab = $state(0)
  let revealed = $state<Record<string, boolean>>({})
  let done = $state<Set<string>>(new Set())

  const storageKey = () => `bibliochad-lab-progress-${bookId}`
  const guide = $derived(record?.guide ?? null)

  function loadProgress() {
    try {
      done = new Set(JSON.parse(localStorage.getItem(storageKey()) ?? '[]') as string[])
    } catch {
      done = new Set()
    }
  }

  function toggleStep(key: string, checked: boolean) {
    const next = new Set(done)
    if (checked) next.add(key)
    else next.delete(key)
    done = next
    try {
      localStorage.setItem(storageKey(), JSON.stringify([...next]))
    } catch {
      /* private mode: progress just won't persist */
    }
  }

  function labProgress(i: number): { done: number; total: number } {
    const lab = guide?.labs[i]
    if (!lab) return { done: 0, total: 0 }
    const total = lab.steps.length
    let n = 0
    for (let j = 0; j < total; j++) if (done.has(`${i}:${j}`)) n++
    return { done: n, total }
  }

  onMount(async () => {
    loadProgress()
    try {
      const [s, o] = await Promise.all([api.tutorStatus(), api.labOverview(bookId)])
      status = s
      record = o.lab
      stale = o.stale
    } catch (e) {
      error = errorMessage(e)
    }
  })

  async function generate() {
    if (record && !window.confirm('Rebuild the labs? The current guide for this book will be replaced.')) return
    busy = true
    error = null
    try {
      record = await api.labGenerate(bookId, labCount, focus.trim() || null)
      stale = false
      openLab = 0
      revealed = {}
      toasts.push('success', 'Labs built and saved with this book.')
    } catch (e) {
      error = errorMessage(e)
    } finally {
      busy = false
    }
  }

  async function remove() {
    if (!window.confirm('Delete the labs for this book?')) return
    try {
      await api.labDelete(bookId)
      record = null
      stale = false
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function exportMd() {
    const dest = await saveDialog({
      defaultPath: `${bookTitle.replace(/[\\/:*?"<>|]/g, '_')} - labs.md`,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    })
    if (!dest) return
    try {
      await api.labExport(bookId, dest)
      toasts.push('success', 'Labs exported as Markdown.')
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  // Minimal Mermaid flowchart rendering: boxes in first-appearance order,
  // stacked vertically with arrows for each parsed edge. Anything we can't
  // parse falls back to showing the Mermaid source.
  interface DiagramView {
    nodes: { id: string; label: string }[]
    edges: [string, string][]
  }

  function parseDiagram(mermaid: string): DiagramView | null {
    const nodes = new Map<string, string>()
    const edges: [string, string][] = []
    const nodeRef = /([A-Za-z][A-Za-z0-9_]*)(?:\[([^\]]*)\]|\(([^)]*)\)|\{([^}]*)\})?/g
    for (const raw of mermaid.split('\n')) {
      const line = raw.trim()
      if (!line || /^(graph|flowchart|sequenceDiagram|classDiagram|stateDiagram)/i.test(line)) continue
      const parts = line.split(/--?>|---|==>/)
      if (parts.length < 2) continue
      const ids: string[] = []
      for (const part of parts) {
        nodeRef.lastIndex = 0
        const m = nodeRef.exec(part.trim())
        if (!m) continue
        const id = m[1]
        const label = (m[2] ?? m[3] ?? m[4] ?? '').trim()
        if (!nodes.has(id)) nodes.set(id, label || id)
        else if (label) nodes.set(id, label)
        ids.push(id)
      }
      for (let i = 0; i + 1 < ids.length; i++) edges.push([ids[i], ids[i + 1]])
    }
    if (nodes.size < 2 || edges.length === 0) return null
    return { nodes: [...nodes.entries()].map(([id, label]) => ({ id, label })), edges }
  }
</script>

<div class="lab-panel">
  <h3>Build a lab</h3>
  <p class="muted small intro">
    Turn this book's theory into practice. Your LLM reads the whole book (long books are sampled from the
    beginning, middle and end) and designs a series of hands-on labs with setup, steps, diagrams and checks.
    The guide is saved with the book and loads again every time you open it.
  </p>

  {#if status && !status.ready}
    <div class="notice">
      <strong>Add an API key to start.</strong>
      <p>Open <em>Settings → Learning assistant</em> and paste a key from Anthropic or another LLM provider. Labs use the same key as the Learning zone.</p>
    </div>
  {/if}

  {#if !record}
    <div class="build-form">
      <label class="small">Number of labs
        <select bind:value={labCount}>
          {#each [3, 4, 5, 6, 7] as n}<option value={n}>{n} labs</option>{/each}
        </select>
      </label>
      <label class="small">Focus <span class="muted">(optional)</span>
        <input bind:value={focus} placeholder="e.g. home networking only, exam practice…" maxlength="200" />
      </label>
      <button class="primary" disabled={busy || !status?.ready} onclick={generate}>
        {busy ? 'Analysing the book and building labs…' : 'Build a lab'}
      </button>
      <p class="muted tiny">This sends book text to your chosen provider and can take a minute or two. Their charges apply. Your background and goal from the Learning zone are used to pitch the labs.</p>
    </div>
  {:else if guide}
    <div class="meta muted small">
      <span>{guide.labs.length} labs · built {timeAgo(record.updated_at)} · {record.model}</span>
    </div>
    {#if stale}
      <div class="notice"><strong>This book file has changed since these labs were built.</strong>
        <p>Rebuild to base the labs on the current version.</p></div>
    {/if}
    <div class="row wrap">
      <button class="small-btn" onclick={exportMd}>Export Markdown</button>
      <button class="small-btn" disabled={busy} onclick={generate}>{busy ? 'Rebuilding…' : 'Rebuild labs'}</button>
      <button class="ghost small-btn" onclick={remove}>Delete</button>
    </div>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if busy && record}<p class="muted small">Analysing the whole book… this can take a minute or two.</p>{/if}

  {#if guide}
    {#if guide.overview}<p class="overview">{guide.overview}</p>{/if}
    {#if guide.audience}<p class="small"><strong>Who it's for:</strong> {guide.audience}</p>{/if}
    {#if guide.prerequisites.length}
      <section>
        <h4>Before you start</h4>
        <ul>{#each guide.prerequisites as p}<li>{p}</li>{/each}</ul>
      </section>
    {/if}
    {#if guide.tools_overview}
      <section>
        <h4>Tools and setup at a glance</h4>
        <p class="small pre-wrap">{guide.tools_overview}</p>
      </section>
    {/if}
    {#if guide.safety_notes.length}
      <section>
        <h4>Safety and cost notes</h4>
        <ul>{#each guide.safety_notes as n}<li>{n}</li>{/each}</ul>
      </section>
    {/if}

    {#each guide.labs as lab, i (record!.id + i)}
      {@const prog = labProgress(i)}
      {@const open = openLab === i}
      <article class="lab">
        <button class="lab-head" onclick={() => (openLab = open ? -1 : i)} aria-expanded={open}>
          <span class="lab-title">Lab {i + 1}: {lab.title}</span>
          <span class="muted small">
            {#if lab.difficulty}{lab.difficulty} · {/if}{#if lab.estimated_minutes}~{lab.estimated_minutes} min · {/if}{prog.done}/{prog.total} steps
          </span>
        </button>
        {#if open}
          <div class="lab-body">
            {#if lab.objective}<p><strong>Objective:</strong> {lab.objective}</p>{/if}
            {#if lab.source_refs.length}<p class="small muted">In the book: {lab.source_refs.join('; ')}</p>{/if}
            {#if lab.concepts.length}<p class="small muted">Concepts: {lab.concepts.join(', ')}</p>{/if}

            {#if lab.setup_requirements.length || lab.setup_steps.length}
              <h5>Set up the lab</h5>
              {#if lab.setup_requirements.length}
                <ul>{#each lab.setup_requirements as r}<li>{r}</li>{/each}</ul>
              {/if}
              {#if lab.setup_steps.length}
                <ol>{#each lab.setup_steps as s}<li>{s}</li>{/each}</ol>
              {/if}
            {/if}

            {#if lab.diagram.mermaid || lab.diagram.explanation}
              <h5>{lab.diagram.title || 'Diagram'}{#if lab.diagram.kind} <span class="muted small">({lab.diagram.kind})</span>{/if}</h5>
              {#if lab.diagram.mermaid}
                {@const d = parseDiagram(lab.diagram.mermaid)}
                {#if d}
                  <svg class="diagram" viewBox="0 0 320 {d.nodes.length * 62 + 10}" role="img" aria-label={lab.diagram.title || 'Lab diagram'}>
                    <defs>
                      <marker id="arr-{i}" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto">
                        <path d="M0 0 L7 3 L0 6 z" class="arrow-head" />
                      </marker>
                    </defs>
                    {#each d.edges as [a, b]}
                      {@const ai = d.nodes.findIndex((n) => n.id === a)}
                      {@const bi = d.nodes.findIndex((n) => n.id === b)}
                      {#if ai >= 0 && bi > ai}
                        <line x1="160" y1={ai * 62 + 48} x2="160" y2={bi * 62 + 6} class="edge" marker-end="url(#arr-{i})" />
                      {/if}
                    {/each}
                    {#each d.nodes as n, ni}
                      <rect x="45" y={ni * 62 + 6} width="230" height="42" rx="6" class="node-box" />
                      <text x="160" y={ni * 62 + 31} text-anchor="middle" class="node-text">{n.label.length > 34 ? n.label.slice(0, 33) + '…' : n.label}</text>
                    {/each}
                  </svg>
                {:else}
                  <pre class="diagram-source">{lab.diagram.mermaid}</pre>
                {/if}
              {/if}
              {#if lab.diagram.explanation}<p class="small">{lab.diagram.explanation}</p>{/if}
            {/if}

            <h5>Steps</h5>
            <ol class="steps">
              {#each lab.steps as step, j}
                <li>
                  <label class="step-head">
                    <input type="checkbox" checked={done.has(`${i}:${j}`)} onchange={(e) => toggleStep(`${i}:${j}`, (e.target as HTMLInputElement).checked)} />
                    <strong>{step.title || `Step ${j + 1}`}</strong>
                  </label>
                  {#if step.instructions}<p>{step.instructions}</p>{/if}
                  {#if step.why}<p class="small"><span class="tag">Why</span> {step.why}</p>{/if}
                  {#if step.expected}<p class="small"><span class="tag">You should see</span> {step.expected}</p>{/if}
                  {#if step.tip}<p class="small muted"><span class="tag">Tip</span> {step.tip}</p>{/if}
                </li>
              {/each}
            </ol>

            {#if lab.checks.length}
              <h5>Check yourself</h5>
              <ol class="checks">
                {#each lab.checks as c, j}
                  <li>
                    {c.question}
                    {#if revealed[`${i}:${j}`]}
                      <p class="answer">{c.answer}</p>
                    {:else}
                      <button class="ghost small-btn" onclick={() => (revealed[`${i}:${j}`] = true)}>Show answer</button>
                    {/if}
                  </li>
                {/each}
              </ol>
            {/if}

            {#if lab.troubleshooting.length}
              <h5>If it goes wrong</h5>
              <ul>{#each lab.troubleshooting as t}<li>{t}</li>{/each}</ul>
            {/if}
            {#if lab.extension}
              <h5>Go further</h5>
              <p class="small">{lab.extension}</p>
            {/if}
          </div>
        {/if}
      </article>
    {/each}

    {#if guide.capstone}
      <section>
        <h4>Capstone</h4>
        <p class="small pre-wrap">{guide.capstone}</p>
      </section>
    {/if}
    <p class="muted tiny">Generated from the book text by your chosen provider. Answers and commands can be wrong; check them against the book. Tick steps off as you go — ticks are kept on this device.</p>
  {/if}
</div>

<style>
  .lab-panel { display: flex; flex-direction: column; gap: 8px; }
  h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-2); margin: 14px 0 0; }
  h4 { margin: 10px 0 6px; font-size: 13px; }
  h5 { margin: 12px 0 6px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-2); }
  p { margin: 0 0 6px; }
  .small { font-size: 12px; }
  .tiny { font-size: 11px; }
  .intro { margin: 0; }
  .notice { border: 1px solid var(--accent); border-radius: var(--radius); padding: 8px 10px; }
  .notice p { margin: 4px 0 0; }
  .build-form { display: flex; flex-direction: column; gap: 8px; }
  .build-form label { display: flex; flex-direction: column; gap: 3px; }
  .build-form input, .build-form select { width: 100%; font: inherit; }
  .row { display: flex; gap: 6px; align-items: center; }
  .wrap { flex-wrap: wrap; }
  .small-btn { min-height: 28px; padding: 3px 10px; font-size: 12px; }
  .error { color: var(--danger); font-size: 13px; }
  .overview { font-style: italic; }
  .pre-wrap { white-space: pre-wrap; }
  ul, ol { margin: 0; padding-left: 18px; display: grid; gap: 4px; font-size: 13px; }
  .lab { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .lab-head { width: 100%; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; text-align: left; padding: 10px; background: var(--surface-2); border: none; cursor: pointer; font: inherit; }
  .lab-title { font-weight: 600; font-size: 13px; }
  .lab-body { padding: 10px; font-size: 13px; }
  .steps { list-style: none; padding-left: 0; }
  .steps li { padding: 8px 0; border-bottom: 1px solid var(--border); }
  .steps li:last-child { border-bottom: none; }
  .step-head { display: flex; gap: 8px; align-items: baseline; cursor: pointer; }
  .tag { display: inline-block; font-size: 10px; text-transform: uppercase; letter-spacing: 0.05em; padding: 1px 5px; border-radius: 4px; background: var(--surface-2); }
  .checks .answer { margin-top: 4px; }
  .diagram { width: 100%; max-width: 340px; display: block; margin: 4px auto; }
  .node-box { fill: var(--surface-2); stroke: var(--accent); }
  .node-text { fill: var(--text); font-size: 11px; }
  .edge { stroke: var(--text-2); stroke-width: 1.5; }
  .arrow-head { fill: var(--text-2); }
  .diagram-source { white-space: pre-wrap; font-size: 11px; background: var(--surface-2); border-radius: var(--radius); padding: 8px; overflow-x: auto; }
</style>
