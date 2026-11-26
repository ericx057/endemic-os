import type { NextConfig } from 'next'
import { mcpUrl } from './src/lib/public-url'

mcpUrl(process.env.ENDEMIC_MCP_URL)
if (process.env.ENDEMIC_API_URL && !process.env.ENDEMIC_API_TOKEN) throw new Error('ENDEMIC_API_TOKEN is not set. It is required when ENDEMIC_API_URL is set.')

const config: NextConfig = {
  env: { ENDEMIC_LIVE: String(Boolean(process.env.ENDEMIC_API_URL)) },
}

export default config
