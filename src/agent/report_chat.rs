use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{ChatCompletionsRequest, ChatMessage, DeepSeekClient, DeepSeekConfig};
use tokio::sync::mpsc;

const MAX_REPORT_CONTEXT_CHARS: usize = 24_000;

pub async fn answer_report_question(
    report_markdown: &str,
    question: &str,
    llm_config: &LlmConfig,
) -> Result<String> {
    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "Report chat requires a DeepSeek API key. Configure it in stage 1 or start with --llm."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    let context = truncate_report(report_markdown);
    let request = ChatCompletionsRequest {
        model: llm_config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是 LitScout-RS 的报告问答助手。只能依据用户提供的 Markdown 调研报告回答。不要自行联网，不要编造来源，不要添加报告中不存在的 URL。回答使用中文，并尽量引用报告中已有链接。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!("报告内容：\n\n{context}\n\n用户问题：{question}"),
            },
        ],
        temperature: Some(0.2),
        max_tokens: Some(llm_config.max_tokens.min(2048) as u32),
        stream: Some(false),
        response_format: None,
    };
    let response = client.chat_completions_with_retry(request).await?;
    Ok(response.first_content()?.to_string())
}

pub async fn answer_report_question_streaming(
    report_markdown: &str,
    question: &str,
    llm_config: &LlmConfig,
    delta_tx: mpsc::Sender<String>,
) -> Result<String> {
    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "Report chat requires a DeepSeek API key. Configure it in stage 1 or start with --llm."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    let context = truncate_report(report_markdown);
    let request = ChatCompletionsRequest {
        model: llm_config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是 LitScout-RS 的报告问答助手。只能依据用户提供的 Markdown 调研报告回答。不要自行联网，不要编造来源，不要添加报告中不存在的 URL。回答使用中文，并尽量引用报告中已有链接。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!("报告内容：\n\n{context}\n\n用户问题：{question}"),
            },
        ],
        temperature: Some(0.2),
        max_tokens: Some(llm_config.max_tokens.min(2048) as u32),
        stream: Some(true),
        response_format: None,
    };
    client
        .chat_completions_stream_text(request, Some(delta_tx))
        .await
}

fn truncate_report(report_markdown: &str) -> String {
    if report_markdown.chars().count() <= MAX_REPORT_CONTEXT_CHARS {
        return report_markdown.to_string();
    }
    report_markdown
        .chars()
        .take(MAX_REPORT_CONTEXT_CHARS)
        .collect::<String>()
}
