import type { CitationAuditReport, ResearchRunRecord } from "../api/types";

interface CitationAuditViewProps {
  run: ResearchRunRecord | null;
  audit: CitationAuditReport | null;
}

function CitationAuditView({ run, audit }: CitationAuditViewProps) {
  if (!run) {
    return (
      <div className="empty-state">
        <h2>等待运行</h2>
        <p>CitationAuditor 会在报告草稿生成后执行。</p>
      </div>
    );
  }

  if (!audit) {
    return (
      <div className="empty-state">
        <h2>Audit 尚未生成</h2>
        <p>报告生成后，这里会展示引用白名单和覆盖度检查。</p>
      </div>
    );
  }

  const warningTotal =
    audit.freshness_warnings.length +
    audit.unsupported_paragraph_warnings.length +
    audit.external_url_violations.length;

  return (
    <div className="plan-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">CitationAuditor</p>
          <h2>{audit.url_whitelist_passed ? "引用白名单通过" : "引用白名单失败"}</h2>
        </div>
        <span className="badge">{warningTotal} warnings</span>
      </div>

      <div className="plan-metrics">
        <Metric
          label="段落引用覆盖"
          value={`${Math.round(audit.citation_coverage_ratio * 100)}%`}
        />
        <Metric
          label="来源多样性"
          value={`${Math.round(audit.source_diversity_score * 100)}%`}
        />
        <Metric label="URL 白名单" value={audit.url_whitelist_passed ? "通过" : "失败"} />
      </div>

      <AuditList title="时效性警告" items={audit.freshness_warnings} />
      <AuditList title="无引用段落" items={audit.unsupported_paragraph_warnings} />
      <AuditList title="外部 URL 违规" items={audit.external_url_violations} />
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function AuditList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="phase-card">
      <div className="section-header">
        <h2>{title}</h2>
        <span className="badge">{items.length}</span>
      </div>
      {items.length === 0 ? (
        <p className="muted">没有发现该类问题。</p>
      ) : (
        <ul>
          {items.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

export default CitationAuditView;
