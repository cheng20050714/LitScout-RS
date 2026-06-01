import { useState } from "react";
import ReactMarkdown from "react-markdown";
import { askReportStream } from "../api/client";
import type { FrontendConfig } from "../api/types";

interface ReportChatProps {
  config: FrontendConfig;
  reportMarkdown: string;
  disabled: boolean;
  onNotice: (message: string | null) => void;
}

interface ChatTurn {
  question: string;
  answer: string;
}

function ReportChat({ config, reportMarkdown, disabled, onNotice }: ReportChatProps) {
  const [question, setQuestion] = useState("");
  const [loading, setLoading] = useState(false);
  const [turns, setTurns] = useState<ChatTurn[]>([]);

  async function handleAsk() {
    const trimmed = question.trim();
    if (!trimmed) {
      onNotice("请输入针对报告的追问。");
      return;
    }
    if (disabled) {
      onNotice("报告生成后才能开始追问。");
      return;
    }
    setLoading(true);
    onNotice(null);
    const turnIndex = turns.length;
    setTurns((current) => [...current, { question: trimmed, answer: "" }]);
    setQuestion("");
    try {
      await askReportStream(trimmed, reportMarkdown, config, (event) => {
        if (event.event === "delta" && event.data.text) {
          setTurns((current) =>
            current.map((turn, index) =>
              index === turnIndex
                ? { ...turn, answer: `${turn.answer}${event.data.text}` }
                : turn
            )
          );
        }
      });
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "报告追问失败。");
      setTurns((current) => current.filter((_, index) => index !== turnIndex));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="report-chat">
      <div className="section-header">
        <div>
          <p className="eyebrow">Follow-up</p>
          <h2>针对报告追问</h2>
        </div>
        <span className="badge">{turns.length}</span>
      </div>

      <div className="chat-transcript">
        {turns.length === 0 ? (
          <p className="muted">报告生成后，可以继续追问“哪些项目最适合课程展示？”这类问题。</p>
        ) : (
          turns.map((turn, index) => (
            <article key={`${turn.question}-${index}`} className="qa-turn">
              <h3>{turn.question}</h3>
              {turn.answer ? (
                <div className="markdown-answer">
                  <ReactMarkdown>{turn.answer}</ReactMarkdown>
                </div>
              ) : (
                <p className="muted">DeepSeek 正在流式输出回答...</p>
              )}
            </article>
          ))
        )}
      </div>

      <label className="field">
        <span>问题</span>
        <textarea
          value={question}
          rows={4}
          disabled={disabled || loading}
          onChange={(event) => setQuestion(event.target.value)}
          placeholder="例如：哪些 repo 最值得先读？有哪些论文和项目能对应起来？"
        />
      </label>
      <button className="primary-action" type="button" disabled={disabled || loading} onClick={handleAsk}>
        {loading ? "回答中" : "提交追问"}
      </button>
    </section>
  );
}

export default ReportChat;
