import type { RunPlan } from '@/lib/report'
import { describePacing } from '@/lib/runRequest'

const KIND: Record<RunPlan['target_kind'], string> = { mcp: 'MCP endpoint', http: 'HTTP API (OpenAPI)' }

/** What Endemic decided for the run, read-only. Tool names come from the target and render as text. */
export function PlanSummary({ plan }: { plan: RunPlan }) {
  return (
    <dl className="grid grid-cols-[7.5rem_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
      <dt className="text-muted-foreground">Target</dt>
      <dd>{KIND[plan.target_kind]}</dd>
      <dt className="text-muted-foreground">Tools found</dt>
      <dd>
        <ul className="flex flex-wrap gap-1.5">
          {plan.tools.map((tool) => (
            <li key={tool} className="max-w-full truncate rounded border border-white/25 px-1.5 py-0.5 font-mono text-xs" title={tool}>
              {tool}
              {plan.parallel_tools.includes(tool) && <span className="ml-1.5 font-sans text-white/55">parallel</span>}
            </li>
          ))}
        </ul>
      </dd>
      <dt className="text-muted-foreground">Goal</dt>
      <dd className="num">
        {plan.hold_agents != null
          ? `Hold ${plan.hold_agents} agents`
          : `Find the breaking point${plan.agents_per_stage.length ? `: ${plan.agents_per_stage.join(', ')} agents` : ''}`}
      </dd>
      <dt className="text-muted-foreground">Pacing</dt>
      <dd className="num">{describePacing(plan.pacing)}</dd>
      <dt className="text-muted-foreground">Tasks</dt>
      <dd>
        <details>
          <summary className="cursor-pointer text-white/80">
            {plan.tasks.length} generated task{plan.tasks.length === 1 ? '' : 's'}
          </summary>
          <ol className="mt-2 list-decimal space-y-1 pl-5 text-white/80">
            {plan.tasks.map((task, i) => (
              <li key={i}>{task}</li>
            ))}
          </ol>
        </details>
      </dd>
    </dl>
  )
}
