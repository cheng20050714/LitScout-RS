use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::model::{CategoryGroup, SourceItem};

#[cfg(test)]
pub fn classify_item(item: &mut SourceItem) {
    let rules = default_rules();
    classify_item_with_rules(item, &rules);
}

pub fn classify_items_with_rules(items: &mut [SourceItem], rules: &[ClassificationRule]) {
    items
        .iter_mut()
        .for_each(|item| classify_item_with_rules(item, rules));
}

pub fn classify_item_with_rules(item: &mut SourceItem, rules: &[ClassificationRule]) {
    let haystack = searchable_text(item);
    let mut matched = false;

    for rule in rules {
        if rule
            .keywords
            .iter()
            .any(|keyword| haystack.contains(keyword.as_str()))
        {
            if !item.tags.iter().any(|tag| tag == &rule.label) {
                item.tags.push(rule.label.clone());
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

pub fn group_by_tags(items: &[SourceItem], rules: &[ClassificationRule]) -> Vec<CategoryGroup> {
    let labels = known_labels(rules);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationRule {
    pub label: String,
    pub keywords: Vec<String>,
}

pub fn load_rules(tags_file: Option<&Path>) -> Result<Vec<ClassificationRule>> {
    let Some(path) = tags_file else {
        return Ok(default_rules());
    };
    let content = std::fs::read_to_string(path)?;
    TagDictionary::from_toml_str(&content).map(|dictionary| dictionary.into_rules())
}

pub fn default_rules() -> Vec<ClassificationRule> {
    vec![
        ClassificationRule {
            label: "Agent Framework".to_string(),
            keywords: strings(&[
                "agent framework",
                "agentic framework",
                "multi-agent",
                "agents",
            ]),
        },
        ClassificationRule {
            label: "Tool Calling".to_string(),
            keywords: strings(&["tool calling", "tool-use", "function calling", "tools"]),
        },
        ClassificationRule {
            label: "RAG / Retrieval".to_string(),
            keywords: strings(&["rag", "retrieval", "retriever", "augmented generation"]),
        },
        ClassificationRule {
            label: "Code Agent".to_string(),
            keywords: strings(&["code agent", "coding agent", "software engineering", "swe"]),
        },
        ClassificationRule {
            label: "Web Agent".to_string(),
            keywords: strings(&["web agent", "browser", "webarena", "web navigation"]),
        },
        ClassificationRule {
            label: "Benchmark".to_string(),
            keywords: strings(&["benchmark", "bench", "evaluation", "eval"]),
        },
        ClassificationRule {
            label: "Rust / Systems".to_string(),
            keywords: strings(&["rust", "systems", "compiler", "cargo"]),
        },
    ]
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn known_labels(rules: &[ClassificationRule]) -> HashSet<&str> {
    rules
        .iter()
        .map(|rule| rule.label.as_str())
        .chain(std::iter::once("Other"))
        .collect()
}

fn searchable_text(item: &SourceItem) -> String {
    format!("{} {} {}", item.title, item.summary, item.tags.join(" ")).to_ascii_lowercase()
}

#[derive(Debug, Deserialize)]
struct TagDictionary {
    rules: Vec<TagRule>,
}

impl TagDictionary {
    fn from_toml_str(content: &str) -> Result<Self> {
        let dictionary = toml::from_str::<Self>(content)
            .map_err(|err| AppError::InvalidConfig(format!("invalid tags TOML: {err}")))?;
        if dictionary.rules.is_empty() {
            return Err(AppError::InvalidConfig(
                "tag dictionary must contain at least one [[rules]] entry".to_string(),
            ));
        }
        Ok(dictionary)
    }

    fn into_rules(self) -> Vec<ClassificationRule> {
        self.rules
            .into_iter()
            .map(|rule| ClassificationRule {
                label: rule.label,
                keywords: rule
                    .keywords
                    .into_iter()
                    .map(|keyword| keyword.to_ascii_lowercase())
                    .collect(),
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct TagRule {
    label: String,
    keywords: Vec<String>,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{classify_item, group_by_tags, ClassificationRule, TagDictionary};
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

        let rules = super::default_rules();
        let groups = group_by_tags(&[item], &rules);

        assert!(groups.iter().any(|group| group.name == "Agent Framework"));
        assert!(groups.iter().any(|group| group.name == "Rust / Systems"));
    }

    #[test]
    fn parses_external_tag_dictionary() {
        let dictionary = TagDictionary::from_toml_str(
            r#"
            [[rules]]
            label = "Custom Agent"
            keywords = ["custom-agent", "agent runtime"]
            "#,
        )
        .expect("tags TOML should parse");

        let rules = dictionary.into_rules();

        assert_eq!(
            rules,
            vec![ClassificationRule {
                label: "Custom Agent".to_string(),
                keywords: vec!["custom-agent".to_string(), "agent runtime".to_string()],
            }]
        );
    }
}
