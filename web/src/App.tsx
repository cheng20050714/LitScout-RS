import { useEffect, useMemo, useState } from "react";
import {
  branchStatefulRun,
  continueStatefulRunStream,
  getHealth,
  getStatefulCheckpoints,
  getStatefulCitationAudit,
  getStatefulCoverage,
  getStatefulEvidence,
  reviseStatefulPlan,
  translateReport
} from "./api/client";
import type {
  ChapterNode,
  Checkpoint,
  CitationAuditReport,
  CoverageReport,
  EvidenceMemory,
  FrontendConfig,
  HealthResponse,
  QueryPortfolio,
  ResearchRunRecord,
  StatefulRunResponse,
  StatefulRunStreamEvent
} from "./api/types";
import AgentControlPanel from "./components/AgentControlPanel";
import AgentFollowup from "./components/AgentFollowup";
import CitationAuditView from "./components/CitationAuditView";
import ConfigPanel from "./components/ConfigPanel";
import CoverageMatrix from "./components/CoverageMatrix";
import EvidenceMemoryView from "./components/EvidenceMemoryView";
import PlanTree from "./components/PlanTree";
import ReportView from "./components/ReportView";
import RunTimeline from "./components/RunTimeline";

type Stage = "config" | "research";
type Activity =
  | "idle"
  | "planning"
  | "revising"
  | "plan_ready"
  | "running"
  | "report_ready"
  | "error";
type ActiveView = "plan" | "evidence" | "coverage" | "audit" | "report" | "chat";

const STORAGE_KEY = "litscout-rs-web-config";

