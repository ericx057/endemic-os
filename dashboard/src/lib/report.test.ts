import { describe, expect, it } from 'vitest'
import {
  burstSeries,
  errorRows,
  hotspotRows,
  latencyRows,
  parseReport,
  stageTableRows,
  traceRows,
} from './report'
import sample from '../../fixtures/sample-run.json'

const latency = (p95: number | null, samples = 10) => ({ samples, p95_ms: p95, p99_ms: p95 })

const stage = (overrides: Record<string, unknown> = {}) => ({
  agents: 25,
  held: true,
  peak_active: 25,
  started: 200,
  completed: 180,
  runner_errors: 2,
  target_errors: 8,
  inference_errors: 4,
  inference_throttles: 6,
  tokens: 1500,
  input_tokens: 1000,
  output_tokens: 500,
  traffic: {
    tool_calls: 9,
    unbucketed_tool_calls: 0,
    window_ms: 100,
    peak_calls_per_window: 5,
    p95_calls_per_occupied_window: 5,
    session_samples: [
      {
        trace_id: 'abc123',
        dispatches: [
          { at_us: 0, tools: ['search'] },
          { at_us: 1_500_000, tools: ['search', 'refund'] },
        ],
      },
    ],
    calls_per_window: [3, 0, 5, 1],
  },
  hotspots: [
    { tool: 'refund', calls: 1, share: 0.25, target_errors: 1, p95_ms: 40, p99_ms: 41 },
    { tool: 'search', calls: 3, share: 0.75, target_errors: 0, p95_ms: 5, p99_ms: 5 },
  ],
  runner: latency(12),
  target: latency(80),
  inference: latency(900),
  hold_seconds: 60,
  passed: true,
  interrupted: 0,
  ...overrides,
})

const report = (stages: unknown[]) => ({
  id: 3,
  status: 'reached_ceiling',
  highest_passing_agents: 25,
  stages,
})

describe('parseReport', () => {
  it('accepts a full report', () => {
    const parsed = parseReport(report([stage()]))
    expect(parsed.id).toBe(3)
    expect(parsed.status).toBe('reached_ceiling')
    expect(parsed.stages[0].traffic.calls_per_window).toEqual([3, 0, 5, 1])
  })

  it('fills fields that older reports lack', () => {
    const old = stage({
      hotspots: undefined,
      traffic: {
        tool_calls: 2,
        unbucketed_tool_calls: 0,
        window_ms: 100,
        peak_calls_per_window: 2,
        p95_calls_per_occupied_window: null,
        session_samples: [{ dispatches: [{ at_us: 10, tools: ['a'] }] }],
      },
    })
    const parsed = parseReport(report([old])).stages[0]
    expect(parsed.hotspots).toEqual([])
    expect(parsed.traffic.calls_per_window).toEqual([])
    expect(parsed.traffic.session_samples[0].trace_id).toBe('')
  })

  it('accepts a report with no traffic at all', () => {
    const parsed = parseReport(report([stage({ traffic: undefined })])).stages[0]
    expect(parsed.traffic.session_samples).toEqual([])
    expect(parsed.traffic.p95_calls_per_occupied_window).toBeNull()
  })

  it.each([
    [null, 'not a JSON object'],
    [[], 'not a JSON object'],
    [{ status: 'running', stages: [] }, 'id'],
    [{ id: 1, status: 'exploded', stages: [] }, 'status'],
    [{ id: 1, status: 'running' }, 'stages'],
    [report([{ agents: 'many' }]), 'stages[0].agents'],
    [report([stage({ target: { samples: 1 } })]), 'stages[0].target.p95_ms'],
  ])('rejects %j with a message naming %s', (value, field) => {
    expect(() => parseReport(value)).toThrow(field)
  })

  it('reads the plan Endemic chose, and null for reports without one', () => {
    const plan = {
      target_kind: 'mcp',
      tools: ['search', 'refund'],
      parallel_tools: ['search'],
      tasks: ['Find an order.'],
      hold_agents: null,
      pacing: { start_spread_ms: 0, think_min_ms: 0, think_max_ms: 0, sync_sessions: false },
      agents_per_stage: [25, 50],
    }
    expect(parseReport({ ...report([stage()]), plan }).plan).toEqual(plan)
    expect(parseReport({ ...report([stage()]), plan: { ...plan, hold_agents: 200 } }).plan?.hold_agents).toBe(200)
    expect(parseReport(report([stage()])).plan).toBeNull()
    expect(() => parseReport({ ...report([stage()]), plan: { ...plan, target_kind: 'ftp' } })).toThrow('plan.target_kind')
    expect(() => parseReport({ ...report([stage()]), plan: { ...plan, tasks: 'one' } })).toThrow('plan.tasks')
  })

  it('reads a run that failed before any stage, with its error and partial plan', () => {
    const plan = { target_kind: 'http', tools: [], parallel_tools: [], tasks: [], agents_per_stage: [] }
    const failed = { id: 4, status: 'failed', highest_passing_agents: null, plan, error: 'host resolves to a non-public address', stages: [] }
    const parsed = parseReport(failed)
    expect(parsed.status).toBe('failed')
    expect(parsed.error).toBe('host resolves to a non-public address')
    expect(parseReport(report([stage()])).error).toBeNull()
    expect(() => parseReport({ ...failed, error: 7 })).toThrow('error')
  })

  it('parses the bundled fixture', () => {
    const parsed = parseReport(sample)
    expect(parsed.stages.map((s) => s.agents)).toEqual([25, 50, 100, 200, 400])
  })
})

