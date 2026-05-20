// JSON shapes mirroring the hallucinator-api crate's response types.

export interface BBox {
  x0: number
  y0: number
  x1: number
  y1: number
}

export type FieldStatusKind = 'matched' | 'potential_mismatch' | 'unverifiable'

export type AuthorStatusKind = 'matched' | 'potential_lookalike' | 'not_in_db'

export interface FieldStatus {
  kind: FieldStatusKind
  value?: string
  cited?: string
  db_value?: string
}

export interface AuthorFieldStatus {
  cited: string
  status: {
    kind: AuthorStatusKind
    db_match?: string
    db_candidate?: string
  }
}

export interface ValidationReport {
  title_status: { kind: 'matched' | 'not_found_in_any_db' }
  source_db: string | null
  authors: AuthorFieldStatus[]
  journal: FieldStatus | null
  year: FieldStatus | null
  volume: FieldStatus | null
  issue: FieldStatus | null
  pages: FieldStatus | null
  doi: FieldStatus | null
}

export type ApiStatus = 'pending' | 'verified' | 'mismatch' | 'not_found'

export interface ApiValidationResult {
  title: string
  raw_citation: string
  ref_authors: string[]
  status: ApiStatus
  mismatch_kinds: string[]
  source: string | null
  found_authors: string[]
  paper_url: string | null
  failed_dbs: string[]
  doi: string | null
  arxiv_id: string | null
  retraction: {
    is_retracted: boolean
    retraction_doi: string | null
    retraction_source: string | null
  } | null
  report: ValidationReport | null
  page_number: number | null
  bboxes: BBox[]
}

export type JobState = 'pending' | 'running' | 'done' | 'failed'

export interface JobStatusJson {
  job_id: string
  filename: string
  state: JobState
  page_count?: number
  completed?: number
  total?: number
  references?: ApiValidationResult[]
  error?: string
}
