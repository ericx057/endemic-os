import { Bar, BarChart, CartesianGrid, ReferenceLine, Tooltip, XAxis, YAxis } from 'recharts'
import { ChartContainer } from '@/components/ui/chart'
import { burstSeries, type StageReport } from '@/lib/report'
import { TooltipBox } from './mono'
import { AXIS } from './format'

const config = { calls: { label: 'Tool calls', color: '#ffffff' } }

export function BurstsChart({ stage }: { stage: StageReport }) {
  const { points, p95, windowMs } = burstSeries(stage)
  if (points.length === 0) {
    return <p className="text-sm text-muted-foreground">No per-window counts in this stage. Older runners report only totals.</p>
  }
  return (
    <div className="flex h-full flex-col gap-1">
      <p className="text-xs text-muted-foreground">
        Tool calls per {windowMs} ms window. Peak {stage.traffic.peak_calls_per_window.toLocaleString()}, p95 of busy windows{' '}
        {p95 ?? 'n/a'}.
      </p>
      <ChartContainer config={config} className="aspect-auto min-h-0 w-full flex-1">
        <BarChart data={points} margin={{ top: 22, right: 12, bottom: 4, left: 4 }} barCategoryGap={0} accessibilityLayer>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="seconds" {...AXIS} interval="preserveStartEnd" minTickGap={40} tickFormatter={(v: number) => `${Math.round(v)} s`} />
          <YAxis width={44} allowDecimals={false} {...AXIS} />
          <Tooltip
            cursor={{ fill: '#ffffff', fillOpacity: 0.12 }}
            content={({ active, payload }) => {
              const point = active ? payload?.[0]?.payload : null
              return point ? (
                <TooltipBox
                  title={`${point.seconds.toFixed(1)} s`}
                  rows={[{ label: 'Tool calls', value: point.calls.toLocaleString() }]}
                />
              ) : null
            }}
          />
          <Bar dataKey="calls" fill="#ffffff" isAnimationActive={false} />
          {p95 != null && (
            <ReferenceLine
              y={p95}
              stroke="#ffffff"
              strokeDasharray="5 4"
              label={({ viewBox }: { viewBox?: { x?: number; y?: number; width?: number } }) => {
                const right = (viewBox?.x ?? 0) + (viewBox?.width ?? 0)
                const y = viewBox?.y ?? 0
                return (
                  <g>
                    <rect x={right - 138} y={y - 18} width={138} height={16} fill="#000" />
                    <text x={right - 4} y={y - 6} textAnchor="end" fill="#fff" fontSize={11}>
                      p95 busy window: {p95}
                    </text>
                  </g>
                )
              }}
            />
          )}
        </BarChart>
      </ChartContainer>
    </div>
  )
}
