import { describe, expect, it } from 'vitest'
import { explanationLines } from './explanation'
import { parseReport } from './report'

const explanation = {
  agents: 8,
  criterion: 'error_budget',
  baseline_agents: 4,
  error_rate: { tool: 'search', from: 0.1, to: 0.5, began_at_agents: 4 },
  p95_ms: { tool: 'refund', from: 40, to: 200, began_at_agents: 8 },
  sample_trace_ids: ['trace-search', 'trace-refund'],
}

const report = { id: 3, status: 'failed_stage', highest_passing_agents: 4, explanation, stages: [] }

describe('failure explanation', () => {
  it('parses the explanation and reads it as sentences', () => {
    const parsed = parseReport(report).explanation
    expect(parsed).toEqual(explanation)
    expect(explanationLines(parsed!)).toEqual({
      headline: 'Failed at 8 agents: errors and interruptions went over the error budget.',
      baseline: 'Compared with the last passing stage at 4 agents.',
      rises: ['search error rate rose from 10% to 50%, starting at 4 agents.', 'refund p95 rose from 40 ms to 200 ms, starting at 8 agents.'],
      traces: ['trace-search', 'trace-refund'],
    })
  })

  it('reads a first-stage failure without a baseline or rises', () => {
    const lines = explanationLines({ ...explanation, criterion: 'sessions_completed', baseline_agents: null, error_rate: null, p95_ms: null, agents: 2 } as const)
    expect(lines.headline).toBe('Failed at 2 agents: no session completed.')
    expect(lines.baseline).toBe('No stage passed, so tools are ranked by their values in this stage.')
    expect(lines.rises).toEqual(['No tool error rate or p95 rose.'])
  })

  it('treats a missing explanation as none and rejects an unknown criterion', () => {
    expect(parseReport({ ...report, explanation: undefined }).explanation).toBeNull()
    expect(() => parseReport({ ...report, explanation: { ...explanation, criterion: 'vibes' } })).toThrow('explanation.criterion')
  })
})
