import { explanationLines } from '@/lib/explanation'
import type { Explanation } from '@/lib/report'

export function FailureExplanation({ explanation }: { explanation: Explanation }) {
  const lines = explanationLines(explanation)
  return (
    <section aria-labelledby="why-it-broke" className="grid gap-3 rounded-md border border-white/60 p-4">
      <h2 id="why-it-broke" className="text-sm font-medium text-muted-foreground">
        Why it broke
      </h2>
      <p className="display text-2xl font-bold tracking-tight">{lines.headline}</p>
      <p className="text-sm text-muted-foreground">{lines.baseline}</p>
      <ul className="grid gap-1 text-sm">
        {lines.rises.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      {lines.traces.length > 0 && (
        <div className="text-sm">
          <h3 className="mb-1 text-muted-foreground">Sessions that called these tools</h3>
          <ul className="flex flex-wrap gap-2">
            {lines.traces.map((trace) => (
              <li key={trace} className="num rounded border border-border px-2 py-0.5">
                {trace}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}
