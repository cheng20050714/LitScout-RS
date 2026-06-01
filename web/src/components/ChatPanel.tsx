import { useState } from "react";
import { createPlan, revisePlan } from "../api/client";
import type { FrontendConfig, PlanResponse } from "../api/types";

interface ChatPanelProps {
  config: FrontendConfig;
  currentPlan: PlanResponse | null;
  hasServerLlm: boolean;
  onPlanGenerated: (plan: PlanResponse) => void;
  onNotice: (message: string | null) => void;
  onActivityChange: (activity: "idle" | "planning" | "revising" | "plan_ready") => void;
}

function ChatPanel({
  config,
  currentPlan,
  hasServerLlm,
  onPlanGenerated,
  onNotice,
  onActivityChange
}: ChatPanelProps) {
  const [topic, setTopic] = useState("Rust Agent Framework 的开源项目和论文调研");
  const [feedback, setFeedback] = useState("");
  const [githubLimit, setGithubLimit] = useState(10);
  const [arxivLimit, setArxivLimit] = useState(10);
  const [loading, setLoading] = useState(false);

  async function handleCreatePlan() {
    const trimmed = topic.trim();
    if (!trimmed) {
      onNotice("请输入调研主题。");
      return;
    }
    if (!config.deepseek_api_key?.trim() && !hasServerLlm) {
      onNotice("请先在阶段 1 配置 DeepSeek API Key。");
      return;
    }

    setLoading(true);
    onActivityChange("planning");
    onNotice(null);
    try {
      const plan = await createPlan({
        topic: trimmed,
        github_limit: githubLimit,
        arxiv_limit: arxivLimit,
        language: "zh-CN",
        config
      });
      onPlanGenerated(plan);
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "SearchPlan 生成失败。");
      onActivityChange("idle");
    } finally {
      setLoading(false);
    }
  }

  async function handleRevisePlan() {
    if (!currentPlan) {
      onNotice("当前没有可修改的搜索计划。");
      return;
    }
    const trimmed = feedback.trim();
    if (!trimmed) {
      onNotice("请输入修改意见。");
      return;
    }

    setLoading(true);
    onActivityChange("revising");
    onNotice(null);
    try {
      const revised = await revisePlan(currentPlan, trimmed, config);
      onPlanGenerated(revised);
      setFeedback("");
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "SearchPlan 修改失败。");
      onActivityChange("plan_ready");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="chat-stack">
      <label className="field">
        <span>调研主题</span>
        <textarea
          value={topic}
          onChange={(event) => setTopic(event.target.value)}
          rows={5}
        />
      </label>

      <div className="limit-grid">
        <label className="field">
          <span>GitHub</span>
          <input
            min={1}
            type="number"
            value={githubLimit}
            onChange={(event) => setGithubLimit(Number(event.target.value))}
          />
        </label>
        <label className="field">
          <span>arXiv</span>
          <input
            min={1}
            type="number"
            value={arxivLimit}
            onChange={(event) => setArxivLimit(Number(event.target.value))}
          />
        </label>
      </div>

      {loading && (
        <div className="progress-banner" role="status" aria-live="polite">
          <span className="progress-spinner" />
          <span>{currentPlan ? "正在修改搜索计划" : "正在生成搜索计划"}</span>
        </div>
      )}

      <button className="primary-action" type="button" onClick={handleCreatePlan} disabled={loading}>
        {loading ? "生成中" : "生成搜索计划"}
      </button>

      <label className="field">
        <span>修改意见</span>
        <textarea
          value={feedback}
          onChange={(event) => setFeedback(event.target.value)}
          rows={4}
          placeholder="例如：增加 benchmark 方向，减少泛化 Agent 论文。"
        />
      </label>

      <button className="secondary-action" type="button" onClick={handleRevisePlan} disabled={loading || !currentPlan}>
        修改当前计划
      </button>
    </div>
  );
}

export default ChatPanel;
