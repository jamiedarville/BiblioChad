<script lang="ts">
  // "Build a lab" as a full page: the lab gets the same reading space as
  // the book. The guide is saved with the book in Rust (lab_guides), so it
  // is simply here again on every open. In split view this component sits
  // beside the book; container queries collapse its layout when narrow.
  import { onMount } from 'svelte'
  import { save as saveDialog } from '@tauri-apps/plugin-dialog'
  import { api, errorMessage, labDiagrams, type LabDiagram, type LabGuideRecord, type TutorStatus } from '../lib/api'
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
  let currentLab = $state(0)
  let revealed = $state<Record<string, boolean>>({})
  let done = $state<Set<string>>(new Set())

  const storageKey = () => `bibliochad-lab-progress-${bookId}`
  const guide = $derived(record?.guide ?? null)
  const lab = $derived(guide && guide.labs.length ? guide.labs[Math.min(currentLab, guide.labs.length - 1)] : null)
  const totals = $derived.by(() => {
    if (!guide) return { done: 0, total: 0 }
    let total = 0
    guide.labs.forEach((l, i) => {
      total += l.steps.length
    })
    return { done: [...done].filter((k) => guide.labs.some((l, i) => k.startsWith(`${i}:`) && Number(k.split(':')[1]) < l.steps.length)).length, total }
  })

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
    const l = guide?.labs[i]
    if (!l) return { done: 0, total: 0 }
    let n = 0
    for (let j = 0; j < l.steps.length; j++) if (done.has(`${i}:${j}`)) n++
    return { done: n, total: l.steps.length }
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
      currentLab = 0
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
      currentLab = 0
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

  async function copyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code)
      toasts.push('info', 'Copied.')
    } catch {
      toasts.push('error', 'Copy failed — select the code and copy it manually.')
    }
  }

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

