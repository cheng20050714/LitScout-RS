use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
    Queued,
    FetchingText,
    FetchingJinaHtml,
    FetchingJinaPdf,
    DownloadingPdf,
    ExtractingPdfText,
    TextReady,
    GeneratingNote,
    Ready,
    TextFailed,
    NoteFailed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextCoverage {
    FullTextHtml,
    FullTextPdf,
    MarkdownProxy,
    PartialText,
    AbstractOnly,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteQuality {
    FullText,
    AbstractOnly,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextFetchAttempt {
    pub kind: String,
    pub status: String,
    pub source_url: Option<String>,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub char_count: Option<usize>,
    #[serde(default)]
    pub page_count: Option<usize>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub retry_after_ms: Option<u64>,
    #[serde(default)]
    pub elapsed_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperTextMeta {
    pub coverage: TextCoverage,
    pub source_url: String,
    pub extractor: String,
    pub char_count: usize,
    #[serde(default)]
    pub page_count: Option<usize>,
    pub quality_score: f32,
    pub generated_at: DateTime<Utc>,
    pub cache_ttl_seconds: i64,
    #[serde(default)]
    pub attempts: Vec<TextFetchAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperTextBundle {
    pub text: String,
    pub source_url: String,
    pub coverage: TextCoverage,
    pub meta: PaperTextMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaperNote {
    pub tldr: String,
    pub motivation: String,
    pub method: String,
    pub result: String,
    pub conclusion: String,
    pub core_problem: String,
    pub contributions: Vec<String>,
    pub method_map: Vec<String>,
    pub experiment_matrix: Vec<String>,
    pub limitations: Vec<String>,
    pub reproducibility_notes: Vec<String>,
    pub relation_to_research_topic: String,
    pub recommended_questions: Vec<String>,
    pub markdown: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaperChatMessage {
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingLibraryItem {
    pub paper_key: String,
    pub source_item_id: String,
    pub evidence_id: String,
    pub run_id: Option<String>,
    pub title: String,
    pub abs_url: String,
    pub pdf_url: Option<String>,
    pub summary: String,
    pub added_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: ReadingStatus,
    pub text_coverage: Option<TextCoverage>,
    pub text: Option<String>,
    pub text_source_url: Option<String>,
    #[serde(default)]
    pub text_meta: Option<PaperTextMeta>,
    #[serde(default)]
    pub note_quality: Option<NoteQuality>,
    pub note: Option<PaperNote>,
    #[serde(default)]
    pub chat_history: Vec<PaperChatMessage>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingLibrarySummary {
    pub paper_key: String,
    pub source_item_id: String,
    pub evidence_id: String,
    pub run_id: Option<String>,
    pub title: String,
    pub abs_url: String,
    pub pdf_url: Option<String>,
    pub summary: String,
    pub added_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: ReadingStatus,
    pub text_coverage: Option<TextCoverage>,
    #[serde(default)]
    pub text_meta: Option<PaperTextMeta>,
    #[serde(default)]
    pub note_quality: Option<NoteQuality>,
    pub has_note: bool,
    pub error: Option<String>,
}

impl From<&ReadingLibraryItem> for ReadingLibrarySummary {
    fn from(item: &ReadingLibraryItem) -> Self {
        Self {
            paper_key: item.paper_key.clone(),
            source_item_id: item.source_item_id.clone(),
            evidence_id: item.evidence_id.clone(),
            run_id: item.run_id.clone(),
            title: item.title.clone(),
            abs_url: item.abs_url.clone(),
            pdf_url: item.pdf_url.clone(),
            summary: item.summary.clone(),
            added_at: item.added_at,
            updated_at: item.updated_at,
            status: item.status.clone(),
            text_coverage: item.text_coverage.clone(),
            text_meta: item.text_meta.clone(),
            note_quality: item.note_quality.clone(),
            has_note: item.note.is_some(),
            error: item.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadingLibraryItem, ReadingStatus, TextCoverage};

    #[test]
    fn legacy_reading_item_without_new_fields_still_loads() {
        let body = r#"{
          "paper_key": "arxiv-1706.03762",
          "source_item_id": "arxiv:1706.03762",
          "evidence_id": "ev-1",
          "run_id": null,
          "title": "Attention Is All You Need",
          "abs_url": "https://arxiv.org/abs/1706.03762",
          "pdf_url": "https://arxiv.org/pdf/1706.03762",
          "summary": "Transformer paper.",
          "added_at": "2026-06-11T00:00:00Z",
          "updated_at": "2026-06-11T00:00:00Z",
          "status": "failed",
          "text_coverage": "abstract_only",
          "text": null,
          "text_source_url": null,
          "note": null,
          "chat_history": [],
          "error": "old failure"
        }"#;

        let item: ReadingLibraryItem = serde_json::from_str(body).unwrap();
        assert_eq!(item.status, ReadingStatus::Failed);
        assert_eq!(item.text_coverage, Some(TextCoverage::AbstractOnly));
        assert!(item.text_meta.is_none());
        assert!(item.note_quality.is_none());
    }
}
