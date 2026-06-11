use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use reqwest::header::{CONTENT_LENGTH, RETRY_AFTER, USER_AGENT};

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::reading::library;
use crate::reading::models::{
    PaperTextBundle, PaperTextMeta, ReadingLibraryItem, ReadingStatus, TextCoverage,
    TextFetchAttempt,
};

const DEFAULT_FULL_MIN_CHARS: usize = 8_000;
const DEFAULT_SHORT_MIN_CHARS: usize = 4_000;
const DEFAULT_MIN_QUALITY_SCORE: f32 = 0.72;
const DEFAULT_MAX_PDF_BYTES: usize = 50 * 1024 * 1024;
const DEFAULT_MAX_PAGES: usize = 100;
const DEFAULT_CACHE_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

pub type StatusFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
pub type StatusCallback<'a> = &'a mut (dyn FnMut(ReadingStatus) -> StatusFuture + Send);

#[derive(Debug, Clone)]
pub struct FullTextConfig {
    pub timeout_secs: u64,
    pub min_chars: usize,
    pub short_min_chars: usize,
    pub min_quality_score: f32,
    pub max_pdf_bytes: usize,
    pub max_pages: usize,
    pub cache_ttl_seconds: i64,
    pub jina_api_key: Option<String>,
}

impl FullTextConfig {
    pub fn from_env(timeout_secs: u64) -> Self {
        Self {
            timeout_secs,
            min_chars: env_usize("PDF_TEXT_MIN_CHARS", DEFAULT_FULL_MIN_CHARS),
            short_min_chars: env_usize("PDF_TEXT_SHORT_MIN_CHARS", DEFAULT_SHORT_MIN_CHARS),
            min_quality_score: env_f32("PDF_TEXT_MIN_QUALITY_SCORE", DEFAULT_MIN_QUALITY_SCORE),
            max_pdf_bytes: env_usize("PDF_MAX_BYTES", DEFAULT_MAX_PDF_BYTES),
            max_pages: env_usize("PDF_MAX_PAGES", DEFAULT_MAX_PAGES),
            cache_ttl_seconds: env_i64("PDF_TEXT_CACHE_TTL_SECONDS", DEFAULT_CACHE_TTL_SECONDS),
            jina_api_key: std::env::var("JINA_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArxivPaperUrls {
    #[cfg_attr(not(test), allow(dead_code))]
    pub arxiv_id: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub abs_url: String,
    pub pdf_url: String,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityReport {
    pub passed: bool,
    pub score: f32,
    pub char_count: usize,
    pub section_markers: usize,
    pub has_abstract: bool,
    pub has_body_marker: bool,
    pub references_ratio: f32,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
struct CandidateText {
    text: String,
    source_url: String,
    coverage: TextCoverage,
    extractor: String,
    page_count: Option<usize>,
}

pub async fn fetch_full_text(
    app_config: &AppConfig,
    item: &ReadingLibraryItem,
    callback: Option<StatusCallback<'_>>,
) -> Result<PaperTextBundle> {
    let config = FullTextConfig::from_env(app_config.timeout_secs);
    fetch_full_text_with_config(app_config, item, &config, callback).await
}

pub async fn fetch_full_text_with_config(
    app_config: &AppConfig,
    item: &ReadingLibraryItem,
    config: &FullTextConfig,
    mut callback: Option<StatusCallback<'_>>,
) -> Result<PaperTextBundle> {
    if let Some(bundle) = load_valid_cached_text(app_config, item, config).await? {
        return Ok(bundle);
    }

    let urls = canonicalize_arxiv_urls(item)?;
    let mut attempts = Vec::new();
    let client = build_client(config)?;

    notify_status(&mut callback, ReadingStatus::FetchingJinaHtml).await?;
    if let Some(candidate) = try_jina(
        &client,
        &urls.html_url,
        TextCoverage::FullTextHtml,
        "jina_html",
        config,
        &mut attempts,
    )
    .await?
    {
        if let Some(bundle) =
            accept_and_cache(app_config, item, candidate, &mut attempts, config).await?
        {
            return Ok(bundle);
        }
    }

    notify_status(&mut callback, ReadingStatus::FetchingJinaPdf).await?;
    if let Some(candidate) = try_jina(
        &client,
        &urls.pdf_url,
        TextCoverage::MarkdownProxy,
        "jina_pdf",
        config,
        &mut attempts,
    )
    .await?
    {
        if let Some(bundle) =
            accept_and_cache(app_config, item, candidate, &mut attempts, config).await?
        {
            return Ok(bundle);
        }
    }

    notify_status(&mut callback, ReadingStatus::DownloadingPdf).await?;
    let pdf_bytes = match get_or_download_pdf(
        &client,
        app_config,
        item,
        &urls.pdf_url,
        config,
        &mut attempts,
    )
    .await?
    {
        Some(bytes) => bytes,
        None => {
            return Ok(failed_bundle(
                item,
                urls.pdf_url,
                attempts,
                config,
                "pdf_download_failed",
            ))
        }
    };

    notify_status(&mut callback, ReadingStatus::ExtractingPdfText).await?;
    match extract_pdf_text(&pdf_bytes, config, &mut attempts).await? {
        Some((text, page_count)) => {
            let candidate = CandidateText {
                text,
                source_url: urls.pdf_url,
                coverage: TextCoverage::FullTextPdf,
                extractor: "pdf_extract".to_string(),
                page_count: Some(page_count),
            };
            if let Some(bundle) =
                accept_and_cache(app_config, item, candidate, &mut attempts, config).await?
            {
                Ok(bundle)
            } else {
                Ok(failed_bundle(
                    item,
                    item.pdf_url.clone().unwrap_or_else(|| item.abs_url.clone()),
                    attempts,
                    config,
                    "quality_gate_failed",
                ))
            }
        }
        None => Ok(failed_bundle(
            item,
            item.pdf_url.clone().unwrap_or_else(|| item.abs_url.clone()),
            attempts,
            config,
            "pdf_extract_failed",
        )),
    }
}

pub fn canonicalize_arxiv_urls(item: &ReadingLibraryItem) -> Result<ArxivPaperUrls> {
    let arxiv_id = library::extract_arxiv_id(&item.abs_url)
        .or_else(|| item.pdf_url.as_deref().and_then(library::extract_arxiv_id))
        .or_else(|| library::extract_arxiv_id(&item.source_item_id))
        .ok_or_else(|| AppError::Workflow("无法从阅读库论文推导 arXiv ID。".to_string()))?;
    Ok(ArxivPaperUrls {
        abs_url: format!("https://arxiv.org/abs/{arxiv_id}"),
        pdf_url: format!("https://arxiv.org/pdf/{arxiv_id}"),
        html_url: format!("https://arxiv.org/html/{arxiv_id}"),
        arxiv_id,
    })
}

pub fn jina_reader_url(target_url: &str) -> String {
    let trimmed = target_url.trim();
    if trimmed.starts_with("https://r.jina.ai/") {
        trimmed.to_string()
    } else {
        format!("https://r.jina.ai/{trimmed}")
    }
}

pub fn evaluate_quality(
    text: &str,
    page_count: Option<usize>,
    config: &FullTextConfig,
) -> QualityReport {
    let char_count = text.chars().count();
    let has_abstract = has_marker(text, r"(?im)^\s*(abstract|摘要)\b");
    let introduction = has_marker(text, r"(?im)^\s*(\d+\.?\s*)?(introduction|背景|引言)\b");
    let method = has_marker(
        text,
        r"(?im)^\s*(\d+\.?\s*)?(method|methods|methodology|approach|model|framework|方法)\b",
    );
    let experiment = has_marker(
        text,
        r"(?im)^\s*(\d+\.?\s*)?(experiment|experiments|evaluation|results|analysis|实验|评估)\b",
    );
    let conclusion = has_marker(
        text,
        r"(?im)^\s*(\d+\.?\s*)?(conclusion|discussion|limitations|结论)\b",
    );
    let references = has_marker(text, r"(?im)^\s*(references|bibliography|参考文献)\b");
    let section_markers = [
        has_abstract,
        introduction,
        method,
        experiment,
        conclusion,
        references,
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    let has_body_marker = introduction || method || experiment || conclusion;
    let references_ratio = references_tail_ratio(text);

    let full_pass = char_count >= config.min_chars
        && section_markers >= 3
        && has_abstract
        && has_body_marker
        && references_ratio < 0.75;
    let short_pass = char_count >= config.short_min_chars
        && page_count.is_some_and(|pages| pages <= 6)
        && has_abstract
        && section_markers >= 3
        && has_body_marker
        && references_ratio < 0.75;
    let score = quality_score(
        char_count,
        section_markers,
        has_abstract,
        has_body_marker,
        references_ratio,
    );
    let passed = (full_pass || short_pass) && score >= config.min_quality_score;
    let reason = if passed {
        None
    } else if char_count < 1_200 {
        Some("too_short".to_string())
    } else if !has_abstract {
        Some("missing_abstract".to_string())
    } else if !has_body_marker {
        Some("missing_body_section".to_string())
    } else if references_ratio >= 0.75 {
        Some("mostly_references".to_string())
    } else if score < config.min_quality_score {
        Some("low_quality_score".to_string())
    } else {
        Some("below_min_chars".to_string())
    };

    QualityReport {
        passed,
        score,
        char_count,
        section_markers,
        has_abstract,
        has_body_marker,
        references_ratio,
        reason,
    }
}

async fn load_valid_cached_text(
    app_config: &AppConfig,
    item: &ReadingLibraryItem,
    config: &FullTextConfig,
) -> Result<Option<PaperTextBundle>> {
    let Some((text, mut meta)) = library::load_text_artifact(app_config, &item.paper_key).await?
    else {
        return Ok(None);
    };
    if cache_expired(meta.generated_at, meta.cache_ttl_seconds) {
        return Ok(None);
    }
    let report = evaluate_quality(&text, meta.page_count, config);
    if !report.passed {
        return Ok(None);
    }
    meta.quality_score = report.score;
    meta.char_count = report.char_count;
    let source_url = meta.source_url.clone();
    let coverage = meta.coverage.clone();
    Ok(Some(PaperTextBundle {
        text,
        source_url,
        coverage,
        meta,
    }))
}

async fn try_jina(
    client: &reqwest::Client,
    target_url: &str,
    coverage: TextCoverage,
    kind: &str,
    config: &FullTextConfig,
    attempts: &mut Vec<TextFetchAttempt>,
) -> Result<Option<CandidateText>> {
    let url = jina_reader_url(target_url);
    let (status, text, retry_after_ms, elapsed_ms, error) =
        fetch_jina_once_or_retry(client, &url, config).await?;
    let char_count = text.as_ref().map(|value| value.chars().count());
    attempts.push(TextFetchAttempt {
        kind: kind.to_string(),
        status: if text.is_some() {
            "ok".to_string()
        } else {
            "failed".to_string()
        },
        source_url: Some(url.clone()),
        http_status: status,
        char_count,
        page_count: None,
        error,
        retry_after_ms,
        elapsed_ms: Some(elapsed_ms),
    });
    Ok(text.map(|text| CandidateText {
        text,
        source_url: url,
        coverage,
        extractor: kind.to_string(),
        page_count: None,
    }))
}

async fn fetch_jina_once_or_retry(
    client: &reqwest::Client,
    url: &str,
    config: &FullTextConfig,
) -> Result<(
    Option<u16>,
    Option<String>,
    Option<u64>,
    u128,
    Option<String>,
)> {
    let started = Instant::now();
    let response = send_jina_request(client, url, config).await;
    match response {
        Ok(response) if response.status().as_u16() == 429 => {
            let retry_after_ms = retry_after_ms(response.headers().get(RETRY_AFTER));
            if let Some(delay) = retry_after_ms {
                tokio::time::sleep(Duration::from_millis(delay.min(5_000))).await;
            }
            let retried = send_jina_request(client, url, config).await;
            match retried {
                Ok(response) => {
                    response_to_text(response, started, Some(retry_after_ms.unwrap_or(0))).await
                }
                Err(err) => Ok((
                    Some(429),
                    None,
                    retry_after_ms,
                    started.elapsed().as_millis(),
                    Some(err.to_string()),
                )),
            }
        }
        Ok(response) => response_to_text(response, started, None).await,
        Err(err) => Ok((
            None,
            None,
            None,
            started.elapsed().as_millis(),
            Some(err.to_string()),
        )),
    }
}

async fn send_jina_request(
    client: &reqwest::Client,
    url: &str,
    config: &FullTextConfig,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    let mut request = client.get(url).header(USER_AGENT, "LitScout-RS/0.1");
    if let Some(api_key) = config.jina_api_key.as_deref() {
        request = request.bearer_auth(api_key);
    }
    request.send().await
}

async fn response_to_text(
    response: reqwest::Response,
    started: Instant,
    retry_after_ms: Option<u64>,
) -> Result<(
    Option<u16>,
    Option<String>,
    Option<u64>,
    u128,
    Option<String>,
)> {
    let status = response.status();
    let status_code = Some(status.as_u16());
    if !status.is_success() {
        return Ok((
            status_code,
            None,
            retry_after_ms,
            started.elapsed().as_millis(),
            Some(format!("http_status_{status}")),
        ));
    }
    let text = response.text().await?.trim().to_string();
    if text.is_empty() {
        return Ok((
            status_code,
            None,
            retry_after_ms,
            started.elapsed().as_millis(),
            Some("empty_text".to_string()),
        ));
    }
    Ok((
        status_code,
        Some(text),
        retry_after_ms,
        started.elapsed().as_millis(),
        None,
    ))
}

async fn get_or_download_pdf(
    client: &reqwest::Client,
    app_config: &AppConfig,
    item: &ReadingLibraryItem,
    pdf_url: &str,
    config: &FullTextConfig,
    attempts: &mut Vec<TextFetchAttempt>,
) -> Result<Option<Vec<u8>>> {
    if let Some(bytes) = library::load_pdf_artifact(app_config, &item.paper_key).await? {
        attempts.push(TextFetchAttempt {
            kind: "pdf_cache".to_string(),
            status: "ok".to_string(),
            source_url: Some(pdf_url.to_string()),
            http_status: None,
            char_count: None,
            page_count: None,
            error: None,
            retry_after_ms: None,
            elapsed_ms: None,
        });
        return Ok(Some(bytes));
    }

    let started = Instant::now();
    let mut last_error = None;
    for attempt_index in 0..2 {
        if attempt_index > 0 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        match download_pdf_once(client, pdf_url, config).await {
            Ok(bytes) => {
                library::save_pdf_artifact(app_config, &item.paper_key, &bytes).await?;
                attempts.push(TextFetchAttempt {
                    kind: "download_pdf".to_string(),
                    status: "ok".to_string(),
                    source_url: Some(pdf_url.to_string()),
                    http_status: Some(200),
                    char_count: None,
                    page_count: None,
                    error: None,
                    retry_after_ms: None,
                    elapsed_ms: Some(started.elapsed().as_millis()),
                });
                return Ok(Some(bytes));
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }
    attempts.push(TextFetchAttempt {
        kind: "download_pdf".to_string(),
        status: "failed".to_string(),
        source_url: Some(pdf_url.to_string()),
        http_status: None,
        char_count: None,
        page_count: None,
        error: last_error,
        retry_after_ms: Some(300),
        elapsed_ms: Some(started.elapsed().as_millis()),
    });
    Ok(None)
}

async fn download_pdf_once(
    client: &reqwest::Client,
    pdf_url: &str,
    config: &FullTextConfig,
) -> Result<Vec<u8>> {
    let response = client
        .get(pdf_url)
        .header(USER_AGENT, "LitScout-RS/0.1")
        .send()
        .await?;
    if let Some(length) = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
    {
        if length > config.max_pdf_bytes {
            return Err(AppError::Workflow(format!(
                "PDF 文件过大：{} bytes，超过上限 {} bytes。",
                length, config.max_pdf_bytes
            )));
        }
    }
    let bytes = response.error_for_status()?.bytes().await?;
    if bytes.len() > config.max_pdf_bytes {
        return Err(AppError::Workflow(format!(
            "PDF 文件过大：{} bytes，超过上限 {} bytes。",
            bytes.len(),
            config.max_pdf_bytes
        )));
    }
    Ok(bytes.to_vec())
}

async fn extract_pdf_text(
    pdf_bytes: &[u8],
    config: &FullTextConfig,
    attempts: &mut Vec<TextFetchAttempt>,
) -> Result<Option<(String, usize)>> {
    let started = Instant::now();
    let bytes = pdf_bytes.to_vec();
    let extracted = tokio::task::spawn_blocking(move || {
        let pages = pdf_extract::extract_text_from_mem_by_pages(&bytes)
            .map_err(|err| AppError::Workflow(format!("PDF 文本提取失败：{err}")))?;
        let page_count = pages.len();
        let text = pages.join("\n\n").trim().to_string();
        Ok::<_, AppError>((text, page_count))
    })
    .await
    .map_err(|err| AppError::Workflow(format!("PDF 文本提取任务失败：{err}")))?;

    match extracted {
        Ok((text, page_count)) => {
            let char_count = text.chars().count();
            let status = if text.is_empty() {
                "empty_text"
            } else if page_count > config.max_pages {
                "too_many_pages"
            } else {
                "ok"
            };
            attempts.push(TextFetchAttempt {
                kind: "pdf_extract".to_string(),
                status: status.to_string(),
                source_url: None,
                http_status: None,
                char_count: Some(char_count),
                page_count: Some(page_count),
                error: if status == "ok" {
                    None
                } else {
                    Some(status.to_string())
                },
                retry_after_ms: None,
                elapsed_ms: Some(started.elapsed().as_millis()),
            });
            if status == "ok" {
                Ok(Some((text, page_count)))
            } else {
                Ok(None)
            }
        }
        Err(err) => {
            attempts.push(TextFetchAttempt {
                kind: "pdf_extract".to_string(),
                status: "failed".to_string(),
                source_url: None,
                http_status: None,
                char_count: None,
                page_count: None,
                error: Some(err.to_string()),
                retry_after_ms: None,
                elapsed_ms: Some(started.elapsed().as_millis()),
            });
            Ok(None)
        }
    }
}

async fn accept_and_cache(
    app_config: &AppConfig,
    item: &ReadingLibraryItem,
    candidate: CandidateText,
    attempts: &mut Vec<TextFetchAttempt>,
    config: &FullTextConfig,
) -> Result<Option<PaperTextBundle>> {
    let report = evaluate_quality(&candidate.text, candidate.page_count, config);
    if !report.passed {
        attempts.push(TextFetchAttempt {
            kind: "quality_gate".to_string(),
            status: "failed".to_string(),
            source_url: Some(candidate.source_url.clone()),
            http_status: None,
            char_count: Some(report.char_count),
            page_count: candidate.page_count,
            error: report.reason,
            retry_after_ms: None,
            elapsed_ms: None,
        });
        return Ok(None);
    }

    let meta = PaperTextMeta {
        coverage: candidate.coverage.clone(),
        source_url: candidate.source_url.clone(),
        extractor: candidate.extractor,
        char_count: report.char_count,
        page_count: candidate.page_count,
        quality_score: report.score,
        generated_at: Utc::now(),
        cache_ttl_seconds: config.cache_ttl_seconds,
        attempts: attempts.clone(),
    };
    library::save_text_artifact(app_config, &item.paper_key, &candidate.text, &meta).await?;
    Ok(Some(PaperTextBundle {
        text: candidate.text,
        source_url: candidate.source_url,
        coverage: candidate.coverage,
        meta,
    }))
}

fn failed_bundle(
    item: &ReadingLibraryItem,
    source_url: String,
    attempts: Vec<TextFetchAttempt>,
    config: &FullTextConfig,
    error: &str,
) -> PaperTextBundle {
    let meta = PaperTextMeta {
        coverage: TextCoverage::Failed,
        source_url: source_url.clone(),
        extractor: "full_text_fetcher".to_string(),
        char_count: 0,
        page_count: None,
        quality_score: 0.0,
        generated_at: Utc::now(),
        cache_ttl_seconds: config.cache_ttl_seconds,
        attempts,
    };
    PaperTextBundle {
        text: format!(
            "# {}\n\nSource: {}\n\nFull-text acquisition failed: {}",
            item.title, item.abs_url, error
        ),
        source_url,
        coverage: TextCoverage::Failed,
        meta,
    }
}

async fn notify_status(
    callback: &mut Option<StatusCallback<'_>>,
    status: ReadingStatus,
) -> Result<()> {
    if let Some(callback) = callback.as_deref_mut() {
        callback(status).await?;
    }
    Ok(())
}

fn build_client(config: &FullTextConfig) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .read_timeout(Duration::from_secs(config.timeout_secs.max(10)))
        .connect_timeout(Duration::from_secs(10))
        .build()?)
}

fn has_marker(text: &str, pattern: &str) -> bool {
    regex::Regex::new(pattern)
        .expect("quality regex should compile")
        .is_match(text)
}

fn quality_score(
    char_count: usize,
    section_markers: usize,
    has_abstract: bool,
    has_body_marker: bool,
    references_ratio: f32,
) -> f32 {
    let length_score = (char_count as f32 / DEFAULT_FULL_MIN_CHARS as f32).min(1.0) * 0.35;
    let section_score = (section_markers as f32 / 4.0).min(1.0) * 0.35;
    let abstract_score = if has_abstract { 0.15 } else { 0.0 };
    let body_score = if has_body_marker { 0.15 } else { 0.0 };
    (length_score + section_score + abstract_score + body_score - references_ratio.min(1.0) * 0.15)
        .clamp(0.0, 1.0)
}

fn references_tail_ratio(text: &str) -> f32 {
    let Some(matched) = regex::Regex::new(r"(?im)^\s*(references|bibliography|参考文献)\b")
        .expect("references regex should compile")
        .find(text)
    else {
        return 0.0;
    };
    let total = text.len().max(1) as f32;
    (text.len() - matched.start()) as f32 / total
}

fn cache_expired(generated_at: DateTime<Utc>, ttl_seconds: i64) -> bool {
    if ttl_seconds <= 0 {
        return true;
    }
    Utc::now().signed_duration_since(generated_at).num_seconds() > ttl_seconds
}

fn retry_after_ms(value: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let text = value?.to_str().ok()?.trim();
    if let Ok(seconds) = text.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    None
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, Utc};

    use super::{
        cache_expired, canonicalize_arxiv_urls, evaluate_quality, jina_reader_url, retry_after_ms,
        FullTextConfig,
    };
    use crate::reading::models::{ReadingLibraryItem, ReadingStatus};

    fn config() -> FullTextConfig {
        FullTextConfig {
            timeout_secs: 30,
            min_chars: 8_000,
            short_min_chars: 4_000,
            min_quality_score: 0.70,
            max_pdf_bytes: 50 * 1024 * 1024,
            max_pages: 100,
            cache_ttl_seconds: 30 * 24 * 60 * 60,
            jina_api_key: None,
        }
    }

    fn item() -> ReadingLibraryItem {
        ReadingLibraryItem {
            paper_key: "arxiv-1706.03762".to_string(),
            source_item_id: "arxiv:1706.03762".to_string(),
            evidence_id: "ev-1".to_string(),
            run_id: None,
            title: "Attention Is All You Need".to_string(),
            abs_url: "https://arxiv.org/abs/1706.03762v1".to_string(),
            pdf_url: Some("https://arxiv.org/pdf/1706.03762v1".to_string()),
            summary: "Transformer paper".to_string(),
            added_at: Utc::now(),
            updated_at: Utc::now(),
            status: ReadingStatus::Queued,
            text_coverage: None,
            text: None,
            text_source_url: None,
            text_meta: None,
            note_quality: None,
            note: None,
            chat_history: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn canonicalizes_arxiv_urls() {
        let urls = canonicalize_arxiv_urls(&item()).unwrap();
        assert_eq!(urls.arxiv_id, "1706.03762v1");
        assert_eq!(urls.abs_url, "https://arxiv.org/abs/1706.03762v1");
        assert_eq!(urls.pdf_url, "https://arxiv.org/pdf/1706.03762v1");
        assert_eq!(urls.html_url, "https://arxiv.org/html/1706.03762v1");
    }

    #[test]
    fn builds_jina_url_once() {
        assert_eq!(
            jina_reader_url("https://arxiv.org/html/1706.03762"),
            "https://r.jina.ai/https://arxiv.org/html/1706.03762"
        );
        assert_eq!(
            jina_reader_url("https://r.jina.ai/https://arxiv.org/pdf/1706.03762"),
            "https://r.jina.ai/https://arxiv.org/pdf/1706.03762"
        );
    }

    #[test]
    fn quality_gate_accepts_full_paper_like_text() {
        let text = format!(
            "Abstract\n{}\n\n1 Introduction\n{}\n\n2 Methodology\n{}\n\n3 Experiments\n{}\n\n4 Conclusion\n{}",
            "This paper studies transformers. ".repeat(80),
            "The introduction explains sequence transduction. ".repeat(80),
            "The method uses attention and feed-forward layers. ".repeat(80),
            "Experiments evaluate WMT 2014 and ablations. ".repeat(80),
            "The conclusion summarizes the findings. ".repeat(80),
        );
        let report = evaluate_quality(&text, Some(10), &config());
        assert!(report.passed, "{report:?}");
    }

    #[test]
    fn quality_gate_rejects_abstract_only_text() {
        let text = "Abstract\nThis is only a short abstract.";
        let report = evaluate_quality(text, Some(1), &config());
        assert!(!report.passed);
        assert_eq!(report.reason.as_deref(), Some("too_short"));
    }

    #[test]
    fn quality_gate_rejects_references_only_text() {
        let text = format!(
            "Abstract\n{}\n\nReferences\n{}",
            "Metadata summary. ".repeat(300),
            "[1] citation. ".repeat(600)
        );
        let report = evaluate_quality(&text, Some(12), &config());
        assert!(!report.passed);
    }

    #[test]
    fn retry_after_seconds_to_millis() {
        let value = reqwest::header::HeaderValue::from_static("2");
        assert_eq!(retry_after_ms(Some(&value)), Some(2_000));
    }

    #[test]
    fn cache_expiration_respects_ttl() {
        assert!(!cache_expired(Utc::now(), 60));
        assert!(cache_expired(Utc::now() - ChronoDuration::seconds(120), 60));
    }
}
