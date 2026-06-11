use chrono::Utc;
use serde_json::Value;

use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{
    strip_json_fence, ChatCompletionsRequest, ChatMessage, DeepSeekClient, DeepSeekConfig,
    ResponseFormat,
};
use crate::reading::models::{PaperNote, ReadingLibraryItem, TextCoverage};

const MAX_PAPER_TEXT_CHARS: usize = 36_000;

#[derive(Debug, Clone)]
struct NoteOutput {
    tldr: String,
    motivation: String,
    method: String,
    result: String,
    conclusion: String,
    core_problem: String,
    contributions: Vec<String>,
    method_map: Vec<String>,
    experiment_matrix: Vec<String>,
    limitations: Vec<String>,
    reproducibility_notes: Vec<String>,
    relation_to_research_topic: String,
    recommended_questions: Vec<String>,
}

pub async fn generate_note(item: &ReadingLibraryItem, llm_config: &LlmConfig) -> Result<PaperNote> {
    if matches!(
        item.text_coverage,
        Some(TextCoverage::AbstractOnly | TextCoverage::Failed) | None
    ) {
        return Err(AppError::Workflow(
            "未获取到通过质量门控的论文全文，不能生成详细阅读笔记。".to_string(),
        ));
    }
    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig("生成阅读笔记需要 DeepSeek API Key。".to_string())
    })?;
    let client = DeepSeekClient::new(config)?;
    let paper_text = item.text.as_deref().unwrap_or(&item.summary);
    let text_coverage = item
        .text_coverage
        .as_ref()
        .map(|coverage| format!("{coverage:?}"))
        .unwrap_or_else(|| "unknown".to_string());
    let context = section_aware_context(paper_text, MAX_PAPER_TEXT_CHARS);
    let extraction_meta = item
        .text_meta
        .as_ref()
        .map(|meta| {
            format!(
                "extractor={}, source_url={}, char_count={}, page_count={}, quality_score={:.2}",
                meta.extractor,
                meta.source_url,
                meta.char_count,
                meta.page_count
                    .map(|pages| pages.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                meta.quality_score
            )
        })
        .unwrap_or_else(|| "unknown".to_string());
    let request = ChatCompletionsRequest {
        model: llm_config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是严谨的中文论文阅读助手。只依据用户提供的论文文本和元数据生成深度阅读笔记，不要自行联网，不要编造论文没有出现的实验、数据、GPU、代码链接或结论。输出必须是严格 JSON。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "论文标题：{}\n论文链接：{}\n文本覆盖：{}\n提取元数据：{}\n\n论文文本：\n{}\n\n请输出 JSON，字段包括：tldr, motivation, method, result, conclusion, core_problem, contributions, method_map, experiment_matrix, limitations, reproducibility_notes, relation_to_research_topic, recommended_questions。数组字段至少 3 条，recommended_questions 至少 6 条。中文输出。",
                    item.title, item.abs_url, text_coverage, extraction_meta, context
                ),
            },
        ],
        temperature: Some(0.2),
        max_tokens: Some(llm_config.max_tokens.min(4096) as u32),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    };
    let response = client.chat_completions_with_retry(request).await?;
    let content = response.first_content()?;
    let parsed = parse_note_output(content)?;
    Ok(PaperNote {
        markdown: render_note_markdown(item, &parsed),
        tldr: parsed.tldr,
        motivation: parsed.motivation,
        method: parsed.method,
        result: parsed.result,
        conclusion: parsed.conclusion,
        core_problem: parsed.core_problem,
        contributions: parsed.contributions,
        method_map: parsed.method_map,
        experiment_matrix: parsed.experiment_matrix,
        limitations: parsed.limitations,
        reproducibility_notes: parsed.reproducibility_notes,
        relation_to_research_topic: parsed.relation_to_research_topic,
        recommended_questions: parsed.recommended_questions,
        generated_at: Utc::now(),
    })
}

fn render_note_markdown(item: &ReadingLibraryItem, note: &NoteOutput) -> String {
    format!(
        "# {}\n\n## 速览\n\n**TLDR**：{}\n\n**Motivation**：{}\n\n**Method**：{}\n\n**Result**：{}\n\n**Conclusion**：{}\n\n## 核心问题\n\n{}\n\n## 关键贡献\n\n{}\n\n## 方法拆解\n\n{}\n\n## 实验矩阵\n\n{}\n\n## 局限与风险\n\n{}\n\n## 复现要点\n\n{}\n\n## 与当前调研主题的关系\n\n{}\n\n## 推荐继续追问\n\n{}\n",
        item.title,
        note.tldr,
        note.motivation,
        note.method,
        note.result,
        note.conclusion,
        note.core_problem,
        bullet_list(&note.contributions),
        bullet_list(&note.method_map),
        bullet_list(&note.experiment_matrix),
        bullet_list(&note.limitations),
        bullet_list(&note.reproducibility_notes),
        note.relation_to_research_topic,
        bullet_list(&note.recommended_questions),
    )
}

