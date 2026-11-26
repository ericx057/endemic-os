import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Textarea } from '@/components/ui/textarea'
import { MAX_USE_CASE_BYTES, byteLength, describePacing, validateRunForm, type RunForm, type RunFormErrors, type RunGoal, type RunRequest } from '@/lib/runRequest'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  initial: RunForm
  initialStep?: 'edit' | 'review'
  /** Absent without a live runner: the review step then explains why Start is unavailable. */
  onStart?: (body: RunRequest, form: RunForm) => Promise<void>
}

function TextField({ id, label, hint, error, ...input }: { id: string; label: string; hint: string; error?: string } & React.ComponentProps<'input'>) {
  return (
    <Field data-invalid={Boolean(error)}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input id={id} aria-invalid={Boolean(error)} {...input} />
      <FieldDescription>{hint}</FieldDescription>
      {error && <FieldError>{error}</FieldError>}
    </Field>
  )
}

const GOALS: { value: RunGoal; label: string; hint: string }[] = [
  { value: 'find_limit', label: 'Find the breaking point', hint: 'Doubles agents each stage until one fails or the deployment ceiling is reached.' },
  { value: 'hold', label: 'Hold a set number of agents', hint: 'One stage at exactly that many agents: does the target hold them?' },
]

function GoalField({ form, error, set }: { form: RunForm; error?: string; set: (patch: Partial<RunForm>) => void }) {
  return (
    <FieldSet>
      <FieldLegend variant="label">Goal</FieldLegend>
      {GOALS.map((goal) => (
        <Field key={goal.value} orientation="horizontal" className="items-start">
          <input id={`goal-${goal.value}`} type="radio" name="goal" className="mt-1 accent-white" checked={form.goal === goal.value} onChange={() => set({ goal: goal.value })} />
          <div className="grid gap-1">
            <FieldLabel htmlFor={`goal-${goal.value}`}>{goal.label}</FieldLabel>
            <FieldDescription>{goal.hint}</FieldDescription>
          </div>
        </Field>
      ))}
      {form.goal === 'hold' && (
        <TextField id="hold-agents" label="Agents" hint="Concurrent agents for the stage, up to the deployment ceiling." type="number" inputMode="numeric" min={1} step={1} value={form.holdAgents} error={error} onChange={(e) => set({ holdAgents: e.target.value })} />
      )}
    </FieldSet>
  )
}

function PacingField({ form, errors, set }: { form: RunForm; errors: RunFormErrors; set: (patch: Partial<RunForm>) => void }) {
  const ms = { type: 'number', inputMode: 'numeric', min: 0, step: 1 } as const
  return (
    <FieldSet>
      <FieldLegend variant="label">Pacing</FieldLegend>
      <FieldDescription>All 0 starts every agent together and begins each session as soon as the last ends.</FieldDescription>
      <TextField id="start-spread" label="Start spread (ms)" hint="Each agent starts at a random time within this window." {...ms} value={form.startSpreadMs} error={errors.startSpreadMs} onChange={(e) => set({ startSpreadMs: e.target.value })} />
      <div className="grid grid-cols-2 gap-4">
        <TextField id="think-min" label="Pause min (ms)" hint="Shortest pause between sessions." {...ms} value={form.thinkMinMs} error={errors.thinkMinMs} onChange={(e) => set({ thinkMinMs: e.target.value })} />
        <TextField id="think-max" label="Pause max (ms)" hint="Longest pause between sessions." {...ms} value={form.thinkMaxMs} error={errors.thinkMaxMs} onChange={(e) => set({ thinkMaxMs: e.target.value })} />
      </div>
      <Field orientation="horizontal" className="items-start">
        <input id="sync-sessions" type="checkbox" className="mt-1 accent-white" checked={form.syncSessions} onChange={(e) => set({ syncSessions: e.target.checked })} />
        <div className="grid gap-1">
          <FieldLabel htmlFor="sync-sessions">Sessions in waves</FieldLabel>
          <FieldDescription>Agents begin every session together, waiting for the slowest.</FieldDescription>
        </div>
      </Field>
    </FieldSet>
  )
}

function Review({ body }: { body: RunRequest }) {
  return (
    <dl className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
      <dt className="text-muted-foreground">Target</dt>
      <dd className="break-all">{body.target.url}</dd>
      <dt className="text-muted-foreground">Kind</dt>
      <dd>{body.target.openapi_url ? `HTTP API, spec at ${body.target.openapi_url}` : 'MCP endpoint'}</dd>
      <dt className="text-muted-foreground">Staging token</dt>
      <dd>Provided. Sent once with this request; the page does not keep it.</dd>
      <dt className="text-muted-foreground">Goal</dt>
      <dd className="num">{body.hold_agents != null ? `Hold ${body.hold_agents} agents for one stage.` : 'Find the breaking point: double agents each stage until one fails or the ceiling is reached.'}</dd>
      <dt className="text-muted-foreground">Pacing</dt>
      <dd className="num">{describePacing(body.pacing)}</dd>
      <dt className="text-muted-foreground">Use case</dt>
      <dd className="whitespace-pre-wrap">{body.use_case ?? 'None. Tasks will come from the discovered tools alone.'}</dd>
      <dt className="text-muted-foreground">Endemic decides</dt>
      <dd>Tools, tasks, model settings and parallel calls.</dd>
    </dl>
  )
}

