import { describe, expect, it } from 'vitest'
import { MAX_USE_CASE_BYTES, describeFault, describePacing, describePersona, emptyFault, emptyPersona, emptyRunForm, runAgainForm, byteLength, validateRunForm, type PersonaForm, type RunForm } from './runRequest'

const filled = (overrides: Partial<RunForm> = {}): RunForm => ({
  url: 'https://staging.example.com/mcp',
  openapiUrl: '',
  bearer: 'staging-token',
  useCase: '',
  goal: 'find_limit',
  holdAgents: '',
  startSpreadMs: '0',
  thinkMinMs: '0',
  thinkMaxMs: '0',
  syncSessions: false,
  faults: [],
  personas: [],
  ...overrides,
})

const errorsOf = (form: RunForm) => {
  const result = validateRunForm(form)
  return result.ok ? {} : result.errors
}

describe('validateRunForm', () => {
  it('builds an MCP request with only the required fields', () => {
    expect(validateRunForm(filled())).toEqual({
      ok: true,
      body: { target: { url: 'https://staging.example.com/mcp' }, credentials: { bearer: 'staging-token' } },
    })
  })

  it('adds openapi_url for an HTTP API and use_case when given', () => {
    const result = validateRunForm(
      filled({ url: 'https://api.example.com', openapiUrl: 'https://api.example.com/openapi.json', useCase: 'Shoppers check out.' }),
    )
    expect(result).toEqual({
      ok: true,
      body: {
        target: { url: 'https://api.example.com', openapi_url: 'https://api.example.com/openapi.json' },
        credentials: { bearer: 'staging-token' },
        use_case: 'Shoppers check out.',
      },
    })
  })

  it('trims URLs but never alters the token or use case text', () => {
    const result = validateRunForm(filled({ url: '  https://a.example.com/mcp ', useCase: ' Two  spaces. ' }))
    expect(result.ok && result.body.target.url).toBe('https://a.example.com/mcp')
    expect(result.ok && result.body.use_case).toBe(' Two  spaces. ')
  })

  it('omits a blank use case', () => {
    const result = validateRunForm(filled({ useCase: '   \n' }))
    expect(result.ok && 'use_case' in result.body).toBe(false)
  })

  it('sends hold_agents only for a held run', () => {
    const held = validateRunForm(filled({ goal: 'hold', holdAgents: ' 200 ' }))
    expect(held.ok && held.body.hold_agents).toBe(200)
    const ramp = validateRunForm(filled({ holdAgents: '200' }))
    expect(ramp.ok && 'hold_agents' in ramp.body).toBe(false)
  })

  it.each(['', '0', '2.5', '-3', 'ten'])('rejects %j agents for a held run', (holdAgents) => {
    expect(Object.keys(errorsOf(filled({ goal: 'hold', holdAgents })))).toEqual(['holdAgents'])
  })

  it('sends pacing only when some of it is set', () => {
    const paced = validateRunForm(filled({ startSpreadMs: '5000', thinkMinMs: '100', thinkMaxMs: '900', syncSessions: true }))
    expect(paced.ok && paced.body.pacing).toEqual({ start_spread_ms: 5000, think_min_ms: 100, think_max_ms: 900, sync_sessions: true })
    const plain = validateRunForm(filled())
    expect(plain.ok && 'pacing' in plain.body).toBe(false)
  })

  it.each([
    [{ startSpreadMs: '-1' }, 'startSpreadMs'],
    [{ thinkMinMs: '1.5' }, 'thinkMinMs'],
    [{ thinkMinMs: '500', thinkMaxMs: '100' }, 'thinkMaxMs'],
  ])('rejects pacing %j', (overrides, field) => {
    expect(Object.keys(errorsOf(filled(overrides)))).toEqual([field])
  })

  it('describes pacing in words', () => {
    expect(describePacing(null)).toBe('All agents start together, no pauses')
    expect(describePacing({ start_spread_ms: 5000, think_min_ms: 0, think_max_ms: 800, sync_sessions: true })).toBe(
      'Starts spread over 5000 ms, 0 to 800 ms between sessions, sessions in waves',
    )
  })

  it('reports every required field on an empty form', () => {
    expect(Object.keys(errorsOf(emptyRunForm())).sort()).toEqual(['bearer', 'url'])
  })

  it.each([
    [{ url: 'staging.example.com' }, 'url'],
    [{ url: 'http://staging.example.com/mcp' }, 'url'],
    [{ url: 'ftp://staging.example.com' }, 'url'],
    [{ openapiUrl: 'http://api.example.com/openapi.json' }, 'openapiUrl'],
    [{ openapiUrl: 'openapi.json' }, 'openapiUrl'],
    [{ bearer: '' }, 'bearer'],
    [{ bearer: '   ' }, 'bearer'],
    [{ bearer: ' token' }, 'bearer'],
    [{ bearer: 'tok\nen' }, 'bearer'],
    [{ useCase: 'x'.repeat(MAX_USE_CASE_BYTES + 1) }, 'useCase'],
    [{ useCase: 'é'.repeat(MAX_USE_CASE_BYTES / 2 + 1) }, 'useCase'],
  ])('rejects %j on %s', (overrides, field) => {
    expect(errorsOf(filled(overrides))).toHaveProperty(field)
  })

  it('accepts a use case of exactly the byte limit', () => {
    expect(validateRunForm(filled({ useCase: 'x'.repeat(MAX_USE_CASE_BYTES) })).ok).toBe(true)
  })

  it('never puts the token in an error message', () => {
    const errors = errorsOf(filled({ bearer: ' secret-value', url: 'nope' }))
    expect(JSON.stringify(errors)).not.toContain('secret-value')
  })
})

