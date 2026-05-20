import { useEffect, useState } from 'react'
import './App.css'
import { getJobStatus, streamJob, uploadPdf } from './api'
import PdfViewer from './PdfViewer'
import type { ApiValidationResult, JobStatusJson } from './types'

export default function App() {
  const [jobId, setJobId] = useState<string | null>(null)
  const [status, setStatus] = useState<JobStatusJson | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)

  useEffect(() => {
    if (!jobId) return
    let cancelled = false

    // Belt-and-suspenders: also fetch one snapshot in case we miss
    // the initial SSE snapshot event due to a race.
    getJobStatus(jobId)
      .then((s) => { if (!cancelled) setStatus(s) })
      .catch(() => { /* SSE will deliver soon */ })

    const close = streamJob(jobId, {
      onSnapshot: (s) => { if (!cancelled) setStatus(s) },
      onExtracted: ({ page_count, references }) => {
        if (cancelled) return
        setStatus((prev) => prev
          ? { ...prev, state: 'running', page_count, references, total: references.length, completed: 0 }
          : prev,
        )
      },
      onReferenceComplete: ({ index, result }) => {
        if (cancelled) return
        setStatus((prev) => {
          if (!prev?.references) return prev
          const refs = prev.references.slice()
          refs[index] = result
          return { ...prev, references: refs, completed: (prev.completed ?? 0) + 1 }
        })
      },
      onDone: () => {
        if (cancelled) return
        setStatus((prev) => prev ? { ...prev, state: 'done' } : prev)
      },
      onFailed: (err) => {
        if (cancelled) return
        setStatus((prev) => prev ? { ...prev, state: 'failed', error: err } : prev)
      },
    })

    return () => { cancelled = true; close() }
  }, [jobId])

  async function handleFile(file: File) {
    setError(null)
    setStatus(null)
    setUploading(true)
    try {
      const { job_id } = await uploadPdf(file)
      setJobId(job_id)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setUploading(false)
    }
  }

  if (!jobId) {
    return (
      <div className="upload-screen">
        <h1>Hallucinator</h1>
        <p>Upload a PDF to check references against academic databases.</p>
        <label className={`drop-zone ${uploading ? 'uploading' : ''}`}>
          <input
            type="file"
            accept="application/pdf,.pdf"
            disabled={uploading}
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) handleFile(f)
            }}
          />
          {uploading ? 'Uploading…' : 'Choose a PDF or drag one here'}
        </label>
        {error && <div className="error">Error: {error}</div>}
      </div>
    )
  }

  return (
    <div className="app">
      <Sidebar status={status} onReset={() => { setJobId(null); setStatus(null) }} />
      <main className="viewer">
        {status && jobId
          ? <PdfViewer jobId={jobId} status={status} />
          : <p>Loading…</p>}
      </main>
    </div>
  )
}

function Sidebar({
  status,
  onReset,
}: { status: JobStatusJson | null; onReset: () => void }) {
  if (!status) return <aside className="sidebar"><p>Loading…</p></aside>

  const refs = status.references ?? []
  const counts = countByStatus(refs)

  return (
    <aside className="sidebar">
      <button className="reset" onClick={onReset}>← New PDF</button>
      <h2 title={status.filename}>{status.filename}</h2>
      <div className={`state state-${status.state}`}>
        {status.state}
        {status.state === 'running' && status.total
          ? ` ${status.completed ?? 0}/${status.total}`
          : null}
      </div>
      <ul className="counts">
        <li><span className="dot verified" /> Verified <strong>{counts.verified}</strong></li>
        <li><span className="dot mismatch" /> Mismatch <strong>{counts.mismatch}</strong></li>
        <li><span className="dot not_found" /> Not found <strong>{counts.not_found}</strong></li>
        <li><span className="dot pending" /> Pending <strong>{counts.pending}</strong></li>
      </ul>
      {status.error && <div className="error">{status.error}</div>}
    </aside>
  )
}

function countByStatus(refs: ApiValidationResult[]) {
  return refs.reduce(
    (acc, r) => { acc[r.status] = (acc[r.status] ?? 0) + 1; return acc },
    { verified: 0, mismatch: 0, not_found: 0, pending: 0 } as Record<string, number>,
  )
}
