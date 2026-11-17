// Lab co-pilot (BC-5) pure logic: find a structured edit proposal in an
// assistant reply and build the before/after preview. The Rust side
// (bc-tutor `validate_proposal`/`apply_proposal`) is authoritative at
// Apply time; these helpers only drive the preview, so an invalid
// proposal shows its problems here and is still refused there.

import { labDiagrams, type LabDiagram, type LabGuide, type LabProposal, type LabStep } from './api'

/** Positional step id, matching bc-tutor `step_id` (1-based in the id). */
export function stepId(labIndex: number, stepIndex: number): string {
  return `lab${labIndex + 1}-step${stepIndex + 1}`
}

export function parseStepId(id: string): { labIndex: number; stepIndex: number } | null {
  const m = /^lab(\d+)-step(\d+)$/.exec(id.trim())
  if (!m) return null
  const labIndex = Number(m[1]) - 1
  const stepIndex = Number(m[2]) - 1
  return labIndex >= 0 && stepIndex >= 0 ? { labIndex, stepIndex } : null
}

export type ProposalParse =
  | { kind: 'none' }
  | { kind: 'proposal'; proposal: LabProposal }
  | { kind: 'invalid'; error: string }

/** Extract a ```lab-proposal block (or a bare JSON object with "changes"). */
export function parseLabProposal(text: string): ProposalParse {
  let json: string | null = null
  const fence = text.indexOf('```lab-proposal')
  if (fence >= 0) {
    const after = text.slice(fence + '```lab-proposal'.length).replace(/^\n/, '')
    const end = after.indexOf('```')
    if (end < 0) return { kind: 'invalid', error: 'The proposed edit is incomplete (its code block never closes), so it cannot be applied.' }
    json = after.slice(0, end).trim()
  } else if (text.includes('"changes"')) {
    const a = text.indexOf('{')
    const b = text.lastIndexOf('}')
    if (a >= 0 && b > a) json = text.slice(a, b + 1)
  }
  if (json == null) return { kind: 'none' }
  try {
    const value = JSON.parse(json) as LabProposal
    if (!value || !Array.isArray(value.changes)) {
      return { kind: 'invalid', error: 'The proposed edit has no list of changes, so it cannot be applied.' }
    }
    return { kind: 'proposal', proposal: value }
  } catch (e) {
    return { kind: 'invalid', error: `The proposed edit could not be read, so it cannot be applied (${e instanceof Error ? e.message : String(e)}).` }
  }
}

/**
 * The reply text to show above a parsed proposal: the fenced
 * lab-proposal block is dropped because the before/after preview
 * already presents it. Anything else is returned unchanged.
 */
export function proposalDisplayText(text: string): string {
  const start = text.indexOf('```lab-proposal')
  if (start < 0 || parseLabProposal(text).kind !== 'proposal') return text
  const close = text.indexOf('```', start + '```lab-proposal'.length)
  return (text.slice(0, start) + text.slice(close + 3)).trim()
}

const EDIT_REQUEST =/\b(change|fix|rewrite|update|edit|modify|replace|correct|adjust)\b/i
const EDIT_CLAIM =
  /\b(has been|have been|was|were|is now|are now|I've|I have|I)\s+(now\s+)?(changed|updated|modified|edited|applied|rewritten|replaced|fixed|corrected|adjusted)\b/i

/**
 * True when a reply carries no proposal but the reader asked for a change
 * or the model claims it made one. Only a lab-proposal block can change
 * the saved lab, so the panel says plainly that nothing changed.
 */
export function needsNoChangeNote(userText: string | null, replyText: string): boolean {
  if (parseLabProposal(replyText).kind !== 'none') return false
  return EDIT_CLAIM.test(replyText) || (userText != null && EDIT_REQUEST.test(userText))
}

/** Mirrors bc-tutor `validate_mermaid` for preview feedback. */
export function mermaidProblem(src: string): string | null {
  const t = (src ?? '').trim()
  if (!t) return 'diagram Mermaid source is empty'
  if (t.includes('```')) return 'diagram Mermaid source must not contain code fences'
  const first = t.split('\n').map((l) => l.trim()).find((l) => l) ?? ''
  if (!/^(graph|flowchart)\s/i.test(first)) return `diagram must start with 'graph TD'/'graph LR' or 'flowchart …', not "${first}"`
  for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}']] as const) {
    if (t.split(open).length !== t.split(close).length) return `diagram has unbalanced '${open}${close}' brackets`
  }
  if (!t.includes('-->') && !t.includes('---') && !t.includes('==>')) return 'diagram has no edges (expected lines like A[Label] --> B[Label])'
  return null
}