fn parse_note_output(content: &str) -> Result<NoteOutput> {
    let value: Value = serde_json::from_str(strip_json_fence(content))
        .map_err(|err| AppError::Llm(format!("阅读笔记不是合法 JSON：{err}; raw={content}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| AppError::Llm(format!("阅读笔记 JSON 顶层必须是对象；raw={content}")))?;
    Ok(NoteOutput {
        tldr: value_to_text(obj.get("tldr")),
        motivation: value_to_text(obj.get("motivation")),
        method: value_to_text(obj.get("method")),
        result: value_to_text(obj.get("result")),
        conclusion: value_to_text(obj.get("conclusion")),
        core_problem: value_to_text(obj.get("core_problem")),
        contributions: value_to_list(obj.get("contributions")),
        method_map: value_to_list(obj.get("method_map")),
        experiment_matrix: value_to_list(obj.get("experiment_matrix")),
        limitations: value_to_list(obj.get("limitations")),
        reproducibility_notes: value_to_list(obj.get("reproducibility_notes")),
        relation_to_research_topic: value_to_text(obj.get("relation_to_research_topic")),
        recommended_questions: value_to_list(obj.get("recommended_questions")),
    })
}

fn value_to_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .map(value_to_inline_text)
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, value)| {
                let text = value_to_inline_text(value);
                if text.is_empty() {
                    key.to_string()
                } else {
                    format!("{key}：{text}")
                }
            })
            .collect::<Vec<_>>()
            .join("；"),
        Some(other) => value_to_inline_text(other),
        None => String::new(),
    }
}

fn value_to_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .flat_map(|item| match item {
                Value::Object(map) => map
                    .iter()
                    .map(|(key, value)| {
                        let text = value_to_inline_text(value);
                        if text.is_empty() {
                            key.to_string()
                        } else {
                            format!("{key}：{text}")
                        }
                    })
                    .collect::<Vec<_>>(),
                other => vec![value_to_inline_text(other)],
            })
            .filter(|item| !item.trim().is_empty())
            .collect(),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, value)| {
                let text = value_to_inline_text(value);
                if text.is_empty() {
                    key.to_string()
                } else {
                    format!("{key}：{text}")
                }
            })
            .filter(|item| !item.trim().is_empty())
            .collect(),
        Some(Value::String(text)) => split_text_list(text),
        Some(other) => {
            let text = value_to_inline_text(other);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            }
        }
        None => Vec::new(),
    }
}

fn value_to_inline_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Array(items) => items
            .iter()
            .map(value_to_inline_text)
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join("；"),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                let text = value_to_inline_text(value);
                if text.is_empty() {
                    key.to_string()
                } else {
                    format!("{key}：{text}")
                }
            })
            .collect::<Vec<_>>()
            .join("；"),
        Value::Null => String::new(),
    }
}

fn split_text_list(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let parts = trimmed
        .lines()
        .map(|line| line.trim().trim_start_matches('-').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        return parts;
    }
    vec![trimmed.to_string()]
}

