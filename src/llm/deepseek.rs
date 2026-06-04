use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use reqwest::header::ACCEPT_ENCODING;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::model::{
    Citation, LlmSynthesis, ScoutReport, SearchAspect, SearchPlan, SearchQuery, SourceItem,
    SourceKind,
};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const CHAT_COMPLETIONS_PATH: &str = "chat/completions";
const MAX_CONTEXT_ITEMS: usize = 20;
const DEEPSEEK_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSeekConfig {
    pub api_key: String,
    pub base_url: String,
    pub main_model: String,
    pub side_model: String,
    pub max_tokens: usize,
    pub timeout_secs: u64,
}

impl DeepSeekConfig {
    pub fn from_llm_config(config: &LlmConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        Some(Self {
            api_key: config.api_key.clone()?,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            main_model: config.main_model.clone(),
            side_model: config
                .side_model
                .clone()
                .unwrap_or_else(|| "deepseek-v4-flash".to_string()),
            max_tokens: config.max_tokens,
            timeout_secs: config.timeout_secs,
        })
    }

    fn chat_completions_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            CHAT_COMPLETIONS_PATH
        )
    }
}

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    config: DeepSeekConfig,
    http: reqwest::Client,
}

impl DeepSeekClient {
    pub fn new(config: DeepSeekConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;
        Ok(Self { config, http })
    }

    pub async fn generate_search_plan(&self, query: &SearchQuery) -> Result<SearchPlan> {
        let request = self.build_search_plan_request(query)?;
        let response = self.chat_completions_with_retry(request).await?;
        let content = response.first_content()?;
        parse_search_plan_content(content, query)
    }

    pub async fn synthesize_report(&self, report: &ScoutReport) -> Result<LlmSynthesisResult> {
        let request = self.build_synthesis_request(report)?;
        let response = self.chat_completions_with_retry(request).await?;
        let content = response.first_content()?;

        match parse_and_validate_synthesis(content, report) {
            Ok(synthesis) => Ok(LlmSynthesisResult {
                synthesis,
                repaired: false,
            }),
            Err(first_err) => {
                warn!("DeepSeek synthesis validation failed, requesting one repair: {first_err}");
                let repair_request =
                    self.build_synthesis_repair_request(report, content, &first_err.to_string())?;
                let repaired_response = self.chat_completions_with_retry(repair_request).await?;
                let repaired_content = repaired_response.first_content()?;
                let synthesis = parse_and_validate_synthesis(repaired_content, report)?;
                Ok(LlmSynthesisResult {
                    synthesis,
                    repaired: true,
                })
            }
        }
    }

    pub async fn translate_report_to_chinese(&self, markdown: &str) -> Result<String> {
        let request = self.build_translation_request(markdown);
        let response = self.chat_completions_with_retry(request).await?;
        let translated = response.first_content()?.to_string();
        validate_translated_markdown(markdown, &translated)?;
        Ok(translated)
    }

