use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::{ScoutReport, SourceItem, SourceKind};

pub fn resolve_output_path_for_time(
    output: &Path,
    topic: &str,
    generated_at: DateTime<Utc>,
) -> PathBuf {
    let is_markdown_file = output
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));

    if is_markdown_file {
        output.to_path_buf()
    } else {
        output.join(format!(
            "{}-{}.md",
            slugify(topic),
            generated_at.format("%Y%m%d-%H%M%S")
        ))
    }
}

pub async fn write_markdown(report: &ScoutReport, output_path: &Path) -> Result<PathBuf> {
    let path = resolve_output_path_for_time(output_path, &report.query.topic, report.generated_at);
    let markdown = render_markdown(report);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, markdown).await?;
    Ok(path)
}

pub fn render_markdown(report: &ScoutReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# LitScout-RS Report: {}\n\n", report.query.topic));

    out.push_str("## 1. Query Summary\n\n");
    out.push_str(&format!("- Topic: `{}`\n", report.query.topic));
    out.push_str(&format!(
        "- GitHub limit: `{}`\n",
        report.query.github_limit
    ));
    out.push_str(&format!("- arXiv limit: `{}`\n", report.query.arxiv_limit));
    out.push_str(&format!(
        "- GitHub repositories collected: `{}`\n",
        report.github_repos.len()
    ));
    out.push_str(&format!(
        "- arXiv papers collected: `{}`\n",
        report.arxiv_papers.len()
    ));
    out.push_str(&format!(
        "- Ranked source items: `{}`\n",
        report.ranked_items.len()
    ));
    out.push('\n');

    out.push_str("### Search Plan\n\n");
    out.push_str(&format!(
        "- LLM generated: `{}`\n",
        report.plan.llm_generated
    ));
    for aspect in &report.plan.aspects {
        out.push_str(&format!(
            "- `{}`: GitHub `{}`; arXiv `{}`\n",
            aspect.name, aspect.github_query, aspect.arxiv_query
        ));
        if let Some(rationale) = &aspect.rationale {
            out.push_str(&format!("  - Rationale: {rationale}\n"));
        }
    }
    out.push('\n');

    if !report.quality.warnings.is_empty() {
        out.push_str("### Quality Warnings\n\n");
        for warning in &report.quality.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
        out.push('\n');
    }

    out.push_str("## 2. Highlights\n\n");
    if let Some(synthesis) = &report.llm_synthesis {
        out.push_str(
            "LLM-assisted synthesis based only on collected GitHub and arXiv sources.\n\n",
        );
        out.push_str(&format!("{}\n\n", synthesis.executive_summary));
        for finding in &synthesis.key_findings {
            out.push_str(&format!("- {finding}\n"));
        }
        if !synthesis.limitations.is_empty() {
            out.push_str("\nLimitations:\n\n");
            for limitation in &synthesis.limitations {
                out.push_str(&format!("- {limitation}\n"));
            }
        }
        out.push('\n');
    } else if report.ranked_items.is_empty() {
        out.push_str("No highlights are available because no source items were collected.\n\n");
    } else {
        for item in report.ranked_items.iter().take(5) {
            out.push_str(&format!(
                "- [{}]({}) ({}, score {:.2})\n",
                item.title,
                item.url,
                source_kind_label(item.kind),
                item.score
            ));
        }
        out.push('\n');
    }

    out.push_str("## 3. GitHub Repositories\n\n");
    render_items(&mut out, &report.ranked_items, SourceKind::GitHub);

    out.push_str("## 4. arXiv Papers\n\n");
    render_items(&mut out, &report.ranked_items, SourceKind::Arxiv);

    out.push_str("## 5. Topic Clusters\n\n");
    if report.groups.is_empty() {
        out.push_str("No topic clusters were generated.\n\n");
    } else {
        for group in &report.groups {
            if group.item_ids.is_empty() {
                continue;
            }
            out.push_str(&format!("### {}\n\n", group.name));
            for item_id in &group.item_ids {
                if let Some(item) = report.ranked_items.iter().find(|item| &item.id == item_id) {
                    out.push_str(&format!("- [{}]({})\n", item.title, item.url));
                }
            }
            out.push('\n');
        }
    }

    out.push_str("## 6. Observations\n\n");
    render_observations(&mut out, report);

    out.push_str("## 7. Suggested Reading Path\n\n");
    if let Some(synthesis) = &report.llm_synthesis {
        for step in &synthesis.recommended_reading_path {
            out.push_str(&format!("- {step}\n"));
        }
        out.push('\n');
    } else if report.ranked_items.is_empty() {
        out.push_str("No reading path can be suggested without collected sources.\n\n");
    } else {
        for (index, item) in report.ranked_items.iter().take(6).enumerate() {
            out.push_str(&format!("{}. [{}]({})\n", index + 1, item.title, item.url));
        }
        out.push('\n');
    }

    out.push_str("## 8. Citation Ledger\n\n");
    if report.citations.citations.is_empty() {
        out.push_str("No citations recorded.\n\n");
    } else {
        for citation in &report.citations.citations {
            out.push_str(&format!(
                "- `{}` [{}]({}) ({})\n",
                citation.id,
                citation.title,
                citation.url,
                source_kind_label(citation.source_kind)
            ));
        }
        out.push('\n');
    }

    out.push_str("## 9. Run Metadata\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!(
        "- Search plan generated by LLM: `{}`\n",
        report.plan.llm_generated
    ));
    out.push_str(&format!("- Quality passed: `{}`\n", report.quality.passed));
    out.push_str(&format!(
        "- Citation count: `{}`\n",
        report.citations.citations.len()
    ));
    out.push('\n');

    out
}

