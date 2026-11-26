import type { ReactNode } from 'react'
import { Responsive, useContainerWidth, type Layout } from 'react-grid-layout'
import { GripVertical, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { BREAKPOINTS, COLS, PANELS, visibleLayouts, withLayouts, type LayoutState, type PanelId } from '@/lib/layout'

const ROW_HEIGHT = 30

function Panel({ id, title, onHide, children }: { id: PanelId; title: string; onHide: () => void; children: ReactNode }) {
  return (
    <section aria-labelledby={`panel-${id}`} className="flex h-full flex-col overflow-hidden rounded-md border border-border bg-black">
      <header className="flex items-center border-b border-border">
        {/* Only this strip starts a drag, so chart hover and table scrolling keep working. */}
        <div className="panel-handle flex min-w-0 flex-1 cursor-grab items-center gap-1.5 px-2 py-1.5 active:cursor-grabbing">
          <GripVertical aria-hidden className="size-3.5 shrink-0 text-white/35" />
          <h2 id={`panel-${id}`} className="truncate text-sm font-medium">
            {title}
          </h2>
        </div>
        <Button variant="ghost" size="icon-xs" aria-label={`Hide ${title}`} className="mr-1 text-white/45 hover:text-white" onClick={onHide}>
          <X aria-hidden />
        </Button>
      </header>
      <div className="min-h-0 flex-1 p-3">{children}</div>
    </section>
  )
}

export function PanelGrid({
  state,
  onChange,
  onHide,
  render,
}: {
  state: LayoutState
  onChange: (state: LayoutState) => void
  onHide: (id: PanelId) => void
  render: (id: PanelId) => ReactNode
}) {
  const { width, containerRef, mounted } = useContainerWidth()
  const visible = PANELS.filter((panel) => !state.hidden.includes(panel.id))
  return (
    <div ref={containerRef}>
      {visible.length === 0 && <p className="py-12 text-center text-sm text-muted-foreground">Every panel is hidden. Turn them back on from Panels.</p>}
      {mounted && (
        <Responsive
          width={width}
          layouts={visibleLayouts(state)}
          breakpoints={BREAKPOINTS}
          cols={COLS}
          rowHeight={ROW_HEIGHT}
          margin={[12, 12]}
          containerPadding={[0, 0]}
          dragConfig={{ handle: '.panel-handle' }}
          resizeConfig={{ handles: ['se'] }}
          onLayoutChange={(_: Layout, layouts) => onChange(withLayouts(state, layouts))}
        >
          {visible.map((panel) => (
            <div key={panel.id}>
              <Panel id={panel.id} title={panel.title} onHide={() => onHide(panel.id)}>
                {render(panel.id)}
              </Panel>
            </div>
          ))}
        </Responsive>
      )}
    </div>
  )
}
