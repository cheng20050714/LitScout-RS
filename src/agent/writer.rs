use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{
    strip_json_fence, ChatCompletionsRequest, ChatMessage, DeepSeekClient, DeepSeekConfig,
    ResponseFormat,
};
use crate::model::{
    ChapterDraft, ChapterNode, CitationLedger, EvidenceItem, EvidenceMemory,
    ParagraphWithCitations, ReportDraft, SourceKind,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tracing::{debug, warn};

const MAX_EVIDENCE_PER_BATCH: usize = 4;
const MAX_BATCH_CONTEXT_CHARS: usize = 10_000;
const MAX_BATCH_PARAGRAPHS: usize = 4;
const MAX_CHAPTER_PARAGRAPHS: usize = 6;
const WRITER_MIN_OUTPUT_TOKENS: usize = 4096;
const WRITER_MAX_OUTPUT_TOKENS: usize = 8192;

pub async fn draft_report_with_llm(
    topic: &str,
    chapters: &[ChapterNode],
    memory: &EvidenceMemory,
    llm_config: &LlmConfig,
) -> Result<ReportDraft> {
    let deepseek_config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "Writer LLM requires an enabled DeepSeek config with API key; refusing to generate a template report."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(deepseek_config.clone())?;
    let mut chapter_drafts = Vec::with_capacity(chapters.len());

    for chapter in chapters {
        let evidence_items = memory.by_chapter(&chapter.id);
        if evidence_items.is_empty() {
            chapter_drafts.push(empty_chapter_draft(chapter));
            continue;
        }

        let draft = draft_chapter_with_llm(chapter, &evidence_items, &client, &deepseek_config)
            .await
            .map_err(|err| {
                AppError::Llm(format!(
                    "Writer LLM failed for chapter `{}`; refusing template fallback: {err}",
                    chapter.title_zh
                ))
            })?;
        chapter_drafts.push(draft);
    }

    let global_summary_zh = synthesize_global_summary_with_llm(
        topic,
        &chapter_drafts,
        memory.items.len(),
        &client,
        &deepseek_config,
    )
    .await
    .map_err(|err| {
        AppError::Llm(format!(
            "Writer global summary failed; refusing deterministic fallback: {err}"
        ))
    })?;

    Ok(ReportDraft {
        title_zh: format!("LitScout-RS 调研报告：{topic}"),
        chapters: chapter_drafts,
        global_summary_zh,
        written_at: Utc::now(),
    })
}

async fn draft_chapter_with_llm(
    chapter: &ChapterNode,
    evidence_items: &[EvidenceItem],
    client: &DeepSeekClient,
    config: &DeepSeekConfig,
) -> Result<ChapterDraft> {
    let batches = batch_chapter_evidence(evidence_items);
    debug!(
        "Writer split chapter `{}` into {} evidence batches",
        chapter.title_zh,
        batches.len()
    );

    let mut batch_drafts = Vec::with_capacity(batches.len());
    for batch in &batches {
        let draft = draft_batch_with_llm(chapter, batch, batches.len(), client, config).await?;
        batch_drafts.push(draft);
    }

    synthesize_chapter_from_batches(chapter, evidence_items, &batch_drafts, client, config).await
}

fn empty_chapter_draft(chapter: &ChapterNode) -> ChapterDraft {
    ChapterDraft {
        chapter_id: chapter.id.clone(),
        title_zh: chapter.title_zh.clone(),
        paragraphs: vec![ParagraphWithCitations {
            text_zh: format!(
                "当前章节 `{}` 尚未获得与章节计划直接匹配的 GitHub/arXiv 证据，因此不生成分析段落。",
                chapter.title_zh
            ),
            cited_evidence_ids: Vec::new(),
        }],
    }
}

#[derive(Debug, Clone)]
struct EvidenceBatch {
    batch_id: usize,
    evidence_items: Vec<EvidenceItem>,
    context_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BatchDraft {
    batch_id: usize,
    findings_summary_zh: String,
    paragraphs: Vec<ParagraphWithCitations>,
}

fn batch_chapter_evidence(evidence_items: &[EvidenceItem]) -> Vec<EvidenceBatch> {
    let mut batches = Vec::new();
    let mut current_items = Vec::new();
    let mut current_chars = 0usize;

    for item in evidence_items {
        let item_chars = evidence_context_chars(item);
        let count_limit_reached = current_items.len() >= MAX_EVIDENCE_PER_BATCH;
        let char_limit_reached =
            !current_items.is_empty() && current_chars + item_chars > MAX_BATCH_CONTEXT_CHARS;

        if count_limit_reached || char_limit_reached {
            batches.push(EvidenceBatch {
                batch_id: batches.len() + 1,
                evidence_items: current_items,
                context_chars: current_chars,
            });
            current_items = Vec::new();
            current_chars = 0;
        }

        current_chars += item_chars;
        current_items.push(item.clone());
    }

    if !current_items.is_empty() {
        batches.push(EvidenceBatch {
            batch_id: batches.len() + 1,
            evidence_items: current_items,
            context_chars: current_chars,
        });
    }

    batches
}

fn evidence_context_chars(item: &EvidenceItem) -> usize {
    item.evidence_id.chars().count()
        + item.title.chars().count()
        + item.url.chars().count()
        + item.evidence_note_zh.chars().count()
        + item.evidence_snippet.chars().count()
}

fn excerpt_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn parse_batch_draft_content(content: &str, batch: &EvidenceBatch) -> Result<BatchDraft> {
    let output =
        serde_json::from_str::<BatchDraftOutput>(strip_json_fence(content)).map_err(|err| {
            AppError::Llm(format!(
                "Writer response was not valid BatchDraft JSON: {err}"
            ))
        })?;
    let findings_summary_zh = output.findings_summary_zh.trim().to_string();
    if findings_summary_zh.is_empty() {
        return Err(AppError::Llm(
            "Writer batch response contained empty findings_summary_zh".to_string(),
        ));
    }
    reject_template_residue(&findings_summary_zh)?;
    let paragraphs = parse_paragraph_outputs(
        output.paragraphs,
        &batch.evidence_items,
        MAX_BATCH_PARAGRAPHS,
    )?;

    Ok(BatchDraft {
        batch_id: batch.batch_id,
        findings_summary_zh,
        paragraphs,
    })
}

async fn draft_batch_with_llm(
    chapter: &ChapterNode,
    batch: &EvidenceBatch,
    total_batches: usize,
    client: &DeepSeekClient,
    config: &DeepSeekConfig,
) -> Result<BatchDraft> {
    let request = build_batch_draft_request(chapter, batch, total_batches, config)?;
    let response = client.chat_completions_with_retry(request).await?;
    let content = response.first_content()?;

    match parse_batch_draft_content(content, batch) {
        Ok(draft) => Ok(draft),
        Err(first_err) => {
            warn!(
                "Writer LLM JSON validation failed for chapter `{}` batch {}; requesting one repair: {first_err}",
                chapter.title_zh, batch.batch_id
            );
            let repair_request = build_batch_draft_repair_request(
                chapter,
                batch,
                total_batches,
                config,
                content,
                &first_err.to_string(),
            )?;
            let repaired_response = client.chat_completions_with_retry(repair_request).await?;
            let repaired_content = repaired_response.first_content()?;
            parse_batch_draft_content(repaired_content, batch)
        }
    }
}

fn build_batch_draft_request(
    chapter: &ChapterNode,
    batch: &EvidenceBatch,
    total_batches: usize,
    config: &DeepSeekConfig,
) -> Result<ChatCompletionsRequest> {
    let context = BatchDraftInput {
        chapter_title: chapter.title_zh.clone(),
        research_question: chapter.research_question.clone(),
        batch_id: batch.batch_id,
        total_batches,
        batch_context_chars: batch.context_chars,
        evidence_sources: batch
            .evidence_items
            .iter()
            .map(EvidenceSourceInput::from)
            .collect(),
    };
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: batch_writer_system_prompt(&chapter.title_zh),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请为下面章节的第 {}/{} 个证据批次生成局部分析草稿。只返回 JSON 对象，不要输出 Markdown 代码围栏之外的解释。\n\n{context_json}",
                    batch.batch_id, total_batches
                ),
            },
        ],
        temperature: Some(0.3),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

