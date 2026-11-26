import { useEffect } from 'react'
import { getRun } from '@/lib/api'
import type { RunReport } from '@/lib/report'

const POLL_MS = 2000

/** Polls GET /runs/{id} every 2 s while a live run is running. */
export function useLiveRun(
  report: RunReport | null,
  live: boolean,
  onReport: (report: RunReport) => void,
  onError: (message: string) => void,
) {
  const id = report?.id
  const running = live && report?.status === 'running'
  useEffect(() => {
    if (!running || id == null) return
    let stopped = false
    const timer = setTimeout(() => {
      getRun(id).then(
        (next) => !stopped && onReport(next),
        (error: Error) => !stopped && onError(error.message),
      )
    }, POLL_MS)
    return () => {
      stopped = true
      clearTimeout(timer)
    }
    // Each new report object schedules the next poll.
  }, [report, running, id, onReport, onError])
}
