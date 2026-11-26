export const AXIS = { stroke: '#525252', tick: { fill: '#a3a3a3', fontSize: 11 }, tickLine: false } as const
export const TOOLTIP_CURSOR = { stroke: '#737373', strokeWidth: 1 }

export const ms = (value: number | null | undefined) =>
  value == null ? 'n/a' : value >= 1000 ? `${(value / 1000).toFixed(value >= 10000 ? 0 : 1)} s` : `${value} ms`
export const pct = (value: number | null | undefined) => (value == null ? 'n/a' : `${value.toFixed(value < 10 ? 1 : 0)}%`)
