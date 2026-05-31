use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, LlmConfig};
use crate::error::Result;
use crate::model::ScoutReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub generated_at: DateTime<Utc>,
    pub output_report: String,
    pub config: SessionConfig,
    pub report: ScoutReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub cache_dir: String,
    pub use_cache: bool,
    pub cache_ttl_hours: u64,
    pub timeout_secs: u64,
    pub enrich: bool,
    pub tags_file: Option<String>,
    pub llm_enabled: bool,
    pub llm_base_url: Option<String>,
    pub llm_main_model: String,
    pub llm_side_model: Option<String>,
    pub llm_max_tokens: usize,
    pub llm_timeout_secs: u64,
}

pub async fn write_session(
    report: &ScoutReport,
    app_config: &AppConfig,
    llm_config: &LlmConfig,
    output_report: &Path,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(&app_config.session_dir).await?;
    let path = app_config.session_dir.join(format!(
        "{}-{}.json",
        slugify(&report.query.topic),
        report.generated_at.format("%Y%m%d-%H%M%S")
    ));
    let record = SessionRecord {
        generated_at: Utc::now(),
        output_report: output_report.display().to_string(),
        config: SessionConfig::from_configs(app_config, llm_config),
        report: report.clone(),
    };
    let body = serde_json::to_string_pretty(&record)?;
    tokio::fs::write(&path, body).await?;
    Ok(path)
}

impl SessionConfig {
    fn from_configs(app_config: &AppConfig, llm_config: &LlmConfig) -> Self {
        Self {
            cache_dir: app_config.cache_dir.display().to_string(),
            use_cache: app_config.use_cache,
            cache_ttl_hours: app_config.cache_ttl_hours,
            timeout_secs: app_config.timeout_secs,
            enrich: app_config.enrich,
            tags_file: app_config
                .tags_file
                .as_ref()
                .map(|path| path.display().to_string()),
            llm_enabled: llm_config.enabled,
            llm_base_url: llm_config.base_url.clone(),
            llm_main_model: llm_config.main_model.clone(),
            llm_side_model: llm_config.side_model.clone(),
            llm_max_tokens: llm_config.max_tokens,
            llm_timeout_secs: llm_config.timeout_secs,
        }
    }
}

fn slugify(topic: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in topic.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "session".to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{DateTime, Utc};

    use super::{write_session, SessionRecord};
    use crate::config::{AppConfig, LlmConfig};
    use crate::model::{CitationLedger, QualityReport, ScoutReport, SearchPlan, SearchQuery};

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn writes_session_without_api_key() {
        let session_dir = temp_dir("session-write");
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let report = ScoutReport {
            query: query.clone(),
            plan: SearchPlan::from_query(&query),
            generated_at: dt(),
            github_repos: vec![],
            arxiv_papers: vec![],
            ranked_items: vec![],
            groups: vec![],
            citations: CitationLedger::default(),
            llm_synthesis: None,
            quality: QualityReport::pass(),
        };
        let app_config = AppConfig {
            github_token: Some("github-secret".to_string()),
            output: session_dir.join("report.md"),
            cache_dir: session_dir.join("cache"),
            session_dir: session_dir.clone(),
            tags_file: None,
            use_cache: true,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        };
        let llm_config = LlmConfig {
            enabled: true,
            api_key: Some("deepseek-secret".to_string()),
            base_url: Some("https://api.deepseek.com".to_string()),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: Some("deepseek-v4-flash".to_string()),
            max_tokens: 4096,
            timeout_secs: 30,
        };

        let path = write_session(&report, &app_config, &llm_config, &app_config.output)
            .await
            .expect("session should write");
        let body = std::fs::read_to_string(path).expect("session should be readable");
        let record: SessionRecord = serde_json::from_str(&body).expect("session should parse");

        assert_eq!(record.report.query.topic, "rust agent");
        assert!(!body.contains("github-secret"));
        assert!(!body.contains("deepseek-secret"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "litscout-rs-{name}-{}-{unique}",
            std::process::id()
        ))
    }
}
