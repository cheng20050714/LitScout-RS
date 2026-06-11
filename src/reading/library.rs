use std::path::PathBuf;

use chrono::Utc;
use regex::Regex;
use tokio::fs;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{EvidenceItem, SourceKind};
use crate::reading::models::{
    PaperTextMeta, ReadingLibraryItem, ReadingLibrarySummary, ReadingStatus,
};

const LIBRARY_DIR: &str = "reading-library";
const PAPERS_DIR: &str = "papers";
const TEXTS_DIR: &str = "texts";
const PDFS_DIR: &str = "pdfs";

pub async fn list_items(app_config: &AppConfig) -> Result<Vec<ReadingLibrarySummary>> {
    let dir = papers_dir(app_config);
    let mut items = Vec::new();
    if !dir.exists() {
        return Ok(items);
    }

    let mut entries = fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let body = fs::read_to_string(path).await?;
        let item: ReadingLibraryItem = serde_json::from_str(&body)?;
        items.push(ReadingLibrarySummary::from(&item));
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.updated_at));
    Ok(items)
}

pub async fn add_item(
    app_config: &AppConfig,
    run_id: Option<String>,
    evidence: EvidenceItem,
) -> Result<ReadingLibraryItem> {
    if !matches!(evidence.source_kind, SourceKind::Arxiv) {
        return Err(AppError::Workflow(
            "只有 arXiv 论文证据可以加入阅读库。".to_string(),
        ));
    }
    let paper_key = paper_key_from_evidence(&evidence)?;
    if let Ok(existing) = get_item(app_config, &paper_key).await {
        return Ok(existing);
    }

    let now = Utc::now();
    let abs_url = normalize_abs_url(&evidence.url, &evidence.source_item_id)?;
    let item = ReadingLibraryItem {
        paper_key,
        source_item_id: evidence.source_item_id,
        evidence_id: evidence.evidence_id,
        run_id,
        title: evidence.title,
        pdf_url: pdf_url_from_abs_url(&abs_url),
        abs_url,
        summary: first_non_empty(&[&evidence.evidence_snippet, &evidence.evidence_note_zh]),
        added_at: now,
        updated_at: now,
        status: ReadingStatus::Queued,
        text_coverage: None,
        text: None,
        text_source_url: None,
        text_meta: None,
        note_quality: None,
        note: None,
        chat_history: Vec::new(),
        error: None,
    };
    save_item(app_config, &item).await?;
    Ok(item)
}

pub async fn get_item(app_config: &AppConfig, paper_key: &str) -> Result<ReadingLibraryItem> {
    let body = fs::read_to_string(item_path(app_config, paper_key)).await?;
    Ok(serde_json::from_str(&body)?)
}

pub async fn save_item(app_config: &AppConfig, item: &ReadingLibraryItem) -> Result<()> {
    fs::create_dir_all(papers_dir(app_config)).await?;
    let body = serde_json::to_string_pretty(item)?;
    fs::write(item_path(app_config, &item.paper_key), body).await?;
    Ok(())
}

pub async fn delete_item(app_config: &AppConfig, paper_key: &str) -> Result<()> {
    let path = item_path(app_config, paper_key);
    if path.exists() {
        fs::remove_file(path).await?;
    }
    let text_path = text_path(app_config, paper_key);
    if text_path.exists() {
        fs::remove_file(text_path).await?;
    }
    let meta_path = text_meta_path(app_config, paper_key);
    if meta_path.exists() {
        fs::remove_file(meta_path).await?;
    }
    let pdf_path = pdf_path(app_config, paper_key);
    if pdf_path.exists() {
        fs::remove_file(pdf_path).await?;
    }
    Ok(())
}

pub fn papers_dir(app_config: &AppConfig) -> PathBuf {
    app_config.session_dir.join(LIBRARY_DIR).join(PAPERS_DIR)
}

pub fn texts_dir(app_config: &AppConfig) -> PathBuf {
    app_config.session_dir.join(LIBRARY_DIR).join(TEXTS_DIR)
}

pub fn pdfs_dir(app_config: &AppConfig) -> PathBuf {
    app_config.session_dir.join(LIBRARY_DIR).join(PDFS_DIR)
}

pub fn text_path(app_config: &AppConfig, paper_key: &str) -> PathBuf {
    texts_dir(app_config).join(format!("{}.txt", safe_file_name(paper_key)))
}

pub fn text_meta_path(app_config: &AppConfig, paper_key: &str) -> PathBuf {
    texts_dir(app_config).join(format!("{}.meta.json", safe_file_name(paper_key)))
}

pub fn pdf_path(app_config: &AppConfig, paper_key: &str) -> PathBuf {
    pdfs_dir(app_config).join(format!("{}.pdf", safe_file_name(paper_key)))
}

pub async fn load_text_artifact(
    app_config: &AppConfig,
    paper_key: &str,
) -> Result<Option<(String, PaperTextMeta)>> {
    let text_path = text_path(app_config, paper_key);
    let meta_path = text_meta_path(app_config, paper_key);
    if !text_path.exists() || !meta_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(text_path).await?;
    let meta_body = fs::read_to_string(meta_path).await?;
    let meta = serde_json::from_str(&meta_body)?;
    Ok(Some((text, meta)))
}

pub async fn save_text_artifact(
    app_config: &AppConfig,
    paper_key: &str,
    text: &str,
    meta: &PaperTextMeta,
) -> Result<()> {
    fs::create_dir_all(texts_dir(app_config)).await?;
    fs::write(text_path(app_config, paper_key), text).await?;
    let body = serde_json::to_string_pretty(meta)?;
    fs::write(text_meta_path(app_config, paper_key), body).await?;
    Ok(())
}

