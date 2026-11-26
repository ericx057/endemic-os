import { useCallback, useEffect, useState } from 'react'
import { Play, RotateCcw, Square } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useLiveRun } from '@/hooks/use-live-run'
import { cancelRun, startRun } from '@/lib/api'
import { DEV_REVIEW_FORM, devFixture, devPanelsOpen, devSheet } from '@/lib/dev-params'
import { hidePanel, loadLayout, resetLayout, saveLayout, type LayoutState, type PanelId } from '@/lib/layout'
import { parseReport, type RunReport } from '@/lib/report'
import { emptyRunForm, runAgainForm, type RunForm, type RunRequest } from '@/lib/runRequest'
import { BurstsChart } from './charts/bursts-chart'
import { ErrorChart } from './charts/error-chart'
import { HotspotsPanel } from './charts/hotspots-panel'
import { LatencyChart } from './charts/latency-chart'
import { TracesChart } from './charts/traces-chart'
import { NewRunSheet } from './new-run-sheet'
import { OpenReport } from './open-report'
import { PanelGrid } from './panel-grid'
import { PanelsMenu } from './panels-menu'
import { RunHeader } from './run-header'
import { StageTable } from './stage-table'

const LIVE = process.env.ENDEMIC_LIVE === 'true'

type Loaded = { report: RunReport; source: string; live: boolean }

export function Dashboard() {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [message, setMessage] = useState('')
  const [stageIndex, setStageIndex] = useState<number | null>(null)
  const [layout, setLayout] = useState<LayoutState>(() => loadLayout(localStorage))
  const [sheetOpen, setSheetOpen] = useState(devSheet != null)
  const [sheetForm, setSheetForm] = useState<RunForm>(process.env.NODE_ENV === 'development' && devSheet ? DEV_REVIEW_FORM : emptyRunForm())
  const [lastTarget, setLastTarget] = useState<RunForm | null>(null)
  const [sheetKey, setSheetKey] = useState(0)

  useEffect(() => {
    if (process.env.NODE_ENV !== 'development' || !devFixture) return
    import('../../fixtures/sample-run.json').then((fixture) =>
      setLoaded({ report: parseReport(fixture.default), source: 'Sample fixture', live: false }),
    )
  }, [])

  const updateLayout = (next: LayoutState) => {
    setLayout(next)
    saveLayout(localStorage, next)
  }
  const onLive = useCallback((report: RunReport) => setLoaded({ report, source: 'Live runner', live: true }), [])
  useLiveRun(loaded?.report ?? null, loaded?.live ?? false, onLive, setMessage)

  const report = loaded?.report ?? null
  const stages = report?.stages ?? []
  const stage = stages[stageIndex ?? stages.length - 1]

  const open = (next: Loaded) => {
    setLoaded(next)
    setStageIndex(null)
    setMessage('')
  }
  const start = async (body: RunRequest, form: RunForm) => {
    const started = await startRun(body)
    setLastTarget(runAgainForm(form))
    open({ report: started, source: 'Live runner', live: true })
  }
  const openSheet = (form: RunForm) => {
    setSheetForm(form)
    setSheetKey((key) => key + 1)
    setSheetOpen(true)
  }

  const render = (id: PanelId) => {
    if (!report || stages.length === 0) return <p className="text-sm text-muted-foreground">No stages yet.</p>
    if (id === 'latency') return <LatencyChart stages={stages} />
    if (id === 'errors') return <ErrorChart stages={stages} />
    if (id === 'stages') return <StageTable stages={stages} />
    if (!stage) return null
    if (id === 'hotspots') return <HotspotsPanel stage={stage} />
    if (id === 'bursts') return <BurstsChart stage={stage} />
    return <TracesChart stage={stage} />
  }

  const actions = (
    <>
      <Button size="sm" onClick={() => openSheet(emptyRunForm())}>
        <Play aria-hidden />
        New run
      </Button>
      {lastTarget && report?.status !== 'running' && (
        <Button size="sm" variant="outline" onClick={() => openSheet(lastTarget)}>
          <RotateCcw aria-hidden />
          Run again
        </Button>
      )}
      {loaded?.live && report?.status === 'running' && (
        <Button size="sm" variant="outline" onClick={() => cancelRun(report.id).catch((error: Error) => setMessage(error.message))}>
          <Square aria-hidden />
          Cancel run
        </Button>
      )}
      <OpenReport onOpen={(next) => open({ report: next, source: 'Saved report', live: false })} onError={setMessage} />
    </>
  )

  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      <RunHeader report={report} source={loaded?.source ?? ''} actions={actions} />
      {!LIVE && (
        <p className="text-sm text-muted-foreground">Live mode is off: start the dashboard with ENDEMIC_API_URL and ENDEMIC_API_TOKEN to run tests. Saved reports open either way.</p>
      )}
      {message && (
        <p role="alert" className="rounded-md border border-white/40 p-3 text-sm">
          {message}
        </p>
      )}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <label className="flex items-center gap-3 text-sm text-muted-foreground">
          Per-stage panels show
          <Select value={String(stageIndex ?? stages.length - 1)} onValueChange={(v) => setStageIndex(Number(v))} disabled={stages.length === 0}>
            <SelectTrigger size="sm" className="w-52 text-foreground" aria-label="Stage">
              <SelectValue placeholder="No stages" />
            </SelectTrigger>
            <SelectContent>
              {stages.map((s, i) => (
                <SelectItem key={i} value={String(i)}>
                  {s.agents} agents, {s.passed ? 'passed' : 'failed'}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <PanelsMenu state={layout} onChange={updateLayout} onReset={() => setLayout(resetLayout(localStorage))} defaultOpen={devPanelsOpen} />
      </div>
      <PanelGrid state={layout} onChange={updateLayout} onHide={(id) => updateLayout(hidePanel(layout, id))} render={render} />
      <NewRunSheet key={sheetKey} open={sheetOpen} onOpenChange={setSheetOpen} initial={sheetForm} initialStep={devSheet === 'review' ? 'review' : 'edit'} onStart={LIVE ? start : undefined} />
    </div>
  )
}
