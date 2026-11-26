import type { ReactNode } from 'react'

export type Shape = 'circle' | 'square' | 'triangle'
export type Series = { key: string; label: string; dash?: string; opacity: number; shape: Shape }

export function Marker({ cx, cy, shape, opacity = 1, size = 4 }: { cx?: number; cy?: number; shape: Shape; opacity?: number; size?: number }) {
  if (cx == null || cy == null || Number.isNaN(cy)) return null
  const common = { fill: '#ffffff', fillOpacity: opacity, stroke: '#000000', strokeWidth: 2 }
  if (shape === 'square') return <rect x={cx - size} y={cy - size} width={size * 2} height={size * 2} {...common} />
  if (shape === 'triangle') {
    const s = size + 1
    return <polygon points={`${cx},${cy - s} ${cx + s},${cy + s} ${cx - s},${cy + s}`} {...common} />
  }
  return <circle cx={cx} cy={cy} r={size} {...common} />
}

export function LineSwatch({ series }: { series: Series }) {
  return (
    <svg width="34" height="12" aria-hidden className="shrink-0">
      <line x1="1" x2="33" y1="6" y2="6" stroke="#fff" strokeOpacity={series.opacity} strokeWidth="2" strokeDasharray={series.dash} />
      <Marker cx={17} cy={6} shape={series.shape} opacity={series.opacity} size={3.5} />
    </svg>
  )
}

export function Legend({ items }: { items: { key: string; label: string; swatch: ReactNode }[] }) {
  return (
    <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground" aria-label="Legend">
      {items.map((item) => (
        <li key={item.key} className="flex items-center gap-1.5">
          {item.swatch}
          {item.label}
        </li>
      ))}
    </ul>
  )
}

export function TooltipBox({ title, rows }: { title: ReactNode; rows: { label: string; value: ReactNode }[] }) {
  return (
    <div className="max-w-72 rounded-md border border-white/25 bg-black px-3 py-2 text-xs shadow-none">
      <div className="mb-1 font-medium text-white">{title}</div>
      <dl className="grid grid-cols-[auto_auto] gap-x-4 gap-y-0.5">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <dt className="text-muted-foreground">{row.label}</dt>
            <dd className="num text-right break-all text-white">{row.value}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}
