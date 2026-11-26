'use client'
import { isValidElement, useState, type ComponentProps, type ReactNode } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Check, Copy } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { codeText } from '@/lib/setup'

function CodeBlock({ children }: { children?: ReactNode }) {
  const [copied, setCopied] = useState(false)
  const code = isValidElement<{ children?: ReactNode }>(children) ? children.props.children : children
  const copy = () =>
    navigator.clipboard.writeText(codeText(code)).then(
      () => setCopied(true),
      () => setCopied(false),
    )
  return (
    <div className="group relative my-4">
      <pre className="overflow-x-auto rounded-md border border-white/15 bg-white/[0.04] p-4 pr-12 text-[13px] leading-relaxed">
        {children}
      </pre>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Copy code"
        className="absolute top-2 right-2 border border-white/15 bg-black text-white/70 hover:bg-black hover:text-white"
        onClick={copy}
        onBlur={() => setCopied(false)}
      >
        {copied ? <Check aria-hidden /> : <Copy aria-hidden />}
      </Button>
      <span className="sr-only" aria-live="polite">
        {copied ? 'Copied' : ''}
      </span>
    </div>
  )
}

const components: ComponentProps<typeof Markdown>['components'] = {
  pre: CodeBlock,
  h1: (props) => <h1 className="mt-2 mb-6 text-3xl font-semibold tracking-tight" {...props} />,
  h2: (props) => <h2 className="mt-12 mb-3 border-t border-white/15 pt-6 text-xl font-semibold" {...props} />,
  h3: (props) => <h3 className="mt-6 mb-2 font-semibold" {...props} />,
  p: (props) => <p className="my-3 leading-7 text-white/80" {...props} />,
  ol: (props) => <ol className="my-3 list-decimal space-y-1 pl-6 text-white/80" {...props} />,
  ul: (props) => <ul className="my-3 list-disc space-y-1 pl-6 text-white/80" {...props} />,
  a: (props) => <a className="text-white underline underline-offset-4 hover:no-underline" {...props} />,
  code: (props) => <code className="rounded bg-white/10 px-1 py-0.5 font-mono text-[0.9em] [pre_&]:bg-transparent [pre_&]:p-0" {...props} />,
  blockquote: (props) => <blockquote className="my-4 border-l-2 border-white/40 pl-4 text-white/80" {...props} />,
  table: (props) => (
    <div className="my-4 overflow-x-auto">
      <table className="w-full text-left text-sm" {...props} />
    </div>
  ),
  th: (props) => <th className="border-b border-white/30 py-2 pr-4 font-medium" {...props} />,
  td: (props) => <td className="border-b border-white/10 py-2 pr-4 align-top text-white/80" {...props} />,
}

export function SetupMarkdown({ markdown }: { markdown: string }) {
  return (
    <Markdown skipHtml remarkPlugins={[remarkGfm]} components={components}>
      {markdown}
    </Markdown>
  )
}
