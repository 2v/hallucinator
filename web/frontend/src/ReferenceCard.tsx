import type { ApiValidationResult, FieldStatus } from './types'

const FIELDS = [
  { key: 'journal', label: 'Journal' },
  { key: 'year', label: 'Year' },
  { key: 'volume', label: 'Volume' },
  { key: 'issue', label: 'Issue' },
  { key: 'pages', label: 'Pages' },
  { key: 'doi', label: 'DOI' },
] as const

export default function ReferenceCard({
  ref,
  index,
}: {
  ref: ApiValidationResult
  index: number
}) {
  return (
    <div className="ref-card">
      <header>
        <span className={`status-pill status-${ref.status}`}>{ref.status}</span>
        <span className="ref-index">#{index + 1}</span>
        {ref.source && <span className="ref-source">via {ref.source}</span>}
      </header>
      <h3>{ref.title || '(no title)'}</h3>
      {ref.paper_url && (
        <a className="paper-link" href={ref.paper_url} target="_blank" rel="noreferrer">
          {ref.paper_url}
        </a>
      )}

      {ref.retraction?.is_retracted && (
        <div className="retraction-warning">
          ⚠ Retracted (source: {ref.retraction.retraction_source ?? 'unknown'})
        </div>
      )}

      {ref.report && (
        <>
          <section>
            <h4>Authors</h4>
            <ul className="author-list">
              {ref.report.authors.map((a, i) => (
                <li key={i} className={`author-${a.status.kind}`}>
                  <span className="cited">{a.cited}</span>
                  {a.status.kind === 'matched' && a.status.db_match && a.status.db_match !== a.cited && (
                    <span className="hint">(DB: {a.status.db_match})</span>
                  )}
                  {a.status.kind === 'potential_lookalike' && (
                    <span className="hint warn">
                      potential swap — DB has <strong>{a.status.db_candidate}</strong>
                    </span>
                  )}
                  {a.status.kind === 'not_in_db' && (
                    <span className="hint warn">not in DB record</span>
                  )}
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h4>Bibliographic fields</h4>
            <table className="biblio-table">
              <tbody>
                {FIELDS.map(({ key, label }) => {
                  const f = ref.report![key] as FieldStatus | null
                  if (!f) return null
                  return (
                    <tr key={key} className={`field-${f.kind}`}>
                      <th>{label}</th>
                      <td>
                        {f.kind === 'matched' && <span>{f.value}</span>}
                        {f.kind === 'potential_mismatch' && (
                          <>
                            <span className="strike">{f.cited}</span> →{' '}
                            <strong>{f.db_value}</strong>
                          </>
                        )}
                        {f.kind === 'unverifiable' && (
                          <>
                            <span>{f.cited}</span>{' '}
                            <span className="hint dim">(DB didn't return this field)</span>
                          </>
                        )}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </section>
        </>
      )}

      {!ref.report && ref.status === 'not_found' && (
        <div className="not-found-msg">
          No matching paper found in any database. This citation may be a
          full hallucination — verify manually before relying on it.
        </div>
      )}
    </div>
  )
}