describe('form helpers', () => {
  it('starts empty', () => {
    expect(emptyRunForm()).toEqual(filled({ url: '', bearer: '' }))
  })

  it('run again keeps the target, use case and goal but requires the token again', () => {
    expect(runAgainForm(filled({ openapiUrl: 'https://a.example.com/o.json', useCase: 'Browse.', goal: 'hold', holdAgents: '200' }))).toEqual({
      url: 'https://staging.example.com/mcp',
      openapiUrl: 'https://a.example.com/o.json',
      bearer: '',
      useCase: 'Browse.',
      goal: 'hold',
      holdAgents: '200',
      startSpreadMs: '0',
      thinkMinMs: '0',
      thinkMaxMs: '0',
      syncSessions: false,
      faults: [],
      personas: [],
    })
  })

  it('counts UTF-8 bytes', () => {
    expect(byteLength('abc')).toBe(3)
    expect(byteLength('é')).toBe(2)
  })

  it('sends faults only when declared, trimmed and typed', () => {
    const faults = [
      { ...emptyFault(), tool: ' search ', percent: '20', lastStage: '2' },
      { ...emptyFault(), tool: 'refund', percent: '100', kind: 'latency' as const, delayMs: '400' },
    ]
    const result = validateRunForm(filled({ faults }))
    expect(result.ok && result.body.faults).toEqual([
      { tool: 'search', first_stage: 1, last_stage: 2, percent: 20, effect: { kind: 'error' } },
      { tool: 'refund', first_stage: 1, last_stage: 1, percent: 100, effect: { kind: 'latency', delay_ms: 400 } },
    ])
    const none = validateRunForm(filled())
    expect(none.ok && 'faults' in none.body).toBe(false)
  })

  it.each([
    [{ tool: '' }, 'Fault 2: name a tool.'],
    [{ firstStage: '3', lastStage: '2' }, 'Fault 2: stages must be whole numbers from 1, first no later than last.'],
    [{ firstStage: '0' }, 'Fault 2: stages must be whole numbers from 1, first no later than last.'],
    [{ percent: '0' }, 'Fault 2: share must be a whole percent from 1 to 100.'],
    [{ percent: '101' }, 'Fault 2: share must be a whole percent from 1 to 100.'],
    [{ kind: 'latency' as const, delayMs: '0' }, 'Fault 2: delay must be whole milliseconds, 1 or more.'],
  ])('rejects fault %j', (overrides, message) => {
    const valid = { ...emptyFault(), tool: 'search', percent: '10' }
    expect(errorsOf(filled({ faults: [valid, { ...valid, ...overrides }] })).faults).toBe(message)
  })

  it('describes a fault in words', () => {
    expect(describeFault({ tool: 'search', first_stage: 2, last_stage: 3, percent: 10, effect: { kind: 'latency', delay_ms: 250 } })).toBe(
      'search: +250 ms on 10% of calls, stages 2 to 3',
    )
    expect(describeFault({ tool: 'refund', first_stage: 1, last_stage: 1, percent: 5, effect: { kind: 'timeout' } })).toBe('refund: timeout on 5% of calls, stage 1')
  })
})

const persona = (overrides: Partial<PersonaForm> = {}): PersonaForm => ({ ...emptyPersona(), name: 'reads', weight: '100', ...overrides })

describe('task mix', () => {
  it('sends task_mix instead of the run-wide use case', () => {
    const result = validateRunForm(
      filled({
        useCase: 'Ignored.',
        personas: [persona({ weight: '70', useCase: 'Browse.', maxToolCalls: '4' }), persona({ name: 'writes', weight: '30' })],
      }),
    )
    expect(result).toEqual({
      ok: true,
      body: {
        target: { url: 'https://staging.example.com/mcp' },
        credentials: { bearer: 'staging-token' },
        task_mix: [
          { name: 'reads', weight: 70, max_tool_calls: 4, use_case: 'Browse.' },
          { name: 'writes', weight: 30 },
        ],
      },
    })
  })

  it.each([
    [[persona({ weight: '60' })], 'Weights must sum to 100%, not 60%.'],
    [[persona({ weight: '50' }), persona({ weight: '50' })], 'Persona names must be unique.'],
    [[persona({ name: ' ' })], 'Persona 1: the name must be 1 to 64 bytes.'],
    [[persona({ weight: '0' })], 'Persona 1: the weight must be a whole percent of at least 1.'],
    [[persona({ maxModelTurns: '0' })], 'Persona 1: turn and tool call limits must be whole numbers of at least 1, or blank.'],
    [[persona({ useCase: 'x'.repeat(MAX_USE_CASE_BYTES + 1) })], 'Persona 1: the use case is over 4,096 bytes.'],
  ])('rejects %j', (personas, message) => {
    expect(errorsOf(filled({ personas }))).toEqual({ personas: message })
  })

  it('describes each persona in words', () => {
    expect(describePersona({ name: 'reads', weight: 70, use_case: 'Browse.' })).toBe('70% of agents: Browse.')
    expect(describePersona({ name: 'writes', weight: 30 })).toBe('30% of agents')
  })
})
