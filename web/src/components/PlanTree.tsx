import { useEffect, useMemo, useState } from "react";
import type { ChapterNode, QueryPortfolio, ResearchRunRecord } from "../api/types";

interface PlanTreeProps {
  run: ResearchRunRecord | null;
  running: boolean;
  onRevise: (chapters: ChapterNode[], queryPortfolio: QueryPortfolio[], feedback: string) => void;
  onApprove: () => void;
}

function PlanTree({ run, running, onRevise, onApprove }: PlanTreeProps) {
  const [chapters, setChapters] = useState<ChapterNode[]>([]);
  const [queryPortfolio, setQueryPortfolio] = useState<QueryPortfolio[]>([]);
  const [feedback, setFeedback] = useState("");

  useEffect(() => {
    setChapters(run?.chapters ?? []);
    setQueryPortfolio(run?.query_portfolio ?? []);
    setFeedback("");
  }, [run?.run_id, run?.updated_at]);

  const brief = run?.brief;
  const planReady = run?.state === "plan_ready";
  const chapterCount = chapters.length;
  const queryCount = useMemo(
    () =>
      queryPortfolio.reduce(
        (total, item) => total + item.github_queries.length + item.arxiv_queries.length,
        0
      ),
    [queryPortfolio]
  );

  if (!run) {
    return (
      <div className="empty-state">
        <h2>等待调研任务</h2>
        <p>创建任务后，这里会显示调研摘要、章节计划和每章查询组合。</p>
      </div>
    );
  }
  const currentRun = run;

  return (
    <div className="plan-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">调研计划</p>
          <h2>{run.topic}</h2>
        </div>
        <span className="badge">{stateLabel(run.state)}</span>
      </div>

      {brief && (
        <section className="phase-card brief-card">
          <h3>调研摘要</h3>
          <dl className="brief-grid">
            <div>
              <dt>用户意图</dt>
              <dd>{brief.user_intent}</dd>
            </div>
            <div>
              <dt>目标读者</dt>
              <dd>{brief.target_audience}</dd>
            </div>
            <div>
              <dt>时间范围</dt>
              <dd>{brief.time_scope}</dd>
            </div>
          </dl>
          <div className="brief-lists">
            <ListBlock title="纳入标准" items={brief.inclusion_criteria} />
            <ListBlock title="排除标准" items={brief.exclusion_criteria} />
            <ListBlock title="成功标准" items={brief.success_criteria} />
          </div>
        </section>
      )}

      {run.plan_warnings.length > 0 && (
        <div className="warning-box">
          {run.plan_warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
        </div>
      )}

      <div className="plan-metrics" aria-label="计划概览">
        <Metric label="章节" value={chapterCount} />
        <Metric label="查询" value={queryCount} />
        <Metric label="GitHub 预算" value={run.policy.github_budget} />
        <Metric label="arXiv 预算" value={run.policy.arxiv_budget} />
      </div>

      <div className="aspect-list">
        {chapters
          .slice()
          .sort((a, b) => a.sort_order - b.sort_order)
          .map((chapter, index) => {
            const portfolio = ensurePortfolio(queryPortfolio, chapter.id);
            return (
              <article className="plan-card chapter-card" key={chapter.id}>
                <div className="plan-card-header">
                  <span className="step-number">{index + 1}</span>
                  <input
                    className="title-input"
                    value={chapter.title_zh}
                    disabled={!planReady || running}
                    onChange={(event) =>
                      patchChapter(chapter.id, { title_zh: event.target.value })
                    }
                  />
                </div>

                <label className="field">
                  <span>研究问题</span>
                  <textarea
                    rows={3}
                    value={chapter.research_question}
                    disabled={!planReady || running}
                    onChange={(event) =>
                      patchChapter(chapter.id, { research_question: event.target.value })
                    }
                  />
                </label>

                <div className="limit-grid">
                  <label className="field">
                  <span>GitHub 查询词</span>
                    <textarea
                      rows={4}
                      value={portfolio.github_queries.join("\n")}
                      disabled={!planReady || running}
                      onChange={(event) =>
                        patchPortfolio(chapter.id, {
                          github_queries: splitLines(event.target.value)
                        })
                      }
                    />
                  </label>
                  <label className="field">
                  <span>arXiv 查询词</span>
                    <textarea
                      rows={4}
                      value={portfolio.arxiv_queries.join("\n")}
                      disabled={!planReady || running}
                      onChange={(event) =>
                        patchPortfolio(chapter.id, {
                          arxiv_queries: splitLines(event.target.value)
                        })
                      }
                    />
                  </label>
                </div>

                <label className="field">
                  <span>规划理由</span>
                  <textarea
                    rows={3}
                    value={portfolio.rationale}
                    disabled={!planReady || running}
                    onChange={(event) =>
                      patchPortfolio(chapter.id, { rationale: event.target.value })
                    }
                  />
                </label>
              </article>
            );
          })}
      </div>

      <label className="field">
        <span>修订备注</span>
        <textarea
          rows={3}
          value={feedback}
          disabled={!planReady || running}
          onChange={(event) => setFeedback(event.target.value)}
          placeholder="例如：强化评测基准章节，降低泛化搜索词。"
        />
      </label>

      <div className="action-row">
        <button
          className="secondary-action"
          type="button"
          disabled={!planReady || running || chapters.length === 0}
          onClick={() => onRevise(chapters, queryPortfolio, feedback)}
        >
          保存计划修订
        </button>
        <button
          className="primary-action"
          type="button"
          disabled={!planReady || running || chapters.length === 0}
          onClick={onApprove}
        >
          批准并开始调研
        </button>
      </div>
    </div>
  );

  function patchChapter(id: string, patch: Partial<ChapterNode>) {
    setChapters((current) =>
      current.map((chapter) => (chapter.id === id ? { ...chapter, ...patch } : chapter))
    );
  }

  function patchPortfolio(id: string, patch: Partial<QueryPortfolio>) {
    setQueryPortfolio((current) => {
      const exists = current.some((item) => item.chapter_id === id);
      const next = current.map((item) =>
        item.chapter_id === id ? { ...item, ...patch } : item
      );
      if (exists) {
        return next;
      }
      return [
        ...next,
        {
          chapter_id: id,
          github_queries: [],
          arxiv_queries: [],
          rationale: "",
          budget: currentRun.policy.github_budget + currentRun.policy.arxiv_budget,
          ...patch
        }
      ];
    });
  }
}

function ListBlock({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <h3>{title}</h3>
      <ul>
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ensurePortfolio(items: QueryPortfolio[], chapterId: string): QueryPortfolio {
  return (
    items.find((item) => item.chapter_id === chapterId) ?? {
      chapter_id: chapterId,
      github_queries: [],
      arxiv_queries: [],
      rationale: "",
      budget: 1
    }
  );
}

function splitLines(value: string): string[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

function stateLabel(state: string) {
  return (
    {
      created: "未开始",
      plan_ready: "计划待确认",
      fetching: "抓取资料中",
      evidence_ready: "证据已整理",
      synthesis_ready: "报告已生成",
      completed: "已完成",
      failed: "失败"
    }[state] ?? state
  );
}

export default PlanTree;
