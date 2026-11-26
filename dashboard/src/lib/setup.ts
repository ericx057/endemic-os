import type { ReactNode } from 'react'

export const MCP_PLACEHOLDER = 'https://endemic.example.com/mcp'

/** Puts the deployment's MCP URL into docs/mcp-clients.md snippets. */
export const withMcpUrl = (markdown: string, url: string) => markdown.split(MCP_PLACEHOLDER).join(url)

/** The text a code block's copy button writes: its literal children, minus markdown's trailing newline. */
export function codeText(children: ReactNode): string {
  const flatten = (node: ReactNode): string =>
    typeof node === 'string' || typeof node === 'number'
      ? String(node)
      : Array.isArray(node)
        ? node.map(flatten).join('')
        : ''
  return flatten(children).replace(/\n$/, '')
}
