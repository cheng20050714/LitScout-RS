import { useState } from "react";
import type { FrontendConfig, RunPolicy, StatefulRunResponse } from "../api/types";
import { createStatefulRun } from "../api/client";

interface AgentControlPanelProps {
  config: FrontendConfig;
  hasServerLlm: boolean;
  onRunCreated: (response: StatefulRunResponse) => void;
  onNotice: (message: string | null) => void;
  onActivityChange: (activity: "idle" | "planning" | "plan_ready" | "error") => void;
}

function AgentControlPanel({
  config,
  hasServerLlm,
  onRunCreated,
  onNotice,
  onActivityChange
}: AgentControlPanelProps) {
  const [topic, setTopic] = useState("Rust Agent Framework 的开源项目和论文调研");
  const [githubBudget, setGithubBudget] = useState(10);
  const [arxivBudget, setArxivBudget] = useState(10);
  const [maxAspects, setMaxAspects] = useState(3);
  const [skipPlanCritic, setSkipPlanCritic] = useState(false);
  const [skipCoverageCritic, setSkipCoverageCritic] = useState(false);
  const [loading, setLoading] = useState(false);

  async function handleCreateRun() {
    const trimmed = topic.trim();
    if (!trimmed) {
      onNotice("请输入调研主题。");
      return;
    }
    if (!config.deepseek_api_key?.trim() && !hasServerLlm) {
      onNotice("请先在阶段 1 配置 DeepSeek API Key。");
      return;
    }

    const policy: RunPolicy = {
      max_research_rounds: 1,
      max_aspects_per_round: clamp(maxAspects, 1, 3),
      github_budget: clamp(githubBudget, 1, 50),
      arxiv_budget: clamp(arxivBudget, 1, 50),
      auto_approve_plan: false,
      allow_github_enrich: false,
      require_citation_audit: true,
      skip_plan_critic: skipPlanCritic,
      skip_coverage_critic: skipCoverageCritic,
      max_llm_calls_per_run: 10
    };

    setLoading(true);
    onActivityChange("planning");
    onNotice(null);
    try {
      const response = await createStatefulRun(trimmed, policy, config);
      onRunCreated(response);
      onActivityChange("plan_ready");
    } catch (error) {
      onActivityChange("error");
      onNotice(error instanceof Error ? error.message : "Agent Run 创建失败。");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="chat-stack">
      <label className="field">
        <span>中文研究问题</span>
        <textarea value={topic} onChange={(event) => setTopic(event.target.value)} rows={6} />
      </label>

      <div className="limit-grid">
        <label className="field">
          <span>GitHub 预算</span>
          <input
            min={1}
            max={50}
            type="number"
            value={githubBudget}
            onChange={(event) => setGithubBudget(Number(event.target.value))}
          />
        </label>
        <label className="field">
          <span>arXiv 预算</span>
          <input
            min={1}
            max={50}
            type="number"
            value={arxivBudget}
            onChange={(event) => setArxivBudget(Number(event.target.value))}
          />
        </label>
      </div>

      <label className="field">
        <span>章节上限</span>
        <input
          min={1}
          max={3}
          type="number"
          value={maxAspects}
          onChange={(event) => setMaxAspects(Number(event.target.value))}
        />
      </label>

      <div className="switch-row">
        <label>
          <input
            type="checkbox"
            checked={skipPlanCritic}
            onChange={(event) => setSkipPlanCritic(event.target.checked)}
          />
          跳过 PlanCritic
        </label>
        <label>
          <input
            type="checkbox"
            checked={skipCoverageCritic}
            onChange={(event) => setSkipCoverageCritic(event.target.checked)}
          />
          跳过 CoverageCritic
        </label>
      </div>

      {loading && (
        <div className="progress-banner" role="status" aria-live="polite">
          <span className="progress-spinner" />
          <span>正在生成 ResearchBrief 与 ChapterPlan</span>
        </div>
      )}

      <button className="primary-action" type="button" onClick={handleCreateRun} disabled={loading}>
        {loading ? "生成中" : "创建 Agent Run"}
      </button>
    </div>
  );
}

function clamp(value: number, min: number, max: number) {
  if (Number.isNaN(value)) {
    return min;
  }
  return Math.max(min, Math.min(max, value));
}

export default AgentControlPanel;
