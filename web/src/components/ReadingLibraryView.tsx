import { useEffect, useMemo, useRef, useState } from "react";
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
  const [generatingPaperKey, setGeneratingPaperKey] = useState<string | null>(null);
  const [generationQueue, setGenerationQueue] = useState<string[]>([]);
  const [listCollapsed, setListCollapsed] = useState(false);
  const [question, setQuestion] = useState("");
  const [asking, setAsking] = useState(false);
  const [turns, setTurns] = useState<PaperTurn[]>([]);
  const selectedKeyRef = useRef<string | null>(null);
  const generationQueueRef = useRef<string[]>([]);
  const generatingPaperKeyRef = useRef<string | null>(null);

  const selectedKey = activePaperKey ?? items[0]?.paper_key ?? null;

  useEffect(() => {
    selectedKeyRef.current = selectedKey;
  }, [selectedKey]);

  useEffect(() => {
    generationQueueRef.current = generationQueue;
  }, [generationQueue]);

  useEffect(() => {
    generatingPaperKeyRef.current = generatingPaperKey;
  }, [generatingPaperKey]);

  useEffect(() => {
    if (!selectedKey) {
      setActiveItem(null);
      setTurns([]);
      return;
    }
    let cancelled = false;
    setActiveItem(null);
    setTurns([]);
    setLoadingItem(true);
    getReadingLibraryItem(selectedKey)
      .then((response) => {
        if (cancelled) return;
        setActiveItem(response.item);
        setTurns(
          rebuildTurnsFromHistory(response.item.chat_history ?? [])
        );
      })
      .catch((error: Error) => {
        if (!cancelled) onNotice(error.message);
      })
      .finally(() => {
        if (!cancelled) setLoadingItem(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedKey, onNotice]);

  const activeSummary = useMemo(
    () => items.find((item) => item.paper_key === selectedKey) ?? null,
    [items, selectedKey]
  );
  const qualityWarning = activeItem ? noteQualityWarning(activeItem) : null;
  const selectedGenerationState =
    selectedKey && selectedKey === generatingPaperKey
      ? "generating"
      : selectedKey && generationQueue.includes(selectedKey)
        ? "queued"
        : "idle";
  const activeDisplayStatus =
    selectedGenerationState === "generating"
      ? "generating_note"
      : activeItem?.status ?? activeSummary?.status;

  async function handleGenerateNote() {
    if (!selectedKey) return;
    const runningPaperKey = generatingPaperKeyRef.current;
    if (runningPaperKey) {
      if (runningPaperKey === selectedKey || generationQueueRef.current.includes(selectedKey)) return;
      enqueueGeneration(selectedKey);
      onNotice("已加入阅读笔记生成队列。");
      return;
    }
    void runGeneration(selectedKey);
  }

  function enqueueGeneration(paperKey: string) {
    if (generationQueueRef.current.includes(paperKey)) return;
    const nextQueue = [...generationQueueRef.current, paperKey];
    generationQueueRef.current = nextQueue;
    setGenerationQueue(nextQueue);
  }

  async function runGeneration(paperKey: string) {
    generatingPaperKeyRef.current = paperKey;
    setGeneratingPaperKey(paperKey);
    onNotice(null);
    try {
      const response = await generateReadingNote(paperKey, config);
      if (selectedKeyRef.current === paperKey) {
        setActiveItem(response.item);
      }
      onItemUpdated(response.item);
      const failed = ["failed", "text_failed", "note_failed"].includes(response.item.status);
      onNotice(
        failed
          ? response.item.error ?? `${response.item.title}：${statusLabel(response.item.status)}。`
          : `${response.item.title}：阅读笔记已生成。`
      );
    } catch (error) {
      onNotice(error instanceof Error ? error.message : "阅读笔记生成失败。");
    } finally {
      const [nextPaperKey, ...remaining] = generationQueueRef.current;
      generationQueueRef.current = remaining;
      setGenerationQueue(remaining);
      if (nextPaperKey) {
        void runGeneration(nextPaperKey);
      } else {
        generatingPaperKeyRef.current = null;
        setGeneratingPaperKey(null);
      }
    }
  }

  async function handleDelete() {
    if (!selectedKey) return;
    if (selectedGenerationState !== "idle") {
      onNotice("这篇论文正在生成或排队中，暂时不能删除。");
      return;
    }
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
    <section className={listCollapsed ? "reading-library-layout list-collapsed" : "reading-library-layout"}>
      <aside
        className={listCollapsed ? "library-list library-list-rail phase-card" : "library-list phase-card"}
        aria-label="阅读库论文列表"
      >
        {listCollapsed ? (
          <button
            className="library-rail-button"
            type="button"
            aria-label="展开论文列表"
            aria-expanded={false}
            onClick={() => setListCollapsed(false)}
            title="展开论文列表"
          >
            <span className="library-rail-count">{items.length}</span>
            <span className="library-rail-label">阅读库</span>
            <span className="library-rail-arrow">›</span>
          </button>
        ) : (
          <>
            <div className="section-header library-list-header">
              <div>
                <p className="eyebrow">阅读库</p>
                <h2>{items.length} 篇论文</h2>
              </div>
              <button
                className="icon-button library-collapse-button"
                type="button"
                aria-label="隐藏论文列表"
                aria-expanded={true}
                onClick={() => setListCollapsed(true)}
                title="隐藏论文列表"
              >
                ‹
              </button>
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
                  <small>
                    {runtimeStatusLabel(item, generatingPaperKey, generationQueue)} · {coverageLabel(item.text_coverage)}
                  </small>
                </button>
              ))}
            </div>
          </>
        )}
      </aside>

      <section className="library-reader">
        <div className="phase-card">
          <div className="section-header">
            <div>
              <p className="eyebrow">Paper Reader</p>
              <h2>{activeItem?.title ?? activeSummary?.title ?? "论文加载中"}</h2>
            </div>
            <span className="badge">
              {selectedGenerationState === "queued" ? "排队中" : statusLabel(activeDisplayStatus)}
            </span>
          </div>

          {loadingItem ? (
            <p className="muted">正在读取论文状态...</p>
          ) : activeItem ? (
            <>
              <div className="reader-actions">
                <a className="secondary-action" href={activeItem.abs_url} target="_blank" rel="noreferrer">
                  打开 arXiv
                </a>
                <button
                  className="secondary-action"
                  type="button"
                  disabled={selectedGenerationState !== "idle"}
                  onClick={handleGenerateNote}
                >
                  {generateButtonLabel(
                    selectedGenerationState,
                    Boolean(generatingPaperKey),
                    Boolean(activeItem.note)
                  )}
                </button>
                <button
                  className="secondary-action danger-action"
                  type="button"
                  disabled={selectedGenerationState !== "idle"}
                  onClick={handleDelete}
                >
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
            <p className="muted">选择一篇论文查看状态。</p>
          )}
        </div>

        {activeItem && (
          <div className="library-content-grid">
            <article className="phase-card library-note">
              <div className="section-header">
                <div>
                  <p className="eyebrow">Reading Note</p>
                  <h3>论文笔记</h3>
                </div>
              </div>
              {activeItem.note ? (
                <ReactMarkdown>{activeItem.note.markdown}</ReactMarkdown>
              ) : (
                <div className="empty-state inline-empty">
                  <p>还没有生成阅读笔记。</p>
                </div>
              )}
            </article>

            <aside className="phase-card paper-chat">
              <div className="section-header">
                <div>
                  <p className="eyebrow">Paper Chat</p>
                  <h3>追问论文</h3>
                </div>
              </div>
              <div className="paper-chat-turns">
                {turns.length === 0 ? (
                  <p className="muted">围绕这篇论文继续提问。</p>
                ) : (
                  turns.map((turn, index) => (
                    <div className="paper-chat-turn" key={`${turn.question}-${index}`}>
                      <strong>Q: {turn.question}</strong>
                      <ReactMarkdown>{turn.answer || "..."}</ReactMarkdown>
                    </div>
                  ))
                )}
              </div>
              <div className="paper-chat-input">
                <textarea
                  value={question}
                  onChange={(event) => setQuestion(event.target.value)}
                  placeholder="例如：这篇论文的核心方法和局限是什么？"
                  rows={3}
                />
                <button className="primary-action" type="button" onClick={handleAsk} disabled={asking}>
                  {asking ? "回答中..." : "发送"}
                </button>
              </div>
            </aside>
          </div>
        )}
      </section>
    </section>
  );
}

function rebuildTurnsFromHistory(history: ReadingLibraryItem["chat_history"]): PaperTurn[] {
  const turns: PaperTurn[] = [];
  let pendingQuestion: string | null = null;
  for (const message of history) {
    if (message.role === "user") {
      pendingQuestion = message.content;
    } else if (message.role === "assistant" && pendingQuestion) {
      turns.push({ question: pendingQuestion, answer: message.content });
      pendingQuestion = null;
    }
  }
  return turns;
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

function runtimeStatusLabel(
  item: ReadingLibrarySummary,
  generatingPaperKey: string | null,
  generationQueue: string[]
) {
  if (item.paper_key === generatingPaperKey) return "阅读笔记生成中";
  if (generationQueue.includes(item.paper_key)) return "排队中";
  return statusLabel(item.status);
}

function generateButtonLabel(
  state: "idle" | "generating" | "queued",
  hasRunningJob: boolean,
  hasNote: boolean
) {
  if (state === "generating") return "阅读笔记生成中";
  if (state === "queued") return "已加入生成队列";
  if (hasRunningJob) return "加入生成队列";
  return hasNote ? "重新生成笔记" : "生成阅读笔记";
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
