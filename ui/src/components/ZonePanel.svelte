<script lang="ts">
  // The learning assistant: maps the reader's Zone of Proximal Development
  // for the passage on screen. Hints are revealed one at a time so support
  // fades in only as far as it's needed.
  import { onMount } from 'svelte'
  import {
    api,
    errorMessage,
    type Locator,
    type TutorStatus,
    type ZoneFeedback,
    type ZoneMap,
    type ZoneProfile,
  } from '../lib/api'
  import { toasts } from '../lib/toast.svelte'
  import { timeAgo } from '../lib/util'
  import { zoneFeedbackLine, zoneLoading } from '../lib/chad'

  let {
    bookId,
    chad,
    context,
    onJump,
  }: {
    bookId: number
    chad: boolean
    /** Where the reader is right now, or null before the book has loaded. */
    context: () => { locator: Locator; label: string; passage: string | null } | null
    onJump: (loc: Locator) => void
  } = $props()

  let status = $state<TutorStatus | null>(null)
  let profile = $state<ZoneProfile | null>(null)
  let maps = $state<ZoneMap[]>([])
  let currentId = $state<string | null>(null)
  let background = $state('')
  let goal = $state('')
  let busy = $state(false)
  let busyLine = $state('')
  let error = $state<string | null>(null)
  let editingProfile = $state(false)

  // What the reader has revealed in the map on screen.
  let hintsShown = $state<Record<number, number>>({})
  let explained = $state<Record<number, boolean>>({})
  let answers = $state<Record<string, boolean>>({})

  const current = $derived(maps.find((m) => m.id === currentId) ?? maps[0] ?? null)
  const profileDirty = $derived(!!profile && (background.trim() !== profile.background || goal.trim() !== profile.goal))

  const STRETCH_LABELS = ['', 'Lots of support', 'More support', 'Balanced', 'More challenge', 'Maximum challenge']

  onMount(async () => {
    try {
      const [s, o] = await Promise.all([api.tutorStatus(), api.zoneOverview(bookId)])
      status = s
      profile = o.profile
      maps = o.maps
      background = o.profile.background
      goal = o.profile.goal
      editingProfile = !o.profile.background && !o.profile.goal
    } catch (e) {
      error = errorMessage(e)
    }
  })

  function show(id: string) {
    currentId = id
    hintsShown = {}
    explained = {}
    answers = {}
  }

  async function saveProfile() {
    try {
      profile = await api.zoneSetProfile(bookId, background, goal)
      background = profile.background
      goal = profile.goal
      editingProfile = false
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function generate() {
    const ctx = context()
    if (!ctx) return
    if (profileDirty) await saveProfile()
    busy = true
    busyLine = zoneLoading(chad)
    error = null
    try {
      const m = await api.zoneGenerate(bookId, ctx.locator, ctx.label, ctx.passage)
      maps = [m, ...maps]
      show(m.id)
    } catch (e) {
      error = errorMessage(e)
    } finally {
      busy = false
    }
  }

  async function rate(feedback: ZoneFeedback) {
    if (!current) return
    const m = current
    try {
      profile = await api.zoneFeedback(m.id, feedback)
      maps = maps.map((x) => (x.id === m.id ? { ...x, feedback } : x))
      toasts.push('info', zoneFeedbackLine(chad, feedback))
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }

  async function remove(m: ZoneMap) {
    try {
      await api.zoneDelete(m.id)
      maps = maps.filter((x) => x.id !== m.id)
      if (currentId === m.id) show(maps[0]?.id ?? '')
    } catch (e) {
      toasts.push('error', errorMessage(e))
    }
  }
</script>

<div class="zone-panel">
  <h3>Your learning zone</h3>
  <p class="muted small intro">
    What you can do alone, what you can reach with a little help, and what can wait. Based on Vygotsky's
    <em>Zone of Proximal Development</em>.
  </p>

  {#if status && !status.ready}
    <div class="notice">
      <strong>Add an API key to start.</strong>
      <p>Open <em>Settings → Learning assistant</em> and paste a key from Anthropic or another LLM provider.</p>
    </div>
  {/if}

  {#if profile}
    <section class="profile">
      {#if editingProfile}
        <label>What you already know about this subject
          <textarea rows="2" bind:value={background} placeholder="e.g. School biology, nothing since"></textarea>
        </label>
        <label>Why you're reading this book
          <textarea rows="2" bind:value={goal} placeholder="e.g. To follow the science in the news"></textarea>
        </label>
        <div class="row">
          <button onclick={saveProfile} disabled={!profileDirty}>Save</button>
          {#if profile.background || profile.goal}<button class="ghost" onclick={() => { background = profile!.background; goal = profile!.goal; editingProfile = false }}>Cancel</button>{/if}
        </div>
      {:else}
        <div class="row between">
          <span class="muted small">{profile.background || profile.goal ? 'Your background and goal are saved for this book.' : 'Tell the assistant about you for better maps.'}</span>
          <button class="ghost small-btn" onclick={() => (editingProfile = true)}>Edit</button>
        </div>
      {/if}
      <div class="stretch" title="Moves with your ratings">
        <span class="muted small">Stretch</span>
        <span class="dots" aria-label="Stretch {profile.stretch} of 5">
          {#each [1, 2, 3, 4, 5] as i}<span class:on={i <= profile.stretch}></span>{/each}
        </span>
        <span class="small">{STRETCH_LABELS[profile.stretch]}</span>
      </div>
    </section>
  {/if}

  <button class="primary map-btn" disabled={busy || !status?.ready} onclick={generate}>
    {busy ? busyLine : 'Map my zone for this page'}
  </button>
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if current}
    {@const g = current.guide}
    <div class="meta muted small">
      <button class="ghost link" onclick={() => onJump(current.locator)} title="Go to this spot">{current.label}</button>
      · {timeAgo(current.created_at)}
    </div>

    <div class="rings" aria-hidden="true">
      <svg viewBox="0 0 120 120" width="76" height="76">
        <circle cx="60" cy="60" r="56" class="r-later" />
        <circle cx="60" cy="60" r="38" class="r-zone" />
        <circle cx="60" cy="60" r="18" class="r-alone" />
      </svg>
      <ul class="legend small">
        <li><span class="key k-alone"></span>{g.can_do_alone.length} you've got</li>
        <li><span class="key k-zone"></span>{g.zone.length} in your zone</li>
        <li><span class="key k-later"></span>{g.not_yet.length} for later</li>
      </ul>
    </div>

    {#if g.summary}<p class="summary">{g.summary}</p>{/if}

    {#if g.can_do_alone.length}
      <section class="ring alone">
        <h4>You've got this</h4>
        <ul>
          {#each g.can_do_alone as k}
            <li><strong>{k.idea}</strong>{#if k.because}<span class="muted"> — {k.because}</span>{/if}</li>
          {/each}
        </ul>
      </section>
    {/if}

    <section class="ring zone">
      <h4>Your zone: reachable with help</h4>
      {#each g.zone as z, i (current.id + i)}
        {@const shown = hintsShown[i] ?? 0}
        <article class="item">
          <strong class="concept">{z.concept}</strong>
          {#if z.why_it_matters}<p>{z.why_it_matters}</p>{/if}
          {#if z.try_it}
            <p class="try"><span class="tag">Try it</span> {z.try_it}</p>
          {/if}
          {#each z.hints.slice(0, shown) as h, j}
            <p class="hint"><span class="tag">Hint {j + 1}</span> {h}</p>
          {/each}
          {#if explained[i]}<p class="explain">{z.explanation}</p>{/if}
          {#if answers[`z${i}`]}<p class="answer"><span class="tag">Answer</span> {z.answer}</p>{/if}
          <div class="row wrap">
            {#if shown < z.hints.length}
              <button class="small-btn" onclick={() => (hintsShown[i] = shown + 1)}>{shown === 0 ? 'Give me a hint' : 'Another hint'}</button>
            {/if}
            {#if !explained[i] && z.explanation && shown >= z.hints.length}
              <button class="small-btn" onclick={() => (explained[i] = true)}>Explain it fully</button>
            {/if}
            {#if z.answer && !answers[`z${i}`]}
              <button class="ghost small-btn" onclick={() => (answers[`z${i}`] = true)}>Show answer</button>
            {/if}
          </div>
        </article>
      {/each}
    </section>

    {#if g.not_yet.length}
      <section class="ring later">
        <h4>Not yet</h4>
        <ul>
          {#each g.not_yet as n}
            <li><strong>{n.idea}</strong>{#if n.come_back_when}<span class="muted"> — {n.come_back_when}</span>{/if}</li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if g.check.length}
      <section class="ring">
        <h4>Check yourself</h4>
        <ol class="checks">
          {#each g.check as c, i}
            <li>
              {c.question}
              {#if answers[`c${i}`]}
                <p class="answer">{c.answer}</p>
              {:else}
                <button class="ghost small-btn" onclick={() => (answers[`c${i}`] = true)}>Show answer</button>
              {/if}
            </li>
          {/each}
        </ol>
      </section>
    {/if}

    {#if g.next_step}<p class="next"><span class="tag">Next</span> {g.next_step}</p>{/if}

    <section class="feedback">
      <span class="small">How did this map feel?</span>
      <div class="seg">
        {#each [['too_easy', 'Too easy'], ['just_right', 'Just right'], ['too_hard', 'Too hard']] as [f, l]}
          <button class="small-btn" class:primary={current.feedback === f} onclick={() => rate(f as ZoneFeedback)}>{l}</button>
        {/each}
      </div>
      <span class="muted small">Your rating sets how far the next map stretches you.</span>
    </section>

    <div class="row between foot">
      <span class="muted small" title="Model">{current.model}</span>
      <button class="ghost small-btn" onclick={() => remove(current)}>Delete map</button>
    </div>
  {:else if status?.ready && !busy}
    <p class="muted small">No maps for this book yet. Turn to a page you want to understand, then map it.</p>
  {/if}

  {#if maps.length > 1}
    <h4 class="earlier-h">Earlier maps</h4>
    <ul class="earlier">
      {#each maps as m (m.id)}
        <li>
          <button class="ghost" class:on={current?.id === m.id} onclick={() => show(m.id)}>
            <span>{m.label}</span>
            <span class="muted small">{m.guide.zone.map((z) => z.concept).join(' · ')}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <p class="muted tiny">The passage on screen and what you write above are sent to your chosen provider. Answers can be wrong; check them against the book.</p>
</div>

<style>
  .zone-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
    margin: 14px 0 0;
  }
  h4 {
    margin: 0 0 6px;
    font-size: 13px;
  }
  p {
    margin: 0 0 6px;
  }
  .intro {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .tiny {
    font-size: 11px;
    margin-top: 10px;
  }
  .notice {
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 8px 10px;
  }
  .notice p {
    margin: 4px 0 0;
  }
  .profile {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }
  textarea {
    width: 100%;
    resize: vertical;
    font: inherit;
  }
  .row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .wrap {
    flex-wrap: wrap;
  }
  .between {
    justify-content: space-between;
  }
  .small-btn {
    min-height: 28px;
    padding: 3px 10px;
    font-size: 12px;
  }
  .stretch {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dots span {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 1px solid var(--accent);
  }
  .dots span.on {
    background: var(--accent);
  }
  .map-btn {
    width: 100%;
    white-space: normal;
  }
  .error {
    color: var(--danger);
    font-size: 13px;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .link {
    padding: 0 2px;
    min-height: 0;
    text-decoration: underline;
    font-size: 12px;
  }
  .rings {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .rings circle {
    stroke-width: 2;
  }
  .r-later {
    fill: transparent;
    stroke: var(--text-2);
    stroke-dasharray: 4 4;
  }
  .r-zone {
    fill: color-mix(in srgb, var(--accent) 28%, transparent);
    stroke: var(--accent);
  }
  .r-alone {
    fill: color-mix(in srgb, var(--ok) 55%, transparent);
    stroke: var(--ok);
  }
  .legend {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 3px;
  }
  .key {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    margin-right: 6px;
    vertical-align: -1px;
  }
  .k-alone {
    background: var(--ok);
  }
  .k-zone {
    background: var(--accent);
  }
  .k-later {
    border: 1px dashed var(--text-2);
  }
  .summary {
    font-style: italic;
  }
  .ring {
    border-left: 3px solid var(--border);
    padding: 2px 0 2px 10px;
  }
  .ring.alone {
    border-color: var(--ok);
  }
  .ring.zone {
    border-color: var(--accent);
  }
  .ring.later {
    border-left-style: dashed;
    border-color: var(--text-2);
  }
  .ring ul,
  .checks {
    margin: 0;
    padding-left: 18px;
    display: grid;
    gap: 4px;
  }
  .item {
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
  }
  .item:last-child {
    border-bottom: none;
  }
  .concept {
    display: block;
    margin-bottom: 4px;
  }
  .tag {
    display: inline-block;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text-2);
    margin-right: 4px;
  }
  .hint {
    padding-left: 6px;
  }
  .explain {
    background: var(--surface-2);
    border-radius: 6px;
    padding: 6px 8px;
  }
  .answer {
    color: var(--text-2);
    margin: 4px 0;
  }
  .next {
    margin-top: 4px;
  }
  .feedback {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }
  .seg {
    display: flex;
    gap: 4px;
  }
  .seg button {
    flex: 1;
  }
  .foot {
    margin-top: 2px;
  }
  .earlier-h {
    margin-top: 12px;
  }
  .earlier {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .earlier button {
    width: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    white-space: normal;
  }
  .earlier button.on {
    background: var(--surface-2);
  }
</style>
