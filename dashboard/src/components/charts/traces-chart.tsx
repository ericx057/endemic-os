import { CartesianGrid, Scatter, ScatterChart, Tooltip, XAxis, YAxis } from 'recharts'
import { ChartContainer } from '@/components/ui/chart'
import { traceRows, type StageReport } from '@/lib/report'
import { TooltipBox } from './mono'
import { AXIS } from './format'

const config = { dispatch: { label: 'Dispatch', color: '#ffffff' } }
const MARK_PX_PER_TOOL = 4

type Point = { seconds: number; session: number; count: number; tools: string[]; traceId: string }

function DispatchMark({ cx, cy, payload }: { cx?: number; cy?: number; payload?: Point }) {
  if (cx == null || cy == null || !payload) return null
  const width = Math.max(2, payload.count * MARK_PX_PER_TOOL)
  return <rect x={cx - width / 2} y={cy - 7} width={width} height={14} rx={1} fill="#ffffff" stroke="#000" strokeWidth={1} />
}

export function TracesChart({ stage }: { stage: StageReport }) {
  const rows = traceRows(stage)
  if (rows.length === 0) {
    return <p className="text-sm text-muted-foreground">No sampled sessions in this stage.</p>
  }
  const points: Point[] = rows.flatMap((row) => row.dispatches.map((d) => ({ ...d, session: row.session, traceId: row.traceId })))
  const label = (session: number) => {
    const id = rows[session - 1]?.traceId
    return id ? `#${session} ${id.slice(0, 8)}` : `#${session}`
  }
  return (
    <div className="flex h-full flex-col gap-1">
      <p className="text-xs text-muted-foreground">
        Each row is one sampled session; each mark is a tool dispatch, wider when it sent more tools at once.
      </p>
      <ChartContainer config={config} className="aspect-auto min-h-0 w-full flex-1">
        <ScatterChart margin={{ top: 8, right: 16, bottom: 4, left: 4 }} accessibilityLayer>
          <CartesianGrid horizontal={false} />
          <XAxis type="number" dataKey="seconds" {...AXIS} domain={[0, 'auto']} tickFormatter={(v: number) => `${v} s`} name="time" />
          <YAxis
            type="number"
            dataKey="session"
            reversed
            domain={[0.5, rows.length + 0.5]}
            ticks={rows.map((row) => row.session)}
            interval={0}
            width={112}
            tickFormatter={label}
            {...AXIS}
          />
          <Tooltip
            cursor={false}
            content={({ active, payload }) => {
              const point = active ? (payload?.[0]?.payload as Point | undefined) : undefined
              return point ? (
                <TooltipBox
                  title={`Session ${point.session} at ${point.seconds.toFixed(2)} s`}
                  rows={[
                    { label: 'Trace id', value: point.traceId || 'not recorded' },
                    { label: `Tools (${point.count})`, value: point.tools.join(', ') },
                  ]}
                />
              ) : null
            }}
          />
          <Scatter data={points} shape={DispatchMark} isAnimationActive={false} />
        </ScatterChart>
      </ChartContainer>
    </div>
  )
}
