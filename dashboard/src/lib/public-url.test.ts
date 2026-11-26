import { describe, expect, it } from 'vitest'
import { mcpUrl } from './public-url'

describe('mcpUrl', () => {
  it('accepts https only', () => {
    expect(mcpUrl('https://endemic.example.com/mcp')).toBe('https://endemic.example.com/mcp')
    expect(() => mcpUrl('mailto:support@example.com')).toThrow('ENDEMIC_MCP_URL must use https:')
    expect(() => mcpUrl(undefined)).toThrow('ENDEMIC_MCP_URL is not set')
    expect(() => mcpUrl('endemic.example.com')).toThrow('not a valid URL')
  })
})
