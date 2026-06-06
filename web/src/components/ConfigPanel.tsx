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

  return (
    <form
      className="config-stack"
      onSubmit={(event) => {
        event.preventDefault();
        handleSave();
      }}
    >
      <div className="phase-card">
        <p className="eyebrow">Stage 01</p>
        <h2>连接调研服务</h2>
        <p className="microcopy">
          API Key 只保存在本地浏览器会话中，并随请求发送到本机 Rust 服务。
        </p>
      </div>

      <label className="field">
        <span>模型服务 API Key</span>
        <input
          ref={apiKeyRef}
          autoComplete="off"
          type="password"
          value={draft.deepseek_api_key ?? ""}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? "deepseek-api-key-error" : undefined}
          onChange={(event) => update("deepseek_api_key", event.target.value)}
          placeholder={health?.llm_enabled ? "后端已启用，可留空" : "sk-..."}
        />
        {error && (
          <p className="field-error" id="deepseek-api-key-error">
            {error}
          </p>
        )}
      </label>

      <label className="field">
        <span>GitHub 访问令牌</span>
        <input
          autoComplete="off"
          type="password"
          value={draft.github_token ?? ""}
          onChange={(event) => update("github_token", event.target.value)}
          placeholder="可选，用于提高 GitHub API 请求额度"
        />
      </label>

      <div className="limit-grid">
        <label className="field">
          <span>服务地址</span>
          <input
            value={draft.deepseek_base_url ?? ""}
            onChange={(event) => update("deepseek_base_url", event.target.value)}
          />
        </label>
        <label className="field">
          <span>写作模型</span>
          <input
            value={draft.deepseek_model ?? ""}
            onChange={(event) => update("deepseek_model", event.target.value)}
          />
        </label>
      </div>

      <label className="field">
        <span>规划模型</span>
        <input
          value={draft.deepseek_side_model ?? ""}
          onChange={(event) => update("deepseek_side_model", event.target.value)}
        />
      </label>

      <button className="primary-action" type="submit">
        保存配置
      </button>
    </form>
  );
}

export default ConfigPanel;
