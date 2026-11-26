import type { ReactNode } from 'react'
import { Badge } from '@/components/ui/badge'
import type { RunReport, RunStatus } from '@/lib/report'
import { PlanSummary } from './plan-summary'

const STATUS: Record<RunStatus, string> = {
  running: 'Running',
  cancelled: 'Cancelled',
  failed: 'Failed before the first stage',
  failed_stage: 'Stopped at a failing stage',
  reached_ceiling: 'Reached the agent ceiling',
  completed: 'Completed',
}

function Headline({ report }: { report: RunReport }) {
  const held = report.highest_passing_agents
  const last = report.stages.at(-1)
  const hold = report.plan?.hold_agents
  if (hold != null && report.status === 'failed_stage') {
    return (
      <>
        <span className="num">{hold}</span> agents did not hold
      </>
    )
  }
  if (held != null) {
    return (
      <>
        <span className="num">{held}</span> agents held
      </>
    )
  }
  if (report.status === 'running') return <>{last ? `Stage at ${last.agents} agents done` : 'First stage running'}</>
  if (report.status === 'failed') return <>Run failed</>
  return <>No stage passed</>
}

export function RunHeader({ report, source, actions }: { report: RunReport | null; source: string; actions: ReactNode }) {
  return (
    <section aria-label="Run" className="grid grid-cols-[minmax(0,1fr)] gap-6 border-b border-border pb-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,34rem)]">
      <div className="grid content-start gap-4">
        <div className="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
          {report ? (
            <>
              <Badge variant={report.status === 'running' ? 'default' : 'outline'} className="gap-1.5">
                {report.status === 'running' && <span aria-hidden className="size-1.5 animate-pulse rounded-full bg-black motion-reduce:animate-none" />}
                {STATUS[report.status]}
              </Badge>
              <span className="num">Run {report.id}</span>
              <span>{source}</span>
            </>
          ) : (
            <span>No run loaded</span>
          )}
        </div>
        <h1 className="display text-6xl leading-[0.95] font-bold tracking-tight sm:text-7xl">
          {report ? <Headline report={report} /> : 'Endemic'}
        </h1>
        {report?.error && <p className="max-w-[60ch] text-sm">{report.error}</p>}
        {!report && <p className="max-w-[60ch] text-sm text-muted-foreground">Start a run against a live runner, or open a saved report to read it.</p>}
        <div className="flex flex-wrap gap-2">{actions}</div>
      </div>
      {report?.plan ? (
        <div className="rounded-md border border-border p-4">
          <h2 className="mb-3 text-sm font-medium">What Endemic chose</h2>
          <PlanSummary plan={report.plan} />
        </div>
      ) : report ? (
        <p className="self-end text-sm text-muted-foreground">This report predates recorded run plans.</p>
      ) : null}
    </section>
  )
}
