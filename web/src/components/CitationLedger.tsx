import type { Citation, PlanResponse } from "../api/types";

interface CitationLedgerProps {
  citations: Citation[];
  plan: PlanResponse | null;
}

function CitationLedger({ citations, plan }: CitationLedgerProps) {
  return (
    <section className="ledger">
      <div className="section-header small">
        <div>
          <p className="eyebrow">Ledger</p>
          <h2>引用账本</h2>
        </div>
        <span className="badge">{citations.length}</span>
      </div>

      {citations.length === 0 ? (
        <p className="muted">
          {plan ? "执行调研后会显示来源引用。" : "生成计划后继续执行调研。"}
        </p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>标题</th>
              <th>来源</th>
            </tr>
          </thead>
          <tbody>
            {citations.map((citation) => (
              <tr key={citation.id}>
                <td>{citation.id}</td>
                <td>
                  <a href={citation.url} target="_blank" rel="noreferrer">
                    {citation.title}
                  </a>
                </td>
                <td>{citation.source_kind}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

export default CitationLedger;
