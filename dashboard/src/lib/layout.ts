
export const LAYOUT_KEY = 'endemic.dashboard.layout.v2'
const VERSION = 2

export const PANELS = [
  { id: 'latency', title: 'Latency p95 by agents', lg: { x: 0, y: 0, w: 6, h: 11 }, min: { w: 4, h: 8 } },
  { id: 'errors', title: 'Error share by source', lg: { x: 6, y: 0, w: 6, h: 11 }, min: { w: 4, h: 8 } },
  { id: 'hotspots', title: 'What agents visit most', lg: { x: 0, y: 11, w: 12, h: 10 }, min: { w: 4, h: 8 } },
  { id: 'bursts', title: 'Tool-call bursts', lg: { x: 0, y: 21, w: 12, h: 10 }, min: { w: 4, h: 7 } },
  { id: 'traces', title: 'Session traces', lg: { x: 0, y: 31, w: 12, h: 12 }, min: { w: 5, h: 8 } },
  { id: 'stages', title: 'Every stage', lg: { x: 0, y: 43, w: 12, h: 12 }, min: { w: 6, h: 6 } },
] as const

export type PanelId = (typeof PANELS)[number]['id']
export type Breakpoint = 'lg' | 'sm'
export const BREAKPOINTS: Record<Breakpoint, number> = { lg: 900, sm: 0 }
export const COLS: Record<Breakpoint, number> = { lg: 12, sm: 1 }

export type Item = { i: string; x: number; y: number; w: number; h: number }
export type Layouts = Record<Breakpoint, Item[]>
export type LayoutState = { version: typeof VERSION; hidden: PanelId[]; layouts: Layouts }
type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem' | 'removeItem'>

const ids: readonly string[] = PANELS.map((panel) => panel.id)
const isPanel = (value: unknown): value is PanelId => typeof value === 'string' && ids.includes(value)

const defaultItem = (id: string, breakpoint: Breakpoint): Item => {
  const index = ids.indexOf(id)
  const panel = PANELS[index]
  if (breakpoint === 'lg') return { i: id, ...panel.lg }
  const y = PANELS.slice(0, index).reduce((sum, p) => sum + p.lg.h, 0)
  return { i: id, x: 0, y, w: 1, h: panel.lg.h }
}

export const defaultState = (): LayoutState => ({
  version: VERSION,
  hidden: [],
  layouts: { lg: ids.map((id) => defaultItem(id, 'lg')), sm: ids.map((id) => defaultItem(id, 'sm')) },
})

const count = (value: unknown, min: number) => typeof value === 'number' && Number.isInteger(value) && value >= min

const parseItems = (value: unknown): Item[] | null => {
  if (!Array.isArray(value)) return null
  const items = value.map((item) =>
    item && isPanel(item.i) && count(item.x, 0) && count(item.y, 0) && count(item.w, 1) && count(item.h, 1)
      ? { i: item.i, x: item.x, y: item.y, w: item.w, h: item.h }
      : null,
  )
  const valid = items.filter((item): item is Item => item !== null)
  const unique = new Set(valid.map((item) => item.i)).size === valid.length
  return valid.length === items.length && unique ? valid : null
}

export function parseState(raw: string | null): LayoutState {
  try {
    const value = raw == null ? null : JSON.parse(raw)
    const lg = parseItems(value?.layouts?.lg)
    const sm = parseItems(value?.layouts?.sm)
    const hidden = Array.isArray(value?.hidden) && value.hidden.every(isPanel) ? [...new Set<PanelId>(value.hidden)] : null
    if (value?.version !== VERSION || !lg || !sm || !hidden) return defaultState()
    return { version: VERSION, hidden, layouts: { lg, sm } }
  } catch {
    return defaultState()
  }
}

export function loadLayout(storage: Storage): LayoutState {
  try {
    return parseState(storage.getItem(LAYOUT_KEY))
  } catch {
    return defaultState()
  }
}

export function saveLayout(storage: Storage, state: LayoutState): void {
  try {
    storage.setItem(LAYOUT_KEY, JSON.stringify(state))
  } catch {
  }
}

export function resetLayout(storage: Storage): LayoutState {
  try {
    storage.removeItem(LAYOUT_KEY)
  } catch {
  }
  return defaultState()
}

const merge = (previous: Item[], next: readonly Item[]): Item[] => {
  const updated = new Map(next.filter((item) => isPanel(item.i)).map((item) => [item.i, item]))
  const kept = previous.filter((item) => !updated.has(item.i))
  return [...kept, ...[...updated.values()].map(({ i, x, y, w, h }) => ({ i, x, y, w, h }))]
}

export function withLayouts(state: LayoutState, next: Partial<Record<Breakpoint, readonly Item[]>>): LayoutState {
  return {
    ...state,
    layouts: { lg: merge(state.layouts.lg, next.lg ?? []), sm: merge(state.layouts.sm, next.sm ?? []) },
  }
}

export const hidePanel = (state: LayoutState, id: PanelId): LayoutState =>
  state.hidden.includes(id) ? state : { ...state, hidden: [...state.hidden, id] }

export const showPanel = (state: LayoutState, id: PanelId): LayoutState => ({
  ...state,
  hidden: state.hidden.filter((hidden) => hidden !== id),
})

export function visibleLayouts(state: LayoutState) {
  const forBreakpoint = (breakpoint: Breakpoint) =>
    PANELS.filter((panel) => !state.hidden.includes(panel.id)).map((panel) => {
      const item = state.layouts[breakpoint].find((entry) => entry.i === panel.id) ?? defaultItem(panel.id, breakpoint)
      const minW = Math.min(panel.min.w, COLS[breakpoint])
      return { ...item, w: Math.max(item.w, minW), h: Math.max(item.h, panel.min.h), minW, minH: panel.min.h }
    })
  return { lg: forBreakpoint('lg'), sm: forBreakpoint('sm') }
}
