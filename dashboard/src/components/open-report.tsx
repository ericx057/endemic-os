import { useId, useRef } from 'react'
import { FileJson } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { parseReport, type RunReport } from '@/lib/report'

/** Opens a saved GET /runs/{id} JSON file. Reports a clear message for anything else. */
export function OpenReport({ onOpen, onError }: { onOpen: (report: RunReport) => void; onError: (message: string) => void }) {
  const input = useRef<HTMLInputElement>(null)
  const id = useId()
  const read = async (file: File) => {
    try {
      const text = await file.text()
      let json: unknown
      try {
        json = JSON.parse(text)
      } catch {
        throw new Error(`${file.name} is not JSON. Save the runner's GET /runs/{id} response and open that file.`)
      }
      onOpen(parseReport(json))
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error))
    }
  }
  return (
    <>
      <input
        ref={input}
        id={id}
        type="file"
        accept="application/json,.json"
        className="sr-only"
        tabIndex={-1}
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void read(file)
          e.target.value = ''
        }}
      />
      <Button variant="outline" size="sm" onClick={() => input.current?.click()}>
        <FileJson aria-hidden />
        Open report
      </Button>
    </>
  )
}
