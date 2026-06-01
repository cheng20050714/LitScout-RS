use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::{AppConfig, LlmConfig};
use crate::dedup;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{DeepSeekClient, DeepSeekConfig, LlmSynthesisResult};
use crate::model::{
    ArxivPaper, CitationLedger, GitHubRepo, QualityReport, ScoutReport, SearchPlan, SearchQuery,
    SourceItem,
};
use crate::report;
use crate::session;
use crate::sources::{arxiv, github};
use crate::{cache, classify, quality, ranking};

const GITHUB_SOURCE: &str = "github";
const ARXIV_SOURCE: &str = "arxiv";

#[derive(Debug, Clone)]
pub struct WorkflowRunResult {
    pub output_path: PathBuf,
    pub session_path: Option<PathBuf>,
    pub report: ScoutReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum WorkflowEvent {
    FetchStarted {
        source: String,
    },
    SourceFinished {
        source: String,
        count: usize,
    },
    RankingFinished {
        total: usize,
    },
    ClassificationFinished {
        total: usize,
    },
    SynthesisStarted,
    QualityWarning {
        message: String,
    },
    ReportReady {
        output_report: String,
        session_path: Option<String>,
    },
}

pub async fn run(
    query: SearchQuery,
    app_config: AppConfig,
    llm_config: LlmConfig,
) -> Result<PathBuf> {
    Ok(run_for_report(query, app_config, llm_config)
        .await?
        .output_path)
}

pub async fn run_for_report(
    query: SearchQuery,
    app_config: AppConfig,
    llm_config: LlmConfig,
) -> Result<WorkflowRunResult> {
    let mut emit = |_| {};
    run_inner(query, None, app_config, llm_config, &mut emit).await
}

pub async fn run_with_plan_for_report(
    query: SearchQuery,
    plan: SearchPlan,
    app_config: AppConfig,
    llm_config: LlmConfig,
) -> Result<WorkflowRunResult> {
    let mut emit = |_| {};
    run_inner(query, Some(plan), app_config, llm_config, &mut emit).await
}

pub async fn run_with_plan_events<F>(
    query: SearchQuery,
    plan: SearchPlan,
    app_config: AppConfig,
    llm_config: LlmConfig,
    mut emit: F,
) -> Result<WorkflowRunResult>
where
    F: FnMut(WorkflowEvent),
{
    run_inner(query, Some(plan), app_config, llm_config, &mut emit).await
}

async fn run_inner<F>(
    query: SearchQuery,
    explicit_plan: Option<SearchPlan>,
    app_config: AppConfig,
    llm_config: LlmConfig,
    emit: &mut F,
) -> Result<WorkflowRunResult>
where
    F: FnMut(WorkflowEvent),
{
    ensure_llm_ready(&llm_config)?;

    if app_config.use_cache {
        tokio::fs::create_dir_all(&app_config.cache_dir).await?;
    }

    let mut warnings = Vec::new();
    let plan = match explicit_plan {
        Some(plan) => plan,
        None => match generate_search_plan(&query, &llm_config).await {
            Ok(plan) => plan,
            Err(err) => {
                let message = format!("DeepSeek SearchPlan failed: {err}; using original topic.");
                warn!("{message}");
                warnings.push(message);
                SearchPlan::from_query(&query)
            }
        },
    };

    emit(WorkflowEvent::FetchStarted {
        source: GITHUB_SOURCE.to_string(),
    });
    emit(WorkflowEvent::FetchStarted {
        source: ARXIV_SOURCE.to_string(),
    });
    let (github_result, arxiv_result) = tokio::join!(
        fetch_github_with_cache_for_plan(&plan, &query, &app_config),
        fetch_arxiv_with_cache_for_plan(&plan, &query, &app_config)
    );

    let (github_repos, github_ok) = match github_result {
        Ok(repos) => {
            emit(WorkflowEvent::SourceFinished {
                source: GITHUB_SOURCE.to_string(),
                count: repos.len(),
            });
            (repos, true)
        }
        Err(err) => {
            let message = format!("GitHub fetch failed: {err}");
            warn!("{message}");
            emit(WorkflowEvent::QualityWarning {
                message: message.clone(),
            });
            warnings.push(message);
            (Vec::new(), false)
        }
    };
    let (arxiv_papers, arxiv_ok) = match arxiv_result {
        Ok(papers) => {
            emit(WorkflowEvent::SourceFinished {
                source: ARXIV_SOURCE.to_string(),
                count: papers.len(),
            });
            (papers, true)
        }
        Err(err) => {
            let message = format!("arXiv fetch failed: {err}");
            warn!("{message}");
            emit(WorkflowEvent::QualityWarning {
                message: message.clone(),
            });
            warnings.push(message);
            (Vec::new(), false)
        }
    };

    if !github_ok && !arxiv_ok {
        return Err(AppError::Workflow(
            "both GitHub and arXiv failed, so no report could be generated".to_string(),
        ));
    }

    build_and_write_report(
        query,
        plan,
        app_config,
        llm_config,
        github_repos,
        arxiv_papers,
        warnings,
        emit,
    )
    .await
}

async fn fetch_github_with_cache_for_plan(
    plan: &SearchPlan,
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<GitHubRepo>> {
    let aspects = bounded_aspects(plan);
    let mut repos = Vec::new();
    let mut failures = Vec::new();

    for aspect in aspects {
        let github_limit = aspect.github_limit.max(1);
        let aspect_query = SearchQuery {
            topic: aspect.github_query.clone(),
            github_limit,
            arxiv_limit: query.arxiv_limit,
        };
        match fetch_github_with_cache(&aspect_query, config).await {
            Ok(mut items) => repos.append(&mut items),
            Err(err) => {
                let message = format!("GitHub aspect `{}` failed: {err}", aspect.name);
                warn!("{message}");
                failures.push(message);
            }
        }
    }

    repos.truncate(query.github_limit);
    if repos.is_empty() && !failures.is_empty() {
        return Err(AppError::Workflow(failures.join("; ")));
    }
    Ok(repos)
}

async fn fetch_arxiv_with_cache_for_plan(
    plan: &SearchPlan,
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<ArxivPaper>> {
    let aspects = bounded_aspects(plan);
    let mut papers = Vec::new();
    let mut failures = Vec::new();

    for aspect in aspects {
        let arxiv_limit = aspect.arxiv_limit.max(1);
        let aspect_query = SearchQuery {
            topic: aspect.arxiv_query.clone(),
            github_limit: query.github_limit,
            arxiv_limit,
        };
        match fetch_arxiv_with_cache(&aspect_query, config).await {
            Ok(mut items) => papers.append(&mut items),
            Err(err) => {
                let message = format!("arXiv aspect `{}` failed: {err}", aspect.name);
                warn!("{message}");
                failures.push(message);
            }
        }
    }

    papers.truncate(query.arxiv_limit);
    if papers.is_empty() && !failures.is_empty() {
        return Err(AppError::Workflow(failures.join("; ")));
    }
    Ok(papers)
}

fn bounded_aspects(plan: &SearchPlan) -> Vec<&crate::model::SearchAspect> {
    plan.aspects.iter().take(3).collect()
}

async fn fetch_github_with_cache(
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<GitHubRepo>> {
    if let Some(repos) =
        cache::load_source_cache(config, query, GITHUB_SOURCE, query.github_limit).await?
    {
        return Ok(repos);
    }

    let repos = github::search_repositories(query, config).await?;
    if let Err(err) =
        cache::save_source_cache(config, query, GITHUB_SOURCE, query.github_limit, &repos).await
    {
        warn!("Failed to write GitHub cache: {err}");
    }
    Ok(repos)
}

async fn fetch_arxiv_with_cache(
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<ArxivPaper>> {
    if let Some(papers) =
        cache::load_source_cache(config, query, ARXIV_SOURCE, query.arxiv_limit).await?
    {
        return Ok(papers);
    }

    let papers = arxiv::search_papers(query, config).await?;
    if let Err(err) =
        cache::save_source_cache(config, query, ARXIV_SOURCE, query.arxiv_limit, &papers).await
    {
        warn!("Failed to write arXiv cache: {err}");
    }
    Ok(papers)
}

async fn build_and_write_report(
    query: SearchQuery,
    plan: SearchPlan,
    app_config: AppConfig,
    llm_config: LlmConfig,
    github_repos: Vec<GitHubRepo>,
    arxiv_papers: Vec<ArxivPaper>,
    mut warnings: Vec<String>,
    emit: &mut impl FnMut(WorkflowEvent),
) -> Result<WorkflowRunResult> {
    ensure_llm_ready(&llm_config)?;

    let mut source_items = github_repos
        .iter()
        .map(SourceItem::from)
        .collect::<Vec<SourceItem>>();
    source_items.extend(arxiv_papers.iter().map(SourceItem::from));

    let rules = classify::load_rules(app_config.tags_file.as_deref())?;
    let deduped_items = dedup::dedup_by_id(source_items);
    let mut ranked_items = ranking::rank_items(&query, deduped_items);
    emit(WorkflowEvent::RankingFinished {
        total: ranked_items.len(),
    });
    classify::classify_items_with_rules(&mut ranked_items, &rules);
    emit(WorkflowEvent::ClassificationFinished {
        total: ranked_items.len(),
    });
    let groups = classify::group_by_tags(&ranked_items, &rules);
    let citations = CitationLedger::from_items(&ranked_items);

    let mut report = ScoutReport {
        query: query.clone(),
        plan,
        generated_at: Utc::now(),
        github_repos,
        arxiv_papers,
        ranked_items,
        groups,
        citations,
        llm_synthesis: None,
        quality: QualityReport::pass(),
    };

    let mut llm_repaired = false;
    if llm_config.enabled {
        emit(WorkflowEvent::SynthesisStarted);
        match synthesize_with_deepseek(&llm_config, &report).await {
            Ok(result) => {
                llm_repaired = result.repaired;
                report.llm_synthesis = Some(result.synthesis);
            }
            Err(err) => {
                let message = format!("DeepSeek synthesis failed: {err}; using rule-based report.");
                warn!("{message}");
                emit(WorkflowEvent::QualityWarning {
                    message: message.clone(),
                });
                warnings.push(message);
            }
        }
    }

    report.quality = quality::evaluate(&report, llm_config.enabled);
    report.quality.llm_repaired = llm_repaired;
    if app_config.github_token.is_none() {
        warnings.push(
            "No GitHub token provided; unauthenticated GitHub API mode was used.".to_string(),
        );
    }
    if !warnings.is_empty() {
        report.quality.passed = false;
        report.quality.warnings.extend(warnings);
    }
    for warning in &report.quality.warnings {
        emit(WorkflowEvent::QualityWarning {
            message: warning.clone(),
        });
    }

    let output_path = report::write_markdown(&report, &app_config.output).await?;
    let session_path =
        match session::write_session(&report, &app_config, &llm_config, &output_path).await {
            Ok(path) => Some(path),
            Err(err) => {
                warn!("Failed to write session JSON: {err}");
                None
            }
        };
    emit(WorkflowEvent::ReportReady {
        output_report: output_path.display().to_string(),
        session_path: session_path.as_ref().map(|path| path.display().to_string()),
    });
    print_run_summary(&report, &output_path, session_path.as_ref());
    Ok(WorkflowRunResult {
        output_path,
        session_path,
        report,
    })
}

fn ensure_llm_ready(llm_config: &LlmConfig) -> Result<()> {
    if llm_config.enabled && llm_config.api_key.is_none() {
        return Err(AppError::InvalidConfig(
            "`--llm` requires a DeepSeek API key. Set DEEPSEEK_API_KEY or pass --deepseek-api-key."
                .to_string(),
        ));
    }
    Ok(())
}

async fn synthesize_with_deepseek(
    llm_config: &LlmConfig,
    report: &ScoutReport,
) -> Result<LlmSynthesisResult> {
    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "`--llm` requires a DeepSeek API key. Set DEEPSEEK_API_KEY or pass --deepseek-api-key."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    client.synthesize_report(report).await
}

async fn generate_search_plan(query: &SearchQuery, llm_config: &LlmConfig) -> Result<SearchPlan> {
    if !llm_config.enabled {
        return Ok(SearchPlan::from_query(query));
    }
    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "`--llm` requires a DeepSeek API key. Set DEEPSEEK_API_KEY or pass --deepseek-api-key."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    client.generate_search_plan(query).await
}

fn print_run_summary(report: &ScoutReport, output_path: &PathBuf, session_path: Option<&PathBuf>) {
    println!("Query: {}", report.query.topic);
    println!("GitHub repositories: {}", report.github_repos.len());
    println!("arXiv papers: {}", report.arxiv_papers.len());
    println!("Deduplicated items: {}", report.ranked_items.len());
    println!("Output report: {}", output_path.display());
    if let Some(path) = session_path {
        println!("Session JSON: {}", path.display());
    }
    for warning in &report.quality.warnings {
        eprintln!("Warning: {warning}");
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{DateTime, Utc};

    use super::build_and_write_report;
    use crate::config::{AppConfig, LlmConfig};
    use crate::model::{ArxivPaper, GitHubRepo, SearchQuery};

    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn workflow_builds_report_from_mock_data_without_network() {
        let output = temp_output("workflow-mock");
        let query = SearchQuery {
            topic: "rust agent framework".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let plan = crate::model::SearchPlan::from_query(&query);
        let config = AppConfig {
            github_token: Some("token".to_string()),
            output: output.clone(),
            cache_dir: output.with_extension("cache"),
            session_dir: output.with_extension("sessions"),
            tags_file: None,
            use_cache: false,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        };
        let llm_config = LlmConfig::from_env(false, 30);

        let mut events = Vec::new();
        let result = build_and_write_report(
            query,
            plan,
            config,
            llm_config,
            vec![sample_repo()],
            vec![sample_paper()],
            Vec::new(),
            &mut |event| events.push(event),
        )
        .await
        .expect("mock workflow should write a report");

        let markdown =
            std::fs::read_to_string(result.output_path).expect("report should be readable");
        assert!(markdown.contains("# LitScout-RS 调研报告：rust agent framework"));
        assert!(markdown.contains("https://github.com/acme/rust-agent"));
        assert!(markdown.contains("https://arxiv.org/abs/2501.00001"));
        assert!(markdown.contains("## 9. 引用账本"));
        assert!(events
            .iter()
            .any(|event| matches!(event, super::WorkflowEvent::ReportReady { .. })));
    }

    #[tokio::test]
    async fn llm_enabled_without_key_returns_clear_error() {
        let output = temp_output("workflow-missing-llm-key");
        let query = SearchQuery {
            topic: "rust agent framework".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let plan = crate::model::SearchPlan::from_query(&query);
        let config = AppConfig {
            github_token: Some("token".to_string()),
            output: output.clone(),
            cache_dir: output.with_extension("cache"),
            session_dir: output.with_extension("sessions"),
            tags_file: None,
            use_cache: false,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        };
        let llm_config = LlmConfig {
            enabled: true,
            api_key: None,
            base_url: Some("https://api.deepseek.com".to_string()),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: Some("deepseek-v4-flash".to_string()),
            max_tokens: 4096,
            timeout_secs: 30,
        };

        let mut emit = |_| {};
        let err = build_and_write_report(
            query,
            plan,
            config,
            llm_config,
            vec![sample_repo()],
            vec![sample_paper()],
            Vec::new(),
            &mut emit,
        )
        .await
        .expect_err("missing DeepSeek key should fail clearly");

        assert!(err.to_string().contains("DEEPSEEK_API_KEY"));
    }

    fn temp_output(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "litscout-rs-{name}-{}-{unique}",
                std::process::id()
            ))
            .join("report.md")
    }

    fn sample_repo() -> GitHubRepo {
        GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework with tool calling".to_string()),
            stars: 100,
            forks: 10,
            language: Some("Rust".to_string()),
            updated_at: dt("2026-05-20T00:00:00Z"),
            topics: vec!["rust".to_string(), "agent".to_string()],
            readme_excerpt: None,
        }
    }

    fn sample_paper() -> ArxivPaper {
        ArxivPaper {
            arxiv_id: "2501.00001v1".to_string(),
            title: "Rust Agent Frameworks for Tool Calling".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A study of Rust agent frameworks and tool calling.".to_string(),
            published_at: dt("2026-05-01T00:00:00Z"),
            updated_at: None,
            categories: vec!["cs.AI".to_string(), "cs.SE".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001".to_string(),
            pdf_url: None,
        }
    }
}
