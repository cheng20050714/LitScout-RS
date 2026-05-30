use std::collections::{BTreeMap, HashSet};

use crate::model::{CategoryGroup, SourceItem};

pub fn classify_items(mut items: Vec<SourceItem>) -> Vec<SourceItem> {
    items.iter_mut().for_each(classify_item);
    items
}

pub fn classify_item(item: &mut SourceItem) {
    let haystack = searchable_text(item);
    let mut matched = false;

    for rule in classification_rules() {
        if rule
            .keywords
            .iter()
            .any(|keyword| haystack.contains(keyword))
        {
            if !item.tags.iter().any(|tag| tag == rule.label) {
                item.tags.push(rule.label.to_string());
            }
            item.classification_reasons
                .push(format!("{} matched {:?}", rule.label, rule.keywords));
            matched = true;
        }
    }

    if !matched {
        item.tags.push("Other".to_string());
        item.classification_reasons
            .push("No rule matched; classified as Other".to_string());
    }
}

pub fn group_by_tags(items: &[SourceItem]) -> Vec<CategoryGroup> {
    let labels = known_labels();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for item in items {
        let matched_labels = item
            .tags
            .iter()
            .filter(|tag| labels.contains(tag.as_str()))
            .cloned()
            .collect::<Vec<_>>();

        if matched_labels.is_empty() {
            groups
                .entry("Other".to_string())
                .or_default()
                .push(item.id.clone());
        } else {
            for label in matched_labels {
                groups.entry(label).or_default().push(item.id.clone());
            }
        }
    }

    groups
        .into_iter()
        .map(|(name, item_ids)| CategoryGroup { name, item_ids })
        .collect()
}

struct ClassificationRule {
    label: &'static str,
    keywords: &'static [&'static str],
}

fn classification_rules() -> &'static [ClassificationRule] {
    &[
        ClassificationRule {
            label: "Agent Framework",
            keywords: &[
                "agent framework",
                "agentic framework",
                "multi-agent",
                "agents",
            ],
        },
        ClassificationRule {
            label: "Tool Calling",
            keywords: &["tool calling", "tool-use", "function calling", "tools"],
        },
        ClassificationRule {
            label: "RAG / Retrieval",
            keywords: &["rag", "retrieval", "retriever", "augmented generation"],
        },
        ClassificationRule {
            label: "Code Agent",
            keywords: &["code agent", "coding agent", "software engineering", "swe"],
        },
        ClassificationRule {
            label: "Web Agent",
            keywords: &["web agent", "browser", "webarena", "web navigation"],
        },
        ClassificationRule {
            label: "Benchmark",
            keywords: &["benchmark", "bench", "evaluation", "eval"],
        },
        ClassificationRule {
            label: "Rust / Systems",
            keywords: &["rust", "systems", "compiler", "cargo"],
        },
    ]
}

fn known_labels() -> HashSet<&'static str> {
    classification_rules()
        .iter()
        .map(|rule| rule.label)
        .chain(std::iter::once("Other"))
        .collect()
}

fn searchable_text(item: &SourceItem) -> String {
    format!("{} {} {}", item.title, item.summary, item.tags.join(" ")).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{classify_item, group_by_tags};
    use crate::model::{GitHubRepo, SourceItem};

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn classifies_multiple_labels() {
        let repo = GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework with tool calling benchmark".to_string()),
            stars: 1,
            forks: 1,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec!["rust".to_string()],
            readme_excerpt: None,
        };
        let mut item = SourceItem::from(&repo);

        classify_item(&mut item);

        assert!(item.tags.iter().any(|tag| tag == "Agent Framework"));
        assert!(item.tags.iter().any(|tag| tag == "Tool Calling"));
        assert!(item.tags.iter().any(|tag| tag == "Benchmark"));
        assert!(item.tags.iter().any(|tag| tag == "Rust / Systems"));
        assert!(!item.classification_reasons.is_empty());
    }

    #[test]
    fn groups_items_by_known_classification_tags() {
        let repo = GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework".to_string()),
            stars: 1,
            forks: 1,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec!["rust".to_string()],
            readme_excerpt: None,
        };
        let mut item = SourceItem::from(&repo);
        classify_item(&mut item);

        let groups = group_by_tags(&[item]);

        assert!(groups.iter().any(|group| group.name == "Agent Framework"));
        assert!(groups.iter().any(|group| group.name == "Rust / Systems"));
    }
}