function App() {
  const [stage, setStage] = useState<Stage>("config");
  const [config, setConfig] = useState<FrontendConfig>(() => loadConfig());
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [activeView, setActiveView] = useState<ActiveView>("plan");
  const [notice, setNotice] = useState<string | null>(null);
  const [activity, setActivity] = useState<Activity>("idle");
  const [progress, setProgress] = useState(0);
  const [progressLabel, setProgressLabel] = useState("等待配置");
  const [agentRun, setAgentRun] = useState<ResearchRunRecord | null>(null);
  const [events, setEvents] = useState<StatefulRunStreamEvent[]>([]);
  const [evidenceMemory, setEvidenceMemory] = useState<EvidenceMemory | null>(null);
  const [coverageReport, setCoverageReport] = useState<CoverageReport | null>(null);
  const [citationAudit, setCitationAudit] = useState<CitationAuditReport | null>(null);
  const [checkpoints, setCheckpoints] = useState<Checkpoint[]>([]);
  const [reportMarkdown, setReportMarkdown] = useState("");
  const [outputPath, setOutputPath] = useState<string | null>(null);
  const [translating, setTranslating] = useState(false);
  const [branching, setBranching] = useState(false);

  useEffect(() => {
    getHealth()
      .then(setHealth)
      .catch((error: Error) => setNotice(error.message));
  }, []);

  const reportPreview = useMemo(() => {
    if (reportMarkdown) {
      return reportMarkdown;
    }
    if (!agentRun) {
      return "";
    }
    const chapters = agentRun.chapters
      .map((chapter, index) => {
        const portfolio = agentRun.query_portfolio.find(
          (item) => item.chapter_id === chapter.id
        );
        return `${index + 1}. ${chapter.title_zh}
   - 问题：${chapter.research_question}
   - GitHub：${(portfolio?.github_queries ?? []).map((query) => `\`${query}\``).join(" / ")}
   - arXiv：${(portfolio?.arxiv_queries ?? []).map((query) => `\`${query}\``).join(" / ")}`;
      })
      .join("\n\n");

    return `# Stage 3 Agent Run 预览：${agentRun.topic}

## ResearchBrief

- 用户意图：${agentRun.brief?.user_intent ?? "尚未生成"}
- 时间范围：${agentRun.brief?.time_scope ?? "尚未生成"}

## ChapterPlan

${chapters}

## 当前状态

状态机处于 \`${agentRun.state}\`。批准计划后将执行 GitHub/arXiv 抓取、EvidenceMemory 构建、CoverageCritic、Writer 和 CitationAuditor。`;
  }, [agentRun, reportMarkdown]);

  function handleConfigSaved(nextConfig: FrontendConfig) {
    setConfig(nextConfig);
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(maskEmpty(nextConfig)));
    setStage("research");
    setActivity("idle");
    setProgress(18);
    setProgressLabel("配置已保存");
    setNotice(null);
  }

  function handleRunCreated(response: StatefulRunResponse) {
    setAgentRun(response.run);
    setEvidenceMemory(null);
    setCoverageReport(null);
    setCitationAudit(null);
    setReportMarkdown(response.run.report_markdown ?? "");
    setOutputPath(response.run.output_report ?? null);
    setEvents([]);
    setProgress(45);
    setProgressLabel("PlanReady：等待用户审查");
    setActiveView("plan");
    setNotice(null);
    refreshCheckpoints(response.run.run_id);
  }

  async function handleRevisePlan(
    chapters: ChapterNode[],
    queryPortfolio: QueryPortfolio[],
    feedback: string
  ) {
    if (!agentRun) {
      setNotice("请先创建 Agent Run。");
      return;
    }
    setActivity("revising");
    setNotice(null);
    try {
      const response = await reviseStatefulPlan(
        agentRun.run_id,
        chapters,
        queryPortfolio,
        feedback
      );
      setAgentRun(response.run);
      setProgress(48);
      setProgressLabel("计划修订已保存");
      setActivity("plan_ready");
      await refreshCheckpoints(response.run.run_id);
    } catch (error) {
      setActivity("error");
      setNotice(error instanceof Error ? error.message : "计划修订失败。");
    }
  }

  async function handleApproveRun() {
    if (!agentRun) {
      setNotice("请先创建 Agent Run。");
      return;
    }
    setActivity("running");
    setActiveView("report");
    setReportMarkdown("");
    setOutputPath(null);
    setProgress(52);
    setProgressLabel("已批准计划，准备执行抓取");
    setNotice(null);
    setEvents([]);

    try {
      const response = await continueStatefulRunStream(agentRun.run_id, (event) => {
        setEvents((current) => [...current, event]);
        applyStatefulEvent(event, setProgress, setProgressLabel, setAgentRun);
      });
      setAgentRun(response.run);
      setProgress(100);
      setProgressLabel("Completed：报告已生成");
      setActivity("report_ready");
      setOutputPath(response.run.output_report ?? null);
      setReportMarkdown(response.run.report_markdown ?? "");
      await refreshArtifacts(response.run.run_id);
      setActiveView("report");
    } catch (error) {
      setActivity("error");
      setProgressLabel("Agent Run 失败");
      setNotice(error instanceof Error ? error.message : "Agent Run 执行失败。");
      setEvents((current) => [
        ...current,
        {
          event: "run_failed",
          data: { error: error instanceof Error ? error.message : "unknown error" }
        }
      ]);
    }
  }

  async function handleTranslateReport() {
    if (!reportMarkdown) {
      setNotice("报告生成后才能翻译。");
      return;
    }
    setTranslating(true);
    setNotice(null);
    try {
      const response = await translateReport(reportMarkdown, config);
      setReportMarkdown(response.translated_markdown);
      setNotice("报告已翻译为中文，并保留原始引用链接。");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "报告翻译失败。");
    } finally {
      setTranslating(false);
    }
  }

  async function handleBranch(checkpointId: string) {
    if (!agentRun) {
      return;
    }
    setBranching(true);
    setNotice(null);
    try {
      const response = await branchStatefulRun(agentRun.run_id, checkpointId);
      setAgentRun(response.run);
      setEvidenceMemory(null);
      setCoverageReport(null);
      setCitationAudit(null);
      setReportMarkdown("");
      setOutputPath(null);
      setEvents([]);
      setProgress(45);
      setProgressLabel("已从 PlanReady checkpoint 创建新分支");
      setActiveView("plan");
      await refreshCheckpoints(response.run.run_id);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "创建分支失败。");
    } finally {
      setBranching(false);
    }
  }

  async function refreshArtifacts(runId: string) {
    const [evidence, coverage, audit, checkpointList] = await Promise.allSettled([
      getStatefulEvidence(runId),
      getStatefulCoverage(runId),
      getStatefulCitationAudit(runId),
      getStatefulCheckpoints(runId)
    ]);
    if (evidence.status === "fulfilled") {
      setEvidenceMemory(evidence.value.evidence_memory);
    }
    if (coverage.status === "fulfilled") {
      setCoverageReport(coverage.value.coverage_report);
    }
    if (audit.status === "fulfilled") {
      setCitationAudit(audit.value.citation_audit);
    }
    if (checkpointList.status === "fulfilled") {
      setCheckpoints(checkpointList.value.checkpoints);
    }
  }

  async function refreshCheckpoints(runId: string) {
    try {
      const response = await getStatefulCheckpoints(runId);
      setCheckpoints(response.checkpoints);
    } catch {
      setCheckpoints([]);
    }
  }

  return (
    <main className="app-shell">
      <aside className="rail" aria-label="阶段导航">
        <div className="brand-mark">
          <span>LS</span>
        </div>
        <button
          className={stage === "config" ? "rail-step active" : "rail-step"}
          type="button"
          onClick={() => setStage("config")}
        >
          <span>01</span>
          配置
        </button>
        <button
          className={stage === "research" ? "rail-step active" : "rail-step"}
          type="button"
          onClick={() => setStage("research")}
        >
          <span>02</span>
          Agent
        </button>
      </aside>

      <section className="pane command-pane" aria-label="配置和输入">
        <div className="brand-row">
          <div>
            <p className="eyebrow">LitScout-RS</p>
            <h1>有状态研究控制台</h1>
          </div>
          <span className={`status-dot ${health?.status === "ok" ? "ok" : ""}`} />
        </div>

        {stage === "config" ? (
          <ConfigPanel
            config={config}
            health={health}
            onSave={handleConfigSaved}
            onNotice={setNotice}
          />
        ) : (
          <AgentControlPanel
            config={config}
            hasServerLlm={Boolean(health?.llm_enabled)}
            onRunCreated={handleRunCreated}
            onNotice={setNotice}
            onActivityChange={(nextActivity) => {
              setActivity(nextActivity);
              if (nextActivity === "planning") {
                setProgress(32);
                setProgressLabel("正在生成 ResearchBrief 与 ChapterPlan");
              }
            }}
          />
        )}
      </section>

      <section className="pane workspace-pane" aria-label="Agent 工作区">
        <div className="tabbar agent-tabs" role="tablist">
          {[
            ["plan", "计划"],
            ["evidence", "证据"],
            ["coverage", "覆盖"],
            ["audit", "审计"],
            ["report", "报告"],
            ["chat", "追问"]
          ].map(([id, label]) => (
            <button
              key={id}
              role="tab"
              aria-selected={activeView === id}
              className={activeView === id ? "active" : ""}
              type="button"
              onClick={() => setActiveView(id as ActiveView)}
            >
              {label}
            </button>
          ))}
        </div>

        {activeView === "plan" ? (
          <PlanTree
            run={agentRun}
            running={activity === "running"}
            onRevise={handleRevisePlan}
            onApprove={handleApproveRun}
          />
        ) : activeView === "evidence" ? (
          <EvidenceMemoryView run={agentRun} memory={evidenceMemory} />
        ) : activeView === "coverage" ? (
          <CoverageMatrix run={agentRun} coverage={coverageReport} memory={evidenceMemory} />
        ) : activeView === "audit" ? (
          <CitationAuditView run={agentRun} audit={citationAudit} />
        ) : activeView === "report" ? (
          <ReportView
            markdown={reportPreview}
            canTranslate={Boolean(reportMarkdown)}
            translating={translating}
            onTranslate={handleTranslateReport}
          />
        ) : (
          <AgentFollowup run={agentRun} onNotice={setNotice} />
        )}
      </section>

      <section className="pane status-pane" aria-label="运行状态和 checkpoint">
        <div className="status-stack">
          <div className="section-header">
            <div>
              <p className="eyebrow">Telemetry</p>
              <h2>状态遥测</h2>
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
              <dd>{readableActivity(activity)}</dd>
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
        </div>

        <RunTimeline
          run={agentRun}
          events={events}
          checkpoints={checkpoints}
          branching={branching}
          onBranch={handleBranch}
        />
      </section>
    </main>
  );
}

