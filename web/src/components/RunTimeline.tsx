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
            <p className="eyebrow">Run Timeline</p>
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
          <h2>Checkpoints</h2>
          <span className="badge">{checkpoints.length}</span>
        </div>
        {checkpoints.length === 0 ? (
          <p className="muted">暂无 checkpoint。</p>
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
                  创建分支
                </button>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="event-log">
        <h3>Stateful 事件流</h3>
        {events.length === 0 ? (
          <p className="muted">等待 Agent 事件</p>
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
      created: "Created",
      plan_ready: "PlanReady",
      fetching: "Fetching",
      evidence_ready: "EvidenceReady",
      synthesis_ready: "SynthesisReady",
      completed: "Completed",
      failed: "Failed"
    }[state] ?? state
  );
}

export default RunTimeline;
