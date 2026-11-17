import { describe, expect, it } from 'vitest'
import type { LabGuide, LabProposal } from './api'
import { attachmentLabel, buildProposalPreview, mermaidProblem, needsNoChangeNote, parseLabProposal, parseStepId, proposalDisplayText, stepId } from './labchat'

const guide: LabGuide = {
  overview: 'o',
  audience: '',
  prerequisites: [],
  tools_overview: '',
  capstone: '',
  safety_notes: [],
  labs: [
    {
      title: 'Net',
      objective: '',
      difficulty: '',
      estimated_minutes: 0,
      source_refs: [],
      concepts: [],
      setup_requirements: [],
      setup_steps: [],
      steps: [
        { title: 'Find IP', instructions: 'Run ipconfig.', code: 'ipconfig', why: '', expected: '', tip: '' },
        { title: 'Ping', instructions: 'Ping router.', code: '', why: '', expected: '', tip: '' },
      ],
      diagrams: [{ title: 'LAN', kind: 'topology', mermaid: 'graph LR\n R[Router] --> L[Laptop]', explanation: 'Ex' }],
      diagram: { title: '', kind: '', mermaid: '', explanation: '' },
      checks: [],
      troubleshooting: [],
      extension: '',
    },
  ],
}

const fenced = (p: unknown) => `Fixed it:\n\`\`\`lab-proposal\n${JSON.stringify(p)}\n\`\`\`\nApply if it looks right.`

describe('step ids', () => {
  it('round-trip and reject malformed ids', () => {
    expect(stepId(0, 1)).toBe('lab1-step2')
    expect(parseStepId('lab1-step2')).toEqual({ labIndex: 0, stepIndex: 1 })
    expect(parseStepId('lab0-step1')).toBeNull()
    expect(parseStepId('step-2')).toBeNull()
    expect(attachmentLabel(0, null)).toBe('Lab 1')
    expect(attachmentLabel(0, 1)).toBe('Lab 1 · Step 2')
  })
})

describe('parseLabProposal', () => {
  it('plain chat (clarify/troubleshoot) has no proposal', () => {
    expect(parseLabProposal('Your ping failed because the gateway differs.').kind).toBe('none')
  })

  it('extracts a fenced proposal', () => {
    const p: LabProposal = {
      changes: [{ kind: 'replace_step', lab_index: 0, step_id: 'lab1-step2', step: { title: 'Ping gateway', instructions: 'Ping it.', code: '', why: '', expected: '', tip: '' } }],
    }
    const parsed = parseLabProposal(fenced(p))
    expect(parsed.kind).toBe('proposal')
    if (parsed.kind === 'proposal') expect(parsed.proposal.changes).toHaveLength(1)
  })

  it('reports broken proposals as invalid, never as plain chat', () => {
    expect(parseLabProposal('```lab-proposal\n{nope}\n```').kind).toBe('invalid')
    expect(parseLabProposal('```lab-proposal\n{"changes":[]}').kind).toBe('invalid') // unclosed fence
    expect(parseLabProposal('```lab-proposal\n{"summary":"x"}\n```').kind).toBe('invalid') // no changes list
  })
})

describe('buildProposalPreview', () => {
  it('builds before/after for a valid step + diagram proposal', () => {
    const p: LabProposal = {
      summary: 'clearer',
      changes: [
        { kind: 'replace_step', lab_index: 0, step_id: 'lab1-step2', step: { title: 'Ping gateway', instructions: 'Ping it.', code: 'ping x', why: '', expected: '', tip: '' } },
        { kind: 'replace_diagram', lab_index: 0, diagram_index: 0, diagram: { title: 'Flow', kind: 'flowchart', mermaid: 'graph TD\n A[Open] --> B[Read]', explanation: 'Flow.' } },
      ],
    }
    const { entries, errors } = buildProposalPreview(guide, p)
    expect(errors).toEqual([])
    expect(entries.map((e) => e.target)).toEqual(['Lab 1 · Step 2', 'Lab 1 · Diagram 1'])
    expect(entries[0].before).toContain('Ping router.')
    expect(entries[0].after).toContain('Ping gateway')
  })

  it('refuses unknown step ids, bad Mermaid and empty proposals', () => {
    const cases: LabProposal[] = [
      { changes: [] },
      { changes: [{ kind: 'replace_step', lab_index: 0, step_id: 'lab1-step9', step: { title: 'X', instructions: 'Y', why: '', expected: '', tip: '' } }] },
      { changes: [{ kind: 'replace_step', lab_index: 1, step_id: 'lab1-step1', step: { title: 'X', instructions: 'Y', why: '', expected: '', tip: '' } }] },
      { changes: [{ kind: 'replace_diagram', lab_index: 0, diagram_index: 0, diagram: { title: 'D', kind: '', mermaid: 'A --> B[', explanation: '' } }] },
    ]
    for (const p of cases) expect(buildProposalPreview(guide, p).errors.length).toBeGreaterThan(0)
    expect(mermaidProblem('graph TD\n A[X] --> B[Y]')).toBeNull()
    expect(mermaidProblem('graph TD\n A[X]')).toContain('no edges')
  })
})

describe('needsNoChangeNote', () => {
  it('flags a reply that claims an edit without a proposal (observed live with a small local model)', () => {
    const reply = 'The lab step has been changed to use a 25-minute timer instead of 20. Here is the updated step: ...'
    expect(needsNoChangeNote('What does step 1 do?', reply)).toBe(true)
  })
  it('flags an explicit edit request that got no proposal back', () => {
    expect(needsNoChangeNote('Please change step 1 to a 25 minute timer.', 'Sure, a 25 minute timer works well.')).toBe(true)
  })
  it('stays quiet for plain clarifying turns', () => {
    expect(needsNoChangeNote('Why does step 2 matter?', 'It counts each paragraph as a rep.')).toBe(false)
  })
  it('stays quiet when a proposal block is present (the preview handles it)', () => {
    const reply = 'Here is a fix:\n```lab-proposal\n{"summary":"x","changes":[]}\n```'
    expect(needsNoChangeNote('Please change step 1.', reply)).toBe(false)
  })
})

describe('proposalDisplayText', () => {
  const block = '```lab-proposal\n{"summary":"x","changes":[{"kind":"replace_step","lab_index":0,"step_id":"lab1-step1","step":{"title":"T"}}]}\n```'
  it('drops a valid proposal block, keeping the explanation around it', () => {
    expect(proposalDisplayText(`Here is the change.\n${block}\nApply it if it looks right.`)).toBe('Here is the change.\n\nApply it if it looks right.')
    expect(proposalDisplayText(block)).toBe('')
  })
  it('leaves invalid or absent proposals visible', () => {
    expect(proposalDisplayText('```lab-proposal\n{not json}\n```')).toBe('```lab-proposal\n{not json}\n```')
    expect(proposalDisplayText('Just an answer.')).toBe('Just an answer.')
  })
})
