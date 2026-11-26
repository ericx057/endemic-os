import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { stageTableRows, type StageReport } from '@/lib/report'
import { describeFault } from '@/lib/runRequest'
import { ms, pct } from './charts/format'

const COLUMNS = ['Agents', 'Result', 'Started', 'Completed', 'Error share', 'Throttled', 'Target p95', 'Inference p95', 'Runner p95 / p99', 'Tool calls', 'Peak burst', 'Injected calls', 'Ended by injection', 'Faults active']

export function StageTable({ stages }: { stages: StageReport[] }) {
  return (
    <div className="h-full overflow-auto">
      <Table>
        <TableHeader>
          <TableRow>
            {COLUMNS.map((name, i) => (
              <TableHead key={name} className={i > 1 ? 'text-right' : undefined}>
                {name}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {stageTableRows(stages).map((row, i) => (
            <TableRow key={i}>
              <TableCell className="num font-medium">{row.agents}</TableCell>
              <TableCell>
                <Badge variant={row.passed ? 'default' : 'outline'}>{row.passed ? 'Passed' : row.held ? 'Failed' : 'Did not hold'}</Badge>
              </TableCell>
              <TableCell className="num text-right">{row.started.toLocaleString()}</TableCell>
              <TableCell className="num text-right">{row.completed.toLocaleString()}</TableCell>
              <TableCell className="num text-right">{pct(row.errorShare)}</TableCell>
              <TableCell className="num text-right">{pct(row.throttled)}</TableCell>
              <TableCell className="num text-right">{ms(row.targetP95)}</TableCell>
              <TableCell className="num text-right">{ms(row.inferenceP95)}</TableCell>
              <TableCell className="num text-right">
                {ms(row.runnerP95)} / {ms(row.runnerP99)}
              </TableCell>
              <TableCell className="num text-right">{row.toolCalls.toLocaleString()}</TableCell>
              <TableCell className="num text-right">{row.peakBurst.toLocaleString()}</TableCell>
              <TableCell className="num text-right">{row.injectedCalls.toLocaleString()}</TableCell>
              <TableCell className="num text-right">{pct(row.injectedFailures)}</TableCell>
              <TableCell className="text-right">{row.faults.length ? row.faults.map((fault, j) => <p key={j}>{describeFault(fault)}</p>) : 'None'}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
