import { useEffect, useMemo, useState } from "react";
import {
  addReadingLibraryItem,
  branchStatefulRun,
  continueStatefulRunStream,
  getHealth,
  listReadingLibrary,
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
  EvidenceItem,
  EvidenceMemory,
  FrontendConfig,
  HealthResponse,
  QueryPortfolio,
  ReadingLibraryItem,
  ReadingLibrarySummary,
  ResearchRunRecord,
  StatefulRunResponse,
  StatefulRunStreamEvent
} from "./api/types";
import AgentControlPanel from "./components/AgentControlPanel";
import CitationAuditView from "./components/CitationAuditView";
import ConfigHalftoneVisual from "./components/ConfigHalftoneVisual";
import ConfigPanel from "./components/ConfigPanel";
import CoverageMatrix from "./components/CoverageMatrix";
import EvidenceMemoryView from "./components/EvidenceMemoryView";
import PlanTree from "./components/PlanTree";
import ReadingLibraryView from "./components/ReadingLibraryView";
import ReportView from "./components/ReportView";
import RunTimeline from "./components/RunTimeline";

type Stage = "config" | "research" | "library";
type Activity =
  | "idle"
  | "planning"
  | "revising"
  | "plan_ready"
  | "running"
  | "report_ready"
  | "error";
type ActiveView = "plan" | "evidence" | "coverage" | "audit" | "report";

const STORAGE_KEY = "litscout-rs-web-config";

