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

  return (
    <div className="status-stack">
      <section className="phase-card">
        <div className="section-header">
          <div>
            <p className="eyebrow">任务进度</p>
            <h2>{run?.run_id ?? "未创建"}</h2>
          </div>
          <span className="badge">{run ? stateLabel(run.state) : "Idle"}</span>
        </div>
        <ol className="timeline-list">
          {STATES.map((state, index) => (
            <li
              key={state}
              className={
                index < activeIndex
                  ? "done"
                  : index === activeIndex
                    ? "active"
                    : run?.state === "failed"
                      ? "blocked"
                      : ""
              }
            >
              <span>{index + 1}</span>
              <strong>{stateLabel(state)}</strong>
            </li>
          ))}
        </ol>
      </section>

      <section className="phase-card">
        <div className="section-header">
          <h2>版本检查点</h2>
          <span className="badge">{checkpoints.length}</span>
        </div>
        {checkpoints.length === 0 ? (
          <p className="muted">暂无检查点。</p>
        ) : (
          <div className="checkpoint-list">
            {checkpoints.map((checkpoint) => (
              <article key={checkpoint.checkpoint_id} className="checkpoint-row">
                <div>
                  <strong>{stateLabel(checkpoint.state)}</strong>
                  <p className="muted">{new Date(checkpoint.created_at).toLocaleString()}</p>
                </div>
                <button
                  className="secondary-action"
                  type="button"
                  disabled={!checkpoint.rollback_allowed || branching}
                  onClick={() => onBranch(checkpoint.checkpoint_id)}
                >
                  从这里新建分支
                </button>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="event-log">
        <h3>事件记录</h3>
        {events.length === 0 ? (
          <p className="muted">等待任务事件</p>
        ) : (
          events.slice(-8).map((event, index) => (
            <pre key={`${event.event}-${index}`}>{JSON.stringify(event, null, 2)}</pre>
          ))
        )}
      </section>
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
