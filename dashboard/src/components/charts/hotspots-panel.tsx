import { Bar, BarChart, LabelList, Tooltip, XAxis, YAxis } from 'recharts'
import { ChartContainer } from '@/components/ui/chart'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { hotspotRows, type StageReport } from '@/lib/report'
import { TooltipBox } from './mono'
import { AXIS, ms, pct } from './format'

const config = { calls: { label: 'Calls', color: '#ffffff' } }
const short = (tool: string) => (tool.length > 18 ? `${tool.slice(0, 17)}…` : tool)

export function HotspotsPanel({ stage }: { stage: StageReport }) {
  const rows = hotspotRows(stage)
  if (rows.length === 0) {
    return <p className="text-sm text-muted-foreground">No per-tool data in this stage. Older runners do not report hotspots.</p>
  }
  return (
    <Tabs defaultValue="chart" className="flex h-full flex-col">
      <TabsList className="self-start">
        <TabsTrigger value="chart">Chart</TabsTrigger>
        <TabsTrigger value="table">Table</TabsTrigger>
      </TabsList>
      <TabsContent value="chart" className="min-h-0 flex-1">
        <ChartContainer config={config} className="aspect-auto h-full w-full">
          <BarChart data={rows} layout="vertical" margin={{ top: 4, right: 88, bottom: 4, left: 4 }} barCategoryGap="22%" accessibilityLayer>
            <XAxis type="number" hide />
            <YAxis
              type="category"
              dataKey="tool"
              width={140}
              interval={0}
              {...AXIS}
              tick={{ ...AXIS.tick, fill: '#ffffff' }}
              tickFormatter={short}
            />
            <Tooltip
              cursor={{ fill: '#ffffff', fillOpacity: 0.06 }}
              content={({ active, payload }) => {
                const row = active ? payload?.[0]?.payload : null
                return row ? (
                  <TooltipBox
                    title={row.tool}
                    rows={[
                      { label: 'Calls', value: row.calls.toLocaleString() },
                      { label: 'Share of calls', value: pct(row.share) },
                      { label: 'Error rate', value: pct(row.errorRate) },
                      { label: 'p95 / p99', value: `${ms(row.p95)} / ${ms(row.p99)}` },
                    ]}
                  />
                ) : null
              }}
            />
            <Bar dataKey="calls" fill="#ffffff" radius={[0, 3, 3, 0]} isAnimationActive={false}>
              <LabelList dataKey="share" position="right" fill="#a3a3a3" fontSize={11} formatter={(v) => pct(Number(v))} />
            </Bar>
          </BarChart>
        </ChartContainer>
      </TabsContent>
      <TabsContent value="table" className="min-h-0 flex-1 overflow-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Tool</TableHead>
              <TableHead className="text-right">Calls</TableHead>
              <TableHead className="text-right">Share</TableHead>
              <TableHead className="text-right">Error rate</TableHead>
              <TableHead className="text-right">p95 / p99</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.tool}>
                <TableCell className="max-w-48 truncate font-mono text-xs" title={row.tool}>
                  {row.tool}
                </TableCell>
                <TableCell className="num text-right">{row.calls.toLocaleString()}</TableCell>
                <TableCell className="num text-right">{pct(row.share)}</TableCell>
                <TableCell className="num text-right">{pct(row.errorRate)}</TableCell>
                <TableCell className="num text-right">
                  {ms(row.p95)} / {ms(row.p99)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TabsContent>
    </Tabs>
  )
}