fn render_items(out: &mut String, items: &[SourceItem], kind: SourceKind) {
    let mut rendered = false;
    for item in items.iter().filter(|item| item.kind == kind) {
        rendered = true;
        out.push_str(&format!("### [{}]({})\n\n", item.title, item.url));
        out.push_str(&format!("- Type: `{}`\n", source_kind_label(item.kind)));
        out.push_str(&format!("- Score: `{:.2}`\n", item.score));
        out.push_str(&format!("- Tags: `{}`\n", display_tags(&item.tags)));
        out.push_str(&format!("- Source URL: <{}>\n", item.url));
        if !item.score_reasons.is_empty() {
            out.push_str(&format!(
                "- Score reasons: `{}`\n",
                item.score_reasons.join("; ")
            ));
        }
        if !item.classification_reasons.is_empty() {
            out.push_str(&format!(
                "- Classification reasons: `{}`\n",
                item.classification_reasons.join("; ")
            ));
        }
        out.push_str("\n");
        out.push_str(&format!("{}\n\n", truncate(&item.summary, 700)));
    }

    if !rendered {
        out.push_str("No items collected for this source.\n\n");
    }
}

fn render_observations(out: &mut String, report: &ScoutReport) {
    if report.ranked_items.is_empty() {
        out.push_str("- No source data was available for observation.\n\n");
        return;
    }

    let github_count = report
        .ranked_items
        .iter()
        .filter(|item| item.kind == SourceKind::GitHub)
        .count();
    let arxiv_count = report
        .ranked_items
        .iter()
        .filter(|item| item.kind == SourceKind::Arxiv)
        .count();

    out.push_str(&format!(
        "- The report contains `{github_count}` GitHub items and `{arxiv_count}` arXiv items after normalization.\n"
    ));
    if let Some(top) = report.ranked_items.first() {
        out.push_str(&format!(
            "- Highest ranked source: [{}]({}) with score `{:.2}`.\n",
            top.title, top.url, top.score
        ));
    }
    out.push('\n');
}

fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::GitHub => "GitHub",
        SourceKind::Arxiv => "arXiv",
    }
}

