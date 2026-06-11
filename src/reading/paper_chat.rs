use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{ChatCompletionsRequest, ChatMessage, DeepSeekConfig};
use crate::reading::models::ReadingLibraryItem;

const MAX_PAPER_CHAT_CONTEXT_CHARS: usize = 28_000;

pub fn build_paper_chat_request(
    item: &ReadingLibraryItem,
    question: &str,
    llm_config: &LlmConfig,
) -> Result<ChatCompletionsRequest> {
    let _config = DeepSeekConfig::from_llm_config(llm_config)
        .ok_or_else(|| AppError::InvalidConfig("论文追问需要 DeepSeek API Key。".to_string()))?;
    let note = item
        .note
        .as_ref()
        .map(|note| note.markdown.as_str())
        .unwrap_or("尚未生成阅读笔记。");
    let text = item.text.as_deref().unwrap_or(&item.summary);
    let context = truncate_chars(
        &format!(
            "论文标题：{}\n论文链接：{}\n\n阅读笔记：\n{}\n\n论文文本：\n{}",
            item.title, item.abs_url, note, text
        ),
        MAX_PAPER_CHAT_CONTEXT_CHARS,
    );
    Ok(ChatCompletionsRequest {
        model: llm_config.main_model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是 LitScout-RS 的论文阅读问答助手。只能依据提供的单篇论文文本、元数据和阅读笔记回答。不要自行联网，不要新增论文外 URL。如果论文中没有足够信息，直接说明不足。回答使用中文。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!("{context}\n\n用户问题：{question}"),
            },
        ],
        temperature: Some(0.2),
        max_tokens: Some(llm_config.max_tokens.min(2048) as u32),
        stream: Some(true),
        response_format: None,
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}
