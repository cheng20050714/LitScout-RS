import type { EvidenceItem, EvidenceMemory, ResearchRunRecord } from "../api/types";

interface EvidenceMemoryViewProps {
  run: ResearchRunRecord | null;
  memory: EvidenceMemory | null;
}

function EvidenceMemoryView({ run, memory }: EvidenceMemoryViewProps) {
  if (!run) {
    return (
      <div className="empty-state">
        <h2>等待运行</h2>
        <p>调研任务创建后，证据库会按章节展示。</p>
      </div>
    );
  }

  if (!memory) {
    return (
      <div className="empty-state">
        <h2>证据尚未生成</h2>
        <p>批准计划后，GitHub 与 arXiv 的结果会汇总到这里。</p>
      </div>
    );
  }

  const failedAttempts = memory.query_attempts.filter((attempt) => attempt.error);

  return (
    <div className="plan-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">证据库</p>
          <h2>{memory.items.length} 条证据</h2>
        </div>
        <span className="badge">{memory.query_attempts.length} 次查询</span>
      </div>

      {failedAttempts.length > 0 && (
        <div className="warning-box">
          {failedAttempts.map((attempt) => (
            <p key={attempt.query_id}>
              {attempt.source} `{attempt.query}`：{attempt.error}
            </p>
          ))}
        </div>
      )}

      {run.chapters.map((chapter) => {
        const items = memory.items.filter((item) => item.chapter_ids.includes(chapter.id));
        return (
          <section className="phase-card evidence-section" key={chapter.id}>
            <div className="section-header">
              <div>
                <p className="eyebrow">{chapter.id}</p>
                <h2>{chapter.title_zh}</h2>
              </div>
              <span className="badge">{items.length}</span>
            </div>
            {items.length === 0 ? (
              <p className="muted">该章节暂无证据，覆盖度检查会标记缺口。</p>
            ) : (
              <div className="evidence-grid">
                {items.map((item) => (
                  <EvidenceCard key={item.evidence_id} item={item} memory={memory} />
                ))}
              </div>
            )}
          </section>
        );
      })}

      <section className="phase-card">
        <h3>查询记录</h3>
        <table>
          <thead>
            <tr>
              <th>来源</th>
              <th>章节</th>
              <th>查询</th>
              <th>结果</th>
              <th>状态</th>
            </tr>
          </thead>
          <tbody>
            {memory.query_attempts.map((attempt) => (
              <tr key={attempt.query_id}>
                <td>{attempt.source}</td>
                <td>{attempt.chapter_id}</td>
                <td>{attempt.query}</td>
                <td>{attempt.result_count}</td>
                <td>{attempt.error ? "失败" : "成功"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}

function EvidenceCard({ item, memory }: { item: EvidenceItem; memory: EvidenceMemory }) {
  const attempts = memory.query_attempts.filter((attempt) =>
    item.query_attempt_ids.includes(attempt.query_id)
  );

  return (
    <article className="evidence-card">
      <div className="evidence-card-top">
        <span className="badge">{sourceLabel(item.source_kind)}</span>
        <span className="badge">{item.citation_id}</span>
      </div>
      <h3>
        <a href={item.url} target="_blank" rel="noreferrer">
          {item.title}
        </a>
      </h3>
      <p>{item.evidence_note_zh}</p>
      <p className="muted">{item.evidence_snippet}</p>
      <div className="lineage-list">
        {attempts.map((attempt) => (
          <span key={attempt.query_id}>{attempt.query}</span>
        ))}
      </div>
    </article>
  );
}

function sourceLabel(kind: EvidenceItem["source_kind"]) {
  return String(kind).toLowerCase() === "github" ? "GitHub" : "arXiv";
}

export default EvidenceMemoryView;