describe('chart rows', () => {
  it('latency rows hold p95 per source, null when unmeasured', () => {
    const parsed = parseReport(report([stage(), stage({ agents: 50, inference: latency(null, 0) })]))
    expect(latencyRows(parsed.stages)).toEqual([
      { agents: 25, target: 80, inference: 900, runner: 12 },
      { agents: 50, target: 80, inference: null, runner: 12 },
    ])
  })

  it('error rows are percentages of started sessions by source', () => {
    const parsed = parseReport(report([stage({ interrupted: 10 }), stage({ started: 0 })]))
    expect(errorRows(parsed.stages)).toEqual([
      { agents: 25, target: 4, inference: 2, throttled: 3, runner: 1, interrupted: 5 },
      { agents: 25, target: 0, inference: 0, throttled: 0, runner: 0, interrupted: 0 },
    ])
  })

  it('hotspot rows rank by calls with share and error rate in percent', () => {
    const rows = hotspotRows(parseReport(report([stage()])).stages[0])
    expect(rows).toEqual([
      { tool: 'search', calls: 3, share: 75, errorRate: 0, p95: 5, p99: 5 },
      { tool: 'refund', calls: 1, share: 25, errorRate: 100, p95: 40, p99: 41 },
    ])
  })

  it('burst series converts windows to seconds and keeps the p95 line', () => {
    const series = burstSeries(parseReport(report([stage()])).stages[0])
    expect(series.p95).toBe(5)
    expect(series.windowMs).toBe(100)
    expect(series.points).toEqual([
      { seconds: 0, calls: 3 },
      { seconds: 0.1, calls: 0 },
      { seconds: 0.2, calls: 5 },
      { seconds: 0.3, calls: 1 },
    ])
  })

  it('trace rows give each sampled session dispatches in seconds', () => {
    const rows = traceRows(parseReport(report([stage()])).stages[0])
    expect(rows).toEqual([
      {
        session: 1,
        traceId: 'abc123',
        dispatches: [
          { seconds: 0, count: 1, tools: ['search'] },
          { seconds: 1.5, count: 2, tools: ['search', 'refund'] },
        ],
      },
    ])
  })

  it('stage table rows summarize every stage', () => {
    const [row] = stageTableRows(parseReport(report([stage({ interrupted: 10 })])).stages)
    expect(row).toEqual({
      agents: 25,
      passed: true,
      held: true,
      started: 200,
      completed: 180,
      errorShare: 15,
      throttled: 3,
      targetP95: 80,
      inferenceP95: 900,
      runnerP95: 12,
      runnerP99: 12,
      toolCalls: 9,
      peakBurst: 5,
    })
  })
})
