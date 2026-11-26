import type { Fault, FaultEffect, Pacing } from './runRequest'
import { bool, describeErrors, fail, isObject, list, num, object, optNum, requiredList, str } from './json'

export const RUN_STATUSES = ['running', 'cancelled', 'failed', 'failed_stage', 'reached_ceiling', 'completed'] as const
export type RunStatus = (typeof RUN_STATUSES)[number]
export const CRITERIA = ['held', 'all_agents_active', 'sessions_completed', 'runner_p95', 'runner_p99', 'error_budget'] as const
export type Criterion = (typeof CRITERIA)[number]
export type Rise = { tool: string; from: number | null; to: number; began_at_agents: number }
export type Explanation = {
  agents: number
  criterion: Criterion
  baseline_agents: number | null
  error_rate: Rise | null
  p95_ms: Rise | null
  sample_trace_ids: string[]
}

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
export type Injected = { delayed: number; errors: number; timeouts: number }
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
  injected_failures: number
  injected: Record<string, Injected>
  faults: Fault[]
  runner: Latencies
  target: Latencies
  inference: Latencies
  hold_seconds: number
  passed: boolean
  failed_criterion: Criterion | null
  interrupted: number
}
export type PlanPersona = {
  name: string
  weight: number
  max_model_turns: number
  max_tool_calls: number
  tasks: { start: number; end: number }
}
export type RunPlan = {
  target_kind: 'mcp' | 'http'
  tools: string[]
  parallel_tools: string[]
  tasks: string[]
  hold_agents: number | null
  pacing: Pacing | null
  faults: Fault[]
  task_mix: PlanPersona[]
  agents_per_stage: number[]
}
export type RunReport = {
  id: number
  status: RunStatus
  highest_passing_agents: number | null
  plan: RunPlan | null
  error: string | null
  stages: StageReport[]
  explanation: Explanation | null
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

const effect = (value: unknown, path: string): FaultEffect => {
  const o = object(value, path)
  if (o.kind === 'latency') return { kind: 'latency', delay_ms: num(o.delay_ms, `${path}.delay_ms`) }
  if (o.kind === 'error' || o.kind === 'timeout') return { kind: o.kind }
  return fail(`${path}.kind`, 'latency, error, or timeout')
}

const fault = (value: unknown, path: string): Fault => {
  const o = object(value, path)
  return {
    tool: str(o.tool, `${path}.tool`),
    first_stage: num(o.first_stage, `${path}.first_stage`),
    last_stage: num(o.last_stage, `${path}.last_stage`),
    percent: num(o.percent, `${path}.percent`),
    effect: effect(o.effect, `${path}.effect`),
  }
}

const injected = (value: unknown, path: string): Record<string, Injected> => {
  const o = value == null ? {} : object(value, path)
  return Object.fromEntries(
    Object.entries(o).map(([tool, counts]) => {
      const c = object(counts, `${path}.${tool}`)
      const n = (key: string) => num(c[key], `${path}.${tool}.${key}`)
      return [tool, { delayed: n('delayed'), errors: n('errors'), timeouts: n('timeouts') }]
    }),
  )
}

const criterion = (value: unknown, path: string): Criterion => {
  const name = str(value, path)
  return CRITERIA.includes(name as Criterion) ? (name as Criterion) : fail(path, CRITERIA.join(', '))
}

const rise = (value: unknown, path: string): Rise | null => {
  if (value == null) return null
  const o = object(value, path)
  return {
    tool: str(o.tool, `${path}.tool`),
    from: optNum(o.from, `${path}.from`),
    to: num(o.to, `${path}.to`),
    began_at_agents: num(o.began_at_agents, `${path}.began_at_agents`),
  }
}

const explanation = (value: unknown, path: string): Explanation | null => {
  if (value == null) return null
  const o = object(value, path)
  return {
    agents: num(o.agents, `${path}.agents`),
    criterion: criterion(o.criterion, `${path}.criterion`),
    baseline_agents: optNum(o.baseline_agents, `${path}.baseline_agents`),
    error_rate: rise(o.error_rate, `${path}.error_rate`),
    p95_ms: rise(o.p95_ms, `${path}.p95_ms`),
    sample_trace_ids: requiredList(o.sample_trace_ids, `${path}.sample_trace_ids`, str),
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
    injected_failures: optNum(o.injected_failures, `${path}.injected_failures`) ?? 0,
    injected: injected(o.injected, `${path}.injected`),
    faults: list(o.faults, `${path}.faults`, fault),
    runner: latencies(o.runner, `${path}.runner`),
    target: latencies(o.target, `${path}.target`),
    inference: latencies(o.inference, `${path}.inference`),
    hold_seconds: n('hold_seconds'),
    passed: bool(o.passed, `${path}.passed`),
    failed_criterion: o.failed_criterion == null ? null : criterion(o.failed_criterion, `${path}.failed_criterion`),
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

const planPersona = (value: unknown, path: string): PlanPersona => {
  const o = object(value, path)
  const tasks = object(o.tasks, `${path}.tasks`)
  return {
    name: str(o.name, `${path}.name`),
    weight: num(o.weight, `${path}.weight`),
    max_model_turns: num(o.max_model_turns, `${path}.max_model_turns`),
    max_tool_calls: num(o.max_tool_calls, `${path}.max_tool_calls`),
    tasks: { start: num(tasks.start, `${path}.tasks.start`), end: num(tasks.end, `${path}.tasks.end`) },
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
    faults: list(o.faults, `${path}.faults`, fault),
    task_mix: list(o.task_mix, `${path}.task_mix`, planPersona),
    agents_per_stage: requiredList(o.agents_per_stage, `${path}.agents_per_stage`, num),
  }
}

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
      explanation: explanation(value.explanation, 'explanation'),
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
    faults: s.faults,
    injectedCalls: Object.values(s.injected).reduce((sum, c) => sum + c.delayed + c.errors + c.timeouts, 0),
    injectedFailures: percent(s.injected_failures, s.started),
  }))
}