function applyStatefulEvent(
  event: StatefulRunStreamEvent,
  setProgress: (updater: (current: number) => number) => void,
  setProgressLabel: (label: string) => void,
  setAgentRun: (updater: (current: ResearchRunRecord | null) => ResearchRunRecord | null) => void
) {
  if (event.event === "agent") {
    const agentEvent = event.data as { event: string; data?: Record<string, unknown> };
    if (agentEvent.event === "state_changed") {
      const state = agentEvent.data?.state as ResearchRunRecord["state"] | undefined;
      if (state) {
        setAgentRun((current) => (current ? { ...current, state } : current));
        setProgress((current) => Math.max(current, progressForState(state)));
        setProgressLabel(`${stateLabel(state)}：状态已推进`);
      }
    } else if (agentEvent.event === "evidence_ready") {
      setProgress((current) => Math.max(current, 74));
      setProgressLabel(`EvidenceMemory 已生成，共 ${agentEvent.data?.total ?? 0} 条`);
    } else if (agentEvent.event === "coverage_ready") {
      setProgress((current) => Math.max(current, 80));
      setProgressLabel(`CoverageCritic 完成，缺口 ${agentEvent.data?.gaps ?? 0} 个`);
    } else if (agentEvent.event === "citation_audit_ready") {
      setProgress((current) => Math.max(current, 92));
      setProgressLabel("CitationAuditor 完成");
    } else if (agentEvent.event === "checkpoint_created") {
      setProgress((current) => Math.max(current, 58));
      setProgressLabel("Checkpoint 已写入");
    }
  }
  if (event.event === "run_ready") {
    const response = event.data as StatefulRunResponse;
    setAgentRun(() => response.run);
    setProgress((current) => Math.max(current, progressForState(response.run.state)));
    setProgressLabel(`${stateLabel(response.run.state)}：run 已更新`);
  }
}