    pub(crate) async fn chat_completions_with_retry(
        &self,
        request: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse> {
        let mut last_err = None;
        for attempt in 1..=DEEPSEEK_MAX_ATTEMPTS {
            match self.chat_completions(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(err) if attempt < DEEPSEEK_MAX_ATTEMPTS => {
                    let delay = Duration::from_millis(500 * attempt as u64);
                    warn!(
                        "DeepSeek request failed on attempt {attempt}/{DEEPSEEK_MAX_ATTEMPTS}, retrying after {:?}: {err}",
                        delay
                    );
                    last_err = Some(err);
                    tokio::time::sleep(delay).await;
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            AppError::Llm("DeepSeek request failed without an error payload".to_string())
        }))
    }

    pub async fn chat_completions(
        &self,
        request: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse> {
        let response = self
            .http
            .post(self.config.chat_completions_url())
            .bearer_auth(&self.config.api_key)
            .header(ACCEPT_ENCODING, "identity")
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(AppError::HttpStatus {
                service: "DeepSeek",
                status: status.as_u16(),
                body,
            });
        }

        serde_json::from_str(&body).map_err(AppError::from)
    }

    pub(crate) fn side_model(&self) -> String {
        self.config.side_model.clone()
    }

    fn build_synthesis_request(&self, report: &ScoutReport) -> Result<ChatCompletionsRequest> {
        let context = build_llm_context(report);
        let context_json = serde_json::to_string_pretty(&context)?;
        let max_tokens = self.config.max_tokens.min(u32::MAX as usize) as u32;

        Ok(ChatCompletionsRequest {
            model: self.config.main_model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt().to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "请分析以下 LitScout-RS 上下文，并只返回约定的 JSON 对象。\n\n{context_json}"
                    ),
                },
            ],
            temperature: Some(0.2),
            max_tokens: Some(max_tokens),
            stream: Some(false),
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
        })
    }

    fn build_synthesis_repair_request(
        &self,
        report: &ScoutReport,
        previous_output: &str,
        validation_error: &str,
    ) -> Result<ChatCompletionsRequest> {
        let context = build_llm_context(report);
        let context_json = serde_json::to_string_pretty(&context)?;
        let max_tokens = self.config.max_tokens.min(u32::MAX as usize) as u32;

        Ok(ChatCompletionsRequest {
            model: self.config.main_model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt().to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "请修复上一次 JSON 输出，使其满足校验规则。只返回修复后的 JSON 对象。\n\n校验错误：\n{validation_error}\n\n上一次输出：\n{previous_output}\n\n允许使用的上下文：\n{context_json}"
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

    fn build_search_plan_request(&self, query: &SearchQuery) -> Result<ChatCompletionsRequest> {
        let max_tokens = self.config.max_tokens.min(2048).min(u32::MAX as usize) as u32;
        let content = format!(
            "Create a bounded LitScout-RS search plan for this topic: `{}`.\nReturn only JSON with this shape: {{\"aspects\":[{{\"name\":\"core concept\",\"github_query\":\"...\",\"arxiv_query\":\"...\",\"rationale\":\"...\"}}]}}.\nRules: create 1 to 3 aspects; keep each query short; use only GitHub/arXiv-searchable terms; do not add web sources.",
            query.topic
        );

        Ok(ChatCompletionsRequest {
            model: self.config.side_model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "You create small deterministic search plans for a Rust CLI that only queries GitHub and arXiv. Return JSON only."
                        .to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content,
                },
            ],
            temperature: Some(0.2),
            max_tokens: Some(max_tokens),
            stream: Some(false),
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
        })
    }

    fn build_translation_request(&self, markdown: &str) -> ChatCompletionsRequest {
        let max_tokens = self.config.max_tokens.min(u32::MAX as usize) as u32;
        ChatCompletionsRequest {
            model: self.config.main_model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "你是 LitScout-RS 的中文报告翻译器。把用户提供的 Markdown 调研报告翻译成自然、准确的中文。必须保留 Markdown 结构、代码样式、repo 名、论文标题、citation id、所有 URL 和链接目标；不得新增来源、不得删除链接、不得解释翻译过程。只输出翻译后的 Markdown。".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: markdown.to_string(),
                },
            ],
            temperature: Some(0.1),
            max_tokens: Some(max_tokens),
            stream: Some(false),
            response_format: None,
        }
    }
}

fn system_prompt() -> &'static str {
    "你是 LitScout-RS 的 LLM 分析层。只能使用提供的 GitHub 与 arXiv JSON 上下文；不得联网、不得编造来源、不得添加引用账本之外的 URL。返回单个合法 JSON 对象，字段必须且只能包含：executive_summary: string, key_findings: string[], recommended_reading_path: string[], limitations: string[], used_citation_ids: string[]。executive_summary、key_findings、recommended_reading_path、limitations 必须使用中文；原始 repo 名、论文标题、citation id 和 URL 保持原样。每条分析性结论都应包含 Markdown 链接，链接 URL 必须来自 citation_ledger。"
}

fn build_llm_context(report: &ScoutReport) -> LlmReportContext {
    let citation_by_item = report
        .citations
        .citations
        .iter()
        .map(|citation| (citation.source_item_id.as_str(), citation))
        .collect::<HashMap<_, _>>();

    let sources = report
        .ranked_items
        .iter()
        .take(MAX_CONTEXT_ITEMS)
        .filter_map(|item| {
            citation_by_item
                .get(item.id.as_str())
                .map(|citation| source_context(item, citation))
        })
        .collect();

    LlmReportContext {
        topic: report.query.topic.clone(),
        sources,
        citation_ledger: report.citations.citations.clone(),
        required_output: LlmRequiredOutput {
            executive_summary: "中文摘要，包含指向 citation URL 的 Markdown 链接".to_string(),
            key_findings: "中文要点数组，每条都应由 citation URL 支撑".to_string(),
            recommended_reading_path: "中文阅读路径数组，步骤中保留 citation URL".to_string(),
            limitations: "中文局限性数组，说明数据覆盖或来源限制".to_string(),
            used_citation_ids: "array containing only IDs from citation_ledger".to_string(),
        },
    }
}

