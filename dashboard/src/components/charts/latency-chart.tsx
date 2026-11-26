import { CartesianGrid, LabelList, Line, LineChart, Tooltip, XAxis, YAxis } from 'recharts'
import { ChartContainer } from '@/components/ui/chart'
import { latencyRows, type StageReport } from '@/lib/report'
import { Legend, LineSwatch, Marker, TooltipBox, type Series } from './mono'
import { AXIS, TOOLTIP_CURSOR, ms } from './format'

const SERIES: Series[] = [
  { key: 'inference', label: 'Inference', opacity: 1, shape: 'square', dash: '7 4' },
  { key: 'target', label: 'Target tools', opacity: 1, shape: 'circle' },
  { key: 'runner', label: 'Runner overhead', opacity: 0.6, shape: 'triangle', dash: '2 3' },
]
const config = Object.fromEntries(SERIES.map((s) => [s.key, { label: s.label, color: '#ffffff' }]))

/** Powers of ten that bracket the data, so the log axis starts and ends on a labeled tick. */
function decades(rows: ReturnType<typeof latencyRows>) {
  const values = rows.flatMap((r) => [r.target, r.inference, r.runner]).filter((v): v is number => v != null && v > 0)
  const low = Math.floor(Math.log10(Math.min(...values, 10)))
  const high = Math.ceil(Math.log10(Math.max(...values, 10)))
  return Array.from({ length: high - low + 1 }, (_, i) => 10 ** (low + i))
}

export function LatencyChart({ stages }: { stages: StageReport[] }) {
  const rows = latencyRows(stages)
  const last = rows.length - 1
  const ticks = decades(rows)
  return (
    <div className="flex h-full flex-col gap-2">
      <Legend items={SERIES.map((s) => ({ key: s.key, label: `${s.label} p95`, swatch: <LineSwatch series={s} /> }))} />
      <ChartContainer config={config} className="aspect-auto min-h-0 w-full flex-1">
        <LineChart data={rows} margin={{ top: 8, right: 96, bottom: 4, left: 4 }} accessibilityLayer>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="agents" {...AXIS} label={{ value: 'agents', position: 'insideBottomRight', offset: -2, fill: '#a3a3a3', fontSize: 11 }} />
          <YAxis scale="log" domain={[ticks[0], ticks.at(-1)!]} ticks={ticks} interval={0} tickFormatter={ms} width={52} {...AXIS} />
          <Tooltip
            cursor={TOOLTIP_CURSOR}
            content={({ active, payload }) =>
              active && payload?.length ? (
                <TooltipBox
                  title={`${payload[0].payload.agents} agents`}
                  rows={SERIES.map((s) => ({ label: `${s.label} p95`, value: ms(payload[0].payload[s.key]) }))}
                />
              ) : null
            }
          />
          {SERIES.map((s) => (
            <Line
              key={s.key}
              dataKey={s.key}
              stroke="#ffffff"
              strokeOpacity={s.opacity}
              strokeWidth={2}
              strokeDasharray={s.dash}
              isAnimationActive={false}
              dot={(props) => <Marker key={`${s.key}-${props.index}`} cx={props.cx} cy={props.cy} shape={s.shape} opacity={s.opacity} />}
              activeDot={(props) => <Marker cx={props.cx} cy={props.cy} shape={s.shape} size={6} />}
            >
              <LabelList
                content={({ x, y, index }) =>
                  index === last ? (
                    <text x={Number(x) + 10} y={Number(y) + 4} fill="#ffffff" fillOpacity={Math.max(s.opacity, 0.75)} fontSize={11}>
                      {s.label}
                    </text>
                  ) : null
                }
              />
            </Line>
          ))}
        </LineChart>
      </ChartContainer>
    </div>
  )
}