function App() {
  const [stage, setStage] = useState<Stage>("config");
  const [config, setConfig] = useState<FrontendConfig>(() => loadConfig());
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [activeView, setActiveView] = useState<ActiveView>("plan");
  const [notice, setNotice] = useState<string | null>(null);
  const [activity, setActivity] = useState<Activity>("idle");
  const [progress, setProgress] = useState(0);
  const [displayProgress, setDisplayProgress] = useState(0);
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
  const [libraryItems, setLibraryItems] = useState<ReadingLibrarySummary[]>([]);
  const [activePaperKey, setActivePaperKey] = useState<string | null>(null);
  const [addingEvidenceId, setAddingEvidenceId] = useState<string | null>(null);

  useEffect(() => {
    getHealth()
      .then(setHealth)
      .catch((error: Error) => setNotice(error.message));
    refreshLibrary();
  }, []);

  const progressActive = activity === "planning" || activity === "revising" || activity === "running";
  const progressCap = useMemo(
    () => progressSoftCap(activity, agentRun?.state, progress),
    [activity, agentRun?.state, progress]
  );

  useEffect(() => {
    const timer = window.setInterval(() => {
      setDisplayProgress((current) => {
        const target = clampProgress(progress);
        const cap = clampProgress(progressActive ? progressCap : target);
        const distanceToTarget = target - current;

        if (Math.abs(distanceToTarget) < 0.2 && (!progressActive || current >= cap)) {
          return target;
        }

        if (current < target) {
          return clampProgress(current + Math.max(distanceToTarget * 0.2, 0.35));
        }

        if (current > target && (!progressActive || current > cap)) {
          return clampProgress(current - Math.max((current - target) * 0.18, 0.35));
        }

        if (progressActive && current < cap) {
          return clampProgress(current + Math.min(Math.max((cap - current) * 0.018, 0.08), 0.32));
        }

        return current;
      });
    }, 180);

    return () => window.clearInterval(timer);
  }, [progress, progressActive, progressCap]);

  const reportPreview = useMemo(() => {
    if (reportMarkdown) return reportMarkdown;
    if (!agentRun) return "";
    const chapters = agentRun.chapters
      .map((chapter, index) => {
        const portfolio = agentRun.query_portfolio.find(
          (item) => item.chapter_id === chapter.id
        );
        return `${index + 1}. ${chapter.title_zh}
   - 问题：${chapter.research_question}
   - GitHub：${(portfolio?.github_queries ?? []).map((q) => `\`${q}\``).join(" / ") || "—"}
   - arXiv：${(portfolio?.arxiv_queries ?? []).map((q) => `\`${q}\``).join(" / ") || "—"}`;
      })
      .join("\n\n");

    return `# 调研任务预览：${agentRun.topic}

## 调研摘要

- 用户意图：${agentRun.brief?.user_intent ?? "尚未生成"}
- 时间范围：${agentRun.brief?.time_scope ?? "尚未生成"}

## 章节计划

${chapters}

## 当前状态

当前任务处于「${stateLabel(agentRun.state)}」。批准计划后将启动 GitHub/arXiv 抓取与报告生成。`;
  }, [agentRun, reportMarkdown]);

  function handleConfigSaved(nextConfig: FrontendConfig) {
    setConfig(nextConfig);
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(compactSessionConfig(nextConfig)));
    setStage("research");
    setActivity("idle");
    setProgress(18);
    setProgressLabel("配置已保存到当前浏览器会话");
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
    setProgressLabel("计划待确认 — 请审查章节计划");
    setActiveView("plan");
    setNotice(null);
    refreshCheckpoints(response.run.run_id);
  }

  async function handleRevisePlan(
    chapters: ChapterNode[],
    queryPortfolio: QueryPortfolio[],
    feedback: string
  ) {
    if (!agentRun) { setNotice("请先创建调研任务。"); return; }
    setActivity("revising");
    setNotice(null);
    try {
      const response = await reviseStatefulPlan(agentRun.run_id, chapters, queryPortfolio, feedback);
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
    if (!agentRun) { setNotice("请先创建调研任务。"); return; }
    setActivity("running");
    setActiveView("report");
    setReportMarkdown("");
    setOutputPath(null);
    setProgress(52);
    setProgressLabel("已批准 — 正在抓取文献资料");
    setNotice(null);
    setEvents([]);

    try {
      const response = await continueStatefulRunStream(agentRun.run_id, config, (event) => {
        setEvents((current) => [...current, event]);
        applyStatefulEvent(event, setProgress, setProgressLabel, setAgentRun);
      });
      setAgentRun(response.run);
      setProgress(100);
      setProgressLabel("报告已生成");
      setActivity("report_ready");
      setOutputPath(response.run.output_report ?? null);
      setReportMarkdown(response.run.report_markdown ?? "");
      await refreshArtifacts(response.run.run_id);
      setActiveView("report");
    } catch (error) {
      setActivity("error");
      setProgressLabel("调研任务执行失败");
      setNotice(error instanceof Error ? error.message : "调研任务执行失败。");
      setEvents((current) => [
        ...current,
        { event: "run_failed", data: { error: error instanceof Error ? error.message : "unknown error" } }
      ]);
    }
  }

  async function handleTranslateReport() {
    if (!reportMarkdown) { setNotice("报告生成后才能翻译。"); return; }
    setTranslating(true);
    setNotice(null);
    try {
      const response = await translateReport(reportMarkdown, config);
      setReportMarkdown(response.translated_markdown);
      setNotice("报告已翻译为中文，并保留原始引用链接。");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "报告翻译失败。");
    } finally { setTranslating(false); }
  }

  async function handleBranch(checkpointId: string) {
    if (!agentRun) return;
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
      setProgressLabel("已从检查点创建新分支");
      setActiveView("plan");
      await refreshCheckpoints(response.run.run_id);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "创建分支失败。");
    } finally { setBranching(false); }
  }

  async function handleAddToLibrary(item: EvidenceItem) {
    if (!agentRun) { setNotice("请先创建调研任务。"); return; }
    setAddingEvidenceId(item.evidence_id);
    setNotice(null);
    try {
      const response = await addReadingLibraryItem(agentRun.run_id, item);
      await refreshLibrary();
      setActivePaperKey(response.item.paper_key);
      setNotice("论文已加入阅读库。");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "加入阅读库失败。");
    } finally {
      setAddingEvidenceId(null);
    }
  }

  function handleLibraryItemUpdated(item: ReadingLibraryItem) {
    setLibraryItems((current) =>
      current
        .map((summary) =>
          summary.paper_key === item.paper_key
            ? {
                paper_key: item.paper_key,
                source_item_id: item.source_item_id,
                evidence_id: item.evidence_id,
                run_id: item.run_id,
                title: item.title,
                abs_url: item.abs_url,
                pdf_url: item.pdf_url,
                summary: item.summary,
                added_at: item.added_at,
                updated_at: item.updated_at,
                status: item.status,
                text_coverage: item.text_coverage,
                text_meta: item.text_meta,
                note_quality: item.note_quality,
                has_note: Boolean(item.note),
                error: item.error
              }
            : summary
        )
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
    );
  }

  async function refreshArtifacts(runId: string) {
    const [evidence, coverage, audit, checkpointList] = await Promise.allSettled([
      getStatefulEvidence(runId),
      getStatefulCoverage(runId),
      getStatefulCitationAudit(runId),
      getStatefulCheckpoints(runId)
    ]);
    if (evidence.status === "fulfilled") setEvidenceMemory(evidence.value.evidence_memory);
    if (coverage.status === "fulfilled") setCoverageReport(coverage.value.coverage_report);
    if (audit.status === "fulfilled") setCitationAudit(audit.value.citation_audit);
    if (checkpointList.status === "fulfilled") setCheckpoints(checkpointList.value.checkpoints);
  }

  async function refreshCheckpoints(runId: string) {
    try {
      const response = await getStatefulCheckpoints(runId);
      setCheckpoints(response.checkpoints);
    } catch { setCheckpoints([]); }
  }

  async function refreshLibrary() {
    try {
      const response = await listReadingLibrary();
      setLibraryItems(response.items);
      setActivePaperKey((current) => current ?? response.items[0]?.paper_key ?? null);
    } catch {
      setLibraryItems([]);
    }
  }

  return (
    <main
      className={
        stage === "config"
          ? "app-shell config-only"
          : stage === "library"
            ? "app-shell library-mode"
            : "app-shell"
      }
    >
      {/* Rail navigation */}
      <aside className="rail" aria-label="阶段导航">
        <div className="brand-mark">LS</div>
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
          调研
        </button>
        <button
          className={stage === "library" ? "rail-step active" : "rail-step"}
          type="button"
          onClick={() => setStage("library")}
        >
          <span>03</span>
          阅读库
        </button>
      </aside>

      {stage !== "library" && (
        <section className="pane command-pane" aria-label="配置和输入">
          <div className="brand-row">
            <div>
              <p className="eyebrow">LitScout-RS</p>
              <h1>{stage === "config" ? "连接配置" : "文献调研"}</h1>
            </div>
            <span className={`status-dot ${health?.status === "ok" ? "ok" : ""}`} />
          </div>

          {stage === "config" ? (
            <ConfigPanel config={config} health={health} onSave={handleConfigSaved} onNotice={setNotice} />
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
                  setProgressLabel("正在生成调研摘要与章节计划");
                }
              }}
            />
          )}

          {notice && (
            <div className="notice-box error-tone" role="alert" style={{ marginTop: 16 }}>
              {notice}
            </div>
          )}
        </section>
      )}

      {stage === "config" && (
        <section className="pane config-visual-pane" aria-label="配置页视觉区域">
          <ConfigHalftoneVisual />
        </section>
      )}

      {/* Right pane: workspace */}
      {stage === "research" && (
        <section className="pane workspace-pane" aria-label="调研工作区">
          {/* Tabs */}
          <div className="tabbar agent-tabs" role="tablist">
            {([
              ["plan", "计划"],
              ["evidence", "证据"],
              ["coverage", "覆盖"],
              ["audit", "引用"],
              ["report", "报告"]
            ] as const).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={activeView === id}
                className={activeView === id ? "active" : ""}
                type="button"
                onClick={() => setActiveView(id)}
              >
                {label}
              </button>
            ))}
          </div>

          {/* Telemetry drawer */}
          <details className="telemetry-drawer">
            <summary>
              <div>
                <p className="eyebrow">运行状态</p>
                <h2>进度、检查点与事件</h2>
                <p className="telemetry-summary-hint">点击查看详细进度 · {progressLabel}</p>
              </div>
              <span className={progressActive ? "badge progress-badge is-active" : "badge progress-badge"}>
                {Math.round(displayProgress)}%
              </span>
            </summary>
            <div className="telemetry-grid">
              {/* Status card */}
              <div className="status-stack telemetry-card">
                <div
                  className={progressActive ? "progress-meter is-active" : "progress-meter"}
                  role="progressbar"
                  aria-label={progressLabel}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(displayProgress)}
                >
                  <div style={{ width: `${Math.min(displayProgress, 100)}%` }} />
                </div>
                <p className="progress-label">{progressLabel}</p>

                <dl className="status-list">
                  <div><dt>当前任务</dt><dd>{readableActivity(activity)}</dd></div>
                  <div><dt>本机服务</dt><dd>{health?.status === "ok" ? "运行中" : "未知"}</dd></div>
                  <div><dt>模型服务</dt><dd>{health?.llm_enabled ? "后端已启用" : "本页配置"}</dd></div>
                  <div><dt>GitHub 令牌</dt><dd>{health?.github_token_configured ? "后端已配置" : "按请求传入"}</dd></div>
                </dl>

                {outputPath && (
                  <div className="notice-box" role="status" aria-live="polite" style={{ marginTop: 8 }}>
                    报告已写入：{outputPath}
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
            </div>
          </details>

          {/* Active view */}
          {activeView === "plan" ? (
            <PlanTree run={agentRun} running={activity === "running"} onRevise={handleRevisePlan} onApprove={handleApproveRun} />
          ) : activeView === "evidence" ? (
            <EvidenceMemoryView
              run={agentRun}
              memory={evidenceMemory}
              libraryItems={libraryItems}
              addingEvidenceId={addingEvidenceId}
              onAddToLibrary={handleAddToLibrary}
            />
          ) : activeView === "coverage" ? (
            <CoverageMatrix run={agentRun} coverage={coverageReport} memory={evidenceMemory} />
          ) : activeView === "audit" ? (
            <CitationAuditView run={agentRun} audit={citationAudit} />
          ) : (
            <ReportView markdown={reportPreview} canTranslate={Boolean(reportMarkdown)} translating={translating} onTranslate={handleTranslateReport} />
          )}
        </section>
      )}
      {stage === "library" && (
        <section className="pane workspace-pane library-workspace" aria-label="阅读库工作区">
          {notice && (
            <div className="notice-box error-tone library-notice" role="alert">
              {notice}
            </div>
          )}
          <ReadingLibraryView
            config={config}
            items={libraryItems}
            activePaperKey={activePaperKey}
            onSelectPaper={setActivePaperKey}
            onItemsChange={setLibraryItems}
            onItemUpdated={handleLibraryItemUpdated}
            onNotice={setNotice}
          />
        </section>
      )}
    </main>
  );
}

/* ── helpers ── */

function applyStatefulEvent(
  event: StatefulRunStreamEvent,
  setProgress: (updater: (current: number) => number) => void,
  setProgressLabel: (label: string) => void,
  setAgentRun: (updater: (current: ResearchRunRecord | null) => ResearchRunRecord | null) => void
) {
  if (event.event === "agent") {
    const ae = event.data as { event: string; data?: Record<string, unknown> };
    if (ae.event === "state_changed") {
      const state = ae.data?.state as ResearchRunRecord["state"] | undefined;
      if (state) {
        setAgentRun((c) => (c ? { ...c, state } : c));
        setProgress((c) => Math.max(c, progressForState(state)));
        setProgressLabel(`${stateLabel(state)}：状态已推进`);
      }
    } else if (ae.event === "evidence_ready") {
      setProgress((c) => Math.max(c, 74));
      setProgressLabel(`证据库已生成，共 ${ae.data?.total ?? 0} 条`);
    } else if (ae.event === "coverage_ready") {
      setProgress((c) => Math.max(c, 80));
      setProgressLabel(`覆盖度检查完成，缺口 ${ae.data?.gaps ?? 0} 个`);
    } else if (ae.event === "citation_audit_ready") {
      setProgress((c) => Math.max(c, 92));
      setProgressLabel("引用检查完成");
    } else if (ae.event === "checkpoint_created") {
      setProgress((c) => Math.max(c, 58));
      setProgressLabel("检查点已保存");
    }
  }
  if (event.event === "run_ready") {
    const r = event.data as StatefulRunResponse;
    setAgentRun(() => r.run);
    setProgress((c) => Math.max(c, progressForState(r.run.state)));
    setProgressLabel(`${stateLabel(r.run.state)}：任务已更新`);
  }
}

function progressForState(state: ResearchRunRecord["state"]) {
  return (
    { created: 24, plan_ready: 45, fetching: 62, evidence_ready: 78, synthesis_ready: 92, completed: 100, failed: 100 }[state] ?? 0
  );
}

function progressSoftCap(activity: Activity, state: ResearchRunRecord["state"] | undefined, target: number) {
  if (activity === "planning") return Math.max(target, 42);
  if (activity === "revising") return Math.max(target, 52);
  if (activity !== "running") return target;

  const cap = (
    {
      created: 54,
      plan_ready: 58,
      fetching: 76,
      evidence_ready: 88,
      synthesis_ready: 97,
      completed: 100,
      failed: 100
    } satisfies Record<ResearchRunRecord["state"], number>
  )[state ?? "fetching"];

  return Math.max(target, cap);
}

function clampProgress(value: number) {
  return Math.max(0, Math.min(100, value));
}

function readableActivity(activity: Activity) {
  return (
    { idle: "空闲", planning: "正在生成计划", revising: "正在保存修订", plan_ready: "计划待批准", running: "正在执行调研任务", report_ready: "报告已生成", error: "出现错误" }[activity] ?? activity
  );
}

function stateLabel(state: string) {
  return (
    { created: "未开始", plan_ready: "计划待确认", fetching: "抓取资料中", evidence_ready: "证据已整理", synthesis_ready: "报告已生成", completed: "已完成", failed: "失败" }[state] ?? state
  );
}

function loadConfig(): FrontendConfig {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return { deepseek_base_url: "https://api.deepseek.com", deepseek_model: "deepseek-v4-pro", deepseek_side_model: "deepseek-v4-flash" };
    return { deepseek_base_url: "https://api.deepseek.com", deepseek_model: "deepseek-v4-pro", deepseek_side_model: "deepseek-v4-flash", ...JSON.parse(raw) };
  } catch {
    return { deepseek_base_url: "https://api.deepseek.com", deepseek_model: "deepseek-v4-pro", deepseek_side_model: "deepseek-v4-flash" };
  }
}

function compactSessionConfig(config: FrontendConfig): FrontendConfig {
  return Object.fromEntries(Object.entries(config).filter(([, v]) => typeof v === "string" && v.trim())) as FrontendConfig;
}

export default App;
