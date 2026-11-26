// Types and pure transforms for the runner's GET /runs/{id} JSON (src/api.rs,
// src/runner/metrics.rs, hotspots.rs, traffic.rs). Older reports may lack
// hotspots, calls_per_window and trace_id.
import type { Pacing } from './runRequest'
import { bool, describeErrors, fail, isObject, list, num, object, optNum, requiredList, str } from './json'

export const RUN_STATUSES = ['running', 'cancelled', 'failed', 'failed_stage', 'reached_ceiling', 'completed'] as const
export type RunStatus = (typeof RUN_STATUSES)[number]

export type Latencies = { samples: number; p95_ms: number | null; p99_ms: number | null }
export type Hotspot = {
  tool: string
  calls: number
  share: number
  target_errors: number
  p95_ms: number | null
  p99_ms: number | null
}
export type Dispatch = { at_us: number; tools: string[] }
export type SessionSample = { trace_id: string; dispatches: Dispatch[] }
export type TrafficReport = {
  tool_calls: number
  unbucketed_tool_calls: number
  window_ms: number
  peak_calls_per_window: number
  p95_calls_per_occupied_window: number | null
  session_samples: SessionSample[]
  calls_per_window: number[]
}
export type StageReport = {
  agents: number
  held: boolean
  peak_active: number
  started: number
  completed: number
  runner_errors: number
  target_errors: number
  inference_errors: number
  inference_throttles: number
  tokens: number
  input_tokens: number
  output_tokens: number
  traffic: TrafficReport
  hotspots: Hotspot[]
  runner: Latencies
  target: Latencies
  inference: Latencies
  hold_seconds: number
  passed: boolean
  interrupted: number
}
/** What Endemic decided for the run; absent in reports from older runners. */
export type RunPlan = {
  target_kind: 'mcp' | 'http'
  tools: string[]
  parallel_tools: string[]
  tasks: string[]
  hold_agents: number | null
  pacing: Pacing | null
  agents_per_stage: number[]
}
export type RunReport = {
  id: number
  status: RunStatus
  highest_passing_agents: number | null
  plan: RunPlan | null
  /** Why a `failed` run stopped before any stage; null otherwise and in older reports. */
  error: string | null
  stages: StageReport[]
}

const latencies = (value: unknown, path: string): Latencies => {
  const o = object(value, path)
  if (!('p95_ms' in o)) fail(`${path}.p95_ms`, 'a number or null')
  return { samples: num(o.samples, `${path}.samples`), p95_ms: optNum(o.p95_ms, `${path}.p95_ms`), p99_ms: optNum(o.p99_ms, `${path}.p99_ms`) }
}

const hotspot = (value: unknown, path: string): Hotspot => {
  const o = object(value, path)
  return {
    tool: str(o.tool, `${path}.tool`),
    calls: num(o.calls, `${path}.calls`),
    share: num(o.share, `${path}.share`),
    target_errors: num(o.target_errors, `${path}.target_errors`),
    p95_ms: optNum(o.p95_ms, `${path}.p95_ms`),
    p99_ms: optNum(o.p99_ms, `${path}.p99_ms`),
  }
}

const sessionSample = (value: unknown, path: string): SessionSample => {
  const o = object(value, path)
  return {
    trace_id: o.trace_id == null ? '' : str(o.trace_id, `${path}.trace_id`),
    dispatches: list(o.dispatches, `${path}.dispatches`, (entry, p) => {
      const d = object(entry, p)
      return { at_us: num(d.at_us, `${p}.at_us`), tools: list(d.tools, `${p}.tools`, str) }
    }),
  }
}

const traffic = (value: unknown, path: string): TrafficReport => {
  const o = value == null ? {} : object(value, path)
  return {
    tool_calls: optNum(o.tool_calls, `${path}.tool_calls`) ?? 0,
    unbucketed_tool_calls: optNum(o.unbucketed_tool_calls, `${path}.unbucketed_tool_calls`) ?? 0,
    window_ms: optNum(o.window_ms, `${path}.window_ms`) ?? 100,
    peak_calls_per_window: optNum(o.peak_calls_per_window, `${path}.peak_calls_per_window`) ?? 0,
    p95_calls_per_occupied_window: optNum(o.p95_calls_per_occupied_window, `${path}.p95_calls_per_occupied_window`),
    session_samples: list(o.session_samples, `${path}.session_samples`, sessionSample),
    calls_per_window: list(o.calls_per_window, `${path}.calls_per_window`, num),
  }
}

const stageReport = (value: unknown, path: string): StageReport => {
  const o = object(value, path)
  const n = (key: string) => num(o[key], `${path}.${key}`)
  return {
    agents: n('agents'),
    held: bool(o.held, `${path}.held`),
    peak_active: n('peak_active'),
    started: n('started'),
    completed: n('completed'),
    runner_errors: n('runner_errors'),
    target_errors: n('target_errors'),
    inference_errors: n('inference_errors'),
    inference_throttles: n('inference_throttles'),
    tokens: n('tokens'),
    input_tokens: n('input_tokens'),
    output_tokens: n('output_tokens'),
    traffic: traffic(o.traffic, `${path}.traffic`),
    hotspots: list(o.hotspots, `${path}.hotspots`, hotspot),
    runner: latencies(o.runner, `${path}.runner`),
    target: latencies(o.target, `${path}.target`),
    inference: latencies(o.inference, `${path}.inference`),
    hold_seconds: n('hold_seconds'),
    passed: bool(o.passed, `${path}.passed`),
    interrupted: optNum(o.interrupted, `${path}.interrupted`) ?? 0,
  }
}

