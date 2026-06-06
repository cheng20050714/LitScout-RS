use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::LlmConfig;
use crate::error::{AppError, Result};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const CHAT_COMPLETIONS_PATH: &str = "chat/completions";
const DEEPSEEK_MAX_ATTEMPTS: usize = 3;
const DEEPSEEK_CONNECT_TIMEOUT_SECS: u64 = 15;

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
        let read_timeout = Duration::from_secs(config.timeout_secs);
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(DEEPSEEK_CONNECT_TIMEOUT_SECS))
            .read_timeout(read_timeout)
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .build()?;
        Ok(Self { config, http })
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
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        let content_encoding = content_encoding_label(response.headers());
        let body = response.text().await.map_err(|err| {
            let timeout_hint = if err.is_timeout() {
                "; timeout while waiting for DeepSeek response body"
            } else {
                ""
            };
            AppError::Llm(format!(
                "DeepSeek HTTP response body read failed (status {}, content-encoding `{}`{}): {err}",
                status.as_u16(),
                content_encoding,
                timeout_hint
            ))
        })?;

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

fn content_encoding_label(headers: &reqwest::header::HeaderMap) -> String {
    headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("identity")
        .to_string()
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
        let choice = self
            .choices
            .first()
            .ok_or_else(|| AppError::Llm("DeepSeek response had no choices".to_string()))?;
        if matches!(choice.finish_reason.as_deref(), Some("length")) {
            return Err(AppError::Llm(
                "DeepSeek response was truncated with finish_reason=length; increase output budget or reduce batch context"
                    .to_string(),
            ));
        }

        let content = choice.message.content.as_deref().map(str::trim);
        if let Some(content) = content.filter(|content| !content.is_empty()) {
            return Ok(content);
        }

        let finish_reason = choice.finish_reason.as_deref().unwrap_or("unknown");
        if choice
            .message
            .reasoning_content
            .as_deref()
            .map(str::trim)
            .is_some_and(|reasoning| !reasoning.is_empty())
        {
            return Err(AppError::Llm(format!(
                "DeepSeek response contained reasoning_content but no final message content (finish_reason={finish_reason}); increase max_tokens or use a non-reasoning model for JSON writing"
            )));
        }

        Err(AppError::Llm(format!(
            "DeepSeek response had no message content (finish_reason={finish_reason})"
        )))
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

#[cfg(test)]
mod tests {
    use super::{
        content_encoding_label, extract_urls, validate_translated_markdown, ChatChoice,
        ChatCompletionsRequest, ChatCompletionsResponse, ChatMessage, ChatResponseMessage,
        DeepSeekClient, DeepSeekConfig, ResponseFormat,
    };
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_ENCODING};

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

    #[test]
    fn first_content_rejects_truncated_response() {
        let response = ChatCompletionsResponse {
            id: None,
            model: None,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatResponseMessage {
                    role: Some("assistant".to_string()),
                    content: Some("{\"paragraphs\":[".to_string()),
                    reasoning_content: None,
                },
                finish_reason: Some("length".to_string()),
            }],
        };

        let err = response
            .first_content()
            .expect_err("length finish_reason should fail");

        assert!(err.to_string().contains("finish_reason=length"));
    }

    #[test]
    fn first_content_explains_reasoning_only_response() {
        let response = ChatCompletionsResponse {
            id: None,
            model: None,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatResponseMessage {
                    role: Some("assistant".to_string()),
                    content: Some("".to_string()),
                    reasoning_content: Some("我需要先分析，但没有输出最终 JSON。".to_string()),
                },
                finish_reason: Some("stop".to_string()),
            }],
        };

        let err = response
            .first_content()
            .expect_err("reasoning-only response should fail");

        assert!(err.to_string().contains("reasoning_content"));
        assert!(err.to_string().contains("no final message content"));
    }

    #[test]
    fn labels_missing_content_encoding_as_identity() {
        assert_eq!(content_encoding_label(&HeaderMap::new()), "identity");
    }

    #[test]
    fn trims_content_encoding_for_diagnostics() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(" gzip "));

        assert_eq!(content_encoding_label(&headers), "gzip");
    }
}