fn source_context(item: &SourceItem, citation: &Citation) -> LlmSourceContext {
    LlmSourceContext {
        citation_id: citation.id.clone(),
        item_id: item.id.clone(),
        kind: item.kind,
        title: item.title.clone(),
        url: item.url.clone(),
        summary: item.summary.clone(),
        evidence_snippet: item.evidence_snippet.clone(),
        tags: item.tags.clone(),
        score: item.score,
        score_reasons: item.score_reasons.clone(),
        classification_reasons: item.classification_reasons.clone(),
    }
}

fn parse_synthesis_content(content: &str) -> Result<LlmSynthesis> {
    let json = strip_json_fence(content);
    serde_json::from_str::<LlmSynthesis>(json).map_err(|err| {
        AppError::Llm(format!(
            "DeepSeek response was not a valid LlmSynthesis JSON object: {err}"
        ))
    })
}

fn parse_and_validate_synthesis(content: &str, report: &ScoutReport) -> Result<LlmSynthesis> {
    let synthesis = parse_synthesis_content(content)?;
    validate_synthesis(&synthesis, report)?;
    Ok(synthesis)
}

fn parse_search_plan_content(content: &str, query: &SearchQuery) -> Result<SearchPlan> {
    let json = strip_json_fence(content);
    let output = serde_json::from_str::<SearchPlanOutput>(json).map_err(|err| {
        AppError::Llm(format!(
            "DeepSeek response was not a valid SearchPlan JSON object: {err}"
        ))
    })?;

    let aspects = output
        .aspects
        .into_iter()
        .take(3)
        .filter_map(|aspect| {
            let github_query = aspect.github_query.trim().to_string();
            let arxiv_query = aspect.arxiv_query.trim().to_string();
            if github_query.is_empty() || arxiv_query.is_empty() {
                return None;
            }
            Some(SearchAspect {
                name: non_empty_or_default(aspect.name, "llm aspect"),
                github_query,
                arxiv_query,
                github_limit: split_limit(query.github_limit, 3),
                arxiv_limit: split_limit(query.arxiv_limit, 3),
                rationale: aspect.rationale.filter(|value| !value.trim().is_empty()),
            })
        })
        .collect::<Vec<_>>();

    if aspects.is_empty() {
        return Err(AppError::Llm(
            "DeepSeek SearchPlan did not contain usable aspects".to_string(),
        ));
    }

    Ok(SearchPlan {
        original_topic: query.topic.clone(),
        aspects,
        llm_generated: true,
    })
}

fn non_empty_or_default(value: String, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn split_limit(limit: usize, parts: usize) -> usize {
    if parts == 0 {
        return limit.max(1);
    }
    limit.div_ceil(parts).max(1)
}

pub(crate) fn strip_json_fence(content: &str) -> &str {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }

    let Some(first_newline) = trimmed.find('\n') else {
        return trimmed;
    };
    let body = &trimmed[first_newline + 1..];
    body.strip_suffix("```").unwrap_or(body).trim()
}

fn validate_synthesis(synthesis: &LlmSynthesis, report: &ScoutReport) -> Result<()> {
    let citation_ids = report
        .citations
        .citations
        .iter()
        .map(|citation| citation.id.as_str())
        .collect::<HashSet<_>>();
    let citation_urls = report
        .citations
        .citations
        .iter()
        .map(|citation| citation.url.as_str())
        .collect::<HashSet<_>>();

    if !report.citations.citations.is_empty() && synthesis.used_citation_ids.is_empty() {
        return Err(AppError::Llm(
            "DeepSeek synthesis did not declare any used citation IDs".to_string(),
        ));
    }

    for citation_id in &synthesis.used_citation_ids {
        if !citation_ids.contains(citation_id.as_str()) {
            return Err(AppError::Llm(format!(
                "DeepSeek synthesis referenced missing citation id `{citation_id}`"
            )));
        }
    }

    let text = synthesis_text(synthesis);
    let used_urls = extract_urls(&text);
    if !report.citations.citations.is_empty() && used_urls.is_empty() {
        return Err(AppError::Llm(
            "DeepSeek synthesis did not include any citation URLs".to_string(),
        ));
    }
    for url in used_urls {
        if !citation_urls.contains(url.as_str()) {
            return Err(AppError::Llm(format!(
                "DeepSeek synthesis included URL outside CitationLedger: {url}"
            )));
        }
    }

    Ok(())
}

