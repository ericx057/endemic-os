import { describe, expect, it } from 'vitest'
import { MAX_USE_CASE_BYTES, describePacing, emptyRunForm, runAgainForm, byteLength, validateRunForm, type RunForm } from './runRequest'

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
    })
  })

  it('counts UTF-8 bytes', () => {
    expect(byteLength('abc')).toBe(3)
    expect(byteLength('é')).toBe(2)
  })
})
