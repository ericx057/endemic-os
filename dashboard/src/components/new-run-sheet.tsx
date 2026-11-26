import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Textarea } from '@/components/ui/textarea'
import { MAX_USE_CASE_BYTES, byteLength, describeFault, describePacing, describePersona, emptyFault, emptyPersona, validateRunForm, type FaultForm, type FaultKind, type PersonaForm, type RunForm, type RunFormErrors, type RunGoal, type RunRequest } from '@/lib/runRequest'

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

const FAULT_KINDS: { value: FaultKind; label: string }[] = [
  { value: 'error', label: 'Fail with an error' },
  { value: 'latency', label: 'Add latency' },
  { value: 'timeout', label: 'Time out' },
]

function FaultRow({ index, fault, update, remove }: { index: number; fault: FaultForm; update: (patch: Partial<FaultForm>) => void; remove: () => void }) {
  const whole = { type: 'number', inputMode: 'numeric', step: 1 } as const
  const id = (name: string) => `fault-${index}-${name}`
  return (
    <div className="grid gap-3 rounded border border-white/20 p-3">
      <div className="grid grid-cols-2 gap-4">
        <TextField id={id('tool')} label="Tool" hint="As the target names it." value={fault.tool} onChange={(e) => update({ tool: e.target.value })} />
        <Field>
          <FieldLabel htmlFor={id('kind')}>Effect</FieldLabel>
          <Select value={fault.kind} onValueChange={(kind) => update({ kind: kind as FaultKind })}>
            <SelectTrigger id={id('kind')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FAULT_KINDS.map((kind) => (
                <SelectItem key={kind.value} value={kind.value}>
                  {kind.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>
      <div className="grid grid-cols-3 gap-4">
        <TextField id={id('first')} label="First stage" hint="From 1." {...whole} min={1} value={fault.firstStage} onChange={(e) => update({ firstStage: e.target.value })} />
        <TextField id={id('last')} label="Last stage" hint="Inclusive." {...whole} min={1} value={fault.lastStage} onChange={(e) => update({ lastStage: e.target.value })} />
        <TextField id={id('percent')} label="Share (%)" hint="Of its calls." {...whole} min={1} max={100} value={fault.percent} onChange={(e) => update({ percent: e.target.value })} />
      </div>
      {fault.kind === 'latency' && (
        <TextField id={id('delay')} label="Delay (ms)" hint="Below the runner's target timeout." {...whole} min={1} value={fault.delayMs} onChange={(e) => update({ delayMs: e.target.value })} />
      )}
      <Button variant="outline" size="sm" className="justify-self-end" onClick={remove}>
        Remove fault
      </Button>
    </div>
  )
}

function FaultsField({ form, error, set }: { form: RunForm; error?: string; set: (patch: Partial<RunForm>) => void }) {
  const update = (index: number, patch: Partial<FaultForm>) => set({ faults: form.faults.map((fault, i) => (i === index ? { ...fault, ...patch } : fault)) })
  return (
    <FieldSet data-invalid={Boolean(error)}>
      <FieldLegend variant="label">Faults (optional)</FieldLegend>
      <FieldDescription>The runner fails, delays or times out its own calls to a tool during the chosen stages. Nothing extra reaches the target, and injected failures are reported apart from target errors.</FieldDescription>
      {form.faults.map((fault, index) => (
        <FaultRow key={index} index={index} fault={fault} update={(patch) => update(index, patch)} remove={() => set({ faults: form.faults.filter((_, i) => i !== index) })} />
      ))}
      <Button variant="outline" size="sm" className="justify-self-start" onClick={() => set({ faults: [...form.faults, emptyFault()] })}>
        Add fault
      </Button>
      {error && <FieldError>{error}</FieldError>}
    </FieldSet>
  )
}

function PersonaFields({ persona, number, update, remove }: { persona: PersonaForm; number: number; update: (patch: Partial<PersonaForm>) => void; remove: () => void }) {
  const id = (field: string) => `persona-${number}-${field}`
  const whole = { type: 'number', inputMode: 'numeric', min: 1, step: 1 } as const
  return (
    <FieldSet className="rounded border border-white/20 p-3">
      <div className="flex items-center justify-between">
        <FieldLegend variant="label">Persona {number}</FieldLegend>
        <Button variant="outline" size="sm" onClick={remove}>
          Remove
        </Button>
      </div>
      <div className="grid grid-cols-2 gap-4">
        <TextField id={id('name')} label="Name" hint="Unique in the mix." value={persona.name} onChange={(e) => update({ name: e.target.value })} />
        <TextField id={id('weight')} label="Weight (%)" hint="Share of agents." {...whole} max={100} value={persona.weight} onChange={(e) => update({ weight: e.target.value })} />
        <TextField id={id('turns')} label="Max model turns" hint="Blank: the server limit." {...whole} value={persona.maxModelTurns} onChange={(e) => update({ maxModelTurns: e.target.value })} />
        <TextField id={id('calls')} label="Max tool calls" hint="Blank: the server limit." {...whole} value={persona.maxToolCalls} onChange={(e) => update({ maxToolCalls: e.target.value })} />
      </div>
      <Field>
        <FieldLabel htmlFor={id('use-case')}>Use case (optional)</FieldLabel>
        <Textarea id={id('use-case')} rows={3} value={persona.useCase} onChange={(e) => update({ useCase: e.target.value })} />
      </Field>
    </FieldSet>
  )
}

function TaskMixField({ form, error, set }: { form: RunForm; error?: string; set: (patch: Partial<RunForm>) => void }) {
  const update = (index: number) => (patch: Partial<PersonaForm>) => set({ personas: form.personas.map((persona, i) => (i === index ? { ...persona, ...patch } : persona)) })
  const remove = (index: number) => () => set({ personas: form.personas.filter((_, i) => i !== index) })
  return (
    <FieldSet data-invalid={Boolean(error)}>
      <FieldLegend variant="label">Task mix (optional)</FieldLegend>
      <FieldDescription>Personas split the agents by weight, summing to 100%. With any persona, each sets its own use case.</FieldDescription>
      {form.personas.map((persona, index) => (
        <PersonaFields key={index} persona={persona} number={index + 1} update={update(index)} remove={remove(index)} />
      ))}
      <Button variant="outline" size="sm" className="justify-self-start" onClick={() => set({ personas: [...form.personas, emptyPersona()] })}>
        Add persona
      </Button>
      {error && <FieldError>{error}</FieldError>}
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
      {body.task_mix ? (
        <>
          <dt className="text-muted-foreground">Task mix</dt>
          <dd>
            <ul className="space-y-1">
              {body.task_mix.map((persona) => (
                <li key={persona.name}>
                  <span className="font-medium">{persona.name}</span>: {describePersona(persona)}
                </li>
              ))}
            </ul>
          </dd>
        </>
      ) : (
        <>
          <dt className="text-muted-foreground">Use case</dt>
          <dd className="whitespace-pre-wrap">{body.use_case ?? 'None. Tasks will come from the discovered tools alone.'}</dd>
        </>
      )}
      <dt className="text-muted-foreground">Faults</dt>
      <dd className="num">{body.faults ? body.faults.map((fault, i) => <p key={i}>{describeFault(fault)}</p>) : 'None'}</dd>
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
              {form.personas.length === 0 && (
                <Field data-invalid={Boolean(errors.useCase)}>
                  <FieldLabel htmlFor="use-case">Average user (optional)</FieldLabel>
                  <Textarea id="use-case" rows={4} placeholder="Shoppers search the catalog, compare two items, and check out." value={form.useCase} aria-invalid={Boolean(errors.useCase)} aria-describedby="use-case-bytes" onChange={(e) => set({ useCase: e.target.value })} />
                  <FieldDescription id="use-case-bytes" className="num">
                    {bytes.toLocaleString()} of {MAX_USE_CASE_BYTES.toLocaleString()} bytes. Steers the tasks agents are given.
                  </FieldDescription>
                  {errors.useCase && <FieldError>{errors.useCase}</FieldError>}
                </Field>
              )}
              <PacingField form={form} errors={errors} set={set} />
              <TaskMixField form={form} error={errors.personas} set={set} />
              <FaultsField form={form} error={errors.faults} set={set} />
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
