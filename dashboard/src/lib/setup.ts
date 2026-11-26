import type { ReactNode } from 'react'

export const MCP_PLACEHOLDER = 'https://endemic.example.com/mcp'

export const withMcpUrl = (markdown: string, url: string) => markdown.split(MCP_PLACEHOLDER).join(url)

export function codeText(children: ReactNode): string {
  const flatten = (node: ReactNode): string =>
    typeof node === 'string' || typeof node === 'number'
      ? String(node)
      : Array.isArray(node)
        ? node.map(flatten).join('')
        : ''
  return flatten(children).replace(/\n$/, '')
}
