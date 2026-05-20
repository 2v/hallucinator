// Thin fetch wrappers around the hallucinator-api endpoints.

import type { JobStatusJson } from './types'

export async function uploadPdf(file: File): Promise<{ job_id: string; filename: string }> {
  const form = new FormData()
  form.append('file', file)
  const resp = await fetch('/api/validate-pdf', { method: 'POST', body: form })
  if (!resp.ok) {
    throw new Error(`upload failed: ${resp.status} ${await resp.text()}`)
  }
  return resp.json()
}

export async function getJobStatus(jobId: string): Promise<JobStatusJson> {
  const resp = await fetch(`/api/validate-pdf/${encodeURIComponent(jobId)}`)
  if (!resp.ok) throw new Error(`status fetch failed: ${resp.status}`)
  return resp.json()
}

export function pdfUrl(jobId: string): string {
  return `/api/pdf/${encodeURIComponent(jobId)}`
}

/// Subscribe to SSE updates for a job. `onSnapshot` fires once with the
/// initial state; per-reference updates fire `onReferenceComplete`.
/// Returns a cleanup function that closes the connection.
export function streamJob(
  jobId: string,
  handlers: {
    onSnapshot?: (s: JobStatusJson) => void
    onExtracted?: (data: { page_count: number; references: import('./types').ApiValidationResult[] }) => void
    onReferenceComplete?: (data: { index: number; result: import('./types').ApiValidationResult }) => void
    onDone?: () => void
    onFailed?: (error: string) => void
  },
): () => void {
  const es = new EventSource(`/api/validate-pdf/${encodeURIComponent(jobId)}/stream`)

  es.addEventListener('snapshot', (e) => {
    try {
      handlers.onSnapshot?.(JSON.parse((e as MessageEvent).data))
    } catch { /* ignore */ }
  })
  es.addEventListener('extracted', (e) => {
    try {
      handlers.onExtracted?.(JSON.parse((e as MessageEvent).data))
    } catch { /* ignore */ }
  })
  es.addEventListener('reference_complete', (e) => {
    try {
      handlers.onReferenceComplete?.(JSON.parse((e as MessageEvent).data))
    } catch { /* ignore */ }
  })
  es.addEventListener('done', () => {
    handlers.onDone?.()
    es.close()
  })
  es.addEventListener('failed', (e) => {
    try {
      const { error } = JSON.parse((e as MessageEvent).data) as { error: string }
      handlers.onFailed?.(error)
    } catch {
      handlers.onFailed?.('unknown error')
    }
    es.close()
  })

  return () => es.close()
}
