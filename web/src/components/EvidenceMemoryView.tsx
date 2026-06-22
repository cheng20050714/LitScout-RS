import type {
  EvidenceItem,
  EvidenceMemory,
  EvidenceSelectionReport,
  ReadingLibrarySummary,
  ResearchRunRecord
} from "../api/types";

interface EvidenceMemoryViewProps {
  run: ResearchRunRecord | null;
  memory: EvidenceMemory | null;
  libraryItems: ReadingLibrarySummary[];
  addingEvidenceId: string | null;
  onAddToLibrary: (item: EvidenceItem) => void;
}

function EvidenceMemoryView({
  run,
  memory,
  libraryItems,
  addingEvidenceId,
  onAddToLibrary
}: EvidenceMemoryViewProps) {
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

      {memory.selection_report && (
        <SelectionSummary report={memory.selection_report} />
      )}

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
                  <EvidenceCard
                    key={item.evidence_id}
                    item={item}
                    memory={memory}
                    inLibrary={libraryItems.some(
                      (libraryItem) => libraryItem.source_item_id === item.source_item_id
                    )}
                    adding={addingEvidenceId === item.evidence_id}
                    onAddToLibrary={onAddToLibrary}
                  />
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

function SelectionSummary({ report }: { report: EvidenceSelectionReport }) {
  const reasons = (report.rejection_reasons ?? []).slice(0, 3);

  return (
    <section className="selection-summary" aria-label="证据筛选摘要">
      <div className="selection-metrics">
        <SelectionMetric label="候选" value={report.raw_item_count} />
        <SelectionMetric label="合并后" value={report.merged_item_count} />
        <SelectionMetric label="精选" value={report.accepted_item_count} />
        <SelectionMetric label="拒绝" value={report.rejected_item_count} />
      </div>
      {reasons.length > 0 && (
        <div className="selection-reasons">
          {reasons.map((reason) => (
            <span className="badge" key={reason.reason}>
              {reasonLabel(reason.reason)} × {reason.count}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function SelectionMetric({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function EvidenceCard({
  item,
  memory,
  inLibrary,
  adding,
  onAddToLibrary
}: {
  item: EvidenceItem;
  memory: EvidenceMemory;
  inLibrary: boolean;
  adding: boolean;
  onAddToLibrary: (item: EvidenceItem) => void;
}) {
  const attempts = memory.query_attempts.filter((attempt) =>
    item.query_attempt_ids.includes(attempt.query_id)
  );
  const isArxiv = String(item.source_kind).toLowerCase() === "arxiv";

  return (
    <article className="evidence-card">
      <div className="evidence-card-top">
        <div className="evidence-badges">
          <span className="badge">{sourceLabel(item.source_kind)}</span>
          <span className="badge">{item.citation_id}</span>
        </div>
        {isArxiv && (
          <button
            className={`evidence-add-button${inLibrary ? " added" : ""}`}
            type="button"
            title={inLibrary ? "已加入阅读库" : "加入阅读库"}
            disabled={inLibrary || adding}
            onClick={() => onAddToLibrary(item)}
            aria-label={inLibrary ? "已加入阅读库" : "加入阅读库"}
          >
            {adding ? "..." : inLibrary ? "✓" : "+"}
          </button>
        )}
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
  switch (String(kind).toLowerCase()) {
    case "github":
    case "git_hub":
      return "GitHub";
    case "arxiv":
      return "arXiv";
    case "academic_index":
    case "academicindex":
      return "Academic Index";
    case "bibliography":
      return "Bibliography";
    default:
      return String(kind);
  }
}

function reasonLabel(reason: string) {
  switch (reason) {
    case "empty_content_without_verifiable_metadata":
      return "内容不足";
    case "no_successful_lineage":
      return "无成功链路";
    case "no_topic_match":
      return "主题不匹配";
    case "academic_index_no_title_or_summary_match":
      return "标题摘要不匹配";
    case "bibliography_weak_title_match":
      return "书目标题弱匹配";
    case "bibliography_missing_metadata":
      return "书目元数据不足";
    case "bibliography_ratio_limit":
      return "书目占比限制";
    case "rust_plant_disease_ambiguity":
      return "Rust 语义歧义";
    default:
      return reason;
  }
}

export default EvidenceMemoryView;