fn display_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        "none".to_string()
    } else {
        tags.join(", ")
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
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
        "report".to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, Utc};

    use super::{render_markdown, resolve_output_path_for_time, slugify};
    use crate::model::{
        ArxivPaper, CategoryGroup, CitationLedger, GitHubRepo, LlmSynthesis, QualityReport,
        ScoutReport, SearchPlan, SearchQuery, SourceItem,
    };

    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn slugifies_topic() {
        assert_eq!(slugify("Rust Agent Framework"), "rust-agent-framework");
    }

    #[test]
    fn resolve_output_path_file_vs_dir() {
        assert_eq!(
            resolve_output_path_for_time(
                Path::new("reports/agent.md"),
                "Rust Agent Framework",
                dt("2026-05-30T12:34:56Z")
            ),
            Path::new("reports/agent.md")
        );
        assert_eq!(
            resolve_output_path_for_time(
                Path::new("reports"),
                "Rust Agent Framework",
                dt("2026-05-30T12:34:56Z")
            ),
            Path::new("reports/rust-agent-framework-20260530-123456.md")
        );
    }

    #[test]
    fn markdown_contains_required_sections_and_links() {
        let report = sample_report();
        let markdown = render_markdown(&report);

        for section in [
            "# LitScout-RS Report: rust agent framework",
            "## 1. Query Summary",
            "## 2. Highlights",
            "## 3. GitHub Repositories",
            "## 4. arXiv Papers",
            "## 5. Topic Clusters",
            "## 6. Observations",
            "## 7. Suggested Reading Path",
            "## 8. Citation Ledger",
            "## 9. Run Metadata",
        ] {
            assert!(markdown.contains(section), "missing section {section}");
        }
        assert!(markdown.contains("https://github.com/acme/rust-agent"));
        assert!(markdown.contains("https://arxiv.org/abs/2501.00001"));
    }

    #[test]
    fn markdown_marks_llm_synthesis() {
        let mut report = sample_report();
        report.llm_synthesis = Some(LlmSynthesis {
            executive_summary: "See [repo](https://github.com/acme/rust-agent).".to_string(),
            key_findings: vec!["Grounded finding.".to_string()],
            recommended_reading_path: vec!["Read the repo first.".to_string()],
            limitations: vec!["Small sample.".to_string()],
            used_citation_ids: vec!["C1".to_string()],
        });

        let markdown = render_markdown(&report);

        assert!(markdown.contains("LLM-assisted synthesis"));
        assert!(markdown.contains("Limitations:"));
    }

    fn sample_report() -> ScoutReport {
        let query = SearchQuery {
            topic: "rust agent framework".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let repo = GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework".to_string()),
            stars: 42,
            forks: 3,
            language: Some("Rust".to_string()),
            updated_at: dt("2026-05-30T12:00:00Z"),
            topics: vec!["rust".to_string(), "Agent Framework".to_string()],
            readme_excerpt: None,
        };
        let paper = ArxivPaper {
            arxiv_id: "2501.00001".to_string(),
            title: "Rust Agents for Tool Calling".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A paper about Rust agents and tool calling.".to_string(),
            published_at: dt("2026-05-01T12:00:00Z"),
            updated_at: None,
            categories: vec!["cs.AI".to_string(), "Agent Framework".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001".to_string(),
            pdf_url: None,
        };
        let mut ranked_items = vec![SourceItem::from(&repo), SourceItem::from(&paper)];
        ranked_items[0].score = 12.5;
        ranked_items[1].score = 10.0;
        let citations = CitationLedger::from_items(&ranked_items);

        ScoutReport {
            query: query.clone(),
            plan: SearchPlan::from_query(&query),
            generated_at: dt("2026-05-30T12:34:56Z"),
            github_repos: vec![repo],
            arxiv_papers: vec![paper],
            ranked_items,
            groups: vec![CategoryGroup {
                name: "Agent Framework".to_string(),
                item_ids: vec![
                    "github:acme/rust-agent".to_string(),
                    "arxiv:2501.00001".to_string(),
                ],
            }],
            citations,
            llm_synthesis: None,
            quality: QualityReport::pass(),
        }
    }
}
