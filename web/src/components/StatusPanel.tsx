import type { Citation, HealthResponse, PlanResponse, RunEvent } from "../api/types";
import CitationLedger from "./CitationLedger";

interface StatusPanelProps {
  health: HealthResponse | null;
  plan: PlanResponse | null;
  notice: string | null;
  activity: string;
  progress: number;
  progressLabel: string;
  outputPath: string | null;
  events: RunEvent[];
  citations: Citation[];
}

function StatusPanel({
  health,
  plan,
  notice,
  activity,
  progress,
  progressLabel,
  outputPath,
  events,
  citations
}: StatusPanelProps) {
  const readableActivity =
    {
      idle: "空闲",
      planning: "正在生成搜索计划",
      plan_ready: "搜索计划已生成",
      running: "正在执行 GitHub/arXiv 调研",
      report_ready: "报告已生成",
      error: "出现错误"
    }[activity] ?? activity;

  return (
    <div className="status-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">Telemetry</p>
          <h2>运行遥测</h2>
        </div>
        <span className="badge">{Math.round(progress)}%</span>
      </div>

      <div
        className="progress-meter"
        role="progressbar"
        aria-label={progressLabel}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress)}
      >
        <div style={{ width: `${Math.min(progress, 100)}%` }} />
      </div>
      <p className="progress-label">{progressLabel}</p>

      <dl className="status-list">
        <div>
          <dt>当前任务</dt>
          <dd>{readableActivity}</dd>
        </div>
        <div>
          <dt>服务</dt>
          <dd>{health?.status ?? "unknown"}</dd>
        </div>
        <div>
          <dt>DeepSeek</dt>
          <dd>{health?.llm_enabled ? "后端启用" : "前端配置"}</dd>
        </div>
        <div>
          <dt>GitHub Token</dt>
          <dd>{health?.github_token_configured ? "后端已配置" : "按请求传入"}</dd>
        </div>
      </dl>

      {outputPath && (
        <div className="notice-box" role="status" aria-live="polite">
          报告已写入：{outputPath}
        </div>
      )}
      {notice && (
        <div className="notice-box error-tone" role="alert">
          {notice}
        </div>
      )}

      <div className="event-log">
        <h3>事件流</h3>
        {events.length === 0 ? (
          <p className="muted">等待用户操作</p>
        ) : (
          events.map((event, index) => (
            <pre key={`${event.event}-${index}`}>{JSON.stringify(event, null, 2)}</pre>
          ))
        )}
      </div>

      <CitationLedger citations={citations} plan={plan} />
    </div>
  );
}

export default StatusPanel;
