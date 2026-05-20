# Hallucinator Web

A human-facing PDF reference validator, plus a JSON API designed for direct
use and future MCP integration.

```
web/
├── backend/   Rust (axum) JSON API at :8787 — wraps the hallucinator-core
│              validation pipeline and adds PDF-coordinate location data
└── frontend/  Vite + React + TypeScript at :5173 — uploads a PDF, renders
               it via pdf.js, overlays colored boxes per reference, hover
               popover shows the per-field validation report
```

## Development

Run the two servers in parallel:

```bash
# Terminal 1: backend
cd web/backend
cargo run --release
# → http://127.0.0.1:8787

# Terminal 2: frontend (Vite proxies /api/* to the backend)
cd web/frontend
npm install            # first time
npm run dev
# → http://127.0.0.1:5173
```

Open <http://127.0.0.1:5173/>, drop in a PDF, watch the references stream
in. Verified refs draw a green box, mismatches orange, not-found pink.
Hover for the per-author + per-field report.

## API (direct / MCP)

The backend is also a standalone JSON API. No PDF required.

```bash
# Single reference
curl -s -X POST http://127.0.0.1:8787/api/validate-reference \
  -H 'content-type: application/json' \
  -d '{
    "title": "Attention Is All You Need",
    "authors": ["Ashish Vaswani", "Noam Shazeer"],
    "year": 2017
  }' | jq .

# Batch (max 200 per call)
curl -s -X POST http://127.0.0.1:8787/api/validate-references \
  -H 'content-type: application/json' \
  -d '[{"title": "..."}, {"title": "..."}]'

# PDF upload (multipart)
curl -X POST http://127.0.0.1:8787/api/validate-pdf -F "file=@paper.pdf"
# → { "job_id": "...", "filename": "paper.pdf" }
curl http://127.0.0.1:8787/api/validate-pdf/$JOB_ID            # polling
curl http://127.0.0.1:8787/api/validate-pdf/$JOB_ID/stream     # SSE
curl http://127.0.0.1:8787/api/pdf/$JOB_ID > orig.pdf          # raw PDF
```

The response shape (`ApiValidationResult`) carries per-field signals:

- `status` — `verified | mismatch | not_found | pending`
- `mismatch_kinds` — empty unless `status = mismatch`; subset of `author | doi | arxiv_id`
- `report.authors` — one entry per cited author, `matched | potential_lookalike | not_in_db`
- `report.{journal, year, volume, issue, pages, doi}` — `matched | potential_mismatch | unverifiable`
- `page_number`, `bboxes` — PDF location (only present for PDF-uploaded refs)

## Config

The backend loads API keys and offline DB paths from the same
`~/.config/hallucinator/config.toml` the CLI uses. No separate
configuration needed.