<div class="lab-page">
  <div class="lab-inner">
    <header class="lab-top">
      <div>
        <h2>Labs for {bookTitle}</h2>
        <p class="muted">
          Turn the book's theory into practice. Labs are saved with this book and come back every time you open it.
        </p>
      </div>
      {#if guide && record}
        <div class="toolbar">
          <button class="small-btn" onclick={exportMd}>Export Markdown</button>
          <button class="small-btn" disabled={busy} onclick={generate}>{busy ? 'Rebuilding…' : 'Rebuild labs'}</button>
          <button class="ghost small-btn" onclick={remove}>Delete</button>
        </div>
      {/if}
    </header>

    {#if status && !status.ready}
      <div class="notice">
        <strong>Add an API key to start.</strong>
        <p>Open <em>Settings → Learning assistant</em> and paste a key from Anthropic or another LLM provider. Labs use the same key as the Learning zone.</p>
      </div>
    {/if}

    {#if !record}
      <section class="build">
        <h3>Build the lab series</h3>
        <p>
          Your LLM reads the whole book — short books in full, long books sampled from the beginning, middle and
          end, plus the contents — and designs a sequenced series of hands-on labs: theory recap, learning
          outcomes, full setup, diagrams, and detailed step-by-step instructions with exact commands where the
          book supports them.
        </p>
        <div class="build-form">
          <label>Number of labs
            <select bind:value={labCount}>
              {#each [3, 4, 5, 6, 7] as n}<option value={n}>{n} labs</option>{/each}
            </select>
          </label>
          <label>Focus <span class="muted">(optional)</span>
            <input bind:value={focus} placeholder="e.g. home networking only, exam practice…" maxlength="200" />
          </label>
          <button class="primary" disabled={busy || !status?.ready} onclick={generate}>
            {busy ? 'Analysing the book and building labs…' : 'Build a lab'}
          </button>
        </div>
        <p class="muted small">
          This sends book text to your chosen provider and can take a few minutes — detailed labs are a long
          answer. Their charges apply. Your background and goal from the Learning zone pitch the level.
        </p>
      </section>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if busy && record}<p class="muted">Analysing the whole book… this can take a few minutes.</p>{/if}

    {#if guide && record && lab}
      <p class="meta muted small">
        {guide.labs.length} labs · built {timeAgo(record.updated_at)} · {record.model} ·
        {totals.done}/{totals.total} steps done
      </p>
      {#if totals.total}
        <div class="progress" aria-hidden="true"><div style:width="{(totals.done / totals.total) * 100}%"></div></div>
      {/if}
      {#if stale}
        <div class="notice"><strong>This book file has changed since these labs were built.</strong>
          <p>Rebuild to base the labs on the current version.</p></div>
      {/if}

      {#if guide.overview}<p class="overview">{guide.overview}</p>{/if}
      {#if guide.audience}<p><strong>Who it's for:</strong> {guide.audience}</p>{/if}

      <div class="lab-layout">
        <nav class="lab-index" aria-label="Labs">
          {#each guide.labs as l, i}
            {@const prog = labProgress(i)}
            <button class:on={i === currentLab} onclick={() => (currentLab = i)}>
              <span class="idx-title">Lab {i + 1}: {l.title}</span>
              <span class="muted small">
                {#if l.difficulty}{l.difficulty} · {/if}{#if l.estimated_minutes}~{l.estimated_minutes} min · {/if}{prog.done}/{prog.total} steps
              </span>
            </button>
          {/each}
          {#if guide.capstone}
            <button class:on={currentLab === guide.labs.length} onclick={() => (currentLab = guide.labs.length)}>
              <span class="idx-title">Capstone</span>
              <span class="muted small">Final project</span>
            </button>
          {/if}
        </nav>

        <article class="lab-detail">
          {#if currentLab < guide.labs.length}
            <h3>Lab {currentLab + 1}: {lab.title}</h3>
            <p class="muted">
              {#if lab.difficulty}{lab.difficulty}{/if}
              {#if lab.estimated_minutes} · about {lab.estimated_minutes} minutes{/if}
              {#if lab.source_refs.length} · In the book: {lab.source_refs.join('; ')}{/if}
            </p>
            {#if lab.objective}<p class="objective">{lab.objective}</p>{/if}
            {#if lab.deliverable}<p><strong>You will produce:</strong> {lab.deliverable}</p>{/if}
            {#if lab.concepts.length}<p class="muted small">Concepts: {lab.concepts.join(', ')}</p>{/if}

            {#if lab.outcomes && lab.outcomes.length}
              <h4>You will be able to</h4>
              <ul>{#each lab.outcomes as o}<li>{o}</li>{/each}</ul>
            {/if}

            {#if lab.theory}
              <h4>The theory behind this lab</h4>
              {#each lab.theory.split(/\n\s*\n/) as para}
                <p>{para}</p>
              {/each}
            {/if}

            {#if currentLab === 0 && (guide.prerequisites.length || guide.tools_overview || guide.safety_notes.length)}
              {#if guide.prerequisites.length}
                <h4>Before you start</h4>
                <ul>{#each guide.prerequisites as p}<li>{p}</li>{/each}</ul>
              {/if}
              {#if guide.tools_overview}
                <h4>Tools and setup at a glance</h4>
                <p class="pre-wrap">{guide.tools_overview}</p>
              {/if}
              {#if guide.safety_notes.length}
                <h4>Safety and cost notes</h4>
                <ul>{#each guide.safety_notes as n}<li>{n}</li>{/each}</ul>
              {/if}
            {/if}

            {#if lab.setup_requirements.length || lab.setup_steps.length}
              <h4>Set up the lab</h4>
              {#if lab.setup_requirements.length}
                <ul>{#each lab.setup_requirements as r}<li>{r}</li>{/each}</ul>
              {/if}
              {#if lab.setup_steps.length}
                <ol>{#each lab.setup_steps as s}<li>{s}</li>{/each}</ol>
              {/if}
            {/if}

            {#each labDiagrams(lab) as diagram, di}
              <figure class="diagram-fig">
                <figcaption>
                  <strong>{diagram.title || `Diagram ${di + 1}`}</strong>
                  {#if diagram.kind}<span class="muted small"> ({diagram.kind})</span>{/if}
                </figcaption>
                {#if diagram.mermaid}
                  {@const d = parseDiagram(diagram.mermaid)}
                  {#if d}
                    <svg class="diagram" viewBox="0 0 360 {d.nodes.length * 62 + 10}" role="img" aria-label={diagram.title || 'Lab diagram'}>
                      <defs>
                        <marker id="arr-{currentLab}-{di}" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto">
                          <path d="M0 0 L7 3 L0 6 z" class="arrow-head" />
                        </marker>
                      </defs>
                      {#each d.edges as [a, b]}
                        {@const ai = d.nodes.findIndex((n) => n.id === a)}
                        {@const bi = d.nodes.findIndex((n) => n.id === b)}
                        {#if ai >= 0 && bi > ai}
                          <line x1="180" y1={ai * 62 + 48} x2="180" y2={bi * 62 + 6} class="edge" marker-end="url(#arr-{currentLab}-{di})" />
                        {/if}
                      {/each}
                      {#each d.nodes as n, ni}
                        <rect x="55" y={ni * 62 + 6} width="250" height="42" rx="6" class="node-box" />
                        <text x="180" y={ni * 62 + 31} text-anchor="middle" class="node-text">{n.label.length > 38 ? n.label.slice(0, 37) + '…' : n.label}</text>
                      {/each}
                    </svg>
                  {:else}
                    <pre class="diagram-source">{diagram.mermaid}</pre>
                  {/if}
                {/if}
                {#if diagram.explanation}<p>{diagram.explanation}</p>{/if}
              </figure>
            {/each}

            <h4>Steps</h4>
            <ol class="steps">
              {#each lab.steps as step, j}
                <li class:done-step={done.has(`${currentLab}:${j}`)}>
                  <label class="step-head">
                    <input type="checkbox" checked={done.has(`${currentLab}:${j}`)} onchange={(e) => toggleStep(`${currentLab}:${j}`, (e.target as HTMLInputElement).checked)} />
                    <strong>Step {j + 1}: {step.title}</strong>
                  </label>
                  {#if step.instructions}<p>{step.instructions}</p>{/if}
                  {#if step.code}
                    <div class="code-wrap">
                      <pre class="code">{step.code}</pre>
                      <button class="ghost small-btn copy" onclick={() => copyCode(step.code!)}>Copy</button>
                    </div>
                  {/if}
                  {#if step.why}<p><span class="tag">Why</span> {step.why}</p>{/if}
                  {#if step.expected}<p><span class="tag">You should see</span> {step.expected}</p>{/if}
                  {#if step.tip}<p class="muted"><span class="tag">Tip</span> {step.tip}</p>{/if}
                </li>
              {/each}
            </ol>

            {#if lab.checks.length}
              <h4>Check yourself</h4>
              <ol class="checks">
                {#each lab.checks as c, j}
                  <li>
                    {c.question}
                    {#if revealed[`${currentLab}:${j}`]}
                      <p class="answer">{c.answer}</p>
                    {:else}
                      <button class="ghost small-btn" onclick={() => (revealed[`${currentLab}:${j}`] = true)}>Show answer</button>
                    {/if}
                  </li>
                {/each}
              </ol>
            {/if}

            {#if lab.troubleshooting.length}
              <h4>If it goes wrong</h4>
              <ul>{#each lab.troubleshooting as t}<li>{t}</li>{/each}</ul>
            {/if}
            {#if lab.extension}
              <h4>Go further</h4>
              <p>{lab.extension}</p>
            {/if}

            <div class="lab-nav">
              <button disabled={currentLab === 0} onclick={() => (currentLab--)}>← Previous lab</button>
              <button disabled={currentLab >= guide.labs.length - 1 && !guide.capstone} onclick={() => (currentLab++)}>
                {currentLab >= guide.labs.length - 1 ? 'Capstone →' : 'Next lab →'}
              </button>
            </div>
          {:else}
            <h3>Capstone</h3>
            <p class="pre-wrap">{guide.capstone}</p>
            <div class="lab-nav">
              <button onclick={() => (currentLab = guide.labs.length - 1)}>← Last lab</button>
              <span></span>
            </div>
          {/if}
        </article>
      </div>

      <p class="muted tiny">
        Generated from the book text by your chosen provider. Answers and commands can be wrong; check them
        against the book. Tick steps off as you go — ticks are kept on this device. Guides built before this
        version have less detail; rebuild to get the fuller labs.
      </p>
    {/if}
  </div>
</div>

<style>
  .lab-page { height: 100%; overflow-y: auto; container-type: inline-size; background: var(--surface); }
  .lab-inner { max-width: 1020px; margin: 0 auto; padding: 28px 36px 72px; }
  .lab-top { display: flex; justify-content: space-between; gap: 20px; align-items: flex-start; flex-wrap: wrap; }
  .lab-top h2 { margin: 0 0 6px; font-size: 24px; }
  .lab-top p { margin: 0; max-width: 56ch; }
  .toolbar { display: flex; gap: 6px; flex-wrap: wrap; }
  h3 { font-size: 20px; margin: 0 0 8px; }
  h4 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-2); margin: 26px 0 8px; }
  p { line-height: 1.55; }
  .small { font-size: 12px; }
  .tiny { font-size: 11px; margin-top: 28px; }
  .notice { border: 1px solid var(--accent); border-radius: var(--radius); padding: 10px 12px; margin: 16px 0; }
  .notice p { margin: 4px 0 0; }
  .error { color: var(--danger); }
  .build { max-width: 640px; margin-top: 24px; }
  .build-form { display: grid; gap: 10px; margin: 14px 0; }
  .build-form label { display: grid; gap: 4px; font-size: 13px; }
  .overview { font-size: 16px; line-height: 1.6; }
  .objective { font-size: 15px; }
  .meta { margin-top: 18px; }
  .progress { height: 6px; background: var(--surface-2); border-radius: 3px; overflow: hidden; margin: 6px 0 4px; }
  .progress div { height: 100%; background: var(--accent); transition: width 0.2s; }
  .lab-layout { display: grid; grid-template-columns: 250px minmax(0, 1fr); gap: 32px; margin-top: 20px; align-items: start; }
  .lab-index { position: sticky; top: 0; display: grid; gap: 6px; }
  .lab-index button { display: grid; gap: 2px; text-align: left; padding: 9px 10px; font: inherit; }
  .lab-index button.on { border-color: var(--accent); background: var(--surface-2); }
  .idx-title { font-weight: 600; font-size: 13px; }
  .lab-detail { min-width: 0; }
  ul, ol { margin: 0; padding-left: 20px; display: grid; gap: 5px; line-height: 1.5; }
  .pre-wrap { white-space: pre-wrap; }
  .diagram-fig { margin: 18px 0; }
  .diagram-fig figcaption { margin-bottom: 6px; }
  .diagram { width: 100%; max-width: 420px; display: block; margin: 6px auto; }
  .node-box { fill: var(--surface-2); stroke: var(--accent); }
  .node-text { fill: var(--text); font-size: 11px; }
  .edge { stroke: var(--text-2); stroke-width: 1.5; }
  .arrow-head { fill: var(--text-2); }
  .diagram-source { white-space: pre-wrap; font-size: 12px; background: var(--surface-2); border-radius: var(--radius); padding: 10px; overflow-x: auto; }
  .steps { list-style: none; padding-left: 0; gap: 0; }
  .steps li { padding: 14px 0; border-bottom: 1px solid var(--border); }
  .steps li:last-child { border-bottom: none; }
  .steps p { margin: 6px 0 0; }
  .done-step { opacity: 0.72; }
  .step-head { display: flex; gap: 9px; align-items: baseline; cursor: pointer; font-size: 14px; }
  .code-wrap { position: relative; margin-top: 8px; }
  .code { white-space: pre-wrap; overflow-x: auto; background: var(--surface-2); border-radius: var(--radius); padding: 10px 12px; font-size: 12.5px; line-height: 1.5; margin: 0; }
  .copy { position: absolute; top: 6px; right: 6px; }
  .tag { display: inline-block; font-size: 10px; text-transform: uppercase; letter-spacing: 0.05em; padding: 1px 5px; border-radius: 4px; background: var(--surface-2); }
  .checks .answer { margin-top: 4px; }
  .small-btn { min-height: 28px; padding: 3px 10px; font-size: 12px; }
  .lab-nav { display: flex; justify-content: space-between; gap: 10px; margin-top: 28px; }
  @container (max-width: 760px) {
    .lab-inner { padding: 18px 16px 56px; }
    .lab-layout { grid-template-columns: 1fr; gap: 18px; }
    .lab-index { position: static; display: flex; overflow-x: auto; padding-bottom: 4px; }
    .lab-index button { flex: 0 0 210px; }
    .lab-top h2 { font-size: 20px; }
  }
</style>
