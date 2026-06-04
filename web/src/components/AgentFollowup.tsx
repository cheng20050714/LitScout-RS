import { useState } from "react";
import ReactMarkdown from "react-markdown";
import { askStatefulRun } from "../api/client";
import type { ResearchRunRecord } from "../api/types";

interface AgentFollowupProps {
  run: ResearchRunRecord | null;
  onNotice: (message: string | null) => void;
}

interface Turn {
  question: string;
  answer: string;
}

function AgentFollowup({ run, onNotice }: AgentFollowupProps) {
  const [question, setQuestion] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [loading, setLoading] = useState(false);

  async function handleAsk() {
    const trimmed = question.trim();
    if (!run) {
      onNotice("请先创建 Agent Run。");
      return;
    }
    if (!trimmed) {
      onNotice("请输入追问内容。");
      return;
    }
    setLoading(true);
    onNotice(null);
    try {
      const response = await askStatefulRun(run.run_id, trimmed);
      const route = response.route as Record<string, string>;
      const answer =
        route.answer ??
        route.reason ??
        "当前问题需要创建增量研究 run，现有证据不足以直接回答。";
      setTurns((current) => [...current, { question: trimmed, answer }]);
      setQuestion("");
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "Agent 追问失败。");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="report-chat">
      <div className="section-header">
        <div>
          <p className="eyebrow">FollowupRouter</p>
          <h2>基于当前证据追问</h2>
        </div>
        <span className="badge">{run?.state ?? "idle"}</span>
      </div>

      <div className="chat-transcript">
        {turns.length === 0 ? (
          <p className="muted">报告生成后，可以针对 EvidenceMemory 和报告内容提问。</p>
        ) : (
          turns.map((turn, index) => (
            <article className="qa-turn" key={`${turn.question}-${index}`}>
              <strong>Q：{turn.question}</strong>
              <div className="markdown-answer">
                <ReactMarkdown>{turn.answer}</ReactMarkdown>
              </div>
            </article>
          ))
        )}
      </div>

      <label className="field">
        <span>追问</span>
        <textarea
          rows={4}
          value={question}
          disabled={!run || loading}
          onChange={(event) => setQuestion(event.target.value)}
        />
      </label>
      <button
        className="primary-action"
        type="button"
        disabled={!run || loading}
        onClick={handleAsk}
      >
        {loading ? "回答中" : "发送追问"}
      </button>
    </section>
  );
}

export default AgentFollowup;