export function NewRunSheet({ open, onOpenChange, initial, initialStep = 'edit', onStart }: Props) {
  // The parent remounts this sheet (via key) for each opening, so state starts from props.
  const [form, setForm] = useState(initial)
  const [errors, setErrors] = useState<RunFormErrors>({})
  const [review, setReview] = useState<RunRequest | null>(() => {
    const result = validateRunForm(initial)
    return initialStep === 'review' && result.ok ? result.body : null
  })
  const [startError, setStartError] = useState('')
  const [starting, setStarting] = useState(false)

  const close = (next: boolean) => {
    if (!next) setForm((current) => ({ ...current, bearer: '' }))
    onOpenChange(next)
  }
  const set = (patch: Partial<RunForm>) => setForm({ ...form, ...patch })
  const toReview = () => {
    const result = validateRunForm(form)
    setErrors(result.ok ? {} : result.errors)
    setReview(result.ok ? result.body : null)
  }
  const start = async (body: RunRequest) => {
    if (!onStart) return
    setStarting(true)
    setStartError('')
    try {
      await onStart(body, form)
      close(false)
    } catch (error) {
      setStartError(error instanceof Error ? error.message : String(error))
    } finally {
      setStarting(false)
    }
  }
  const bytes = byteLength(form.useCase)

  return (
    <Sheet open={open} onOpenChange={close}>
      <SheetContent className="w-full overflow-y-auto border-white/20 bg-black sm:max-w-lg">
        <SheetHeader>
          <SheetTitle>{review ? 'Review the run' : 'New run'}</SheetTitle>
          <SheetDescription>
            {review ? 'Nothing starts until you press Start run.' : 'Point Endemic at your staging application. It plans and runs the load test.'}
          </SheetDescription>
        </SheetHeader>
        <div className="grid gap-6 px-4">
          {review ? (
            <Review body={review} />
          ) : (
            <>
              <TextField id="target-url" label="Target URL" hint="Your staging MCP endpoint or API base URL, https only." type="url" autoComplete="url" value={form.url} error={errors.url} onChange={(e) => set({ url: e.target.value })} />
              <TextField id="openapi-url" label="OpenAPI spec URL (optional)" hint="Fill this in for an HTTP API. Leave it empty for an MCP endpoint." type="url" value={form.openapiUrl} error={errors.openapiUrl} onChange={(e) => set({ openapiUrl: e.target.value })} />
              <GoalField form={form} error={errors.holdAgents} set={set} />
              <TextField id="bearer" label="Staging bearer token" hint="Write-only: sent with the request, never stored or shown again." type="password" autoComplete="off" spellCheck={false} value={form.bearer} error={errors.bearer} onChange={(e) => set({ bearer: e.target.value })} />
              <Field data-invalid={Boolean(errors.useCase)}>
                <FieldLabel htmlFor="use-case">Average user (optional)</FieldLabel>
                <Textarea id="use-case" rows={4} placeholder="Shoppers search the catalog, compare two items, and check out." value={form.useCase} aria-invalid={Boolean(errors.useCase)} aria-describedby="use-case-bytes" onChange={(e) => set({ useCase: e.target.value })} />
                <FieldDescription id="use-case-bytes" className="num">
                  {bytes.toLocaleString()} of {MAX_USE_CASE_BYTES.toLocaleString()} bytes. Steers the tasks agents are given.
                </FieldDescription>
                {errors.useCase && <FieldError>{errors.useCase}</FieldError>}
              </Field>
              <PacingField form={form} errors={errors} set={set} />
            </>
          )}
          {startError && (
            <p role="alert" className="rounded border border-white/40 p-3 text-sm">
              {startError}
            </p>
          )}
          {review && !onStart && <p className="text-sm text-muted-foreground">Starting needs a live runner. Set ENDEMIC_API_URL and restart the dashboard server.</p>}
        </div>
        <SheetFooter className="flex-row justify-end gap-2">
          {review ? (
            <>
              <Button variant="outline" onClick={() => setReview(null)}>
                Edit
              </Button>
              <Button disabled={!onStart || starting} onClick={() => start(review)}>
                {starting ? 'Starting run' : 'Start run'}
              </Button>
            </>
          ) : (
            <Button onClick={toReview}>Review run</Button>
          )}
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}