fn build_batch_draft_repair_request(
    chapter: &ChapterNode,
    batch: &EvidenceBatch,
    total_batches: usize,
    config: &DeepSeekConfig,
    previous_output: &str,
    validation_error: &str,
) -> Result<ChatCompletionsRequest> {
    let context = BatchDraftInput {
        chapter_title: chapter.title_zh.clone(),
        research_question: chapter.research_question.clone(),
        batch_id: batch.batch_id,
        total_batches,
        batch_context_chars: batch.context_chars,
        evidence_sources: batch
            .evidence_items
            .iter()
            .map(EvidenceSourceInput::from)
            .collect(),
    };
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: batch_writer_system_prompt(&chapter.title_zh),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请修复上一次第 {}/{} 个证据批次的输出，使其满足 JSON schema 和引用约束。只返回修复后的 JSON 对象。\n\n校验错误：\n{validation_error}\n\n上一次输出：\n{previous_output}\n\n允许使用的 evidence：\n{context_json}",
                    batch.batch_id, total_batches
                ),
            },
        ],
        temperature: Some(0.1),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

async fn synthesize_chapter_from_batches(
    chapter: &ChapterNode,
    evidence_items: &[EvidenceItem],
    batch_drafts: &[BatchDraft],
    client: &DeepSeekClient,
    config: &DeepSeekConfig,
) -> Result<ChapterDraft> {
    let request = build_chapter_synthesis_request(chapter, evidence_items, batch_drafts, config)?;
    let response = client.chat_completions_with_retry(request).await?;
    let content = response.first_content()?;

    match parse_chapter_draft_content(content, chapter, evidence_items) {
        Ok(draft) => Ok(draft),
        Err(first_err) => {
            warn!(
                "Writer LLM chapter synthesis validation failed for chapter `{}`; requesting one repair: {first_err}",
                chapter.title_zh
            );
            let repair_request = build_chapter_synthesis_repair_request(
                chapter,
                evidence_items,
                batch_drafts,
                config,
                content,
                &first_err.to_string(),
            )?;
            let repaired_response = client.chat_completions_with_retry(repair_request).await?;
            let repaired_content = repaired_response.first_content()?;
            parse_chapter_draft_content(repaired_content, chapter, evidence_items)
        }
    }
}

