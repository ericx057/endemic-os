export const MAX_USE_CASE_BYTES = 4096

export type RunGoal = 'find_limit' | 'hold'
export type Pacing = { start_spread_ms: number; think_min_ms: number; think_max_ms: number; sync_sessions: boolean }
export type RunForm = {
  url: string
  openapiUrl: string
  bearer: string
  useCase: string
  goal: RunGoal
  holdAgents: string
  startSpreadMs: string
  thinkMinMs: string
  thinkMaxMs: string
  syncSessions: boolean
}
export type RunFormErrors = Partial<Record<keyof RunForm, string>>
export type RunRequest = {
  target: { url: string; openapi_url?: string }
  credentials: { bearer: string }
  use_case?: string
  hold_agents?: number
  pacing?: Pacing
}

export const emptyRunForm = (): RunForm => ({
  url: '',
  openapiUrl: '',
  bearer: '',
  useCase: '',
  goal: 'find_limit',
  holdAgents: '',
  startSpreadMs: '0',
  thinkMinMs: '0',
  thinkMaxMs: '0',
  syncSessions: false,
})

export const runAgainForm = (previous: RunForm): RunForm => ({ ...previous, bearer: '' })

export const describePacing = (pacing: Pacing | null | undefined) => {
  if (!pacing || (pacing.start_spread_ms === 0 && pacing.think_max_ms === 0 && !pacing.sync_sessions)) return 'All agents start together, no pauses'
  return [
    pacing.start_spread_ms > 0 && `starts spread over ${pacing.start_spread_ms} ms`,
    pacing.think_max_ms > 0 && `${pacing.think_min_ms} to ${pacing.think_max_ms} ms between sessions`,
    pacing.sync_sessions && 'sessions in waves',
  ]
    .filter(Boolean)
    .join(', ')
    .replace(/^./, (first) => first.toUpperCase())
}

export const byteLength = (text: string) => new TextEncoder().encode(text).length

const httpsError = (value: string, label: string) => {
  try {
    return new URL(value).protocol === 'https:' ? null : `${label} must start with https://.`
  } catch {
    return `${label} must be a full URL such as https://staging.example.com/mcp.`
  }
}

/** Mirrors the server's rules; on success returns the exact POST /runs body. */
export function validateRunForm(form: RunForm): { ok: true; body: RunRequest } | { ok: false; errors: RunFormErrors } {
  const errors: RunFormErrors = {}
  const url = form.url.trim()
  const openapiUrl = form.openapiUrl.trim()

  const urlError = url ? httpsError(url, 'Target URL') : 'Enter the URL of your staging application.'
  if (urlError) errors.url = urlError
  const specError = openapiUrl ? httpsError(openapiUrl, 'OpenAPI spec URL') : null
  if (specError) errors.openapiUrl = specError

  if (!form.bearer.trim()) errors.bearer = 'Enter the staging bearer token.'
  else if (/\s/.test(form.bearer)) errors.bearer = 'The token contains spaces or line breaks. Paste it again without them.'

  const hasUseCase = form.useCase.trim() !== ''
  const bytes = byteLength(form.useCase)
  if (hasUseCase && bytes > MAX_USE_CASE_BYTES) {
    errors.useCase = `The use case is ${bytes.toLocaleString()} bytes; the limit is ${MAX_USE_CASE_BYTES.toLocaleString()}.`
  }

  const holdAgents = Number(form.holdAgents.trim())
  const holds = form.goal === 'hold'
  if (holds && !(/^\d+$/.test(form.holdAgents.trim()) && holdAgents >= 1)) errors.holdAgents = 'Enter a whole number of agents, 1 or more.'

  const ms = (field: 'startSpreadMs' | 'thinkMinMs' | 'thinkMaxMs') => {
    const text = form[field].trim()
    if (/^\d+$/.test(text)) return Number(text)
    errors[field] = 'Enter whole milliseconds, 0 or more.'
    return 0
  }
  const pacing: Pacing = { start_spread_ms: ms('startSpreadMs'), think_min_ms: ms('thinkMinMs'), think_max_ms: ms('thinkMaxMs'), sync_sessions: form.syncSessions }
  if (!errors.thinkMaxMs && pacing.think_min_ms > pacing.think_max_ms) errors.thinkMaxMs = 'Must be at least the shortest pause.'
  const paced = pacing.start_spread_ms > 0 || pacing.think_max_ms > 0 || pacing.sync_sessions

  if (Object.keys(errors).length > 0) return { ok: false, errors }
  return {
    ok: true,
    body: {
      target: openapiUrl ? { url, openapi_url: openapiUrl } : { url },
      credentials: { bearer: form.bearer },
      ...(hasUseCase ? { use_case: form.useCase } : {}),
      ...(holds ? { hold_agents: holdAgents } : {}),
      ...(paced ? { pacing } : {}),
    },
  }
}
