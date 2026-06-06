import { useRef, useState } from "react";
import type { FrontendConfig, HealthResponse } from "../api/types";

interface ConfigPanelProps {
  config: FrontendConfig;
  health: HealthResponse | null;
  onSave: (config: FrontendConfig) => void;
  onNotice: (message: string | null) => void;
}

function ConfigPanel({ config, health, onSave, onNotice }: ConfigPanelProps) {
  const [draft, setDraft] = useState<FrontendConfig>(config);
  const [error, setError] = useState<string | null>(null);
  const apiKeyRef = useRef<HTMLInputElement>(null);

  function update<K extends keyof FrontendConfig>(key: K, value: string) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function handleSave() {
    const hasDeepSeek = Boolean(draft.deepseek_api_key?.trim() || health?.llm_enabled);
    if (!hasDeepSeek) {
      const message = "请填写模型服务 API Key，或用 --llm 启动后端服务。";
      setError(message);
      onNotice(message);
      apiKeyRef.current?.focus();
      return;
    }
    setError(null);
    onSave(draft);
  }

  const serverHasLlm = Boolean(health?.llm_enabled);
  const serverHasGitHub = Boolean(health?.github_token_configured);

  return (
    <form
      className="config-stack"
      onSubmit={(event) => {
        event.preventDefault();
        handleSave();
      }}
    >
      {/* Hero */}
      <div className="config-hero">
        <p className="eyebrow">Stage 01 — 连接</p>
        <h2>连接调研服务</h2>
        <p>
          API Key 仅保存到当前浏览器会话，不写入项目目录、配置文件或 Git。
          请求执行时会发送至本机 Rust 后端，后端不会将其写入日志、trace 或 checkpoint。
        </p>
      </div>

      {/* Step 1: API Keys */}
      <div className="config-group">
        <div className="config-group-label">
          <span className="step-number">1</span>
          <strong>密钥配置</strong>
        </div>

        <div className="config-grid">
          <label className="field">
            <span>模型服务 API Key</span>
            <input
              ref={apiKeyRef}
              autoComplete="new-password"
              type="password"
              value={draft.deepseek_api_key ?? ""}
              aria-invalid={Boolean(error)}
              aria-describedby={error ? "deepseek-api-key-error" : undefined}
              onChange={(event) => update("deepseek_api_key", event.target.value)}
              placeholder={serverHasLlm ? "后端已启用，可留空" : "sk-..."}
            />
            {serverHasLlm && (
              <small className="muted" style={{ fontSize: 11 }}>
                检测到后端已配置 — 可留空
              </small>
            )}
            {error && (
              <p className="field-error" id="deepseek-api-key-error">
                {error}
              </p>
            )}
          </label>

          <label className="field">
            <span>GitHub 访问令牌</span>
            <input
              autoComplete="new-password"
              type="password"
              value={draft.github_token ?? ""}
              onChange={(event) => update("github_token", event.target.value)}
              placeholder={serverHasGitHub ? "后端已配置" : "可选，提升 API 额度"}
            />
            {serverHasGitHub && (
              <small className="muted" style={{ fontSize: 11 }}>
                检测到后端已配置 — 可留空
              </small>
            )}
          </label>
        </div>
      </div>

      {/* Step 2: Model Selection */}
      <div className="config-group">
        <div className="config-group-label">
          <span className="step-number">2</span>
          <strong>模型选择</strong>
        </div>

        <div className="config-grid">
          <label className="field">
            <span>服务地址</span>
            <input
              value={draft.deepseek_base_url ?? ""}
              onChange={(event) => update("deepseek_base_url", event.target.value)}
              placeholder="https://api.deepseek.com"
            />
          </label>
          <label className="field">
            <span>写作模型（报告生成）</span>
            <input
              value={draft.deepseek_model ?? ""}
              onChange={(event) => update("deepseek_model", event.target.value)}
              placeholder="deepseek-v4-pro"
            />
          </label>
          <label className="field">
            <span>规划模型（轻量任务）</span>
            <input
              value={draft.deepseek_side_model ?? ""}
              onChange={(event) => update("deepseek_side_model", event.target.value)}
              placeholder="deepseek-v4-flash"
            />
          </label>
        </div>
      </div>

      {/* Step 3: Service Status */}
      <div className="config-group">
        <div className="config-group-label">
          <span className="step-number">3</span>
          <strong>服务状态</strong>
        </div>

        <div className="status-list">
          <div>
            <dt>本机服务</dt>
            <dd>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                <span className={`status-dot ${health?.status === "ok" ? "ok" : ""}`} />
                {health?.status === "ok" ? "运行中" : "未连接"}
              </span>
            </dd>
          </div>
          <div>
            <dt>模型服务</dt>
            <dd>{serverHasLlm ? "后端已启用" : "使用本页配置"}</dd>
          </div>
          <div>
            <dt>GitHub 令牌</dt>
            <dd>{serverHasGitHub ? "后端已配置" : "按请求传入"}</dd>
          </div>
          <div>
            <dt>就绪状态</dt>
            <dd>{serverHasLlm || draft.deepseek_api_key?.trim() ? "可连接" : "缺少密钥"}</dd>
          </div>
        </div>
      </div>

      <button className="primary-action" type="submit" style={{ alignSelf: "flex-start" }}>
        保存配置并进入工作台
      </button>
    </form>
  );
}

export default ConfigPanel;