pub(crate) fn validate_translated_markdown(original: &str, translated: &str) -> Result<()> {
    let original_urls = extract_urls(original).into_iter().collect::<HashSet<_>>();
    let translated_urls = extract_urls(translated).into_iter().collect::<HashSet<_>>();

    for url in &original_urls {
        if !translated_urls.contains(url) {
            return Err(AppError::Llm(format!(
                "DeepSeek translation dropped source URL: {url}"
            )));
        }
    }

    for url in &translated_urls {
        if !original_urls.contains(url) {
            return Err(AppError::Llm(format!(
                "DeepSeek translation introduced URL outside original report: {url}"
            )));
        }
    }

    Ok(())
}

fn synthesis_text(synthesis: &LlmSynthesis) -> String {
    let mut text = synthesis.executive_summary.clone();
    for field in [
        &synthesis.key_findings,
        &synthesis.recommended_reading_path,
        &synthesis.limitations,
    ] {
        for value in field {
            text.push('\n');
            text.push_str(value);
        }
    }
    text
}

fn extract_urls(text: &str) -> Vec<String> {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| {
        Regex::new(r#"https?://[A-Za-z0-9._~:/?#@!$&*+,;=%-]+"#).expect("URL regex should compile")
    });

    url_re
        .find_iter(text)
        .map(|match_| trim_url(match_.as_str()).to_string())
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .collect()
}

