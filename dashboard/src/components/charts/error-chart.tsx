import { Bar, BarChart, CartesianGrid, Tooltip, XAxis, YAxis } from 'recharts'
import { ChartContainer } from '@/components/ui/chart'
import { errorRows, type StageReport } from '@/lib/report'
import { Legend, TooltipBox } from './mono'
import { AXIS, pct } from './format'

const SOURCES = [
  { key: 'target', label: 'Target errors', fill: '#ffffff' },
  { key: 'throttled', label: 'Inference throttled', fill: 'url(#hatch)' },
  { key: 'inference', label: 'Inference errors', fill: '#a3a3a3' },
  { key: 'runner', label: 'Runner errors', fill: '#636363' },
  { key: 'interrupted', label: 'Interrupted', fill: '#3d3d3d' },
] as const
const config = Object.fromEntries(SOURCES.map((s) => [s.key, { label: s.label }]))

const Swatch = ({ fill }: { fill: string }) => (
  <svg width="12" height="12" aria-hidden className="shrink-0">
    <defs>
      <pattern id="hatch-swatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
        <rect width="4" height="4" fill="#000" />
        <line x1="0" y1="0" x2="0" y2="4" stroke="#fff" strokeWidth="2" />
      </pattern>
    </defs>
    <rect width="12" height="12" rx="2" fill={fill === 'url(#hatch)' ? 'url(#hatch-swatch)' : fill} stroke="#525252" />
  </svg>
)

export function ErrorChart({ stages }: { stages: StageReport[] }) {
  const rows = errorRows(stages)
  return (
    <div className="flex h-full flex-col gap-2">
      <Legend items={SOURCES.map((s) => ({ key: s.key, label: s.label, swatch: <Swatch fill={s.fill} /> }))} />
      <ChartContainer config={config} className="aspect-auto min-h-0 w-full flex-1">
        <BarChart data={rows} margin={{ top: 8, right: 8, bottom: 4, left: 4 }} barCategoryGap="28%" accessibilityLayer>
          <defs>
            <pattern id="hatch" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
              <rect width="5" height="5" fill="#000" />
              <line x1="0" y1="0" x2="0" y2="5" stroke="#fff" strokeWidth="2.5" />
            </pattern>
          </defs>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="agents" {...AXIS} label={{ value: 'agents', position: 'insideBottomRight', offset: -2, fill: '#a3a3a3', fontSize: 11 }} />
          <YAxis tickFormatter={(v: number) => `${v}%`} width={44} allowDecimals {...AXIS} />
          <Tooltip
            cursor={{ fill: '#ffffff', fillOpacity: 0.06 }}
            content={({ active, payload }) =>
              active && payload?.length ? (
                <TooltipBox
                  title={`${payload[0].payload.agents} agents: share of sessions`}
                  rows={SOURCES.map((s) => ({ label: s.label, value: pct(payload[0].payload[s.key]) }))}
                />
              ) : null
            }
          />
          {SOURCES.map((s, i) => (
            <Bar
              key={s.key}
              dataKey={s.key}
              stackId="errors"
              fill={s.fill}
              stroke="#000"
              strokeWidth={2}
              radius={i === SOURCES.length - 1 ? [3, 3, 0, 0] : 0}
              isAnimationActive={false}
            />
          ))}
        </BarChart>
      </ChartContainer>
    </div>
  )
}
