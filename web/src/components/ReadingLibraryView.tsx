import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import {
  askPaperStream,
  deleteReadingLibraryItem,
  generateReadingNote,
  getReadingLibraryItem
} from "../api/client";
import type {
  ChatStreamEvent,
  FrontendConfig,
  ReadingLibraryItem,
  ReadingLibrarySummary
} from "../api/types";

interface ReadingLibraryViewProps {
  config: FrontendConfig;
  items: ReadingLibrarySummary[];
  activePaperKey: string | null;
  onSelectPaper: (paperKey: string | null) => void;
  onItemsChange: (items: ReadingLibrarySummary[]) => void;
  onItemUpdated: (item: ReadingLibraryItem) => void;
  onNotice: (message: string | null) => void;
}

interface PaperTurn {
  question: string;
  answer: string;
}

function ReadingLibraryView({
  config,
  items,
  activePaperKey,
  onSelectPaper,
  onItemsChange,
  onItemUpdated,
  onNotice
}: ReadingLibraryViewProps) {
  const [activeItem, setActiveItem] = useState<ReadingLibraryItem | null>(null);
  const [loadingItem, setLoadingItem] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [question, setQuestion] = useState("");
  const [asking, setAsking] = useState(false);
  const [turns, setTurns] = useState<PaperTurn[]>([]);

  const selectedKey = activePaperKey ?? items[0]?.paper_key ?? null;

  useEffect(() => {
    if (!selectedKey) {
      setActiveItem(null);
      setTurns([]);
      return;
    }
    setLoadingItem(true);
    getReadingLibraryItem(selectedKey)
      .then((response) => {
        setActiveItem(response.item);
        setTurns(
          rebuildTurnsFromHistory(response.item.chat_history ?? [])
        );
      })
      .catch((error: Error) => onNotice(error.message))
      .finally(() => setLoadingItem(false));
  }, [selectedKey, onNotice]);

  const activeSummary = useMemo(
    () => items.find((item) => item.paper_key === selectedKey) ?? null,
    [items, selectedKey]
  );
  const qualityWarning = activeItem ? noteQualityWarning(activeItem) : null;

  async function handleGenerateNote() {
    if (!selectedKey) return;
    setGenerating(true);
    onNotice(null);
    try {
      const response = await generateReadingNote(selectedKey, config);
      setActiveItem(response.item);
      onItemUpdated(response.item);
      onNotice(response.item.status === "failed" ? response.item.error ?? "笔记生成失败。" : "阅读笔记已生成。");
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "阅读笔记生成失败。");
    } finally {
      setGenerating(false);
    }
  }

  async function handleDelete() {
    if (!selectedKey) return;
    onNotice(null);
    try {
      const response = await deleteReadingLibraryItem(selectedKey);
      onItemsChange(response.items);
      const nextKey = response.items[0]?.paper_key ?? null;
      onSelectPaper(nextKey);
      setActiveItem(null);
      setTurns([]);
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "删除论文失败。");
    }
  }

  async function handleAsk() {
    const trimmed = question.trim();
    if (!selectedKey || !activeItem) {
      onNotice("请先选择一篇论文。");
      return;
    }
    if (!trimmed) {
      onNotice("请输入针对论文的问题。");
      return;
    }
    setAsking(true);
    onNotice(null);
    const turnIndex = turns.length;
    setTurns((current) => [...current, { question: trimmed, answer: "" }]);
    setQuestion("");
    try {
      await askPaperStream(selectedKey, trimmed, config, (event: ChatStreamEvent) => {
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
      const response = await getReadingLibraryItem(selectedKey);
      setActiveItem(response.item);
      onItemUpdated(response.item);
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "论文追问失败。");
      setTurns((current) => current.filter((_, index) => index !== turnIndex));
    } finally {
      setAsking(false);
    }
  }

  if (items.length === 0) {
    return (
      <section className="empty-state">
        <p className="eyebrow">阅读库</p>
        <h2>还没有论文</h2>
        <p>在证据页点击 arXiv 论文卡片右上角的 +，即可把论文加入阅读库。</p>
      </section>
    );
  }

  return (
    <section className="reading-library-layout">
      <aside className="library-list phase-card" aria-label="阅读库论文列表">
        <div className="section-header">
          <div>
            <p className="eyebrow">阅读库</p>
            <h2>{items.length} 篇论文</h2>
          </div>
        </div>
        <div className="library-paper-list">
          {items.map((item) => (
            <button
              key={item.paper_key}
              className={item.paper_key === selectedKey ? "library-paper active" : "library-paper"}
              type="button"
              onClick={() => onSelectPaper(item.paper_key)}
            >
              <span>{item.title}</span>
              <small>{statusLabel(item.status)} · {coverageLabel(item.text_coverage)}</small>
            </button>
          ))}
        </div>
      </aside>

      <section className="library-reader">
        <div className="phase-card">
          <div className="section-header">
            <div>
              <p className="eyebrow">Paper Reader</p>
              <h2>{activeItem?.title ?? activeSummary?.title ?? "论文加载中"}</h2>
            </div>
            <span className="badge">{statusLabel(activeItem?.status ?? activeSummary?.status)}</span>
          </div>

          {loadingItem ? (
            <p className="muted">正在读取论文状态...</p>
          ) : activeItem ? (
            <>
              <div className="reader-actions">
                <a className="secondary-action" href={activeItem.abs_url} target="_blank" rel="noreferrer">
                  打开 arXiv
                </a>
                <button className="secondary-action" type="button" disabled={generating} onClick={handleGenerateNote}>
                  {generating ? "生成中" : activeItem.note ? "重新生成笔记" : "生成阅读笔记"}
                </button>
                <button className="secondary-action danger-action" type="button" onClick={handleDelete}>
                  删除
                </button>
              </div>
              {activeItem.error && <div className="notice-box error-tone">{activeItem.error}</div>}
              {qualityWarning && <div className="notice-box warning-tone">{qualityWarning}</div>}
              {activeItem.status === "text_failed" && (
                <div className="notice-box error-tone">全文获取失败，未生成详细笔记。可检查下方诊断后重试。</div>
              )}
              {activeItem.text_meta && <TextMetaPanel item={activeItem} />}
              <p className="muted">{activeItem.summary}</p>
            </>
          ) : (
            <p className="muted">请选择一篇论文。</p>
          )}
        </div>

        <div className="library-content-grid">
          <article className="markdown-preview library-note">
            {activeItem?.status === "text_failed" ? (
              <div className="empty-state inline-empty">
                <h2>全文获取失败</h2>
                <p>系统没有生成详细阅读笔记。请重试全文抓取，或打开 arXiv 手动确认论文 PDF 是否可访问。</p>
              </div>
            ) : activeItem?.note ? (
              <ReactMarkdown>{activeItem.note.markdown}</ReactMarkdown>
            ) : (
              <div className="empty-state inline-empty">
                <h2>等待阅读笔记</h2>
                <p>点击“生成阅读笔记”后，这里会显示结构化论文笔记。</p>
              </div>
            )}
          </article>

          <aside className="report-chat paper-chat">
            <div className="section-header">
              <div>
                <p className="eyebrow">Paper Q&A</p>
                <h2>围绕这篇论文追问</h2>
              </div>
              <span className="badge">{turns.length}</span>
            </div>

            <div className="chat-transcript">
              {turns.length === 0 ? (
                <p className="muted">可以追问方法细节、实验充分性、复现风险或与当前调研主题的关系。</p>
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
                disabled={!activeItem || asking}
                onChange={(event) => setQuestion(event.target.value)}
                placeholder="例如：这篇论文的实验设计有哪些薄弱点？"
              />
            </label>
            <button className="primary-action" type="button" disabled={!activeItem || asking} onClick={handleAsk}>
              {asking ? "回答中" : "提交问题"}
            </button>
          </aside>
        </div>
      </section>
    </section>
  );
}

function TextMetaPanel({ item }: { item: ReadingLibraryItem }) {
  const meta = item.text_meta;
  if (!meta) return null;
  const lastFailedAttempt = [...meta.attempts].reverse().find((attempt) => attempt.status !== "ok");
  return (
    <div className="text-meta-panel">
      <span>来源：{coverageLabel(meta.coverage)}</span>
      <span>提取器：{meta.extractor}</span>
      <span>字符数：{meta.char_count.toLocaleString()}</span>
      <span>页数：{meta.page_count ?? "未知"}</span>
      <span>质量分：{meta.quality_score.toFixed(2)}</span>
      {lastFailedAttempt && (
        <span>
          最近失败：{attemptLabel(lastFailedAttempt.kind)} / {lastFailedAttempt.error ?? lastFailedAttempt.status}
        </span>
      )}
    </div>
  );
}

function rebuildTurnsFromHistory(history: ReadingLibraryItem["chat_history"]): PaperTurn[] {
  const turns: PaperTurn[] = [];
  for (let index = 0; index < history.length; index += 1) {
    const message = history[index];
    if (message.role !== "user") continue;
    const next = history[index + 1];
    turns.push({
      question: message.content,
      answer: next?.role === "assistant" ? next.content : ""
    });
  }
  return turns;
}

function statusLabel(status?: string | null) {
  return (
    {
      queued: "待生成",
      fetching_text: "抓取中",
      fetching_jina_html: "Jina HTML 获取中",
      fetching_jina_pdf: "Jina PDF 获取中",
      downloading_pdf: "下载 PDF 中",
      extracting_pdf_text: "提取 PDF 文本中",
      text_ready: "文本就绪",
      generating_note: "生成中",
      ready: "已完成",
      text_failed: "全文获取失败",
      note_failed: "笔记生成失败",
      failed: "失败"
    }[status ?? ""] ?? "未知"
  );
}

function coverageLabel(coverage?: string | null) {
  return (
    {
      markdown_proxy: "代理全文",
      full_text_html: "HTML 全文",
      full_text_pdf: "PDF 全文",
      partial_text: "部分全文",
      abstract_only: "摘要级",
      failed: "获取失败"
    }[coverage ?? ""] ?? "未抓取"
  );
}

function attemptLabel(kind: string) {
  return (
    {
      jina_html: "Jina HTML",
      jina_pdf: "Jina PDF",
      download_pdf: "PDF 下载",
      pdf_cache: "PDF 缓存",
      pdf_extract: "PDF 提取",
      quality_gate: "质量门控"
    }[kind] ?? kind
  );
}

function noteQualityWarning(item: ReadingLibraryItem) {
  if (!item.note) return null;
  if (!item.note_quality) {
    return "旧版本生成的阅读笔记，质量未知；建议重新生成以使用全文质量门控。";
  }
  if (item.note_quality === "unknown") {
    return "阅读笔记质量未知；建议重新生成以使用全文质量门控。";
  }
  if (item.note_quality === "abstract_only" || item.text_coverage === "abstract_only") {
    return "这是摘要级旧笔记，不等同于详细全文阅读笔记；请重新生成。";
  }
  return null;
}

export default ReadingLibraryView;