const pacing = (value: unknown, path: string): Pacing | null => {
  if (value == null) return null
  const o = object(value, path)
  return {
    start_spread_ms: num(o.start_spread_ms, `${path}.start_spread_ms`),
    think_min_ms: num(o.think_min_ms, `${path}.think_min_ms`),
    think_max_ms: num(o.think_max_ms, `${path}.think_max_ms`),
    sync_sessions: bool(o.sync_sessions, `${path}.sync_sessions`),
  }
}

const plan = (value: unknown, path: string): RunPlan | null => {
  if (value == null) return null
  const o = object(value, path)
  const kind = o.target_kind === 'mcp' || o.target_kind === 'http' ? o.target_kind : fail(`${path}.target_kind`, 'mcp or http')
  return {
    target_kind: kind,
    tools: requiredList(o.tools, `${path}.tools`, str),
    parallel_tools: requiredList(o.parallel_tools, `${path}.parallel_tools`, str),
    tasks: requiredList(o.tasks, `${path}.tasks`, str),
    hold_agents: optNum(o.hold_agents, `${path}.hold_agents`),
    pacing: pacing(o.pacing, `${path}.pacing`),
    agents_per_stage: requiredList(o.agents_per_stage, `${path}.agents_per_stage`, num),
  }
}

/** Validates unknown JSON as a run report; throws an Error naming the first bad field. */
export function parseReport(value: unknown): RunReport {
  if (!isObject(value)) throw new Error('Not an Endemic run report: the file is not a JSON object.')
  return describeErrors('Not an Endemic run report', () => {
    const status = str(value.status, 'status')
    if (!RUN_STATUSES.includes(status as RunStatus)) fail('status', RUN_STATUSES.join(', '))
    return {
      id: num(value.id, 'id'),
      status: status as RunStatus,
      highest_passing_agents: optNum(value.highest_passing_agents, 'highest_passing_agents'),
      plan: plan(value.plan, 'plan'),
      error: value.error == null ? null : str(value.error, 'error'),
      stages: requiredList(value.stages, 'stages', stageReport),
    }
  })
}

const percent = (part: number, whole: number) => (whole > 0 ? Math.round((10000 * part) / whole) / 100 : 0)

export function latencyRows(stages: StageReport[]) {
  return stages.map((s) => ({ agents: s.agents, target: s.target.p95_ms, inference: s.inference.p95_ms, runner: s.runner.p95_ms }))
}

export function errorRows(stages: StageReport[]) {
  return stages.map((s) => ({
    agents: s.agents,
    target: percent(s.target_errors, s.started),
    inference: percent(s.inference_errors, s.started),
    throttled: percent(s.inference_throttles, s.started),
    runner: percent(s.runner_errors, s.started),
    interrupted: percent(s.interrupted, s.started),
  }))
}

export function hotspotRows(stage: StageReport) {
  return [...stage.hotspots]
    .sort((a, b) => b.calls - a.calls || a.tool.localeCompare(b.tool))
    .map((h) => ({
      tool: h.tool,
      calls: h.calls,
      share: Math.round(h.share * 10000) / 100,
      errorRate: percent(h.target_errors, h.calls),
      p95: h.p95_ms,
      p99: h.p99_ms,
    }))
}

export function burstSeries(stage: StageReport) {
  const windowMs = stage.traffic.window_ms
  return {
    windowMs,
    p95: stage.traffic.p95_calls_per_occupied_window,
    points: stage.traffic.calls_per_window.map((calls, i) => ({ seconds: Math.round(i * windowMs) / 1000, calls })),
  }
}

export function traceRows(stage: StageReport) {
  return stage.traffic.session_samples.map((sample, i) => ({
    session: i + 1,
    traceId: sample.trace_id,
    dispatches: sample.dispatches.map((d) => ({ seconds: d.at_us / 1e6, count: d.tools.length, tools: d.tools })),
  }))
}

export function stageTableRows(stages: StageReport[]) {
  return stages.map((s) => ({
    agents: s.agents,
    passed: s.passed,
    held: s.held,
    started: s.started,
    completed: s.completed,
    errorShare: percent(
      s.target_errors + s.inference_errors + s.inference_throttles + s.runner_errors + s.interrupted,
      s.started,
    ),
    throttled: percent(s.inference_throttles, s.started),
    targetP95: s.target.p95_ms,
    inferenceP95: s.inference.p95_ms,
    runnerP95: s.runner.p95_ms,
    runnerP99: s.runner.p99_ms,
    toolCalls: s.traffic.tool_calls,
    peakBurst: s.traffic.peak_calls_per_window,
  }))
}
