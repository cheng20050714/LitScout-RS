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

const EXAMPLE_TOPICS = [
  "Rust 智能体框架的开源项目和论文调研",
  "可控文本转语音 (TTS) 的最新进展",
  "大语言模型评估基准综述",
  "多智能体强化学习通信机制"
];

function AgentControlPanel({
  config,
  hasServerLlm,
  onRunCreated,
  onNotice,
  onActivityChange
}: AgentControlPanelProps) {
  const [topic, setTopic] = useState("");
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
      onNotice("请先在配置页填写模型服务 API Key。");
      return;
    }

    const policy: RunPolicy = {
      max_research_rounds: 1,
      max_aspects_per_round: clamp(maxAspects, 1, 3),
      github_budget: clamp(githubBudget, 1, 50),
      arxiv_budget: clamp(arxivBudget, 1, 50),
      auto_approve_plan: false,
      allow_github_enrich: true,
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
      onNotice(error instanceof Error ? error.message : "调研任务创建失败。");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="chat-stack">
      {/* Topic input with examples */}
      <div className="config-group">
        <div className="config-group-label">
          <span className="step-number">1</span>
          <strong>调研主题</strong>
        </div>

        <label className="field">
          <textarea
            value={topic}
            onChange={(event) => setTopic(event.target.value)}
            rows={4}
            placeholder="用中文描述你想调研的技术主题，例如「TTS 入门级项目简单调研」"
          />
        </label>

        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 10 }}>
          {EXAMPLE_TOPICS.map((t) => (
            <button
              key={t}
              type="button"
              className="secondary-action"
              style={{ fontSize: 11, padding: "4px 10px", minHeight: 28 }}
              onClick={() => setTopic(t)}
            >
              {t.length > 28 ? t.slice(0, 28) + "…" : t}
            </button>
          ))}
        </div>
      </div>

      {/* Budget & settings */}
      <div className="config-group">
        <div className="config-group-label">
          <span className="step-number">2</span>
          <strong>检索预算</strong>
        </div>

        <div className="limit-grid">
          <label className="field">
            <span>GitHub 仓库上限</span>
            <input
              min={1}
              max={50}
              type="number"
              value={githubBudget}
              onChange={(event) => setGithubBudget(Number(event.target.value))}
            />
          </label>
          <label className="field">
            <span>arXiv 论文上限</span>
            <input
              min={1}
              max={50}
              type="number"
              value={arxivBudget}
              onChange={(event) => setArxivBudget(Number(event.target.value))}
            />
          </label>
        </div>

        <label className="field" style={{ marginTop: 12 }}>
          <span>章节数量上限</span>
          <input
            min={1}
            max={3}
            type="number"
            value={maxAspects}
            onChange={(event) => setMaxAspects(Number(event.target.value))}
          />
        </label>

        <div className="switch-row" style={{ marginTop: 12 }}>
          <label>
            <input
              type="checkbox"
              checked={skipPlanCritic}
              onChange={(event) => setSkipPlanCritic(event.target.checked)}
            />
            跳过计划质量检查
          </label>
          <label>
            <input
              type="checkbox"
              checked={skipCoverageCritic}
              onChange={(event) => setSkipCoverageCritic(event.target.checked)}
            />
            跳过证据覆盖检查
          </label>
        </div>
      </div>

      {/* Loading state */}
      {loading && (
        <div className="progress-banner" role="status" aria-live="polite">
          <span className="progress-spinner" />
          <span>正在生成调研摘要与章节计划…</span>
        </div>
      )}

      <button
        className="primary-action"
        type="button"
        onClick={handleCreateRun}
        disabled={loading}
        style={{ alignSelf: "flex-start" }}
      >
        {loading ? "生成中…" : "创建调研任务"}
      </button>
    </div>
  );
}

function clamp(value: number, min: number, max: number) {
  if (Number.isNaN(value)) return min;
  return Math.max(min, Math.min(max, value));
}

export default AgentControlPanel;