fn batch_writer_system_prompt(chapter_title: &str) -> String {
    format!(
        "你是 LitScout-RS 的技术报告撰稿人，正在为 `{chapter_title}` 章节处理一个 evidence batch。\n\
规则：\n\
1. 使用中文撰写。\n\
2. 先在 findings_summary_zh 中总结本批证据的关键发现、共性和边界。\n\
3. 再对本批来源提炼项目/论文简介、核心亮点或设计思想，以及与章节问题的具体关联。\n\
4. 可合并多个来源做对比分析，但每个段落必须引用至少一个 evidence_id。\n\
5. 只能使用用户提供的 evidence_id 和 URL，不得新增来源、不得编造事实。\n\
6. 不要只重复原文，不要输出“来源链接”模板句。\n\
7. 只返回 JSON：{{\"findings_summary_zh\":\"...\",\"paragraphs\":[{{\"text_zh\":\"...\",\"cited_evidence_ids\":[\"ev-C1\"]}}]}}。"
    )
}

fn writer_max_tokens(config: &DeepSeekConfig) -> u32 {
    config
        .max_tokens
        .max(WRITER_MIN_OUTPUT_TOKENS)
        .min(WRITER_MAX_OUTPUT_TOKENS)
        .min(u32::MAX as usize) as u32
}

fn build_chapter_synthesis_request(
    chapter: &ChapterNode,
    evidence_items: &[EvidenceItem],
    batch_drafts: &[BatchDraft],
    config: &DeepSeekConfig,
) -> Result<ChatCompletionsRequest> {
    let context = ChapterSynthesisInput {
        chapter_title: chapter.title_zh.clone(),
        research_question: chapter.research_question.clone(),
        allowed_evidence_ids: evidence_items
            .iter()
            .map(|item| item.evidence_id.clone())
            .collect(),
        batch_drafts: batch_drafts.to_vec(),
    };
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: chapter_synthesizer_system_prompt(&chapter.title_zh),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请基于已验证的 batch findings 合成章节终稿。只返回 JSON 对象，不要输出 Markdown 代码围栏之外的解释。\n\n{context_json}"
                ),
            },
        ],
        temperature: Some(0.25),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

fn build_chapter_synthesis_repair_request(
    chapter: &ChapterNode,
    evidence_items: &[EvidenceItem],
    batch_drafts: &[BatchDraft],
    config: &DeepSeekConfig,
    previous_output: &str,
    validation_error: &str,
) -> Result<ChatCompletionsRequest> {
    let context = ChapterSynthesisInput {
        chapter_title: chapter.title_zh.clone(),
        research_question: chapter.research_question.clone(),
        allowed_evidence_ids: evidence_items
            .iter()
            .map(|item| item.evidence_id.clone())
            .collect(),
        batch_drafts: batch_drafts.to_vec(),
    };
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: chapter_synthesizer_system_prompt(&chapter.title_zh),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请修复上一次章节综合输出，使其满足 JSON schema 和引用约束。只返回修复后的 JSON 对象。\n\n校验错误：\n{validation_error}\n\n上一次输出：\n{previous_output}\n\n允许使用的 batch findings：\n{context_json}"
                ),
            },
        ],
        temperature: Some(0.1),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

fn chapter_synthesizer_system_prompt(chapter_title: &str) -> String {
    format!(
        "你是 LitScout-RS 的章节综合编辑，正在合成 `{chapter_title}` 章节终稿。\n\
规则：\n\
1. 使用中文撰写自然、分析性的技术报告段落。\n\
2. 只能使用用户提供的 batch findings、batch paragraphs 和 allowed_evidence_ids。\n\
3. 合并重复信息，按主题组织，不要机械罗列每条来源。\n\
4. 已有 evidence 时，不得用“尚未收集到足够证据”替代分析内容。\n\
5. 每个段落必须引用至少一个 allowed_evidence_ids 中的 evidence_id。\n\
6. 不得新增来源、URL 或 evidence_id，不得编造事实。\n\
7. 不要输出“来源链接”模板句或“是本章节的关键来源之一”。\n\
8. 只返回 JSON：{{\"paragraphs\":[{{\"text_zh\":\"...\",\"cited_evidence_ids\":[\"ev-C1\"]}}]}}。"
    )
}