function progressForState(state: ResearchRunRecord["state"]) {
  return (
    {
      created: 24,
      plan_ready: 45,
      fetching: 62,
      evidence_ready: 78,
      synthesis_ready: 92,
      completed: 100,
      failed: 100
    }[state] ?? 0
  );
}

function readableActivity(activity: Activity) {
  return (
    {
      idle: "空闲",
      planning: "正在生成计划",
      revising: "正在保存修订",
      plan_ready: "计划待批准",
      running: "正在执行 Agent Run",
      report_ready: "报告已生成",
      error: "出现错误"
    }[activity] ?? activity
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

function loadConfig(): FrontendConfig {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return {
        deepseek_base_url: "https://api.deepseek.com",
        deepseek_model: "deepseek-v4-pro",
        deepseek_side_model: "deepseek-v4-flash"
      };
    }
    return {
      deepseek_base_url: "https://api.deepseek.com",
      deepseek_model: "deepseek-v4-pro",
      deepseek_side_model: "deepseek-v4-flash",
      ...JSON.parse(raw)
    };
  } catch {
    return {
      deepseek_base_url: "https://api.deepseek.com",
      deepseek_model: "deepseek-v4-pro",
      deepseek_side_model: "deepseek-v4-flash"
    };
  }
}

function maskEmpty(config: FrontendConfig): FrontendConfig {
  return Object.fromEntries(
    Object.entries(config).filter(([, value]) => typeof value === "string" && value.trim())
  ) as FrontendConfig;
}

export default App;
