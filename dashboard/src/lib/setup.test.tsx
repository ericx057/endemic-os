import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { SetupMarkdown } from '@/components/setup-markdown'
import { MCP_PLACEHOLDER, codeText, withMcpUrl } from './setup'

describe('withMcpUrl', () => {
  it('replaces every placeholder with the configured URL', () => {
    const doc = `Use ${MCP_PLACEHOLDER}.\n\n\`\`\`sh\ncodex mcp add endemic --url ${MCP_PLACEHOLDER}\n\`\`\``
    const out = withMcpUrl(doc, 'https://runner.internal.test/mcp')
    expect(out).not.toContain(MCP_PLACEHOLDER)
    expect(out.match(/https:\/\/runner\.internal\.test\/mcp/g)).toHaveLength(2)
  })

  it('leaves the token as an environment variable reference', () => {
    const out = withMcpUrl('Bearer ${API_BEARER_TOKEN}', 'https://a.test/mcp')
    expect(out).toBe('Bearer ${API_BEARER_TOKEN}')
  })

  it('does not interpret $ patterns in the URL', () => {
    expect(withMcpUrl(MCP_PLACEHOLDER, "https://a.test/$&$'")).toBe("https://a.test/$&$'")
  })
})

describe('codeText', () => {
  it('copies the literal code, without the trailing newline markdown adds', () => {
    expect(codeText('{"a": 1}\n')).toBe('{"a": 1}')
    expect(codeText(['claude mcp add ', 'endemic', '\n'])).toBe('claude mcp add endemic')
  })

  it('ignores non-text children', () => {
    expect(codeText([null, 'x', undefined, false, 3])).toBe('x3')
  })
})

describe('SetupMarkdown', () => {
  it('renders hostile HTML as text, never as markup', () => {
    const html = renderToStaticMarkup(
      <SetupMarkdown markdown={'# Hi\n\n<script>alert(1)</script>\n\n<b onclick="x()">bold</b>\n\n[x](javascript:alert(1))'} />,
    )
    expect(html).not.toContain('<script')
    expect(html).not.toContain('<b onclick')
    expect(html).not.toContain('javascript:')
    expect(html).toContain('<h1')
  })

  it('gives every code block a copy button', () => {
    const html = renderToStaticMarkup(<SetupMarkdown markdown={'```sh\necho one\n```\n\n```json\n{}\n```'} />)
    expect(html.match(/aria-label="Copy code"/g)).toHaveLength(2)
  })
})

describe('SetupMarkdown tables', () => {
  it('renders pipe tables as tables', () => {
    const html = renderToStaticMarkup(<SetupMarkdown markdown={'| Tool | Arguments |\n| --- | --- |\n| `get_run` | `id` |'} />)
    expect(html).toContain('<table')
    expect(html).toContain('<code')
  })
})