async fn synthesize_global_summary_with_llm(
    topic: &str,
    chapter_drafts: &[ChapterDraft],
    evidence_count: usize,
    client: &DeepSeekClient,
    config: &DeepSeekConfig,
) -> Result<String> {
    let request = build_global_summary_request(topic, chapter_drafts, evidence_count, config)?;
    let response = client.chat_completions_with_retry(request).await?;
    let content = response.first_content()?;

    match parse_global_summary_content(content) {
        Ok(summary) => Ok(summary),
        Err(first_err) => {
            warn!(
                "Writer LLM global summary validation failed; requesting one repair: {first_err}"
            );
            let repair_request = build_global_summary_repair_request(
                topic,
                chapter_drafts,
                evidence_count,
                config,
                content,
                &first_err.to_string(),
            )?;
            let repaired_response = client.chat_completions_with_retry(repair_request).await?;
            let repaired_content = repaired_response.first_content()?;
            parse_global_summary_content(repaired_content)
        }
    }
}

fn build_global_summary_request(
    topic: &str,
    chapter_drafts: &[ChapterDraft],
    evidence_count: usize,
    config: &DeepSeekConfig,
) -> Result<ChatCompletionsRequest> {
    let context = GlobalSummaryInput::from_report(topic, chapter_drafts, evidence_count);
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: global_synthesizer_system_prompt(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请基于章节终稿生成报告的全局摘要。只返回 JSON 对象，不要输出 Markdown 代码围栏之外的解释。\n\n{context_json}"
                ),
            },
        ],
        temperature: Some(0.25),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

fn build_global_summary_repair_request(
    topic: &str,
    chapter_drafts: &[ChapterDraft],
    evidence_count: usize,
    config: &DeepSeekConfig,
    previous_output: &str,
    validation_error: &str,
) -> Result<ChatCompletionsRequest> {
    let context = GlobalSummaryInput::from_report(topic, chapter_drafts, evidence_count);
    let context_json = serde_json::to_string_pretty(&context)?;
    let max_tokens = writer_max_tokens(config);

    Ok(ChatCompletionsRequest {
        model: config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: global_synthesizer_system_prompt(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请修复上一次全局摘要输出，使其满足 JSON schema 和写作约束。只返回修复后的 JSON 对象。\n\n校验错误：\n{validation_error}\n\n上一次输出：\n{previous_output}\n\n章节终稿上下文：\n{context_json}"
                ),
            },
        ],
        temperature: Some(0.1),
        max_tokens: Some(max_tokens),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    })
}

fn parse_global_summary_content(content: &str) -> Result<String> {
    let output =
        serde_json::from_str::<GlobalSummaryOutput>(strip_json_fence(content)).map_err(|err| {
            AppError::Llm(format!(
                "Writer response was not valid GlobalSummary JSON: {err}"
            ))
        })?;
    let global_summary_zh = output.global_summary_zh.trim().to_string();
    if global_summary_zh.is_empty() {
        return Err(AppError::Llm(
            "Writer global summary response contained empty global_summary_zh".to_string(),
        ));
    }
    reject_template_residue(&global_summary_zh)?;

    Ok(global_summary_zh)
}

fn global_synthesizer_system_prompt() -> String {
    "你是 LitScout-RS 的报告总编辑，负责基于章节终稿生成全局摘要。\n\
规则：\n\
1. 使用中文撰写 1-2 段自然的全局摘要。\n\
2. 概括报告覆盖范围、主要发现、证据边界和不足。\n\
3. 只能使用用户提供的章节终稿和 citation id，不得新增来源、URL 或事实。\n\
4. 不要输出固定模板句，不要写“来源链接”，不要写“是本章节的关键来源之一”。\n\
5. 不要把引用账本逐条改写成摘要。\n\
6. 只返回 JSON：{\"global_summary_zh\":\"...\"}。"
        .to_string()
}

fn parse_chapter_draft_content(
    content: &str,
    chapter: &ChapterNode,
    evidence_items: &[EvidenceItem],
) -> Result<ChapterDraft> {
    let output =
        serde_json::from_str::<ChapterDraftOutput>(strip_json_fence(content)).map_err(|err| {
            AppError::Llm(format!(
                "Writer response was not valid ChapterDraft JSON: {err}"
            ))
        })?;
    let paragraphs =
        parse_paragraph_outputs(output.paragraphs, evidence_items, MAX_CHAPTER_PARAGRAPHS)?;

    Ok(ChapterDraft {
        chapter_id: chapter.id.clone(),
        title_zh: chapter.title_zh.clone(),
        paragraphs,
    })
}

