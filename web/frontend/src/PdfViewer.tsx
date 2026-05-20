import { useEffect, useRef, useState } from 'react'
import * as pdfjsLib from 'pdfjs-dist'
import { pdfUrl } from './api'
import type { ApiValidationResult, JobStatusJson } from './types'
import ReferenceCard from './ReferenceCard'

// Use the bundled module worker so Vite handles it correctly.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).href

const SCALE = 1.5

export default function PdfViewer({
  jobId,
  status,
}: {
  jobId: string
  status: JobStatusJson
}) {
  const refs = status.references ?? []
  // Default page = the first page that has references (the references section).
  const defaultPage = refs.find((r) => r.page_number !== null)?.page_number ?? 0
  const [page, setPage] = useState(defaultPage)
  const [pageCount, setPageCount] = useState<number | null>(null)
  const [pdf, setPdf] = useState<pdfjsLib.PDFDocumentProxy | null>(null)
  const [viewport, setViewport] = useState<{ width: number; height: number } | null>(null)
  const [hoveredRefIdx, setHoveredRefIdx] = useState<number | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)

  // Load PDF once.
  useEffect(() => {
    let cancelled = false
    pdfjsLib.getDocument(pdfUrl(jobId)).promise.then((doc) => {
      if (cancelled) return
      setPdf(doc)
      setPageCount(doc.numPages)
    })
    return () => { cancelled = true }
  }, [jobId])

  // Render the current page when pdf or page changes.
  useEffect(() => {
    if (!pdf || !canvasRef.current) return
    let cancelled = false
    const canvas = canvasRef.current

    pdf.getPage(page + 1).then(async (p) => {
      if (cancelled) return
      const vp = p.getViewport({ scale: SCALE })
      canvas.width = vp.width
      canvas.height = vp.height
      setViewport({ width: vp.width, height: vp.height })
      const ctx = canvas.getContext('2d')!
      const task = p.render({ canvasContext: ctx, viewport: vp, canvas })
      try {
        await task.promise
      } catch { /* cancelled */ }
    })

    return () => { cancelled = true }
  }, [pdf, page])

  const refsOnPage: { idx: number; ref: ApiValidationResult }[] = refs
    .map((ref, idx) => ({ idx, ref }))
    .filter(({ ref }) => ref.page_number === page)

  return (
    <div className="pdf-viewer">
      <div className="pdf-toolbar">
        <button onClick={() => setPage((p) => Math.max(0, p - 1))} disabled={page === 0}>← Prev</button>
        <span>Page {page + 1} {pageCount ? `/ ${pageCount}` : ''}</span>
        <button
          onClick={() => setPage((p) => pageCount ? Math.min(pageCount - 1, p + 1) : p)}
          disabled={pageCount !== null && page >= pageCount - 1}
        >Next →</button>
      </div>
      <div className="pdf-page" style={{ width: viewport?.width, height: viewport?.height }}>
        <canvas ref={canvasRef} />
        {viewport && refsOnPage.flatMap(({ idx, ref }) =>
          ref.bboxes.map((b, bi) => (
            <div
              key={`${idx}-${bi}`}
              className={`overlay overlay-${ref.status}`}
              style={{
                left: b.x0 * SCALE,
                top: b.y0 * SCALE,
                width: (b.x1 - b.x0) * SCALE,
                height: (b.y1 - b.y0) * SCALE,
              }}
              onMouseEnter={() => setHoveredRefIdx(idx)}
              onMouseLeave={() => setHoveredRefIdx((cur) => cur === idx ? null : cur)}
            />
          )),
        )}
        {viewport && hoveredRefIdx !== null && refs[hoveredRefIdx] && refs[hoveredRefIdx].page_number === page && (
          <PopoverFor ref={refs[hoveredRefIdx]} index={hoveredRefIdx} />
        )}
      </div>
    </div>
  )
}

function PopoverFor({ ref, index }: { ref: ApiValidationResult; index: number }) {
  // Position popover above the first bbox.
  const b = ref.bboxes[0]
  if (!b) return null
  return (
    <div
      className="popover"
      style={{
        left: b.x0 * SCALE,
        top: Math.max(0, b.y0 * SCALE - 8),
        transform: 'translateY(-100%)',
      }}
    >
      <ReferenceCard ref={ref} index={index} />
    </div>
  )
}
