import { describe, expect, it } from 'vitest'
import {
  LAYOUT_KEY,
  PANELS,
  defaultState,
  hidePanel,
  loadLayout,
  parseState,
  resetLayout,
  saveLayout,
  showPanel,
  visibleLayouts,
  withLayouts,
  type LayoutState,
} from './layout'

const memory = (initial: Record<string, string> = {}) => {
  const data = new Map(Object.entries(initial))
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
  }
}

const ids = PANELS.map((panel) => panel.id)

describe('defaultState', () => {
  it('places every panel at both breakpoints with nothing hidden', () => {
    const state = defaultState()
    expect(state.version).toBe(2)
    expect(state.hidden).toEqual([])
    expect(state.layouts.lg.map((item) => item.i).sort()).toEqual([...ids].sort())
    expect(state.layouts.sm.map((item) => item.i).sort()).toEqual([...ids].sort())
  })

  it('uses a versioned key', () => {
    expect(LAYOUT_KEY).toBe('endemic.dashboard.layout.v2')
  })
})

describe('parseState', () => {
  const valid = (): LayoutState => ({
    ...defaultState(),
    hidden: ['bursts'],
    layouts: {
      ...defaultState().layouts,
      lg: defaultState().layouts.lg.map((item) => (item.i === 'latency' ? { ...item, x: 6 } : item)),
    },
  })

  it('round-trips a valid state', () => {
    expect(parseState(JSON.stringify(valid()))).toEqual(valid())
  })

  it.each([
    ['missing', null],
    ['not JSON', '{nope'],
    ['an older version', JSON.stringify({ ...defaultState(), version: 1 })],
    ['an unknown panel', JSON.stringify({ ...defaultState(), hidden: ['mystery'] })],
    [
      'a negative position',
      JSON.stringify({
        ...defaultState(),
        layouts: { ...defaultState().layouts, lg: [{ i: 'latency', x: -1, y: 0, w: 6, h: 8 }] },
      }),
    ],
    [
      'a duplicate panel',
      JSON.stringify({
        ...defaultState(),
        layouts: {
          ...defaultState().layouts,
          lg: [
            { i: 'latency', x: 0, y: 0, w: 6, h: 8 },
            { i: 'latency', x: 6, y: 0, w: 6, h: 8 },
          ],
        },
      }),
    ],
    ['a missing breakpoint', JSON.stringify({ version: 2, hidden: [], layouts: { lg: [] } })],
  ])('falls back to the default for %s', (_, raw) => {
    expect(parseState(raw)).toEqual(defaultState())
  })

  it('drops stored minimums and extra fields so only positions persist', () => {
    const lg = defaultState().layouts.lg.map((item) => ({ ...item, minW: 99, static: true }))
    const parsed = parseState(JSON.stringify({ ...defaultState(), layouts: { ...defaultState().layouts, lg } }))
    expect(parsed.layouts.lg).toEqual(defaultState().layouts.lg)
  })
})

describe('storage', () => {
  it('saves under the versioned key and loads it back', () => {
    const storage = memory()
    const state = hidePanel(defaultState(), 'bursts')
    saveLayout(storage, state)
    expect(storage.data.has(LAYOUT_KEY)).toBe(true)
    expect(loadLayout(storage)).toEqual(state)
  })

  it('ignores invalid stored values', () => {
    expect(loadLayout(memory({ [LAYOUT_KEY]: '[]' }))).toEqual(defaultState())
  })

  it('survives storage that throws', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked')
      },
      setItem: () => {
        throw new Error('blocked')
      },
      removeItem: () => {
        throw new Error('blocked')
      },
    }
    expect(loadLayout(broken)).toEqual(defaultState())
    expect(() => saveLayout(broken, defaultState())).not.toThrow()
    expect(resetLayout(broken)).toEqual(defaultState())
  })

  it('reset clears the stored layout and restores every panel', () => {
    const storage = memory()
    saveLayout(storage, hidePanel(defaultState(), 'bursts'))
    expect(resetLayout(storage)).toEqual(defaultState())
    expect(storage.data.has(LAYOUT_KEY)).toBe(false)
  })
})

describe('show and hide', () => {
  it('hides a panel but remembers where it was', () => {
    const moved = withLayouts(defaultState(), {
      lg: [{ i: 'bursts', x: 0, y: 40, w: 4, h: 9 }],
      sm: [],
    })
    const hidden = hidePanel(moved, 'bursts')
    expect(hidden.hidden).toEqual(['bursts'])
    expect(visibleLayouts(hidden).lg.some((item) => item.i === 'bursts')).toBe(false)
    const shown = showPanel(hidden, 'bursts')
    expect(shown.hidden).toEqual([])
    expect(visibleLayouts(shown).lg.find((item) => item.i === 'bursts')).toMatchObject({ x: 0, y: 40, w: 4, h: 9 })
  })

  it('hiding twice is a no-op', () => {
    const once = hidePanel(defaultState(), 'bursts')
    expect(hidePanel(once, 'bursts')).toEqual(once)
  })

  it('shows a panel with no remembered slot in its default slot', () => {
    const forgotten: LayoutState = {
      ...defaultState(),
      hidden: ['traces'],
      layouts: {
        lg: defaultState().layouts.lg.filter((item) => item.i !== 'traces'),
        sm: defaultState().layouts.sm.filter((item) => item.i !== 'traces'),
      },
    }
    const shown = visibleLayouts(showPanel(forgotten, 'traces'))
    const slot = defaultState().layouts.lg.find((item) => item.i === 'traces')
    expect(shown.lg.find((item) => item.i === 'traces')).toMatchObject(slot!)
  })

  it('layout changes from the grid keep hidden panels remembered', () => {
    const hidden = hidePanel(defaultState(), 'bursts')
    const burstsSlot = hidden.layouts.lg.find((item) => item.i === 'bursts')
    const next = withLayouts(hidden, { lg: [{ i: 'latency', x: 6, y: 0, w: 6, h: 8 }], sm: [] })
    expect(next.layouts.lg.find((item) => item.i === 'bursts')).toEqual(burstsSlot)
    expect(next.layouts.lg.find((item) => item.i === 'latency')).toEqual({ i: 'latency', x: 6, y: 0, w: 6, h: 8 })
  })

  it('visible layouts carry minimum sizes so labels never clip', () => {
    const item = visibleLayouts(defaultState()).lg.find((entry) => entry.i === 'latency')
    expect(item?.minW).toBeGreaterThan(0)
    expect(item?.minH).toBeGreaterThan(0)
  })

  it('does not mutate the state it is given', () => {
    const state = defaultState()
    const frozen = JSON.stringify(state)
    hidePanel(state, 'bursts')
    showPanel(hidePanel(state, 'bursts'), 'bursts')
    withLayouts(state, { lg: [{ i: 'latency', x: 6, y: 0, w: 6, h: 8 }], sm: [] })
    expect(JSON.stringify(state)).toBe(frozen)
  })
})
