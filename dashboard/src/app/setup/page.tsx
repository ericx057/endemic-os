import { readFileSync } from 'node:fs'
import path from 'node:path'
import { SetupMarkdown } from '@/components/setup-markdown'
import { withMcpUrl } from '@/lib/setup'

export default function SetupPage() {
  const clients = readFileSync(path.join(process.cwd(), '../docs/mcp-clients.md'), 'utf8')
  return (
    <article className="mx-auto max-w-[78ch] py-8">
      <SetupMarkdown markdown={withMcpUrl(clients, process.env.ENDEMIC_MCP_URL!)} />
    </article>
  )
}