export interface ProposalPreviewEntry {
  /** Heading such as "Lab 1 · Step 2" or "Lab 1 · Diagram 1". */
  target: string
  before: string
  after: string
}

function stepText(s: LabStep): string {
  return [s.title, s.instructions, s.code, s.why, s.expected, s.tip].filter((x) => x && x.trim()).join('\n')
}

function diagramText(d: LabDiagram): string {
  return [d.title, d.mermaid, d.explanation].filter((x) => x && x.trim()).join('\n')
}

/**
 * Validate a proposal against the saved guide and produce one
 * before/after entry per affected step/diagram. Any problem lands in
 * `errors` and the caller must refuse Apply (Rust refuses again anyway).
 */
export function buildProposalPreview(
  guide: LabGuide,
  proposal: LabProposal,
): { entries: ProposalPreviewEntry[]; errors: string[] } {
  const entries: ProposalPreviewEntry[] = []
  const errors: string[] = []
  if (!proposal.changes.length) errors.push('the proposal has no changes')
  const seen = new Set<string>()
  proposal.changes.forEach((change, i) => {
    const n = i + 1
    if (change.kind === 'replace_step') {
      const parsed = parseStepId(change.step_id ?? '')
      if (!parsed) {
        errors.push(`change ${n}: unknown step id "${change.step_id}" (expected e.g. lab1-step2)`)
        return
      }
      if (parsed.labIndex !== change.lab_index) {
        errors.push(`change ${n}: step_id "${change.step_id}" belongs to lab ${parsed.labIndex + 1} but lab_index is ${change.lab_index}`)
        return
      }
      const lab = guide.labs[parsed.labIndex]
      const before = lab?.steps[parsed.stepIndex]
      if (!before) {
        errors.push(`change ${n}: unknown step id "${change.step_id}"`)
        return
      }
      if (seen.has(change.step_id)) errors.push(`change ${n}: step "${change.step_id}" is changed twice`)
      seen.add(change.step_id)
      if (!change.step || (!(change.step.title ?? '').trim() && !(change.step.instructions ?? '').trim())) {
        errors.push(`change ${n}: replacement step needs a title or instructions`)
        return
      }
      entries.push({
        target: `Lab ${parsed.labIndex + 1} · Step ${parsed.stepIndex + 1}`,
        before: stepText(before),
        after: stepText(change.step),
      })
    } else if (change.kind === 'replace_diagram') {
      const lab = guide.labs[change.lab_index]
      const before = lab ? labDiagrams(lab)[change.diagram_index] : undefined
      if (!before) {
        errors.push(`change ${n}: lab ${(change.lab_index ?? 0) + 1} has no diagram ${change.diagram_index}`)
        return
      }
      const key = `diagram:${change.lab_index}:${change.diagram_index}`
      if (seen.has(key)) errors.push(`change ${n}: that diagram is changed twice`)
      seen.add(key)
      const problem = change.diagram ? mermaidProblem(change.diagram.mermaid ?? '') : 'replacement diagram is missing'
      if (problem) {
        errors.push(`change ${n}: ${problem}`)
        return
      }
      entries.push({
        target: `Lab ${change.lab_index + 1} · Diagram ${change.diagram_index + 1}`,
        before: diagramText(before),
        after: diagramText(change.diagram),
      })
    } else {
      errors.push(`change ${n}: unknown kind "${(change as { kind?: string }).kind}"`)
    }
  })
  return { entries, errors }
}

/** Attachment label shown on a lab-chat turn. */
export function attachmentLabel(labIndex: number, stepIndex: number | null): string {
  return stepIndex == null ? `Lab ${labIndex + 1}` : `Lab ${labIndex + 1} · Step ${stepIndex + 1}`
}
