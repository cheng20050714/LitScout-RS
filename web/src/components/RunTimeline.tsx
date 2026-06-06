import type { Checkpoint, ResearchRunRecord, ResearchRunState, StatefulRunStreamEvent } from "../api/types";

interface RunTimelineProps {
  run: ResearchRunRecord | null;
  events: StatefulRunStreamEvent[];
  checkpoints: Checkpoint[];
  branching: boolean;
  onBranch: (checkpointId: string) => void;
}

const STATES: ResearchRunState[] = [
  "created",
  "plan_ready",
  "fetching",
  "evidence_ready",
  "synthesis_ready",
  "completed"
];

function RunTimeline({ run, events, checkpoints, branching, onBranch }: RunTimelineProps) {
  const activeIndex = run ? STATES.indexOf(run.state) : -1;
  const isFailed = run?.state === "failed";

  return (
    <div className="status-stack">
      {/* Timeline */}
      <section>
        <div className="section-header small">
          <h3>任务进度</h3>
          <span className="badge">{run ? stateLabel(run.state) : "待创建"}</span>
        </div>
        <ol className="timeline-list">
          {STATES.map((state, index) => {
            const done = index < activeIndex;
            const active = index === activeIndex;
            const blocked = isFailed && index > activeIndex;
            return (
              <li key={state} className={done ? "done" : active ? "active" : blocked ? "blocked" : ""}>
                <span className="step-num">{index + 1}</span>
                <strong>{stateLabel(state)}</strong>
              </li>
            );
          })}
        </ol>
      </section>

      {/* Checkpoints */}
      <section>
        <div className="section-header small">
          <h3>版本检查点</h3>
          <span className="badge">{checkpoints.length}</span>
        </div>
        {checkpoints.length === 0 ? (
          <p className="muted">暂无检查点 — 运行完成后自动创建。</p>
        ) : (
          <div className="checkpoint-list">
            {checkpoints.map((cp) => (
              <article key={cp.checkpoint_id} className="checkpoint-row">
                <div>
                  <strong style={{ fontSize: 13 }}>{stateLabel(cp.state)}</strong>
                  <p className="muted">{new Date(cp.created_at).toLocaleString("zh-CN")}</p>
                </div>
                {cp.rollback_allowed && (
                  <button
                    className="secondary-action"
                    type="button"
                    disabled={branching}
                    onClick={() => onBranch(cp.checkpoint_id)}
                    style={{ fontSize: 12, minHeight: 30, padding: "4px 10px" }}
                  >
                    {branching ? "创建中…" : "新建分支"}
                  </button>
                )}
              </article>
            ))}
          </div>
        )}
      </section>

      {/* Event log */}
      <details className="event-log" style={{ padding: 0, border: "none", background: "transparent" }}>
        <summary style={{
          cursor: "pointer", padding: "8px 0", fontWeight: 700, fontSize: 13,
          color: "var(--ink-muted)", listStyle: "none"
        }}>
          事件记录 ({events.length})
        </summary>
        {events.length === 0 ? (
          <p className="muted">等待任务事件…</p>
        ) : (
          <div style={{ display: "grid", gap: 4 }}>
            {events.slice(-10).reverse().map((event, index) => (
              <pre key={`${event.event}-${index}`} style={{ margin: 0, fontSize: 10 }}>
                {JSON.stringify(event, null, 1)}
              </pre>
            ))}
          </div>
        )}
      </details>
    </div>
  );
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

export default RunTimeline;
