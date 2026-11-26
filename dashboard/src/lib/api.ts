// Calls go to /api, which the Next.js server proxies to ENDEMIC_API_URL with the
// operator token added server-side. No other network requests are made.
import { parseReport, type RunReport } from './report'
import type { RunRequest } from './runRequest'

export class ApiError extends Error {
  readonly status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

const MESSAGES: Record<number, string> = {
  401: 'The runner rejected the operator token. Check ENDEMIC_API_TOKEN where the dashboard server runs.',
  404: 'The runner has no such run. It keeps only the latest run in memory.',
  409: 'The runner is busy: another run is in progress, or this run already finished.',
  422: 'The runner rejected the request. Check the target URL, the OpenAPI spec URL and the use case length.',
}

async function call(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(`/api${path}`, init)
  if (!response.ok) {
    // A 422 carries the runner's reason, which never includes the token.
    const reason = response.status === 422 ? await response.json().then((body) => body?.error, () => null) : null
    const fallback = MESSAGES[response.status] ?? `The runner answered HTTP ${response.status}.`
    throw new ApiError(response.status, typeof reason === 'string' ? `The runner rejected the request: ${reason}` : fallback)
  }
  return response.status === 202 && init?.method === 'DELETE' ? null : response.json()
}

/** The body carries the staging token; it is sent once and not kept anywhere in the page. */
export const startRun = async (body: RunRequest): Promise<RunReport> =>
  parseReport(await call('/runs', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }))

export const getRun = async (id: number): Promise<RunReport> => parseReport(await call(`/runs/${id}`))

export const cancelRun = async (id: number): Promise<void> => {
  await call(`/runs/${id}`, { method: 'DELETE' })
}