fn bullet_list(items: &[String]) -> String {
    items
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| format!("- {}", item.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

fn section_aware_context(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let sections = split_sections(text);
    if sections.len() <= 1 {
        return truncate_chars(text, max_chars);
    }

    let priority_patterns = [
        "abstract",
        "introduction",
        "method",
        "methods",
        "methodology",
        "approach",
        "experiment",
        "experiments",
        "evaluation",
        "results",
        "analysis",
        "discussion",
        "limitations",
        "conclusion",
    ];
    let mut selected = Vec::new();
    for pattern in priority_patterns {
        if let Some((_, body)) = sections
            .iter()
            .find(|(heading, _)| heading.to_ascii_lowercase().contains(pattern))
        {
            selected.push(body.as_str());
        }
    }
    let section_budget = (max_chars / selected.len().max(1)).max(800);
    let mut context = selected
        .iter()
        .map(|body| truncate_chars(body, section_budget))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    if context.trim().is_empty() {
        context = truncate_chars(text, max_chars);
    }
    truncate_chars(&context, max_chars)
}

fn split_sections(text: &str) -> Vec<(String, String)> {
    let heading_re = regex::Regex::new(
        r"(?im)^\s*(\d+\.?\s*)?(abstract|introduction|method|methods|methodology|approach|model|framework|experiment|experiments|evaluation|results|analysis|discussion|limitations|conclusion|摘要|引言|方法|实验|评估|结论)\b.*$",
    )
    .expect("section regex should compile");
    let matches = heading_re
        .find_iter(text)
        .filter(|matched| matched.as_str().trim().chars().count() <= 120)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Vec::new();
    }
    matches
        .iter()
        .enumerate()
        .map(|(index, matched)| {
            let end = matches
                .get(index + 1)
                .map(|next| next.start())
                .unwrap_or_else(|| text.len());
            (
                matched.as_str().trim().to_string(),
                text[matched.start()..end].trim().to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{generate_note, parse_note_output};
    use crate::config::LlmConfig;
    use crate::reading::models::{ReadingLibraryItem, ReadingStatus, TextCoverage};

    fn disabled_llm_config() -> LlmConfig {
        LlmConfig {
            enabled: false,
            api_key: None,
            base_url: Some("https://api.deepseek.com".to_string()),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: Some("deepseek-v4-flash".to_string()),
            max_tokens: 4096,
            timeout_secs: 30,
        }
    }

    fn item_with_coverage(coverage: TextCoverage) -> ReadingLibraryItem {
        ReadingLibraryItem {
            paper_key: "arxiv-1706.03762".to_string(),
            source_item_id: "arxiv:1706.03762".to_string(),
            evidence_id: "ev-1".to_string(),
            run_id: None,
            title: "Attention Is All You Need".to_string(),
            abs_url: "https://arxiv.org/abs/1706.03762".to_string(),
            pdf_url: Some("https://arxiv.org/pdf/1706.03762".to_string()),
            summary: "summary".to_string(),
            added_at: Utc::now(),
            updated_at: Utc::now(),
            status: ReadingStatus::TextReady,
            text_coverage: Some(coverage),
            text: Some("Abstract\ntext".to_string()),
            text_source_url: Some("https://arxiv.org/pdf/1706.03762".to_string()),
            text_meta: None,
            note_quality: None,
            note: None,
            chat_history: Vec::new(),
            error: None,
        }
    }

    #[tokio::test]
    async fn rejects_abstract_only_before_llm_config_check() {
        let config = disabled_llm_config();
        let err = generate_note(&item_with_coverage(TextCoverage::AbstractOnly), &config)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("未获取到通过质量门控的论文全文"));
    }

    #[tokio::test]
    async fn rejects_failed_coverage_before_llm_config_check() {
        let config = disabled_llm_config();
        let err = generate_note(&item_with_coverage(TextCoverage::Failed), &config)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("未获取到通过质量门控的论文全文"));
    }

    #[test]
    fn parses_flexible_note_shapes_from_llm() {
        let raw = r#"{
          "tldr": "本文系统实证分析了离散单元表示对语音语言模型的影响。",
          "motivation": [
            "离散单元表示在语音语言建模中日益流行。",
            "持续预训练的优化策略尚不明确。"
          ],
          "method": "多设置对比实验。",
          "result": [
            "模型架构影响显著。",
            "数据表示是关键因素。"
          ],
          "conclusion": "需要精心设计架构和训练流程。",
          "core_problem": "缺乏系统实证指导。",
          "contributions": [
            "大规模实证分析。",
            "揭示关键因素。"
          ],
          "method_map": {
            "empirical": "控制变量实验",
            "analysis": "多因素对比分析"
          },
          "experiment_matrix": [
            "模型架构：不同参数量",
            "数据表示：不同离散单元"
          ],
          "limitations": [
            "可能局限于特定基准。"
          ],
          "reproducibility_notes": "复现需依赖作者发布配置。",
          "relation_to_research_topic": "为语音语言模型预训练提供依据。",
          "recommended_questions": [
            "不同离散单元差异是什么？",
            "架构如何影响建模能力？"
          ]
        }"#;

        let note = parse_note_output(raw).expect("flexible note should parse");

        assert!(note.motivation.contains("离散单元表示"));
        assert_eq!(note.method_map[0], "analysis：多因素对比分析");
        assert_eq!(note.reproducibility_notes, vec!["复现需依赖作者发布配置。"]);
        assert_eq!(note.recommended_questions.len(), 2);
    }

    #[test]
    fn section_aware_context_keeps_priority_sections() {
        let text = format!(
            "Abstract\n{}\n\n1 Introduction\n{}\n\n2 Method\n{}\n\nReferences\n{}",
            "abstract signal ".repeat(300),
            "introduction signal ".repeat(300),
            "method signal ".repeat(300),
            "reference signal ".repeat(1000)
        );
        let context = super::section_aware_context(&text, 5_000);
        assert!(context.contains("abstract signal"));
        assert!(context.contains("introduction signal"));
        assert!(context.contains("method signal"));
        assert!(context.len() < text.len());
    }
}
