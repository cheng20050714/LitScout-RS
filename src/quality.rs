use std::collections::HashSet;

use crate::model::{QualityReport, ScoutReport, SourceKind};

const MIN_TOTAL_SOURCES: usize = 2;

pub fn evaluate(report: &ScoutReport, llm_enabled: bool) -> QualityReport {
    let mut warnings = Vec::new();

    if report.ranked_items.is_empty() {
        warnings.push("No source items collected.".to_string());
    }
    if report.ranked_items.len() < MIN_TOTAL_SOURCES {
        warnings.push(format!(
            "Only {} source item(s) collected; recommended minimum is {}.",
            report.ranked_items.len(),
            MIN_TOTAL_SOURCES
        ));
    }

    let has_github = report
        .ranked_items
        .iter()
        .any(|item| item.kind == SourceKind::GitHub);
    let has_arxiv = report
        .ranked_items
        .iter()
        .any(|item| item.kind == SourceKind::Arxiv);

    if !has_github {
        warnings.push("No GitHub repositories collected.".to_string());
    }
    if !has_arxiv {
        warnings.push("No arXiv papers collected.".to_string());
    }

    let mut seen_ids = HashSet::new();
    for item in &report.ranked_items {
        if item.url.trim().is_empty() {
            warnings.push(format!("Source item `{}` has no URL.", item.id));
        }
        if !seen_ids.insert(item.id.as_str()) {
            warnings.push(format!("Duplicate source item id `{}` found.", item.id));
        }
    }

    if llm_enabled {
        validate_llm_citations(report, &mut warnings);
    }

    QualityReport {
        passed: warnings.is_empty(),
        warnings,
        llm_repaired: false,
    }
}

fn validate_llm_citations(report: &ScoutReport, warnings: &mut Vec<String>) {
    let Some(synthesis) = &report.llm_synthesis else {
        return;
    };

    if synthesis.used_citation_ids.is_empty() && !report.citations.citations.is_empty() {
        warnings.push("LLM synthesis did not declare any used citation IDs.".to_string());
        return;
    }

    let citation_ids = report
        .citations
        .citations
        .iter()
        .map(|citation| citation.id.as_str())
        .collect::<HashSet<_>>();

    for citation_id in &synthesis.used_citation_ids {
        if !citation_ids.contains(citation_id.as_str()) {
            warnings.push(format!(
                "LLM synthesis referenced missing citation id `{citation_id}`."
            ));
        }
    }

    let citation_urls = report
        .citations
        .citations
        .iter()
        .map(|citation| citation.url.as_str())
        .collect::<HashSet<_>>();
    let synthesis_text = combined_synthesis_text(synthesis);
    let used_urls = extract_urls(&synthesis_text);
    if !report.citations.citations.is_empty() && used_urls.is_empty() {
        warnings.push("LLM synthesis did not include any source URLs.".to_string());
    }
    for url in used_urls {
        if !citation_urls.contains(url.as_str()) {
            warnings.push(format!(
                "LLM synthesis included URL outside CitationLedger: {url}."
            ));
        }
    }
}

fn combined_synthesis_text(synthesis: &crate::model::LlmSynthesis) -> String {
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
    text.split_whitespace()
        .filter_map(|token| {
            token.find("http").map(|start| {
                token[start..]
                    .trim_matches(|ch: char| {
                        matches!(ch, '(' | ')' | '[' | ']' | '<' | '>' | ',' | '.' | ';')
                    })
                    .to_string()
            })
        })
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::evaluate;
    use crate::model::{
        ArxivPaper, CitationLedger, GitHubRepo, LlmSynthesis, QualityReport, ScoutReport,
        SearchPlan, SearchQuery, SourceItem,
    };

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn warns_when_minimum_count_or_source_coverage_fails() {
        let report = report_with_items(vec![SourceItem::from(&sample_repo())], None);

        let quality = evaluate(&report, false);

        assert!(!quality.passed);
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("recommended minimum")));
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("No arXiv papers")));
    }

    #[test]
    fn warns_for_missing_urls_and_duplicate_ids() {
        let repo = sample_repo();
        let mut first = SourceItem::from(&repo);
        let mut second = SourceItem::from(&repo);
        first.url.clear();
        second.url.clear();
        let report = report_with_items(vec![first, second], None);

        let quality = evaluate(&report, false);

        assert!(!quality.passed);
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("has no URL")));
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("Duplicate source item id")));
    }

    #[test]
    fn warns_when_llm_references_missing_citation() {
        let items = vec![
            SourceItem::from(&sample_repo()),
            SourceItem::from(&sample_paper()),
        ];
        let synthesis = LlmSynthesis {
            executive_summary: "Summary".to_string(),
            key_findings: vec![],
            recommended_reading_path: vec![],
            limitations: vec![],
            used_citation_ids: vec!["C99".to_string()],
        };
        let report = report_with_items(items, Some(synthesis));

        let quality = evaluate(&report, true);

        assert!(!quality.passed);
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("missing citation id")));
    }

    #[test]
    fn warns_when_llm_output_drops_source_urls() {
        let items = vec![
            SourceItem::from(&sample_repo()),
            SourceItem::from(&sample_paper()),
        ];
        let synthesis = LlmSynthesis {
            executive_summary: "Summary without a source link.".to_string(),
            key_findings: vec![],
            recommended_reading_path: vec![],
            limitations: vec![],
            used_citation_ids: vec!["C1".to_string()],
        };
        let report = report_with_items(items, Some(synthesis));

        let quality = evaluate(&report, true);

        assert!(!quality.passed);
        assert!(quality
            .warnings
            .iter()
            .any(|warning| warning.contains("did not include any source URLs")));
    }

    fn report_with_items(items: Vec<SourceItem>, synthesis: Option<LlmSynthesis>) -> ScoutReport {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let github_repos = items
            .iter()
            .filter(|item| item.kind == crate::model::SourceKind::GitHub)
            .map(|_| sample_repo())
            .collect();
        let arxiv_papers = items
            .iter()
            .filter(|item| item.kind == crate::model::SourceKind::Arxiv)
            .map(|_| sample_paper())
            .collect();
        let citations = CitationLedger::from_items(&items);

        ScoutReport {
            query: query.clone(),
            plan: SearchPlan::from_query(&query),
            generated_at: dt(),
            github_repos,
            arxiv_papers,
            ranked_items: items,
            groups: vec![],
            citations,
            llm_synthesis: synthesis,
            quality: QualityReport::pass(),
        }
    }

    fn sample_repo() -> GitHubRepo {
        GitHubRepo {
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
        }
    }

    fn sample_paper() -> ArxivPaper {
        ArxivPaper {
            arxiv_id: "2501.00001".to_string(),
            title: "Rust Agent Framework".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A paper about Rust agents.".to_string(),
            published_at: dt(),
            updated_at: None,
            categories: vec!["cs.AI".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001".to_string(),
            pdf_url: None,
        }
    }
}