fn trim_url(url: &str) -> &str {
    url.trim_matches(|ch: char| {
        matches!(
            ch,
            '(' | ')'
                | '['
                | ']'
                | '<'
                | '>'
                | ','
                | '.'
                | ';'
                | ':'
                | '!'
                | '?'
                | '。'
                | '，'
                | '；'
                | '：'
                | '！'
                | '？'
                | '、'
                | '）'
                | '】'
                | '》'
                | '」'
                | '』'
                | '”'
                | '’'
        )
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseFormat {
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionsResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
}

impl ChatCompletionsResponse {
    pub(crate) fn first_content(&self) -> Result<&str> {
        self.choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(str::trim)
            .filter(|content| !content.is_empty())
            .ok_or_else(|| AppError::Llm("DeepSeek response had no message content".to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatResponseMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatResponseMessage {
    pub role: Option<String>,
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmSynthesisResult {
    pub synthesis: LlmSynthesis,
    pub repaired: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchPlanOutput {
    aspects: Vec<SearchAspect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmReportContext {
    topic: String,
    sources: Vec<LlmSourceContext>,
    citation_ledger: Vec<Citation>,
    required_output: LlmRequiredOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmSourceContext {
    citation_id: String,
    item_id: String,
    kind: SourceKind,
    title: String,
    url: String,
    summary: String,
    evidence_snippet: String,
    tags: Vec<String>,
    score: f64,
    score_reasons: Vec<String>,
    classification_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmRequiredOutput {
    executive_summary: String,
    key_findings: String,
    recommended_reading_path: String,
    limitations: String,
    used_citation_ids: String,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{
        extract_urls, parse_search_plan_content, parse_synthesis_content, validate_synthesis,
        validate_translated_markdown, ChatCompletionsRequest, ChatMessage, DeepSeekClient,
        DeepSeekConfig, ResponseFormat,
    };
    use crate::model::{
        ArxivPaper, CitationLedger, GitHubRepo, QualityReport, ScoutReport, SearchPlan,
        SearchQuery, SourceItem,
    };

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn parses_valid_llm_synthesis_json() {
        let content = r#"{
            "executive_summary": "Start with [repo](https://github.com/acme/rust-agent).",
            "key_findings": ["The repo is relevant [repo](https://github.com/acme/rust-agent)."],
            "recommended_reading_path": ["Read [repo](https://github.com/acme/rust-agent)."],
            "limitations": ["Only one fixture source."],
            "used_citation_ids": ["C1"]
        }"#;

        let synthesis = parse_synthesis_content(content).expect("JSON should parse");

        assert_eq!(synthesis.used_citation_ids, vec!["C1"]);
    }

    #[test]
    fn rejects_synthesis_with_unknown_citation_url() {
        let report = sample_report();
        let synthesis = parse_synthesis_content(
            r#"{
                "executive_summary": "Unknown [source](https://example.com/fake).",
                "key_findings": [],
                "recommended_reading_path": [],
                "limitations": [],
                "used_citation_ids": ["C1"]
            }"#,
        )
        .unwrap();

        let err = validate_synthesis(&synthesis, &report).expect_err("unknown URL should fail");

        assert!(err.to_string().contains("outside CitationLedger"));
    }

    #[test]
    fn extracts_markdown_urls() {
        let urls = extract_urls("See [repo](https://github.com/acme/rust-agent).");

        assert_eq!(urls, vec!["https://github.com/acme/rust-agent"]);
    }

    #[test]
    fn extracts_urls_before_chinese_markdown_punctuation() {
        let urls = extract_urls(
            "参考 [CoCoEmo](https://github.com/wsssy/CoCoEmo)，其中介绍了论文“CoCoEmo”。",
        );

        assert_eq!(urls, vec!["https://github.com/wsssy/CoCoEmo"]);
    }

    #[test]
    fn validates_translation_preserves_urls() {
        let original = "See [repo](https://github.com/acme/rust-agent).";
        let translated = "参见 [repo](https://github.com/acme/rust-agent)。";

        validate_translated_markdown(original, translated).expect("same URL should pass");
    }

    #[test]
    fn rejects_translation_that_adds_new_url() {
        let original = "See [repo](https://github.com/acme/rust-agent).";
        let translated = "参见 [repo](https://github.com/acme/rust-agent) 和 https://example.com。";

        let err = validate_translated_markdown(original, translated)
            .expect_err("new URL should be rejected");

        assert!(err.to_string().contains("introduced URL"));
    }

    #[test]
    fn serializes_json_response_format() {
        let request = ChatCompletionsRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Return JSON".to_string(),
            }],
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stream: Some(false),
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
        };

        let value = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(value["response_format"]["type"], "json_object");
        assert_eq!(value["stream"], false);
    }

    #[test]
    fn parses_search_plan_json_and_limits_aspects() {
        let query = SearchQuery {
            topic: "rust agent framework".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let content = r#"{
            "aspects": [
                {"name":"core","github_query":"rust agent framework","arxiv_query":"rust agent framework","rationale":"core"},
                {"name":"bench","github_query":"agent benchmark rust","arxiv_query":"agent benchmark","rationale":"bench"},
                {"name":"tools","github_query":"tool calling rust","arxiv_query":"tool calling agents","rationale":"tools"},
                {"name":"extra","github_query":"extra","arxiv_query":"extra","rationale":"extra"}
            ]
        }"#;

        let plan = parse_search_plan_content(content, &query).expect("plan should parse");

        assert!(plan.llm_generated);
        assert_eq!(plan.original_topic, "rust agent framework");
        assert_eq!(plan.aspects.len(), 3);
        assert_eq!(plan.aspects[0].github_query, "rust agent framework");
    }

    #[test]
    fn builds_chat_completion_url_without_double_slash() {
        let config = DeepSeekConfig {
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com/".to_string(),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: "deepseek-v4-flash".to_string(),
            max_tokens: 1024,
            timeout_secs: 30,
        };
        let client = DeepSeekClient::new(config).expect("client should build");

        assert_eq!(
            client.config.chat_completions_url(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    fn sample_report() -> ScoutReport {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let repo = GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework".to_string()),
            stars: 10,
            forks: 1,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec!["rust".to_string()],
            readme_excerpt: None,
        };
        let paper = ArxivPaper {
            arxiv_id: "2501.00001".to_string(),
            title: "Rust Agent Paper".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A paper about Rust agents.".to_string(),
            published_at: dt(),
            updated_at: None,
            categories: vec!["cs.AI".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001".to_string(),
            pdf_url: None,
        };
        let items = vec![SourceItem::from(&repo), SourceItem::from(&paper)];
        let citations = CitationLedger::from_items(&items);

        ScoutReport {
            query: query.clone(),
            plan: SearchPlan::from_query(&query),
            generated_at: dt(),
            github_repos: vec![repo],
            arxiv_papers: vec![paper],
            ranked_items: items,
            groups: vec![],
            citations,
            llm_synthesis: None,
            quality: QualityReport::pass(),
        }
    }
}
