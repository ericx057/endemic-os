export const MAX_USE_CASE_BYTES = 4096

export const MAX_PERSONA_NAME_BYTES = 64
export const WEIGHT_TOTAL = 100

export type RunGoal = 'find_limit' | 'hold'
export type PersonaForm = { name: string; weight: string; useCase: string; maxModelTurns: string; maxToolCalls: string }
export type PersonaRequest = { name: string; weight: number; max_model_turns?: number; max_tool_calls?: number; use_case?: string }
export type Pacing = { start_spread_ms: number; think_min_ms: number; think_max_ms: number; sync_sessions: boolean }
export type FaultKind = 'latency' | 'error' | 'timeout'
export type FaultEffect = { kind: 'latency'; delay_ms: number } | { kind: 'error' } | { kind: 'timeout' }
export type Fault = { tool: string; first_stage: number; last_stage: number; percent: number; effect: FaultEffect }
export type FaultForm = { tool: string; firstStage: string; lastStage: string; percent: string; kind: FaultKind; delayMs: string }
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
  faults: FaultForm[]
  personas: PersonaForm[]
}
export type RunFormErrors = Partial<Record<keyof RunForm, string>>
export type RunRequest = {
  target: { url: string; openapi_url?: string }
  credentials: { bearer: string }
  use_case?: string
  hold_agents?: number
  pacing?: Pacing
  faults?: Fault[]
  task_mix?: PersonaRequest[]
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
  faults: [],
  personas: [],
})

export const emptyFault = (): FaultForm => ({ tool: '', firstStage: '1', lastStage: '1', percent: '', kind: 'error', delayMs: '' })

export const emptyPersona = (): PersonaForm => ({ name: '', weight: '', useCase: '', maxModelTurns: '', maxToolCalls: '' })

const wholeNumber = (text: string) => (/^\d+$/.test(text.trim()) ? Number(text.trim()) : null)

function personaBody(persona: PersonaForm, number: number): PersonaRequest | string {
  const label = `Persona ${number}`
  const nameBytes = byteLength(persona.name)
  if (persona.name.trim() === '' || nameBytes > MAX_PERSONA_NAME_BYTES) return `${label}: the name must be 1 to ${MAX_PERSONA_NAME_BYTES} bytes.`
  const weight = wholeNumber(persona.weight)
  if (weight == null || weight < 1) return `${label}: the weight must be a whole percent of at least 1.`
  const limit = (text: string) => (text.trim() === '' ? undefined : wholeNumber(text))
  const [maxModelTurns, maxToolCalls] = [limit(persona.maxModelTurns), limit(persona.maxToolCalls)]
  if (maxModelTurns === null || maxModelTurns === 0 || maxToolCalls === null || maxToolCalls === 0) return `${label}: turn and tool call limits must be whole numbers of at least 1, or blank.`
  const hasUseCase = persona.useCase.trim() !== ''
  if (hasUseCase && byteLength(persona.useCase) > MAX_USE_CASE_BYTES) return `${label}: the use case is over ${MAX_USE_CASE_BYTES.toLocaleString()} bytes.`
  return {
    name: persona.name,
    weight,
    ...(maxModelTurns != null ? { max_model_turns: maxModelTurns } : {}),
    ...(maxToolCalls != null ? { max_tool_calls: maxToolCalls } : {}),
    ...(hasUseCase ? { use_case: persona.useCase } : {}),
  }
}

export function taskMixBody(personas: PersonaForm[]): PersonaRequest[] | string {
  const bodies = personas.map((persona, index) => personaBody(persona, index + 1))
  const problem = bodies.find((body) => typeof body === 'string')
  if (problem) return problem
  const mix = bodies as PersonaRequest[]
  if (new Set(mix.map((persona) => persona.name)).size !== mix.length) return 'Persona names must be unique.'
  const total = mix.reduce((sum, persona) => sum + persona.weight, 0)
  if (total !== WEIGHT_TOTAL) return `Weights must sum to ${WEIGHT_TOTAL}%, not ${total}%.`
  return mix
}

export const describePersona = (persona: PersonaRequest) => `${persona.weight}% of agents${persona.use_case ? `: ${persona.use_case}` : ''}`

const describeEffect = (effect: FaultEffect) => (effect.kind === 'latency' ? `+${effect.delay_ms} ms` : effect.kind)

export const describeFault = (fault: Fault) =>
  `${fault.tool}: ${describeEffect(fault.effect)} on ${fault.percent}% of calls, ${fault.first_stage === fault.last_stage ? `stage ${fault.first_stage}` : `stages ${fault.first_stage} to ${fault.last_stage}`}`

const whole = (text: string, min: number, max = Infinity) => {
  const value = Number(text.trim())
  return /^\d+$/.test(text.trim()) && value >= min && value <= max ? value : null
}

function parseFault(form: FaultForm): Fault | string {
  const tool = form.tool.trim()
  const [first, last, percent] = [whole(form.firstStage, 1), whole(form.lastStage, 1), whole(form.percent, 1, 100)]
  if (!tool) return 'name a tool'
  if (first == null || last == null || first > last) return 'stages must be whole numbers from 1, first no later than last'
  if (percent == null) return 'share must be a whole percent from 1 to 100'
  const range = { tool, first_stage: first, last_stage: last, percent }
  if (form.kind !== 'latency') return { ...range, effect: { kind: form.kind } }
  const delay = whole(form.delayMs, 1)
  if (delay == null) return 'delay must be whole milliseconds, 1 or more'
  return { ...range, effect: { kind: 'latency', delay_ms: delay } }
}

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

  const mixed = form.personas.length > 0
  const hasUseCase = !mixed && form.useCase.trim() !== ''
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

  const parsed = form.faults.map(parseFault)
  const faults = parsed.filter((fault): fault is Fault => typeof fault !== 'string')
  const badFault = parsed.findIndex((fault) => typeof fault === 'string')
  if (badFault >= 0) errors.faults = `Fault ${badFault + 1}: ${parsed[badFault]}.`

  const taskMix = mixed ? taskMixBody(form.personas) : []
  if (typeof taskMix === 'string') errors.personas = taskMix

  if (Object.keys(errors).length > 0 || typeof taskMix === 'string') return { ok: false, errors }
  return {
    ok: true,
    body: {
      target: openapiUrl ? { url, openapi_url: openapiUrl } : { url },
      credentials: { bearer: form.bearer },
      ...(hasUseCase ? { use_case: form.useCase } : {}),
      ...(holds ? { hold_agents: holdAgents } : {}),
      ...(paced ? { pacing } : {}),
      ...(faults.length ? { faults } : {}),
      ...(mixed ? { task_mix: taskMix } : {}),
    },
  }
}
