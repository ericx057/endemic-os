import { ms, pct } from '@/components/charts/format'
import type { Criterion, Explanation, Rise } from './report'

const CRITERION: Record<Criterion, string> = {
  held: 'the stage did not hold for its full duration',
  all_agents_active: 'not every agent became active',
  sessions_completed: 'no session completed',
  runner_p95: 'runner p95 latency went over its limit',
  runner_p99: 'runner p99 latency went over its limit',
  error_budget: 'errors and interruptions went over the error budget',
}

const percent = (fraction: number) => pct(Math.round(fraction * 10000) / 100)

const riseLine = (rise: Rise, what: string, show: (value: number) => string) =>
  rise.from == null
    ? `${rise.tool} had the highest ${what}: ${show(rise.to)}.`
    : `${rise.tool} ${what} rose from ${show(rise.from)} to ${show(rise.to)}, starting at ${rise.began_at_agents} agents.`

export function explanationLines(explanation: Explanation) {
  const rises = [
    explanation.error_rate && riseLine(explanation.error_rate, 'error rate', percent),
    explanation.p95_ms && riseLine(explanation.p95_ms, 'p95', ms),
  ].filter((line): line is string => line != null)
  return {
    headline: `Failed at ${explanation.agents} agents: ${CRITERION[explanation.criterion]}.`,
    baseline:
      explanation.baseline_agents == null
        ? 'No stage passed, so tools are ranked by their values in this stage.'
        : `Compared with the last passing stage at ${explanation.baseline_agents} agents.`,
    rises: rises.length > 0 ? rises : ['No tool error rate or p95 rose.'],
    traces: explanation.sample_trace_ids,
  }
}
