import { LayoutGrid } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { PANELS, hidePanel, showPanel, type LayoutState } from '@/lib/layout'

export function PanelsMenu({
  state,
  onChange,
  onReset,
  defaultOpen,
}: {
  state: LayoutState
  onChange: (state: LayoutState) => void
  onReset: () => void
  defaultOpen?: boolean
}) {
  return (
    <Popover defaultOpen={defaultOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm">
          <LayoutGrid aria-hidden />
          Panels
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64 border-white/25 bg-black">
        <fieldset className="grid gap-2.5">
          <legend className="mb-2 text-sm font-medium">Show panels</legend>
          {PANELS.map((panel) => {
            const shown = !state.hidden.includes(panel.id)
            return (
              <div key={panel.id} className="flex items-center gap-2">
                <Checkbox
                  id={`show-${panel.id}`}
                  checked={shown}
                  onCheckedChange={(on) => onChange(on ? showPanel(state, panel.id) : hidePanel(state, panel.id))}
                />
                <Label htmlFor={`show-${panel.id}`} className="font-normal">
                  {panel.title}
                </Label>
              </div>
            )
          })}
        </fieldset>
        <Button variant="ghost" size="sm" className="mt-3 w-full justify-start" onClick={onReset}>
          Reset layout
        </Button>
      </PopoverContent>
    </Popover>
  )
}
