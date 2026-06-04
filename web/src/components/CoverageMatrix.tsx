import type { CoverageGap, CoverageReport, EvidenceMemory, ResearchRunRecord } from "../api/types";

interface CoverageMatrixProps {
  run: ResearchRunRecord | null;
  coverage: CoverageReport | null;
  memory: EvidenceMemory | null;
}

function CoverageMatrix({ run, coverage, memory }: CoverageMatrixProps) {
  if (!run) {
    return (
      <div className="empty-state">
        <h2>等待运行</h2>
        <p>CoverageCritic 会在证据构建后输出章节覆盖情况。</p>
      </div>
    );
  }

  if (!coverage) {
    return (
      <div className="empty-state">
        <h2>Coverage 尚未生成</h2>
        <p>批准计划并完成抓取后，这里会显示 QueryGap 和 SourceGap。</p>
      </div>
    );
  }

  return (
    <div className="plan-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">CoverageCritic</p>
          <h2>覆盖度 {Math.round(coverage.overall_coverage_score * 100)}%</h2>
        </div>
        <span className="badge">{recommendationLabel(coverage.recommendation)}</span>
      </div>

      {coverage.out_of_scope_notice.length > 0 && (
        <div className="warning-box">
          {coverage.out_of_scope_notice.map((notice) => (
            <p key={notice}>{notice}</p>
          ))}
        </div>
      )}

      <section className="phase-card">
        <table>
          <thead>
            <tr>
              <th>章节</th>
              <th>证据</th>
              <th>缺口</th>
              <th>严重度</th>
              <th>建议</th>
            </tr>
          </thead>
          <tbody>
            {run.chapters.map((chapter) => {
              const gap = coverage.gaps.find((item) => item.chapter_id === chapter.id);
              const evidenceCount =
                memory?.items.filter((item) => item.chapter_ids.includes(chapter.id)).length ?? 0;
              return (
                <tr key={chapter.id}>
                  <td>{chapter.title_zh}</td>
                  <td>{evidenceCount}</td>
                  <td>{gap ? gapKindLabel(gap) : "无"}</td>
                  <td>{gap?.severity ?? "-"}</td>
                  <td>{gap ? gap.explanation : "当前证据满足第一版覆盖要求。"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>

      {coverage.gaps.length > 0 && (
        <section className="phase-card">
          <h3>建议查询</h3>
          <div className="gap-list">
            {coverage.gaps.map((gap) => (
              <article className="gap-card" key={`${gap.chapter_id}-${gap.gap_kind}`}>
                <span className="badge">{gapKindLabel(gap)}</span>
                <p>{gap.explanation}</p>
                {gap.recommended_queries.length > 0 && (
                  <div className="lineage-list">
                    {gap.recommended_queries.map((query) => (
                      <span key={query}>{query}</span>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function gapKindLabel(gap: CoverageGap) {
  return gap.gap_kind === "query_gap" ? "QueryGap" : "SourceGap";
}

function recommendationLabel(value: string) {
  return (
    {
      no_action: "NoAction",
      suggest_new_query: "SuggestNewQuery",
      out_of_scope: "OutOfScope"
    }[value] ?? value
  );
}

export default CoverageMatrix;
