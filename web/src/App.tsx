import { useEffect, useMemo, useState } from "react";
import { getHealth, runResearchStream, translateReport } from "./api/client";
import type { Citation, FrontendConfig, HealthResponse, PlanResponse, RunEvent } from "./api/types";
import ChatPanel from "./components/ChatPanel";
import ConfigPanel from "./components/ConfigPanel";
import PlanPanel from "./components/PlanPanel";
import ReportChat from "./components/ReportChat";
import ReportView from "./components/ReportView";
import StatusPanel from "./components/StatusPanel";

type Stage = "config" | "research";
type Activity =
  | "idle"
  | "planning"
  | "revising"
  | "plan_ready"
  | "running"
  | "report_ready"
  | "error";

const STORAGE_KEY = "litscout-rs-web-config";

function App() {
  const [stage, setStage] = useState<Stage>("config");
  const [config, setConfig] = useState<FrontendConfig>(() => loadConfig());
  const [plan, setPlan] = useState<PlanResponse | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [activeView, setActiveView] = useState<"plan" | "report" | "chat">("plan");
  const [notice, setNotice] = useState<string | null>(null);
  const [activity, setActivity] = useState<Activity>("idle");
  const [progress, setProgress] = useState(0);
  const [progressLabel, setProgressLabel] = useState("等待配置");
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [reportMarkdown, setReportMarkdown] = useState("");
  const [outputPath, setOutputPath] = useState<string | null>(null);
  const [citations, setCitations] = useState<Citation[]>([]);
  const [translating, setTranslating] = useState(false);

  useEffect(() => {
    getHealth()
      .then(setHealth)
      .catch((error: Error) => setNotice(error.message));
  }, []);

  const reportPreview = useMemo(() => {
    if (reportMarkdown) {
      return reportMarkdown;
    }
    if (!plan) {
      return "";
    }
    const aspects = plan.aspects
      .map(
        (aspect, index) =>
          `${index + 1}. ${aspect.name_zh}\n   - GitHub: \`${aspect.github_query}\`\n   - arXiv: \`${aspect.arxiv_query}\`\n   - ${aspect.rationale_zh}`
      )
      .join("\n");

    return `# LitScout-RS 调研报告预览：${plan.original_topic}

## 1. 搜索计划

${aspects}

## 2. 当前状态

报告尚未生成。点击“开始调研”后，系统会执行 GitHub/arXiv 检索并写入 Markdown 报告。`;
  }, [plan, reportMarkdown]);

  function handleConfigSaved(nextConfig: FrontendConfig) {
    setConfig(nextConfig);
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(maskEmpty(nextConfig)));
    setStage("research");
    setActivity("idle");
    setProgress(18);
    setProgressLabel("配置已保存");
    setNotice(null);
  }

  async function handleRunStart() {
    if (!plan) {
      setNotice("请先生成搜索计划。");
      return;
    }
    setActivity("running");
    setActiveView("report");
    setReportMarkdown("");
    setOutputPath(null);
    setCitations([]);
    setProgress(24);
    setProgressLabel("准备执行 GitHub/arXiv 调研");
    setNotice(null);
    setEvents([]);

    try {
      const response = await runResearchStream(plan, config, (event) => {
        setEvents((current) => [...current, event]);
        applyRunEvent(event, setProgress, setProgressLabel);
      });
      setProgress(100);
      setProgressLabel("报告已生成");
      setActivity("report_ready");
      setOutputPath(response.output_report);
      setReportMarkdown(response.report_markdown);
      setCitations(response.citations ?? []);
    } catch (error) {
      setActivity("error");
      setProgressLabel("调研失败");
      setNotice(error instanceof Error ? error.message : "调研执行失败。");
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
          调研
        </button>
      </aside>

      <section className="pane command-pane" aria-label="配置和输入">
        <div className="brand-row">
          <div>
            <p className="eyebrow">LitScout-RS</p>
            <h1>中文研究侦察台</h1>
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
          <ChatPanel
            config={config}
            currentPlan={plan}
            hasServerLlm={Boolean(health?.llm_enabled)}
            onPlanGenerated={(nextPlan) => {
              setPlan(nextPlan);
              setActivity("plan_ready");
              setProgress(45);
              setProgressLabel("搜索计划已生成");
              setActiveView("plan");
              setNotice(null);
            }}
            onNotice={setNotice}
            onActivityChange={(nextActivity) => {
              setActivity(nextActivity);
              if (nextActivity === "planning") {
                setProgress(32);
                setProgressLabel("DeepSeek 正在生成搜索计划");
              }
            }}
          />
        )}
      </section>

      <section className="pane workspace-pane" aria-label="搜索计划和报告">
        <div className="tabbar" role="tablist">
          <button
            role="tab"
            aria-selected={activeView === "plan"}
            className={activeView === "plan" ? "active" : ""}
            type="button"
            onClick={() => setActiveView("plan")}
          >
            搜索计划
          </button>
          <button
            role="tab"
            aria-selected={activeView === "report"}
            className={activeView === "report" ? "active" : ""}
            type="button"
            onClick={() => setActiveView("report")}
          >
            报告
          </button>
          <button
            role="tab"
            aria-selected={activeView === "chat"}
            className={activeView === "chat" ? "active" : ""}
            type="button"
            onClick={() => setActiveView("chat")}
          >
            追问
          </button>
        </div>
        {activeView === "plan" ? (
          <PlanPanel plan={plan} onPlanChange={setPlan} onRunStart={handleRunStart} />
        ) : activeView === "report" ? (
          <ReportView
            markdown={reportPreview}
            canTranslate={Boolean(reportMarkdown)}
            translating={translating}
            onTranslate={handleTranslateReport}
          />
        ) : (
          <ReportChat
            config={config}
            reportMarkdown={reportMarkdown}
            disabled={!reportMarkdown}
            onNotice={setNotice}
          />
        )}
      </section>

      <section className="pane status-pane" aria-label="运行状态和引用">
        <StatusPanel
          health={health}
          plan={plan}
          notice={notice}
          activity={activity}
          progress={progress}
          progressLabel={progressLabel}
          outputPath={outputPath}
          events={events}
          citations={citations}
        />
      </section>
    </main>
  );
}

function applyRunEvent(
  event: RunEvent,
  setProgress: (updater: (current: number) => number) => void,
  setProgressLabel: (label: string) => void
) {
  if (event.event === "fetch_started") {
    const data = event.data as { data?: { source?: string } };
    setProgress((current) => Math.max(current, 34));
    setProgressLabel(`开始抓取 ${data.data?.source ?? "来源"}`);
  } else if (event.event === "source_finished") {
    const data = event.data as { data?: { source?: string; count?: number } };
    setProgress((current) => Math.max(current, 55));
    setProgressLabel(`${data.data?.source ?? "来源"} 完成，获得 ${data.data?.count ?? 0} 条`);
  } else if (event.event === "ranking_finished") {
    setProgress((current) => Math.max(current, 68));
    setProgressLabel("去重与排序完成");
  } else if (event.event === "classification_finished") {
    setProgress((current) => Math.max(current, 76));
    setProgressLabel("分类与主题聚类完成");
  } else if (event.event === "synthesis_started") {
    setProgress((current) => Math.max(current, 86));
    setProgressLabel("DeepSeek 正在生成中文分析");
  } else if (event.event === "quality_warning") {
    setProgress((current) => Math.max(current, 88));
    setProgressLabel("质量门发现警告");
  }
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