fn parse_paragraph_outputs(
    paragraph_outputs: Vec<ChapterParagraphOutput>,
    evidence_items: &[EvidenceItem],
    max_paragraphs: usize,
) -> Result<Vec<ParagraphWithCitations>> {
    let allowed_ids = evidence_items
        .iter()
        .map(|item| item.evidence_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut paragraphs = Vec::new();

    for paragraph in paragraph_outputs.into_iter().take(max_paragraphs) {
        let text_zh = paragraph.text_zh.trim().to_string();
        if text_zh.is_empty() {
            continue;
        }
        reject_template_residue(&text_zh)?;
        let cited_evidence_ids = paragraph
            .cited_evidence_ids
            .into_iter()
            .filter_map(|id| {
                let trimmed = id.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .collect::<Vec<_>>();
        if cited_evidence_ids.is_empty() {
            return Err(AppError::Llm(
                "Writer paragraph did not cite any evidence ID".to_string(),
            ));
        }
        for id in &cited_evidence_ids {
            if !allowed_ids.contains(id.as_str()) {
                return Err(AppError::Llm(format!(
                    "Writer paragraph referenced unknown evidence id `{id}`"
                )));
            }
        }
        paragraphs.push(ParagraphWithCitations {
            text_zh,
            cited_evidence_ids,
        });
    }

    if paragraphs.is_empty() {
        return Err(AppError::Llm(
            "Writer response contained no usable paragraphs".to_string(),
        ));
    }

    Ok(paragraphs)
}

fn reject_template_residue(text: &str) -> Result<()> {
    for marker in [
        "是本章节的关键来源之一",
        "来源链接：",
        "提供了与主题相关的证据",
    ] {
        if text.contains(marker) {
            return Err(AppError::Llm(format!(
                "Writer output still contains legacy template marker `{marker}`"
            )));
        }
    }
    Ok(())
}

pub fn render_report_markdown(draft: &ReportDraft, citations: &CitationLedger) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", draft.title_zh));
    out.push_str("## 1. 全局摘要\n\n");
    out.push_str(&draft.global_summary_zh);
    out.push_str("\n\n");

    for (index, chapter) in draft.chapters.iter().enumerate() {
        out.push_str(&format!("## {}. {}\n\n", index + 2, chapter.title_zh));
        for paragraph in &chapter.paragraphs {
            out.push_str(&paragraph.text_zh);
            if !paragraph.cited_evidence_ids.is_empty() {
                out.push(' ');
                out.push_str(
                    &paragraph
                        .cited_evidence_ids
                        .iter()
                        .map(|id| format!("`{id}`"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            out.push_str("\n\n");
        }
    }

    out.push_str("## 引用账本\n\n");
    if citations.citations.is_empty() {
        out.push_str("- 暂无引用。\n");
    } else {
        for citation in &citations.citations {
            out.push_str(&format!(
                "- `{}` [{}]({}) ({:?})\n",
                citation.id, citation.title, citation.url, citation.source_kind
            ));
        }
    }
    out
}

#[derive(Debug, Serialize)]
struct BatchDraftInput {
    chapter_title: String,
    research_question: String,
    batch_id: usize,
    total_batches: usize,
    batch_context_chars: usize,
    evidence_sources: Vec<EvidenceSourceInput>,
}

#[derive(Debug, Serialize)]
struct ChapterSynthesisInput {
    chapter_title: String,
    research_question: String,
    allowed_evidence_ids: Vec<String>,
    batch_drafts: Vec<BatchDraft>,
}

#[derive(Debug, Serialize)]
struct GlobalSummaryInput {
    topic: String,
    chapter_count: usize,
    evidence_count: usize,
    chapters: Vec<GlobalSummaryChapterInput>,
}

impl GlobalSummaryInput {
    fn from_report(topic: &str, chapter_drafts: &[ChapterDraft], evidence_count: usize) -> Self {
        Self {
            topic: topic.to_string(),
            chapter_count: chapter_drafts.len(),
            evidence_count,
            chapters: chapter_drafts
                .iter()
                .map(GlobalSummaryChapterInput::from)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GlobalSummaryChapterInput {
    chapter_id: String,
    title_zh: String,
    paragraph_count: usize,
    cited_evidence_ids: Vec<String>,
    paragraph_summaries: Vec<String>,
}

impl From<&ChapterDraft> for GlobalSummaryChapterInput {
    fn from(chapter: &ChapterDraft) -> Self {
        let cited_evidence_ids = chapter
            .paragraphs
            .iter()
            .flat_map(|paragraph| paragraph.cited_evidence_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            chapter_id: chapter.chapter_id.clone(),
            title_zh: chapter.title_zh.clone(),
            paragraph_count: chapter.paragraphs.len(),
            cited_evidence_ids,
            paragraph_summaries: chapter
                .paragraphs
                .iter()
                .map(|paragraph| excerpt_chars(&paragraph.text_zh, 500))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSourceInput {
    evidence_id: String,
    title: String,
    url: String,
    source_kind: SourceKind,
    evidence_note_zh: String,
    evidence_snippet: String,
}

impl From<&EvidenceItem> for EvidenceSourceInput {
    fn from(item: &EvidenceItem) -> Self {
        Self {
            evidence_id: item.evidence_id.clone(),
            title: item.title.clone(),
            url: item.url.clone(),
            source_kind: item.source_kind,
            evidence_note_zh: item.evidence_note_zh.clone(),
            evidence_snippet: item.evidence_snippet.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChapterDraftOutput {
    #[serde(default)]
    paragraphs: Vec<ChapterParagraphOutput>,
}

#[derive(Debug, Deserialize)]
struct BatchDraftOutput {
    #[serde(default)]
    findings_summary_zh: String,
    #[serde(default)]
    paragraphs: Vec<ChapterParagraphOutput>,
}

#[derive(Debug, Deserialize)]
struct GlobalSummaryOutput {
    #[serde(default)]
    global_summary_zh: String,
}

#[derive(Debug, Deserialize)]
struct ChapterParagraphOutput {
    text_zh: String,
    #[serde(default)]
    cited_evidence_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::llm::deepseek::DeepSeekConfig;
    use crate::model::{
        ChapterDraft, ChapterNode, EvidenceItem, EvidenceMemory, ParagraphWithCitations, SourceKind,
    };

    use super::{
        batch_chapter_evidence, build_batch_draft_request, build_chapter_synthesis_request,
        build_global_summary_request, draft_report_with_llm, empty_chapter_draft,
        parse_batch_draft_content, parse_chapter_draft_content, parse_global_summary_content,
        writer_max_tokens, BatchDraft, MAX_BATCH_CONTEXT_CHARS, MAX_EVIDENCE_PER_BATCH,
        WRITER_MIN_OUTPUT_TOKENS,
    };

    #[test]
    fn writer_drafts_empty_chapter_with_warning_text() {
        let chapters = vec![ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "核心方向".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["github".to_string()],
            evidence_quota: 1,
            sort_order: 1,
        }];

        let draft = empty_chapter_draft(&chapters[0]);

        assert_eq!(draft.chapter_id, "ch-1");
        assert!(draft.paragraphs[0]
            .text_zh
            .contains("尚未获得与章节计划直接匹配"));
    }

    #[tokio::test]
    async fn llm_writer_refuses_to_template_when_llm_is_disabled() {
        let chapters = vec![sample_chapter()];
        let memory = EvidenceMemory {
            items: vec![sample_evidence("ev-C1", "ch-1")],
            query_attempts: Vec::new(),
            source_lineage: Vec::new(),
        };

        let err = draft_report_with_llm(
            "Rust Agent",
            &chapters,
            &memory,
            &crate::config::LlmConfig::from_env(false, 30),
        )
        .await
        .expect_err("disabled LLM should not produce a template report");

        assert!(err
            .to_string()
            .contains("refusing to generate a template report"));
    }

    #[tokio::test]
    #[ignore = "requires a real DeepSeek API key in DEEPSEEK_API_KEY"]
    async fn writer_real_deepseek_smoke() {
        let chapters = vec![sample_chapter()];
        let memory = EvidenceMemory {
            items: vec![sample_evidence("ev-C1", "ch-1")],
            query_attempts: Vec::new(),
            source_lineage: Vec::new(),
        };
        let config = crate::config::LlmConfig::from_env(true, 120);

        assert!(
            config.api_key.is_some(),
            "DEEPSEEK_API_KEY must be set for this ignored smoke test"
        );

        let draft = draft_report_with_llm("TTS 入门级项目简单调研", &chapters, &memory, &config)
            .await
            .expect("real DeepSeek writer smoke should complete");

        assert!(!draft.global_summary_zh.trim().is_empty());
        assert_eq!(draft.chapters.len(), 1);
        assert!(!draft.chapters[0].paragraphs.is_empty());
        assert!(draft.chapters[0].paragraphs[0]
            .cited_evidence_ids
            .contains(&"ev-C1".to_string()));
        println!(
            "writer_real_deepseek_smoke ok: global_summary_chars={}, chapter_paragraphs={}",
            draft.global_summary_zh.chars().count(),
            draft.chapters[0].paragraphs.len()
        );
    }

    #[test]
    fn parses_llm_chapter_draft_json() {
        let chapter = sample_chapter();
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let content = r#"{
            "paragraphs": [
                {
                    "text_zh": "该项目展示了从 README 提炼出的 ReAct 主循环和工具调用结构，适合作为入门实现分析对象。",
                    "cited_evidence_ids": ["ev-C1"]
                }
            ]
        }"#;

        let draft = parse_chapter_draft_content(content, &chapter, &evidence)
            .expect("valid chapter JSON should parse");

        assert_eq!(draft.chapter_id, "ch-1");
        assert_eq!(draft.paragraphs[0].cited_evidence_ids, vec!["ev-C1"]);
        assert!(draft.paragraphs[0].text_zh.contains("ReAct"));
    }

    #[test]
    fn rejects_llm_chapter_draft_with_unknown_evidence_id() {
        let chapter = sample_chapter();
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let content = r#"{
            "paragraphs": [
                {
                    "text_zh": "引用了不存在的证据。",
                    "cited_evidence_ids": ["ev-C9"]
                }
            ]
        }"#;

        let err = parse_chapter_draft_content(content, &chapter, &evidence)
            .expect_err("unknown evidence id should fail");

        assert!(err.to_string().contains("unknown evidence id"));
    }

    #[test]
    fn rejects_llm_chapter_draft_with_template_residue() {
        let chapter = sample_chapter();
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let content = r#"{
            "paragraphs": [
                {
                    "text_zh": "acme/rust-agent 是本章节的关键来源之一。来源链接：https://github.com/acme/rust-agent",
                    "cited_evidence_ids": ["ev-C1"]
                }
            ]
        }"#;

        let err = parse_chapter_draft_content(content, &chapter, &evidence)
            .expect_err("legacy template residue should fail");

        assert!(err.to_string().contains("legacy template marker"));
    }

    #[test]
    fn batches_small_chapter_evidence_into_one_batch() {
        let evidence = vec![
            sample_evidence("ev-C1", "ch-1"),
            sample_evidence("ev-C2", "ch-1"),
            sample_evidence("ev-C3", "ch-1"),
        ];

        let batches = batch_chapter_evidence(&evidence);

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].batch_id, 1);
        assert_eq!(batches[0].evidence_items.len(), 3);
        assert!(batches[0].context_chars > 0);
    }

    #[test]
    fn batches_chapter_evidence_by_count() {
        let evidence = (1..=9)
            .map(|index| sample_evidence(&format!("ev-C{index}"), "ch-1"))
            .collect::<Vec<_>>();

        let batches = batch_chapter_evidence(&evidence);

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].evidence_items.len(), MAX_EVIDENCE_PER_BATCH);
        assert_eq!(batches[1].evidence_items.len(), MAX_EVIDENCE_PER_BATCH);
        assert_eq!(batches[2].evidence_items.len(), 1);
        assert_eq!(batches[2].batch_id, 3);
    }

    #[test]
    fn batches_chapter_evidence_by_context_chars() {
        let mut first = sample_evidence("ev-C1", "ch-1");
        first.evidence_snippet = "a".repeat(MAX_BATCH_CONTEXT_CHARS / 2 + 200);
        let mut second = sample_evidence("ev-C2", "ch-1");
        second.evidence_snippet = "b".repeat(MAX_BATCH_CONTEXT_CHARS / 2 + 200);

        let batches = batch_chapter_evidence(&[first, second]);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].evidence_items[0].evidence_id, "ev-C1");
        assert_eq!(batches[1].evidence_items[0].evidence_id, "ev-C2");
        assert!(batches[0].context_chars > MAX_BATCH_CONTEXT_CHARS / 2);
    }

    #[test]
    fn parses_llm_batch_draft_json() {
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let batches = batch_chapter_evidence(&evidence);
        let content = r#"{
            "findings_summary_zh": "该批证据说明了项目 README 中的 ReAct 主循环、工具注册和入门示例。",
            "paragraphs": [
                {
                    "text_zh": "该项目的 README 将 ReAct 主循环和工具注册拆成清晰的入门实现，适合用于说明基础 agent 架构。",
                    "cited_evidence_ids": ["ev-C1"]
                }
            ]
        }"#;

        let draft =
            parse_batch_draft_content(content, &batches[0]).expect("valid batch JSON should parse");

        assert_eq!(draft.batch_id, 1);
        assert!(draft.findings_summary_zh.contains("ReAct"));
        assert_eq!(draft.paragraphs[0].cited_evidence_ids, vec!["ev-C1"]);
    }

    #[test]
    fn rejects_llm_batch_draft_with_empty_findings() {
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let batches = batch_chapter_evidence(&evidence);
        let content = r#"{
            "findings_summary_zh": "",
            "paragraphs": [
                {
                    "text_zh": "该项目展示了一个基础实现。",
                    "cited_evidence_ids": ["ev-C1"]
                }
            ]
        }"#;

        let err = parse_batch_draft_content(content, &batches[0])
            .expect_err("empty findings should fail");

        assert!(err.to_string().contains("empty findings_summary_zh"));
    }

    #[test]
    fn rejects_llm_batch_draft_with_unknown_evidence_id() {
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let batches = batch_chapter_evidence(&evidence);
        let content = r#"{
            "findings_summary_zh": "该批证据有一个段落引用了不存在的证据。",
            "paragraphs": [
                {
                    "text_zh": "这里引用了不存在的 evidence id。",
                    "cited_evidence_ids": ["ev-C2"]
                }
            ]
        }"#;

        let err = parse_batch_draft_content(content, &batches[0])
            .expect_err("unknown evidence id should fail");

        assert!(err.to_string().contains("unknown evidence id"));
    }

    #[test]
    fn rejects_llm_batch_draft_with_template_residue() {
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let batches = batch_chapter_evidence(&evidence);
        let content = r#"{
            "findings_summary_zh": "acme/rust-agent 是本章节的关键来源之一。",
            "paragraphs": [
                {
                    "text_zh": "该项目展示了一个基础实现。",
                    "cited_evidence_ids": ["ev-C1"]
                }
            ]
        }"#;

        let err = parse_batch_draft_content(content, &batches[0])
            .expect_err("legacy template residue should fail");

        assert!(err.to_string().contains("legacy template marker"));
    }

    #[test]
    fn batch_request_only_contains_current_batch_evidence() {
        let chapter = sample_chapter();
        let evidence = (1..=5)
            .map(|index| sample_evidence(&format!("ev-C{index}"), "ch-1"))
            .collect::<Vec<_>>();
        let batches = batch_chapter_evidence(&evidence);

        let request = build_batch_draft_request(
            &chapter,
            &batches[1],
            batches.len(),
            &sample_deepseek_config(2048),
        )
        .expect("request should build");
        let user_message = &request.messages[1].content;

        assert!(user_message.contains("\"batch_id\": 2"));
        assert!(user_message.contains("ev-C5"));
        assert!(!user_message.contains("ev-C1"));
        assert_eq!(request.max_tokens, Some(WRITER_MIN_OUTPUT_TOKENS as u32));
    }

    #[test]
    fn writer_max_tokens_raises_too_small_config_for_reasoning_models() {
        let config = sample_deepseek_config(32);

        assert_eq!(writer_max_tokens(&config), WRITER_MIN_OUTPUT_TOKENS as u32);
    }

    #[test]
    fn chapter_synthesis_request_uses_batch_findings_instead_of_raw_evidence() {
        let chapter = sample_chapter();
        let evidence = vec![sample_evidence("ev-C1", "ch-1")];
        let batch_drafts = vec![BatchDraft {
            batch_id: 1,
            findings_summary_zh: "该批证据说明 README 中的 ReAct 主循环适合作为入门样例。"
                .to_string(),
            paragraphs: vec![ParagraphWithCitations {
                text_zh: "该项目展示了 ReAct 主循环和工具注册。".to_string(),
                cited_evidence_ids: vec!["ev-C1".to_string()],
            }],
        }];

        let request = build_chapter_synthesis_request(
            &chapter,
            &evidence,
            &batch_drafts,
            &sample_deepseek_config(4096),
        )
        .expect("request should build");
        let user_message = &request.messages[1].content;

        assert!(user_message.contains("batch_drafts"));
        assert!(user_message.contains("allowed_evidence_ids"));
        assert!(user_message.contains("ReAct 主循环适合作为入门样例"));
        assert!(!user_message.contains("evidence_snippet"));
        assert!(!user_message.contains("README explains ReAct loop"));
    }

    #[test]
    fn parses_llm_global_summary_json() {
        let content = r#"{
            "global_summary_zh": "本报告显示，可控 TTS 入门调研应同时关注基础项目、说话人控制和模型框架边界；现有证据主要来自 GitHub README 与 arXiv 摘要，因此结论应限定在这些来源范围内。"
        }"#;

        let summary =
            parse_global_summary_content(content).expect("valid global summary should parse");

        assert!(summary.contains("可控 TTS"));
        assert!(summary.contains("来源范围"));
    }

    #[test]
    fn rejects_llm_global_summary_with_template_residue() {
        let content = r#"{
            "global_summary_zh": "harinandanmv/text-to-speech 是本章节的关键来源之一。"
        }"#;

        let err =
            parse_global_summary_content(content).expect_err("legacy template marker should fail");

        assert!(err.to_string().contains("legacy template marker"));
    }

    #[test]
    fn global_summary_request_uses_chapter_drafts() {
        let chapter_drafts = vec![ChapterDraft {
            chapter_id: "ch-1".to_string(),
            title_zh: "可控 TTS 基础模型".to_string(),
            paragraphs: vec![ParagraphWithCitations {
                text_zh: "该章节围绕基础 TTS 模型、说话人控制和入门项目进行综合分析。".to_string(),
                cited_evidence_ids: vec!["ev-C1".to_string()],
            }],
        }];

        let request = build_global_summary_request(
            "TTS 入门级项目简单调研",
            &chapter_drafts,
            1,
            &sample_deepseek_config(4096),
        )
        .expect("request should build");
        let user_message = &request.messages[1].content;

        assert!(user_message.contains("TTS 入门级项目简单调研"));
        assert!(user_message.contains("可控 TTS 基础模型"));
        assert!(user_message.contains("ev-C1"));
        assert!(user_message.contains("paragraph_summaries"));
    }

    fn sample_chapter() -> ChapterNode {
        ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "核心方向".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["github".to_string()],
            evidence_quota: 1,
            sort_order: 1,
        }
    }

    fn sample_evidence(evidence_id: &str, chapter_id: &str) -> EvidenceItem {
        EvidenceItem {
            evidence_id: evidence_id.to_string(),
            source_item_id: "github:acme/rust-agent".to_string(),
            citation_id: "C1".to_string(),
            chapter_ids: vec![chapter_id.to_string()],
            query_attempt_ids: vec!["gh-1".to_string()],
            source_kind: SourceKind::GitHub,
            title: "acme/rust-agent".to_string(),
            url: "https://github.com/acme/rust-agent".to_string(),
            evidence_note_zh: "GitHub 仓库 `acme/rust-agent`：Rust agent framework".to_string(),
            evidence_snippet: "README explains ReAct loop, tool registry, and examples."
                .to_string(),
            support_score: None,
        }
    }

    fn sample_deepseek_config(max_tokens: usize) -> DeepSeekConfig {
        DeepSeekConfig {
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: "deepseek-v4-flash".to_string(),
            max_tokens,
            timeout_secs: 120,
        }
    }

    fn _dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
}