pub async fn load_pdf_artifact(app_config: &AppConfig, paper_key: &str) -> Result<Option<Vec<u8>>> {
    let path = pdf_path(app_config, paper_key);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read(path).await?))
}

pub async fn save_pdf_artifact(
    app_config: &AppConfig,
    paper_key: &str,
    bytes: &[u8],
) -> Result<()> {
    fs::create_dir_all(pdfs_dir(app_config)).await?;
    fs::write(pdf_path(app_config, paper_key), bytes).await?;
    Ok(())
}

fn item_path(app_config: &AppConfig, paper_key: &str) -> PathBuf {
    papers_dir(app_config).join(format!("{}.json", safe_file_name(paper_key)))
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub fn paper_key_from_evidence(evidence: &EvidenceItem) -> Result<String> {
    let arxiv_id = extract_arxiv_id(&evidence.url)
        .or_else(|| extract_arxiv_id(&evidence.source_item_id))
        .ok_or_else(|| {
            AppError::Workflow(format!(
                "无法从证据 `{}` 推导 arXiv ID。",
                evidence.evidence_id
            ))
        })?;
    Ok(format!("arxiv-{}", arxiv_id))
}

pub fn normalize_abs_url(url: &str, fallback: &str) -> Result<String> {
    let arxiv_id = extract_arxiv_id(url).or_else(|| extract_arxiv_id(fallback));
    if let Some(arxiv_id) = arxiv_id {
        return Ok(format!("https://arxiv.org/abs/{arxiv_id}"));
    }
    Err(AppError::Workflow(
        "无法构造 arXiv abstract URL。".to_string(),
    ))
}

pub fn pdf_url_from_abs_url(abs_url: &str) -> Option<String> {
    extract_arxiv_id(abs_url).map(|id| format!("https://arxiv.org/pdf/{id}"))
}

pub fn extract_arxiv_id(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text
        .trim_start_matches("arxiv:")
        .replace("/pdf/", "/abs/")
        .trim_end_matches(".pdf")
        .to_string();
    let re = Regex::new(r"(?i)(\d{4}\.\d{4,5}(?:v\d+)?)").expect("regex should compile");
    re.captures(&normalized)
        .and_then(|caps| caps.get(1))
        .map(|matched| matched.as_str().to_string())
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

#[allow(dead_code)]
fn _library_dir(app_config: &AppConfig) -> PathBuf {
    app_config.session_dir.join(LIBRARY_DIR)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        extract_arxiv_id, load_pdf_artifact, load_text_artifact, pdf_path, pdf_url_from_abs_url,
        save_pdf_artifact, save_text_artifact, text_meta_path, text_path,
    };
    use crate::config::AppConfig;
    use crate::reading::models::{PaperTextMeta, TextCoverage};

    fn test_config(session_dir: std::path::PathBuf) -> AppConfig {
        AppConfig {
            github_token: None,
            output: session_dir.join("reports"),
            cache_dir: session_dir.join("cache"),
            session_dir,
            tags_file: None,
            use_cache: true,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        }
    }

    #[test]
    fn extracts_arxiv_id_from_common_forms() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/abs/1706.03762v1").as_deref(),
            Some("1706.03762v1")
        );
        assert_eq!(
            extract_arxiv_id("arxiv:2401.12345").as_deref(),
            Some("2401.12345")
        );
    }

    #[test]
    fn builds_pdf_url_from_abs_url() {
        assert_eq!(
            pdf_url_from_abs_url("https://arxiv.org/abs/1706.03762").as_deref(),
            Some("https://arxiv.org/pdf/1706.03762")
        );
    }

    #[test]
    fn artifact_paths_are_separate_from_papers() {
        let config = test_config(std::path::PathBuf::from("sessions"));
        assert!(text_path(&config, "arxiv:1706.03762")
            .ends_with("reading-library/texts/arxiv-1706.03762.txt"));
        assert!(text_meta_path(&config, "arxiv:1706.03762")
            .ends_with("reading-library/texts/arxiv-1706.03762.meta.json"));
        assert!(pdf_path(&config, "arxiv:1706.03762")
            .ends_with("reading-library/pdfs/arxiv-1706.03762.pdf"));
    }

    #[tokio::test]
    async fn text_and_pdf_artifacts_round_trip() {
        let temp =
            std::env::temp_dir().join(format!("litscout-artifacts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let config = test_config(temp.clone());
        let meta = PaperTextMeta {
            coverage: TextCoverage::FullTextPdf,
            source_url: "https://arxiv.org/pdf/1706.03762".to_string(),
            extractor: "test".to_string(),
            char_count: 42,
            page_count: Some(2),
            quality_score: 0.9,
            generated_at: Utc::now(),
            cache_ttl_seconds: 30 * 24 * 60 * 60,
            attempts: Vec::new(),
        };

        save_text_artifact(&config, "arxiv-1706.03762", "paper text", &meta)
            .await
            .unwrap();
        let (text, loaded_meta) = load_text_artifact(&config, "arxiv-1706.03762")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(text, "paper text");
        assert_eq!(loaded_meta.coverage, TextCoverage::FullTextPdf);

        save_pdf_artifact(&config, "arxiv-1706.03762", b"%PDF")
            .await
            .unwrap();
        assert_eq!(
            load_pdf_artifact(&config, "arxiv-1706.03762")
                .await
                .unwrap()
                .unwrap(),
            b"%PDF"
        );
        std::fs::remove_dir_all(temp).unwrap();
    }
}
