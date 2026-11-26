export function publicUrl(name: string, value: string | undefined, protocols: string[]): string {
  if (!value) throw new Error(`${name} is not set. Set it before running the dashboard.`)
  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw new Error(`${name} is not a valid URL: ${value}`)
  }
  if (!protocols.includes(url.protocol)) {
    throw new Error(`${name} must use ${protocols.join(' or ')}; got ${url.protocol}`)
  }
  return value
}

export const mcpUrl = (value: string | undefined) => publicUrl('ENDEMIC_MCP_URL', value, ['https:'])
