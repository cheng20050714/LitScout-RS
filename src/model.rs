use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchQuery {
    pub topic: String,
    pub github_limit: usize,
    pub arxiv_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub stars: u64,
    pub forks: u64,
    pub language: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub topics: Vec<String>,
    pub readme_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArxivPaper {
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub summary: String,
    pub published_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub categories: Vec<String>,
    pub abs_url: String,
    pub pdf_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[serde(rename = "github", alias = "git_hub", alias = "GitHub")]
    GitHub,
    #[serde(rename = "arxiv", alias = "Arxiv")]
    Arxiv,
    AcademicIndex,
    Bibliography,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceItem {
    pub id: String,
    pub kind: SourceKind,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub evidence_snippet: String,
    pub tags: Vec<String>,
    pub score: f64,
    pub score_reasons: Vec<String>,
    pub classification_reasons: Vec<String>,
    pub score_breakdown: ScoreBreakdown,
    pub published_or_updated_at: Option<DateTime<Utc>>,
    pub metadata: SourceMetadata,
}

impl From<&GitHubRepo> for SourceItem {
    fn from(repo: &GitHubRepo) -> Self {
        let summary = repo.description.clone().unwrap_or_default();
        let evidence_source = repo.readme_excerpt.as_deref().unwrap_or(&summary);
        Self {
            id: format!("github:{}", repo.full_name),
            kind: SourceKind::GitHub,
            title: repo.full_name.clone(),
            url: repo.html_url.clone(),
            evidence_snippet: excerpt(evidence_source, 2000),
            summary,
            tags: repo.topics.clone(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown::default(),
            published_or_updated_at: Some(repo.updated_at),
            metadata: SourceMetadata::GitHub {
                stars: repo.stars,
                forks: repo.forks,
                language: repo.language.clone(),
                topics: repo.topics.clone(),
            },
        }
    }
}

impl From<GitHubRepo> for SourceItem {
    fn from(repo: GitHubRepo) -> Self {
        SourceItem::from(&repo)
    }
}

impl From<&ArxivPaper> for SourceItem {
    fn from(paper: &ArxivPaper) -> Self {
        let stable_id = stable_arxiv_id(&paper.arxiv_id);
        Self {
            id: format!("arxiv:{stable_id}"),
            kind: SourceKind::Arxiv,
            title: paper.title.clone(),
            url: paper.abs_url.clone(),
            summary: paper.summary.clone(),
            evidence_snippet: excerpt(&paper.summary, 280),
            tags: paper.categories.clone(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown::default(),
            published_or_updated_at: Some(paper.published_at),
            metadata: SourceMetadata::Arxiv {
                authors: paper.authors.clone(),
                categories: paper.categories.clone(),
            },
        }
    }
}

impl From<ArxivPaper> for SourceItem {
    fn from(paper: ArxivPaper) -> Self {
        SourceItem::from(&paper)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreBreakdown {
    pub keyword_score: f64,
    pub popularity_score: f64,
    pub recency_score: f64,
    pub source_bonus: f64,
}

impl Default for ScoreBreakdown {
    fn default() -> Self {
        Self {
            keyword_score: 0.0,
            popularity_score: 0.0,
            recency_score: 0.0,
            source_bonus: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceMetadata {
    #[serde(rename = "github", alias = "git_hub")]
    GitHub {
        stars: u64,
        forks: u64,
        language: Option<String>,
        topics: Vec<String>,
    },
    #[serde(rename = "arxiv")]
    Arxiv {
        authors: Vec<String>,
        categories: Vec<String>,
    },
    AcademicIndex {
        authors: Vec<String>,
        venue: Option<String>,
        year: Option<i32>,
        doi: Option<String>,
        citation_count: Option<u64>,
        native_id: String,
        source_name: String,
        #[serde(default)]
        external_ids: Vec<String>,
    },
    Bibliography {
        authors: Vec<String>,
        venue: Option<String>,
        year: Option<i32>,
        doi: Option<String>,
        citation_count: Option<u64>,
        native_id: String,
        source_name: String,
        #[serde(default)]
        external_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Citation {
    pub id: String,
    pub source_item_id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_kind: SourceKind,
}

impl Citation {
    pub fn from_source_item(index: usize, item: &SourceItem) -> Self {
        Self {
            id: format!("C{}", index + 1),
            source_item_id: item.id.clone(),
            title: item.title.clone(),
            url: item.url.clone(),
            snippet: item.evidence_snippet.clone(),
            source_kind: item.kind,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitationLedger {
    pub citations: Vec<Citation>,
}

impl CitationLedger {
    pub fn from_items(items: &[SourceItem]) -> Self {
        Self {
            citations: items
                .iter()
                .enumerate()
                .map(|(index, item)| Citation::from_source_item(index, item))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryGroup {
    pub name: String,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchBrief {
    pub topic: String,
    pub user_intent: String,
    pub target_audience: String,
    pub time_scope: String,
    pub inclusion_criteria: Vec<String>,
    pub exclusion_criteria: Vec<String>,
    pub success_criteria: Vec<String>,
}

impl ResearchBrief {
    pub fn from_topic(topic: &str) -> Self {
        Self {
            topic: topic.to_string(),
            user_intent: format!("围绕 `{topic}` 进行 GitHub 与 arXiv 双源技术调研"),
            target_audience: "需要快速理解技术生态、论文进展和开源实现的中文研究者".to_string(),
            time_scope: "优先近期活跃项目和近年论文；保留关键基础工作".to_string(),
            inclusion_criteria: vec![
                "GitHub 仓库必须来自 GitHub API 抓取结果".to_string(),
                "论文必须来自 arXiv API 抓取结果".to_string(),
                "报告结论必须由 CitationLedger 中的来源支撑".to_string(),
            ],
            exclusion_criteria: vec![
                "不进行任意网页搜索".to_string(),
                "不引用 GitHub/arXiv 之外的新 URL".to_string(),
                "不执行 LLM 生成代码".to_string(),
            ],
            success_criteria: vec![
                "形成可审查的章节计划".to_string(),
                "每条证据可追溯到 query attempt 和 source item".to_string(),
                "最终中文报告保留可点击引用".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChapterNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub title_zh: String,
    pub research_question: String,
    pub required_evidence_kinds: Vec<String>,
    pub evidence_quota: usize,
    pub sort_order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryPortfolio {
    pub chapter_id: String,
    pub github_queries: Vec<String>,
    pub arxiv_queries: Vec<String>,
    pub rationale: String,
    pub budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryAttempt {
    pub query_id: String,
    pub source: String,
    pub query: String,
    pub chapter_id: String,
    pub round: usize,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result_count: usize,
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub rate_limit_info: Option<String>,
    #[serde(default)]
    pub parser_error: Option<String>,
    #[serde(default = "default_query_attempt_citeable")]
    pub is_citeable: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub evidence_id: String,
    pub source_item_id: String,
    pub citation_id: String,
    pub chapter_ids: Vec<String>,
    pub query_attempt_ids: Vec<String>,
    pub source_kind: SourceKind,
    pub title: String,
    pub url: String,
    pub evidence_note_zh: String,
    pub evidence_snippet: String,
    pub support_score: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceQueryLineage {
    #[serde(default)]
    pub lineage_id: String,
    pub source_item_id: String,
    #[serde(default)]
    pub chapter_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    pub query_attempt_ids: Vec<String>,
    #[serde(default)]
    pub returned_item_ids: Vec<String>,
    #[serde(default)]
    pub merged_from_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EvidenceMemory {
    pub items: Vec<EvidenceItem>,
    pub query_attempts: Vec<QueryAttempt>,
    #[serde(default)]
    pub source_lineage: Vec<SourceQueryLineage>,
}

impl EvidenceMemory {
    pub fn by_chapter(&self, chapter_id: &str) -> Vec<EvidenceItem> {
        self.items
            .iter()
            .filter(|item| item.chapter_ids.iter().any(|id| id == chapter_id))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    QueryGap,
    SourceGap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoverageRecommendation {
    NoAction,
    SuggestNewQuery,
    OutOfScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageGap {
    pub chapter_id: String,
    pub gap_kind: GapKind,
    pub explanation: String,
    pub recommended_queries: Vec<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageReport {
    pub gaps: Vec<CoverageGap>,
    pub out_of_scope_notice: Vec<String>,
    pub overall_coverage_score: f64,
    pub recommendation: CoverageRecommendation,
}

impl CoverageReport {
    pub fn pass() -> Self {
        Self {
            gaps: Vec::new(),
            out_of_scope_notice: Vec::new(),
            overall_coverage_score: 1.0,
            recommendation: CoverageRecommendation::NoAction,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParagraphWithCitations {
    pub text_zh: String,
    pub cited_evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChapterDraft {
    pub chapter_id: String,
    pub title_zh: String,
    pub paragraphs: Vec<ParagraphWithCitations>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportDraft {
    pub title_zh: String,
    pub chapters: Vec<ChapterDraft>,
    pub global_summary_zh: String,
    pub written_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CitationAuditReport {
    pub url_whitelist_passed: bool,
    pub citation_coverage_ratio: f64,
    pub source_diversity_score: f64,
    pub freshness_warnings: Vec<String>,
    pub unsupported_paragraph_warnings: Vec<String>,
    pub external_url_violations: Vec<String>,
}

impl CitationAuditReport {
    pub fn pass() -> Self {
        Self {
            url_whitelist_passed: true,
            citation_coverage_ratio: 1.0,
            source_diversity_score: 1.0,
            freshness_warnings: Vec::new(),
            unsupported_paragraph_warnings: Vec::new(),
            external_url_violations: Vec::new(),
        }
    }
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out: String = trimmed.chars().take(max_chars).collect();
    out.push_str("...");
    out
}

fn default_query_attempt_citeable() -> bool {
    true
}

pub fn arxiv_id_from_abs_url(id_url: &str) -> String {
    id_url
        .rsplit_once("/abs/")
        .map(|(_, id)| id)
        .unwrap_or(id_url)
        .trim()
        .to_string()
}

pub fn stable_arxiv_id(arxiv_id: &str) -> String {
    let raw = arxiv_id
        .rsplit_once("/abs/")
        .map(|(_, id)| id)
        .unwrap_or(arxiv_id)
        .trim();

    if let Some(pos) = raw.rfind('v') {
        let suffix = &raw[pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return raw[..pos].to_string();
        }
    }

    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn converts_github_repo_to_source_item() {
        let repo = GitHubRepo {
            owner: "openai".to_string(),
            name: "agents-rs".to_string(),
            full_name: "openai/agents-rs".to_string(),
            html_url: "https://github.com/openai/agents-rs".to_string(),
            description: Some("Rust agent framework".to_string()),
            stars: 120,
            forks: 9,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec!["rust".to_string(), "agents".to_string()],
            readme_excerpt: None,
        };

        let item = SourceItem::from(&repo);

        assert_eq!(item.id, "github:openai/agents-rs");
        assert_eq!(item.kind, SourceKind::GitHub);
        assert_eq!(item.title, "openai/agents-rs");
        assert_eq!(item.url, repo.html_url);
        assert_eq!(item.evidence_snippet, "Rust agent framework");
        assert_eq!(item.tags, vec!["rust", "agents"]);
        assert_eq!(item.published_or_updated_at, Some(dt()));
        assert!(matches!(
            item.metadata,
            SourceMetadata::GitHub { stars: 120, .. }
        ));
    }

    #[test]
    fn converts_arxiv_paper_to_source_item_and_strips_version() {
        let paper = ArxivPaper {
            arxiv_id: "2401.12345v2".to_string(),
            title: "Agent Benchmarks for Tool Use".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A benchmark for evaluating tool-calling agents.".to_string(),
            published_at: dt(),
            updated_at: None,
            categories: vec!["cs.AI".to_string()],
            abs_url: "https://arxiv.org/abs/2401.12345v2".to_string(),
            pdf_url: Some("https://arxiv.org/pdf/2401.12345v2".to_string()),
        };

        let item = SourceItem::from(&paper);

        assert_eq!(item.id, "arxiv:2401.12345");
        assert_eq!(item.kind, SourceKind::Arxiv);
        assert_eq!(item.title, paper.title);
        assert_eq!(item.url, paper.abs_url);
        assert_eq!(item.tags, vec!["cs.AI"]);
        assert!(matches!(
            item.metadata,
            SourceMetadata::Arxiv { ref authors, .. } if authors == &vec!["Ada Lovelace".to_string()]
        ));
    }

    #[test]
    fn citation_ledger_uses_source_items() {
        let paper = ArxivPaper {
            arxiv_id: "https://arxiv.org/abs/2501.00001v1".to_string(),
            title: "Rust for Research Agents".to_string(),
            authors: vec![],
            summary: "A short abstract.".to_string(),
            published_at: dt(),
            updated_at: None,
            categories: vec!["cs.SE".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001v1".to_string(),
            pdf_url: None,
        };
        let item = SourceItem::from(&paper);
        let ledger = CitationLedger::from_items(&[item]);

        assert_eq!(ledger.citations.len(), 1);
        assert_eq!(ledger.citations[0].id, "C1");
        assert_eq!(ledger.citations[0].source_item_id, "arxiv:2501.00001");
    }

    #[test]
    fn query_attempt_deserializes_legacy_shape_with_defaults() {
        let json = r#"{
            "query_id": "gh-1",
            "source": "github",
            "query": "rust agent",
            "chapter_id": "ch-1",
            "round": 1,
            "started_at": "2026-05-30T12:00:00Z",
            "finished_at": null,
            "result_count": 1,
            "error": null
        }"#;

        let attempt: QueryAttempt = serde_json::from_str(json).expect("legacy attempt parses");

        assert_eq!(attempt.source_kind, None);
        assert!(attempt.is_citeable);
        assert_eq!(attempt.http_status, None);
    }

    #[test]
    fn academic_metadata_deserializes_without_external_ids() {
        let json = r#"{
            "kind": "academic_index",
            "authors": ["Ada Lovelace"],
            "venue": "TestConf",
            "year": 2026,
            "doi": "10.1234/tool-agent",
            "citation_count": 42,
            "native_id": "abc123",
            "source_name": "semantic_scholar"
        }"#;

        let metadata: SourceMetadata =
            serde_json::from_str(json).expect("legacy academic metadata parses");

        assert!(matches!(
            metadata,
            SourceMetadata::AcademicIndex {
                ref external_ids,
                ..
            } if external_ids.is_empty()
        ));
    }

    #[test]
    fn source_kind_keeps_legacy_wire_names() {
        assert_eq!(
            serde_json::to_string(&SourceKind::GitHub).expect("serializes"),
            "\"github\""
        );
        assert_eq!(
            serde_json::to_string(&SourceKind::Arxiv).expect("serializes"),
            "\"arxiv\""
        );
        assert_eq!(
            serde_json::to_string(&SourceKind::AcademicIndex).expect("serializes"),
            "\"academic_index\""
        );
    }

    #[test]
    fn source_lineage_deserializes_legacy_shape_with_defaults() {
        let json = r#"{
            "source_item_id": "github:acme/rust-agent",
            "query_attempt_ids": ["gh-1"]
        }"#;

        let lineage: SourceQueryLineage =
            serde_json::from_str(json).expect("legacy lineage parses");

        assert_eq!(lineage.source_item_id, "github:acme/rust-agent");
        assert!(lineage.lineage_id.is_empty());
        assert_eq!(lineage.chapter_id, None);
        assert!(lineage.returned_item_ids.is_empty());
    }
}
