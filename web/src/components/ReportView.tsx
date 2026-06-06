import ReactMarkdown from "react-markdown";

interface ReportViewProps {
  markdown: string;
  canTranslate: boolean;
  translating: boolean;
  onTranslate: () => void;
}

function ReportView({ markdown, canTranslate, translating, onTranslate }: ReportViewProps) {
  if (!markdown) {
    return (
      <div className="empty-state">
        <h2>等待报告</h2>
        <p>搜索计划确认并执行后，这里会显示中文 Markdown 报告。</p>
      </div>
    );
  }

  return (
    <section className="report-view">
      <div className="report-actions">
        <span className="badge">Markdown</span>
        <button
          className="secondary-action"
          type="button"
          disabled={!canTranslate || translating}
          onClick={onTranslate}
        >
          {translating ? "翻译中" : "用模型翻译为中文"}
        </button>
      </div>
      <article className="markdown-preview">
        <ReactMarkdown>{markdown}</ReactMarkdown>
      </article>
    </section>
  );
}

export default ReportView;
