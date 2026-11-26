// Dev-only query parameters for headless screenshots (a file input cannot be
// scripted there). Production builds ignore them.
//   ?fixture=sample      load fixtures/sample-run.json
//   ?layout=<json>       seed the stored panel layout (validated like any stored value)
//   ?panels=open         open the Panels menu
//   ?sheet=form|review   open the New run sheet filled with example values, at the form or review step
import { LAYOUT_KEY } from './layout'
import type { RunForm } from './runRequest'

const params = process.env.NODE_ENV === 'development' ? new URLSearchParams(window.location.search) : new URLSearchParams()

export const devFixture = params.get('fixture') === 'sample'
export const devPanelsOpen = params.get('panels') === 'open'
export const devSheet = params.get('sheet') === 'form' || params.get('sheet') === 'review' ? params.get('sheet') : null

export const DEV_REVIEW_FORM: RunForm = {
  url: 'https://staging.example.com/mcp',
  openapiUrl: '',
  bearer: 'example-token',
  useCase: 'Shoppers look up a recent order, check whether its invoice is paid, and ask for a refund when it shipped late.',
  goal: 'hold',
  holdAgents: '200',
  startSpreadMs: '5000',
  thinkMinMs: '2000',
  thinkMaxMs: '8000',
  syncSessions: false,
}

const layout = params.get('layout')
if (layout != null) {
  try {
    localStorage.setItem(LAYOUT_KEY, layout)
  } catch {
    // Storage blocked: the default layout is used.
  }
}
